"""Robot de réessai du serveur gratuit de Claw Web sur Oracle Cloud.

Les machines Ampere A1 de l'offre « Always Free » manquent souvent (« Out of host capacity »),
surtout dans les régions d'une seule zone (Marseille, Paris). Ce robot, lancé toutes les
10 minutes par le workflow `oracle-a1.yml` tant que l'appli l'a allumé, essaie de créer
l'instance : 2 OCPU et 12 Go, puis 1 OCPU et 6 Go. Il trouve seul le sous-réseau public et l'image
Ubuntu 24.04 pour ARM, et donne à l'instance le script de démarrage de l'exécuteur
(`claw-cloud.sh`) avec un jeton d'inscription neuf, demandé à GitHub au moment même : le jeton
d'une heure ne peut plus expirer avant la création. Une fois l'instance créée, il s'arrête.

Entrées (environnement) : OCI_CONFIG (l'aperçu du fichier de configuration donné par Oracle à la
création de la clé d'API), OCI_KEY (la clé privée, PEM), CLAW_ADMIN_TOKEN (jeton GitHub qui peut
inscrire un exécuteur sur le dépôt des sessions et régler les variables de ce dépôt), ENGINE
(dépôt des sessions), GITHUB_REPOSITORY (dépôt du robot, pour son état).

Le journal d'un dépôt public est lisible de tous : il ne montre ni identifiant Oracle, ni adresse,
ni jeton ; seulement ce qui s'est passé.
"""

import base64
import datetime
import json
import os
import sys
import urllib.error
import urllib.request

SHAPE = "VM.Standard.A1.Flex"
# Du plus grand au plus petit : une petite demande passe plus souvent.
SIZES = ((2, 12), (1, 6))
NAME = "claw"
# États d'une instance qui occupe déjà l'allocation : rien à créer.
ALIVE = {"PROVISIONING", "STARTING", "RUNNING", "STOPPING", "STOPPED"}
SOURCE = "https://raw.githubusercontent.com/abasse-ali/claw_code/main/.github/claw-web/pc/claw-cloud.sh"
API = "https://api.github.com"


class RobotError(Exception):
    """Erreur à dire à l'utilisateur (configuration, droits) : le robot s'arrête en échec."""


def config_from(text, key):
    """La configuration du kit d'Oracle d'après l'aperçu que montre la console (lignes
    `user=…`, `fingerprint=…`, `tenancy=…`, `region=…`) et la clé privée."""
    values = {}
    for line in (text or "").splitlines():
        line = line.split("#", 1)[0].strip()
        if not line or line.startswith("[") or "=" not in line:
            continue
        name, value = line.split("=", 1)
        values[name.strip()] = value.strip()
    missing = [name for name in ("user", "fingerprint", "tenancy", "region") if not values.get(name)]
    if missing:
        raise RobotError(f"configuration d'Oracle incomplète : il manque {', '.join(missing)}")
    if "PRIVATE KEY" not in (key or ""):
        raise RobotError("clé privée d'Oracle absente ou illisible (fichier .pem attendu)")
    return {
        "user": values["user"],
        "fingerprint": values["fingerprint"],
        "tenancy": values["tenancy"],
        "region": values["region"],
        "key_content": key.strip() + "\n",
    }


def github(method, path, token, body=None):
    request = urllib.request.Request(
        API + path, method=method, data=None if body is None else json.dumps(body).encode("utf-8"),
        headers={"Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json",
                 "X-GitHub-Api-Version": "2022-11-28", "User-Agent": "claw-oracle-robot",
                 "Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        raw = response.read()
    return json.loads(raw) if raw else None


def registration_token(engine, token):
    """Jeton d'inscription d'un exécuteur (une heure), demandé juste avant la création."""
    try:
        return github("POST", f"/repos/{engine}/actions/runners/registration-token", token)["token"]
    except (urllib.error.URLError, KeyError, TypeError) as error:
        raise RobotError(f"GitHub refuse le jeton d'inscription de {engine} ({error})") from None


def cloud_init(engine, runner_token):
    """Script de démarrage de l'instance : celui que prépare l'appli."""
    return "\n".join([
        "#!/bin/bash",
        f"# Claw Web : exécuteur des sessions de {engine} (instance créée par le robot).",
        f"curl -fsSL {SOURCE} -o /usr/local/bin/claw-cloud",
        "chmod 755 /usr/local/bin/claw-cloud",
        f"claw-cloud install --repo {engine} --token {runner_token}",
        "",
    ])


def out_of_capacity(error):
    """« Out of host capacity » : pas de machine libre, on réessaiera."""
    return getattr(error, "status", None) == 500 and "capacity" in str(getattr(error, "message", "")).lower()


def too_many_requests(error):
    return getattr(error, "status", None) == 429


class Robot:
    """La création de l'instance. `compute`, `network`, `identity` : les clients du kit
    d'Oracle (ou des doublures pour les essais) ; `token` : rend un jeton d'inscription."""

    def __init__(self, config, compute, network, identity, token, models, say=print):
        self.tenancy = config["tenancy"]
        self.compute, self.network, self.identity = compute, network, identity
        self.token = token
        self.models = models
        self.say = say

    def existing(self):
        instances = self.compute.list_instances(compartment_id=self.tenancy).data
        return next((instance for instance in instances
                     if instance.shape == SHAPE and instance.lifecycle_state in ALIVE), None)

    def public_subnet(self):
        for subnet in self.network.list_subnets(compartment_id=self.tenancy).data:
            if subnet.lifecycle_state == "AVAILABLE" and not subnet.prohibit_public_ip_on_vnic:
                return subnet
        raise RobotError("aucun sous-réseau public : créez le réseau avec l'assistant VCN d'Oracle "
                         "(« Créer un VCN avec connectivité Internet »)")

    def image(self):
        images = self.compute.list_images(
            compartment_id=self.tenancy, operating_system="Canonical Ubuntu",
            operating_system_version="24.04", shape=SHAPE, sort_by="TIMECREATED", sort_order="DESC",
        ).data
        if not images:
            raise RobotError("aucune image Ubuntu 24.04 pour Ampere A1 dans cette région")
        return images[0]

    def domains(self):
        return [domain.name for domain in self.identity.list_availability_domains(compartment_id=self.tenancy).data]

    def launch(self, domain, ocpus, memory, image, subnet, user_data):
        models = self.models
        details = models.LaunchInstanceDetails(
            compartment_id=self.tenancy,
            availability_domain=domain,
            display_name=NAME,
            shape=SHAPE,
            shape_config=models.LaunchInstanceShapeConfigDetails(ocpus=float(ocpus), memory_in_gbs=float(memory)),
            source_details=models.InstanceSourceViaImageDetails(source_type="image", image_id=image.id),
            create_vnic_details=models.CreateVnicDetails(subnet_id=subnet.id, assign_public_ip=True),
            metadata={"user_data": base64.b64encode(user_data.encode("utf-8")).decode("ascii")},
            freeform_tags={"claw": "executeur"},
        )
        return self.compute.launch_instance(details).data

    def run(self, engine):
        """« done » (instance déjà là), « created », « waiting » (pas de place), « busy » (429)."""
        if self.existing() is not None:
            self.say("Une instance Ampere A1 existe déjà : rien à créer.")
            return "done"
        subnet, image, domains = self.public_subnet(), self.image(), self.domains()
        user_data = cloud_init(engine, self.token())
        for ocpus, memory in SIZES:
            for domain in domains:
                try:
                    self.launch(domain, ocpus, memory, image, subnet, user_data)
                except Exception as error:  # noqa: BLE001 (erreurs du kit d'Oracle)
                    if out_of_capacity(error):
                        self.say(f"{ocpus} OCPU et {memory} Go : pas de place pour l'instant.")
                        continue
                    if too_many_requests(error):
                        self.say("Oracle demande de ralentir : prochain essai dans 10 minutes.")
                        return "busy"
                    raise RobotError(f"Oracle refuse la création : {getattr(error, 'code', '')} "
                                     f"{getattr(error, 'message', error)}") from None
                self.say(f"Instance créée : {ocpus} OCPU et {memory} Go. L'exécuteur s'installe "
                         "(15 à 25 minutes), puis apparaît en ligne dans Claw Web.")
                return "created"
        return "waiting"


def save_state(repo, token, outcome, detail):
    """État lu par l'appli (variable ORACLE_ROBOT_STATE). ORACLE_ROBOT passe à « done » quand
    l'instance existe, à « off » après une erreur qui se répéterait à chaque essai (clé refusée,
    pas de sous-réseau public) : le robot s'éteint, au lieu d'échouer toutes les 10 minutes."""
    state = json.dumps({"t": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                        "outcome": outcome, "detail": detail}, ensure_ascii=False)
    settings = [("ORACLE_ROBOT_STATE", state)]
    if outcome in ("created", "done"):
        settings.append(("ORACLE_ROBOT", "done"))
    elif outcome == "error":
        settings.append(("ORACLE_ROBOT", "off"))
    for name, value in settings:
        try:
            github("PATCH", f"/repos/{repo}/actions/variables/{name}", token, {"name": name, "value": value})
        except urllib.error.HTTPError as error:
            if error.code != 404:
                raise
            github("POST", f"/repos/{repo}/actions/variables", token, {"name": name, "value": value})
    # Éteint, le workflow ne se lance plus du tout (aucun passage toutes les 10 minutes).
    if outcome in ("created", "done", "error"):
        github("PUT", f"/repos/{repo}/actions/workflows/oracle-a1.yml/disable", token)


def main():
    import oci  # noqa: PLC0415 (installé par le workflow seulement)

    repo = os.environ.get("GITHUB_REPOSITORY", "")
    token = os.environ.get("CLAW_ADMIN_TOKEN", "").strip()
    engine = os.environ.get("ENGINE", "").strip() or "abasse-ali/claw-sessions"
    lines = []
    say = lambda text: (print(text, flush=True), lines.append(text))  # noqa: E731
    try:
        if not token:
            raise RobotError("jeton GitHub du robot absent (secret CLAW_ADMIN_TOKEN)")
        config = config_from(os.environ.get("OCI_CONFIG", ""), os.environ.get("OCI_KEY", ""))
        robot = Robot(config, oci.core.ComputeClient(config), oci.core.VirtualNetworkClient(config),
                      oci.identity.IdentityClient(config), lambda: registration_token(engine, token),
                      oci.core.models, say)
        outcome = robot.run(engine)
    except RobotError as error:
        say(f"Le robot s'arrête : {error}")
        outcome = "error"
    except oci.exceptions.ServiceError as error:
        # Clé refusée, droits absents : définitif. Panne passagère d'Oracle : on réessaiera.
        if error.status in (401, 403, 404):
            say(f"Le robot s'arrête : Oracle refuse la clé d'API ({error.status} {error.code})")
            outcome = "error"
        else:
            say(f"Oracle ne répond pas bien ({error.status} {error.code}) : prochain essai dans 10 minutes.")
            outcome = "busy"
    except (oci.exceptions.RequestException, urllib.error.URLError) as error:
        say(f"Réseau indisponible ({type(error).__name__}) : prochain essai dans 10 minutes.")
        outcome = "busy"
    if token and repo:
        try:
            save_state(repo, token, outcome, " ".join(lines)[-300:])
        except urllib.error.URLError as error:
            print(f"État non enregistré : {error}", flush=True)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as file:
            file.write("\n".join(lines) + "\n")
    sys.exit(1 if outcome == "error" else 0)


if __name__ == "__main__":
    main()
