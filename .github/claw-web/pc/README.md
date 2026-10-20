# Sessions de Claw Web sur votre machine : serveur gratuit ou PC

Les sessions de Claw Web tournent d'ordinaire sur une machine de GitHub, dans le dépôt privé des
sessions (`claw-sessions`) : elles consomment les minutes GitHub Actions du compte (2 000 par mois
avec GitHub Free). Ici, elles tournent sur votre machine, avec un exécuteur auto-hébergé de
GitHub : aucune minute n'est comptée. Deux machines possibles, qui peuvent coexister :

- **un serveur gratuit dans le nuage** (Oracle Cloud, offre « Always Free »), allumé jour et
  nuit, sans votre PC : `claw-cloud.sh` ;
- **votre PC**, dans Docker Desktop : `claw-pc.ps1`.

Les deux portent l'étiquette `claw-pc`, celle que choisit l'appli (Réglages, Sessions, « Sur ma
machine ») : une session part sur celle qui est libre.

## Serveur gratuit d'Oracle Cloud

L'offre « Always Free » d'Oracle Cloud donne, sans limite de durée, 2 cœurs ARM (Ampere A1), 12 Go
de mémoire et 200 Go de disque (relevé le 20 octobre 2026 dans la documentation d'Oracle ; c'était
4 cœurs et 24 Go avant juin 2026). C'est la seule offre gratuite permanente qui suffit : celle de
Google Cloud (e2-micro) n'a que 1 Go de mémoire et 1 Go de trafic sortant par mois, et les Spaces
Docker de Hugging Face sont devenus payants.

Tout se fait depuis le téléphone, sans PC :

1. Créez un compte sur signup.cloud.oracle.com. Oracle demande une carte pour vérifier votre
   identité ; rien n'est prélevé tant que vous restez dans l'offre gratuite.
2. Dans l'appli : Réglages, Sessions, « Serveur gratuit dans le nuage », « Préparer le script »,
   puis « Copier le script ». Il contient un jeton d'inscription valable une heure.
3. Dans la console d'Oracle : Compute, Instances, *Create instance*. Image : **Ubuntu 24.04**.
   Shape : **Ampere VM.Standard.A1.Flex**, 2 OCPU et 12 Go. Dans *Advanced options*,
   *Management*, collez le script dans *Initialization script* (cloud-init). Créez l'instance.
4. Le serveur installe seul Docker, inscrit l'exécuteur `claw-cloud` dans les premières minutes
   (tant que le jeton est valable), puis construit l'image complète : il apparaît en ligne dans
   l'appli au bout de 15 à 25 minutes. Choisissez « Sur ma machine » dans Sessions.

Ce qu'il faut savoir :

- « Out of capacity » (manque de capacité) à la création : les machines ARM gratuites manquent
  dans la région. Certaines régions, comme Marseille, n'ont qu'un domaine de disponibilité. Dans
  l'ordre : demandez 1 OCPU et 6 Go (agrandissement possible ensuite, instance arrêtée) ;
  réessayez à d'autres heures ; passez le compte en « Pay As You Go » (Facturation, *Upgrade*),
  dont les comptes obtiennent bien plus facilement ces machines, sans frais tant que vous ne
  créez que des ressources « Toujours gratuit » (créez une alerte de budget à 1 €).
- Une machine **E2.1.Micro** (AMD, 1 Go de mémoire, l'autre offre gratuite) ne suffit pas :
  Chrome, Rust et les tests des sessions demandent plusieurs Go. Elle ne prend rien sur
  l'allocation ARM : une instance A1 reste gratuite à côté.
- Oracle peut arrêter une machine gratuite restée inactive sept jours (processeur, réseau et
  mémoire sous 20 %). Relancez-la depuis la console (Instances, *Start*) ; passer le compte en
  « Pay As You Go » l'évite, sans frais tant que vous restez dans l'offre gratuite.
- Jeton expiré avant l'inscription (installation lancée plus d'une heure après la préparation du
  script) : préparez un nouveau script dans l'appli et lancez, dans la console d'Oracle
  (Cloud Shell, ou *Run command* de l'instance) :
  `sudo claw-cloud register --repo abasse-ali/claw-sessions --token NOUVEAU_JETON`.
- Le journal de l'installation est dans `/var/log/cloud-init-output.log`.

### Robot de réessai

Quand Oracle manque de place, le robot réessaie à votre place : toutes les 10 minutes, le
workflow `oracle-a1.yml` du dépôt public du code de Claw (minutes gratuites, sans votre PC) essaie
de créer l'instance, en 2 OCPU et 12 Go puis en 1 OCPU et 6 Go
(`.github/claw-web/oracle/a1_robot.py`). Il trouve seul le sous-réseau public et l'image Ubuntu
24.04 pour ARM, demande à GitHub un jeton d'inscription neuf au moment même (il ne peut donc pas
expirer avant la création), et donne à l'instance le script de démarrage de `claw-cloud.sh`.

- **Démarrer** : dans l'appli, « Serveur gratuit dans le nuage », partie « Robot de réessai ».
  Créez une clé d'API dans la console d'Oracle (profil, *Mon profil*, *Clés d'API*, *Ajouter une
  clé d'API*, « Générer une paire de clés d'API », *Télécharger la clé privée*, *Ajouter*), puis
  collez l'aperçu du fichier de configuration et choisissez le fichier `.pem`. L'appli pose
  chiffrés les secrets `OCI_CONFIG`, `OCI_KEY` et `CLAW_ADMIN_TOKEN` (son jeton GitHub, pour
  inscrire l'exécuteur), règle les variables `ORACLE_ROBOT=on` et `ORACLE_ROBOT_ENGINE`, active le
  workflow et lance un premier essai.
- **Fin** : l'instance créée (ou une A1 déjà là), le robot passe `ORACLE_ROBOT` à `done` et
  désactive son workflow. Une erreur qui se répéterait (clé refusée, pas de sous-réseau public,
  refus d'Oracle) l'éteint aussi (`off`) : l'appli affiche la raison. Une panne passagère
  d'Oracle, un refus pour trop de demandes : il réessaie au tour suivant.
- **Arrêter** : bouton « Arrêter le robot » de l'appli.
- Le journal d'un dépôt public est lisible de tous : le robot n'y écrit ni identifiant Oracle, ni
  adresse, ni jeton. L'état qu'il laisse à l'appli est dans la variable `ORACLE_ROBOT_STATE`.

Commandes sur le serveur (`sudo claw-cloud …`) :

| Commande | Effet |
|---|---|
| `status` | état du conteneur et dernières lignes de son journal |
| `update` | reconstruit l'image (après une mise à jour du code de Claw) et relance le conteneur ; refusé pendant une session |
| `register --repo … --token …` | nouvelle inscription auprès de GitHub |
| `stop` / `start` | arrête ou relance l'exécuteur ; les sessions attendent dans la file de GitHub |
| `remove` | supprime conteneur, volume et images (retirez aussi l'exécuteur dans GitHub : Settings, Actions, Runners) |

Le même script marche sur tout serveur Ubuntu, Debian ou Oracle Linux (Docker CE installé par
`dnf` ; une commande `docker` qui n'est que Podman est refusée), processeur ARM ou Intel/AMD,
4 Go de mémoire au moins. Sur un **serveur déjà en service**, l'appli donne une commande à lancer
en SSH ou dans Cloud Shell, en mode partagé :

```bash
curl -fsSL https://raw.githubusercontent.com/abasse-ali/claw_code/main/.github/claw-web/pc/claw-cloud.sh \
  | sudo bash -s -- install --repo propriétaire/claw-sessions --token JETON --shared
```

`--shared` limite le conteneur à la moitié des processeurs et de la mémoire, avec une priorité
basse : le service qui tourne déjà garde la main. `--cpus 1 --memory 4g` règle les limites à la
main ; elles sont gardées pour les mises à jour (`/opt/claw-cloud/limits`).

## Votre PC (Docker Desktop)

Ce qu'il faut :

- Docker Desktop (moteur Linux, WSL 2), lancé.
- GitHub CLI connecté (`gh auth login`) avec un compte administrateur du dépôt des sessions.
- Environ 4 Go d'espace disque pour l'image (Linux, Python, git, Rust, Node, Chrome et
  l'exécuteur de GitHub).

Dans PowerShell, depuis ce dossier :

```powershell
powershell -ExecutionPolicy Bypass -File .\claw-pc.ps1 install
```

Le script construit l'image (plusieurs minutes la première fois), inscrit l'exécuteur `claw-pc`
auprès de `abasse-ali/claw-sessions` (autre dépôt : `-Engine propriétaire/nom`) et le lance.
Le jeton d'inscription n'est jamais affiché.

Pour que les sessions partent dès que le PC est allumé : Docker Desktop, Settings, General,
« Start Docker Desktop when you sign in ». Le conteneur redémarre avec Docker.

| Commande | Effet |
|---|---|
| `.\claw-pc.ps1 status` | état du conteneur, et de l'exécuteur vu par GitHub (en ligne, en session) |
| `.\claw-pc.ps1 update` | reconstruit l'image et relance le conteneur ; refusé pendant une session |
| `.\claw-pc.ps1 stop` | arrête l'exécuteur ; les sessions attendent dans la file de GitHub |
| `.\claw-pc.ps1 start` | le relance ; les sessions en attente partent |
| `.\claw-pc.ps1 remove` | le désinscrit de GitHub, puis supprime conteneur, volume et image |

## L'image

Une seule image pour les deux machines (`Dockerfile`), en deux étapes : `runner`, l'exécuteur de
GitHub seul, qui sert à l'inscription dans les premières minutes ; puis l'image complète (Python,
git, Rust, Node, un navigateur pour les captures de l'outil `Screenshot`). Sur processeur ARM,
Google Chrome n'existant pas pour Linux, le navigateur est le Chromium de Playwright.

## Ce qui change par rapport à une machine de GitHub

- Une machine de GitHub repart de zéro à chaque session ; votre machine garde ses fichiers. Le
  moteur vide donc le dossier de travail au début de chaque session, et les réglages, la mémoire
  et les caches de Claw vivent dans le dossier temporaire du job (`CLAW_CONFIG_HOME`), vidé à
  chaque session : rien d'une session d'un dépôt ne passe à la suivante.
- Claw tourne avec toutes les autorisations, comme sur GitHub, mais dans le conteneur, jamais en
  root : il ne voit pas les fichiers de la machine.
- Une session attend tant qu'aucune machine n'est en ligne, puis part seule.
- L'exécuteur ne sert que le dépôt privé des sessions. GitHub déconseille un exécuteur
  auto-hébergé pour un dépôt public : n'importe qui pourrait y faire tourner du code.

## Workflow du dépôt des sessions

L'appli passe le choix de la machine au workflow `claw-web.yml` du dépôt des sessions, qui le
transmet au moteur. Ce workflow doit connaître l'entrée `runner`, et donner au moteur le droit
d'écriture sur le dépôt des sessions (captures d'écran rangées sur la branche des fichiers) :

```yaml
on:
  workflow_dispatch:
    inputs:
      # … entrées existantes (repo, base, session, model, team, ultracode)
      runner:
        description: "Machine (vide : GitHub ; claw-pc : votre serveur ou votre PC)"
        required: false
        type: string
        default: ""

permissions:
  actions: read
  contents: write
  issues: write

jobs:
  task:
    uses: abasse-ali/claw_code/.github/workflows/claw-web.yml@main
    with:
      # … entrées existantes
      runner: ${{ inputs.runner }}
    secrets: inherit
```

Sans l'entrée `runner`, l'appli dit que le workflow ne connaît pas encore « Sur ma machine ».
