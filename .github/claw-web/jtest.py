"""JTest : faire tourner le travail d'une session (site, appli web, programme) sur le serveur de
Claw Web (le serveur gratuit d'Oracle, exécuteur `claw-pc` ARM64) et l'ouvrir sur le téléphone
par un lien https.

Le job `jtest` de claw-web.yml lance ce script sur le serveur :
- `start` : clone la branche demandée dans `~/jtest/<session>/`, devine comment lancer le projet
  (ou suit `jtest.json`, ou ce que Claw a demandé), installe ses dépendances, le lance, puis
  ouvre un tunnel Cloudflare gratuit (`cloudflared`, adresse `https://….trycloudflare.com`, sans
  compte ni réglage chez Oracle). Le tunnel ne mène pas au projet mais à la porte
  (jtest_gate.py), qui ne laisse passer que la clé de cette JTest : une clé aléatoire par
  lancement, écrite seulement dans le commentaire de la session (dépôt privé des sessions), que
  Claw Web ajoute au lien « Ouvrir sur le téléphone ». L'adresse seule mène à « Accès réservé ».
  Les processus survivent à la fin du job (RUNNER_TRACKING_ID vidé : l'exécuteur ne les arrête
  pas) et s'arrêtent seuls au bout de quelques heures (`timeout`). Un programme qui n'est pas un
  site s'ouvre dans un terminal web (`ttyd`), protégé en plus par un mot de passe que la porte
  donne elle-même ;
- `stop` : arrête la session JTest.

Il le dit dans la session par un commentaire marqué `<!-- claw-web:jtest {...} -->`, que l'appli
montre en carte (« Ouvrir », « Arrêter ») et que l'agent ne prend pas pour un message.

Entrées (environnement) : SESSION_INPUT (`jtest-<n>` ou `jtest-stop-<n>`), REPO, BRANCH, OPTIONS
(JSON facultatif : command, install, port, cwd, path, kind, hours), JOB_TOKEN (sessions),
REPO_TOKEN (clone), GITHUB_REPOSITORY (dépôt des sessions).
"""

import json
import os
import platform
import re
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path

MARK = "claw-web:jtest"
COMMENT = re.compile(r"<!--\s*claw-web:jtest\s+(\{.*?\})\s*-->", re.S)
HOME = Path(os.environ.get("JTEST_HOME") or Path.home() / "jtest")
BIN = HOME / "bin"
# Au plus trois JTest en ligne sur le serveur (2 cœurs, 12 Go, partagés avec les sessions).
MAX_LIVE = 3
DEFAULT_HOURS = 4
MAX_HOURS = 24
INSTALL_TIMEOUT = 15 * 60
READY_TIMEOUT = 4 * 60
CLOUDFLARED = "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-{arch}"
TTYD = "https://github.com/tsl0922/ttyd/releases/download/1.7.7/ttyd.{arch}"
TUNNEL_URL = re.compile(r"https://[a-z0-9-]+\.trycloudflare\.com")
GATE = Path(__file__).resolve().parent / "jtest_gate.py"
# Ce que l'agent et le texte du commentaire ne montrent jamais.
PRIVATE = ("key", "user", "password")
# Dossiers où chercher le projet quand la racine n'en a pas.
SUBDIRS = ("app", "web", "frontend", "client", "site", "www", "src")
STATIC_DIRS = (".", "docs", "public", "site", "dist", "build", "www")
# Jamais un site à servir : dépendances, environnements, caches.
SKIPPED_DIRS = ("node_modules", "venv", "vendor", "__pycache__", "target")


@dataclass
class Plan:
    """Comment lancer le projet : `web` (un serveur sur `port`) ou `terminal` (un programme dans
    un terminal web)."""

    kind: str
    label: str
    command: str
    install: list = field(default_factory=list)
    cwd: str = "."
    port: int = 0
    env: dict = field(default_factory=dict)
    path: str = "/"


class JTestError(Exception):
    """Ce qui empêche JTest de lancer le projet, dit à l'utilisateur (en français)."""


# --- Ce qu'il faut lancer ----------------------------------------------------------------------

def read_json(path):
    try:
        value = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    return value if isinstance(value, dict) else None


def options_from(root, requested):
    """Les réglages donnés : ceux de Claw ou de l'appli d'abord, puis `jtest.json` du projet."""
    found = {}
    for path in (root / "jtest.json", root / ".claw" / "jtest.json"):
        found.update(read_json(path) or {})
    found.update({key: value for key, value in (requested or {}).items() if value not in (None, "", [])})
    return found


def has_python_module(text, name):
    return re.search(rf"^\s*(from|import)\s+{name}\b", text, re.M) is not None


def python_entry(folder):
    for name in ("app.py", "main.py", "server.py", "wsgi.py", "asgi.py", "run.py", "manage.py"):
        if (folder / name).is_file():
            return name
    files = sorted(path.name for path in folder.glob("*.py"))
    return files[0] if len(files) == 1 else ""


def node_plan(folder, port):
    data = read_json(folder / "package.json") or {}
    deps = {**(data.get("dependencies") or {}), **(data.get("devDependencies") or {})}
    scripts = data.get("scripts") or {}
    if (folder / "pnpm-lock.yaml").is_file():
        install = ["npx -y pnpm@9 install"]
    elif (folder / "yarn.lock").is_file():
        install = ["npx -y yarn@1 install"]
    elif (folder / "package-lock.json").is_file():
        install = ["npm ci"]
    else:
        install = ["npm install"]
    env = {"PORT": str(port), "HOST": "127.0.0.1", "BROWSER": "none"}
    if "next" in deps:
        return Plan("web", "Next.js", f"npx next dev -p {port} -H 127.0.0.1", install, env=env)
    if "astro" in deps:
        return Plan("web", "Astro", f"npx astro dev --host 127.0.0.1 --port {port}", install, env=env)
    if "nuxt" in deps:
        return Plan("web", "Nuxt", f"npx nuxi dev --host 127.0.0.1 --port {port}", install, env=env)
    if "@angular/core" in deps:
        return Plan("web", "Angular", f"npx ng serve --host 127.0.0.1 --port {port} --disable-host-check",
                    install, env=env)
    if "react-scripts" in deps:
        return Plan("web", "Create React App", "npm start", install,
                    env={**env, "DANGEROUSLY_DISABLE_HOST_CHECK": "true"})
    if "vite" in deps or "vite" in str(scripts.get("dev") or ""):
        return Plan("web", "Vite", f"npx vite --host 127.0.0.1 --port {port} --strictPort", install, env=env)
    if scripts.get("dev"):
        return Plan("web", "npm run dev", "npm run dev", install, env=env)
    if scripts.get("start"):
        return Plan("web", "npm start", "npm start", install, env=env)
    # Une page avec un package.json fait par `npm init` (« main »: « app.js », le script de la
    # page) : un site statique, pas un programme Node (session #30, 25 octobre 2026).
    if (folder / "index.html").is_file():
        return None
    main = data.get("main") or ("index.js" if (folder / "index.js").is_file() else "")
    if main:
        return Plan("terminal", f"node {main}", f"node {main}", install)
    return None


def python_plan(folder, port):
    requirements = (folder / "requirements.txt").read_text(encoding="utf-8", errors="replace") \
        if (folder / "requirements.txt").is_file() else ""
    install = ["python3 -m venv .venv"]
    if requirements:
        install.append(".venv/bin/pip install -q -r requirements.txt")
    elif (folder / "pyproject.toml").is_file():
        install.append(".venv/bin/pip install -q .")
    python = ".venv/bin/python"
    if (folder / "manage.py").is_file():
        return Plan("web", "Django", f"{python} manage.py runserver 127.0.0.1:{port}", install)
    entry = python_entry(folder)
    text = (folder / entry).read_text(encoding="utf-8", errors="replace") if entry else ""
    lowered = requirements.lower()
    if "streamlit" in lowered or has_python_module(text, "streamlit"):
        return Plan("web", "Streamlit", f"{python} -m streamlit run {entry or 'app.py'} --server.port {port} "
                    "--server.address 127.0.0.1 --server.headless true", install)
    module = entry[:-3] if entry.endswith(".py") else entry
    fastapi = re.search(r"^(\w+)\s*=\s*FastAPI\(", text, re.M)
    if fastapi:
        if "uvicorn" not in lowered:
            install.append(".venv/bin/pip install -q uvicorn")
        return Plan("web", "FastAPI", f"{python} -m uvicorn {module}:{fastapi.group(1)} --host 127.0.0.1 "
                    f"--port {port}", install)
    flask = re.search(r"^(\w+)\s*=\s*Flask\(", text, re.M)
    if flask:
        return Plan("web", "Flask", f"{python} -m flask --app {module}:{flask.group(1)} run --host 127.0.0.1 "
                    f"--port {port}", install)
    if entry:
        return Plan("terminal", f"python3 {entry}", f"{python} {entry}", install)
    return None


def static_plan(folder, port):
    for name in STATIC_DIRS:
        if (folder / name / "index.html").is_file():
            where = "." if name == "." else name
            return Plan("web", "Site statique" + ("" if where == "." else f" ({where}/)"),
                        f"python3 -m http.server {port} --bind 127.0.0.1 --directory {where}")
    return None


def nearest_page(root, depth=3):
    """Le dossier du `index.html` le moins profond du projet, ailleurs que dans les dossiers connus
    (`site-perso/`, `etape1/`...), sans les dépendances ni les dossiers cachés. Vu le 24 octobre
    2026 (session #29) : un site en HTML rangé dans `site-perso/`, et JTest ne savait pas le lancer."""
    level = [Path(root)]
    for _ in range(depth):
        level = sorted(child for folder in level for child in folder.iterdir()
                       if child.is_dir() and not child.name.startswith(".") and child.name not in SKIPPED_DIRS)
        for folder in level:
            if (folder / "index.html").is_file():
                return folder
    return None


def other_plan(folder):
    if (folder / "Cargo.toml").is_file():
        return Plan("terminal", "cargo run", "cargo run --quiet", ["cargo build --quiet"])
    if (folder / "go.mod").is_file() and shutil.which("go"):
        return Plan("terminal", "go run .", "go run .")
    makefile = folder / "Makefile"
    if makefile.is_file() and re.search(r"^run\s*:", makefile.read_text(encoding="utf-8", errors="replace"), re.M):
        return Plan("terminal", "make run", "make run")
    for name in ("main.sh", "run.sh", "start.sh"):
        if (folder / name).is_file():
            return Plan("terminal", name, f"bash {name}")
    return None


def detect(root, requested=None, port=0):
    """Le plan de lancement du projet cloné dans `root`."""
    root = Path(root)
    options = options_from(root, requested)
    port = int(options.get("port") or port or free_port())
    if options.get("command"):
        install = options.get("install") or []
        install = [install] if isinstance(install, str) else [str(item) for item in install]
        kind = options.get("kind") if options.get("kind") in ("web", "terminal") else "web"
        return Plan(kind, "commande donnée", str(options["command"]).replace("{port}", str(port)),
                    [item.replace("{port}", str(port)) for item in install], str(options.get("cwd") or "."), port,
                    {"PORT": str(port)}, str(options.get("path") or "/"))
    folders = [str(options["cwd"])] if options.get("cwd") else [".", *SUBDIRS]
    for name in folders:
        folder = root / name
        if not folder.is_dir():
            continue
        for build in ((lambda f: node_plan(f, port)) if (folder / "package.json").is_file() else None,
                      (lambda f: python_plan(f, port)) if any(folder.glob("*.py"))
                      or (folder / "requirements.txt").is_file() else None,
                      lambda f: static_plan(f, port), other_plan):
            plan = build(folder) if build else None
            if plan:
                plan.cwd, plan.port = name, port
                plan.path = str(options.get("path") or plan.path)
                return plan
    site = None if options.get("cwd") else nearest_page(root)
    if site is not None:
        where = site.relative_to(root).as_posix()
        # Un projet Node rangé dans un dossier au nom libre : son `index.html` est la page de Vite,
        # qui appelle `/src/main.tsx` ; servie telle quelle, elle restait blanche sur le téléphone
        # (essai D1 de Claw Web, 30 octobre 2026 : l'appli dans `habits-app/`).
        plan = node_plan(site, port) if (site / "package.json").is_file() else None
        if plan:
            plan.cwd, plan.port = where, port
            plan.path = str(options.get("path") or plan.path)
            return plan
        return Plan("web", f"Site statique ({where}/)",
                    f"python3 -m http.server {port} --bind 127.0.0.1 --directory {where}", port=port)
    raise JTestError("JTest ne sait pas lancer ce projet : ajoutez à la racine un fichier `jtest.json`, par "
                     "exemple `{\"command\": \"npm run dev -- --port {port}\"}`, ou demandez à Claw de lancer "
                     "JTest avec la commande.")


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


# --- Processus -----------------------------------------------------------------------------------

def clean_env(environ, extra=None):
    """L'environnement des programmes lancés : ni jeton ni secret (le site est public)."""
    env = {key: value for key, value in environ.items()
           if not re.search(r"TOKEN|SECRET|_KEY$|PASSWORD|^ACTIONS_|^GITHUB_|^RUNNER_", key, re.I)}
    env["RUNNER_TRACKING_ID"] = ""
    env.update(extra or {})
    return env


def spawn(command, cwd, env, log_path, hours):
    """Lance `command` dans son propre groupe de processus, arrêté seul au bout de `hours` heures.
    Rend le numéro du groupe."""
    log = open(log_path, "ab")
    process = subprocess.Popen(["timeout", f"{int(hours * 3600)}s", "bash", "-lc", command], cwd=cwd, env=env,
                               stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                               start_new_session=True)
    log.close()
    return process.pid


def alive(pgid):
    """Un processus du groupe tourne encore (un processus fini, pas encore relevé par son parent,
    ne compte pas)."""
    if not pgid:
        return False
    if os.path.isdir("/proc"):
        for entry in os.listdir("/proc"):
            if not entry.isdigit():
                continue
            try:
                with open(f"/proc/{entry}/stat", encoding="utf-8", errors="replace") as handle:
                    fields = handle.read().rsplit(")", 1)[1].split()
            except (OSError, IndexError):
                continue
            if len(fields) > 2 and fields[2] == str(pgid) and fields[0] != "Z":
                return True
        return False
    try:
        os.killpg(pgid, 0)
        return True
    except (ProcessLookupError, PermissionError, TypeError):
        return False


def kill_group(pgid, grace=5):
    if not pgid or not alive(pgid):
        return
    try:
        os.killpg(pgid, signal.SIGTERM)
    except ProcessLookupError:
        return
    deadline = time.time() + grace
    while time.time() < deadline and alive(pgid):
        time.sleep(0.2)
    try:
        os.killpg(pgid, signal.SIGKILL)
    except ProcessLookupError:
        pass


def tail(path, lines=25):
    try:
        text = Path(path).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""
    return "\n".join(text.rstrip().splitlines()[-lines:])


def wait_http(port, timeout=READY_TIMEOUT, pgid=None):
    """Vrai dès que le serveur répond (n'importe quel statut HTTP)."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        if pgid and not alive(pgid):
            return False
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=5):
                return True
        except urllib.error.HTTPError:
            return True
        except (OSError, ValueError):
            time.sleep(1)
    return False


def tunnel_url(log_path, timeout=90, pgid=None):
    deadline = time.time() + timeout
    while time.time() < deadline:
        found = TUNNEL_URL.search(tail(log_path, 200))
        if found:
            return found.group(0)
        if pgid and not alive(pgid):
            return None
        time.sleep(1)
    return None


def public_answer(url, timeout=10):
    """Ce que l'adresse publique répond depuis Internet : un code HTTP (la porte répond « Accès
    réservé », 403, sans la clé), ou la raison de l'échec (nom inconnu, connexion refusée)."""
    host = urllib.parse.urlsplit(url).hostname or ""
    try:
        socket.getaddrinfo(host, 443)
    except OSError as error:
        return f"nom introuvable ({error})"
    request = urllib.request.Request(url, headers={"User-Agent": "claw-jtest"})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return response.status
    except urllib.error.HTTPError as error:
        return error.code
    except (urllib.error.URLError, OSError) as error:
        return f"pas de réponse ({getattr(error, 'reason', error)})"


def tunnel_ready(url, log_path, timeout=120, pgid=None):
    """Le tunnel enregistré chez Cloudflare, puis l'adresse qui répond depuis Internet. Annoncer
    plus tôt menait à « serveur introuvable » sur le téléphone (vu le 25 octobre 2026), et un
    téléphone qui a demandé le nom trop tôt garde un moment la réponse « inconnu ». Rend None si
    tout va bien, sinon la dernière raison."""
    deadline = time.time() + timeout
    last = "le tunnel ne s'est pas enregistré chez Cloudflare"
    while time.time() < deadline:
        if pgid and not alive(pgid):
            return "le tunnel s'est arrêté : " + tail(log_path, 5).replace("\n", " ")[-300:]
        if "Registered tunnel connection" in tail(log_path, 400):
            answer = public_answer(url)
            if isinstance(answer, int) and answer < 500:
                return None
            last = f"l'adresse ne répond pas encore depuis Internet ({answer})"
        time.sleep(3)
    return last


def arch():
    machine = platform.machine().lower()
    return "arm64" if machine in ("aarch64", "arm64") else "amd64"


def tool(name, url):
    """Un outil téléchargé une fois dans ~/jtest/bin (cloudflared, ttyd)."""
    path = BIN / name
    if path.is_file() and os.access(path, os.X_OK):
        return path
    BIN.mkdir(parents=True, exist_ok=True)
    request = urllib.request.Request(url, headers={"User-Agent": "claw-jtest"})
    with urllib.request.urlopen(request, timeout=120) as response:
        data = response.read()
    temporary = path.with_suffix(".part")
    temporary.write_bytes(data)
    temporary.chmod(0o755)
    temporary.replace(path)
    return path


# --- État sur le serveur -------------------------------------------------------------------------

def state_path(session):
    return HOME / str(session) / "state.json"


def load_state(session):
    return read_json(state_path(session)) or {}


def save_state(session, state):
    path = state_path(session)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(state, ensure_ascii=False, indent=1), encoding="utf-8")


def live_sessions():
    """Les JTest en ligne sur le serveur, du plus ancien au plus récent."""
    found = []
    for path in HOME.glob("*/state.json"):
        state = read_json(path) or {}
        if state.get("pgids") and any(alive(pgid) for pgid in state["pgids"]):
            found.append(state)
    return sorted(found, key=lambda state: state.get("started") or "")


def stop_local(session):
    """Arrête la JTest de la session sur ce serveur ; rend son état d'avant."""
    state = load_state(session)
    for pgid in state.get("pgids") or []:
        kill_group(pgid)
    return state


# --- Commentaire dans la session -----------------------------------------------------------------

def iso(moment):
    return moment.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def comment_body(data):
    """Le commentaire de la session : données pour l'appli, texte pour GitHub et l'historique."""
    payload = json.dumps(data, ensure_ascii=False).replace("-->", "--\\u003e")
    state = data.get("state")
    lines = [f"<!-- {MARK} {payload} -->"]
    branch = f"`{data.get('branch')}`" if data.get("branch") else "la branche de la session"
    if state == "starting":
        lines.append(f"**JTest** : préparation de {branch} sur le serveur…")
    elif state == "online":
        lines.append(f"**JTest** : {data.get('label')} · en ligne jusqu'à {data.get('until_text')} · accès réservé : "
                     "ouvrez-le depuis Claw Web (« Ouvrir sur le téléphone »).")
    elif state == "stopped":
        lines.append("**JTest** arrêté.")
    else:
        lines.append(f"**JTest** n'a pas pu démarrer : {data.get('reason') or 'erreur inconnue'}")
        if data.get("log"):
            lines.append("```\n" + data["log"][-1500:] + "\n```")
    return "\n".join(lines)


def public_data(data):
    """Les données d'une JTest sans sa clé (ni identifiants) : ce que voit l'agent."""
    return {key: value for key, value in (data or {}).items() if key not in PRIVATE}


def write_private(path, data):
    """Un fichier lisible du seul propriétaire (réglages de la porte, avec la clé)."""
    path = Path(path)
    path.unlink(missing_ok=True)
    handle = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(handle, "w", encoding="utf-8") as file:
        json.dump(data, file)


def parse_comment(body):
    found = COMMENT.search(body or "")
    if not found:
        return None
    try:
        return json.loads(found.group(1))
    except ValueError:
        return None


class Session:
    """La session de Claw Web (issue du dépôt des sessions) où JTest écrit."""

    def __init__(self, engine, number, token, call):
        self.engine, self.number, self.token, self.call = engine, number, token, call

    def post(self, data):
        return self.call("POST", f"/repos/{self.engine}/issues/{self.number}/comments", self.token,
                         {"body": comment_body(data)})

    def edit(self, comment_id, data):
        return self.call("PATCH", f"/repos/{self.engine}/issues/comments/{comment_id}", self.token,
                         {"body": comment_body(data)})


# --- Lancement -----------------------------------------------------------------------------------

def clone(repo, branch, token, target):
    if target.exists():
        shutil.rmtree(target)
    target.parent.mkdir(parents=True, exist_ok=True)
    url = f"https://x-access-token:{token}@github.com/{repo}.git" if token else f"https://github.com/{repo}.git"
    done = subprocess.run(["git", "clone", "--quiet", "--depth", "1", "--branch", branch, url, str(target)],
                          capture_output=True, text=True, timeout=600)
    if done.returncode != 0:
        detail = (done.stderr or "").replace(token or "\0", "***").strip()[:300]
        raise JTestError(f"impossible de cloner `{repo}` sur `{branch}` ({detail or 'branche ou dépôt introuvable'}).")
    subprocess.run(["git", "-C", str(target), "remote", "set-url", "origin", f"https://github.com/{repo}.git"],
                   capture_output=True)


def run_install(plan, folder, env, log_path):
    for command in plan.install:
        with open(log_path, "ab") as log:
            log.write(f"\n$ {command}\n".encode())
            log.flush()
            try:
                done = subprocess.run(["bash", "-lc", command], cwd=folder, env=env, stdin=subprocess.DEVNULL,
                                      stdout=log, stderr=subprocess.STDOUT, timeout=INSTALL_TIMEOUT)
            except subprocess.TimeoutExpired:
                raise JTestError(f"l'installation (`{command}`) a dépassé {INSTALL_TIMEOUT // 60} minutes.") from None
        if done.returncode != 0:
            raise JTestError(f"l'installation (`{command}`) a échoué.")


def terminal_command(plan, ttyd, port, user, password):
    """Le programme dans un terminal web : relancé d'Entrée quand il se termine, jamais un shell."""
    loop = (f"while true; do {plan.command}; echo; echo '[programme terminé]'; "
            "read -r -p 'Entrée pour relancer… ' _; done")
    return (f"{ttyd} --port {port} --interface 127.0.0.1 --writable --credential {user}:{password} "
            f"--client-option titleFixed=JTest bash -c {shlex_quote(loop)}")


def shlex_quote(text):
    return "'" + text.replace("'", "'\"'\"'") + "'"


def start(session, repo, branch, requested, token, now=None, hours=None):
    """Lance la JTest ; rend les données du commentaire (état `online`), ou lève JTestError."""
    now = now or datetime.now(timezone.utc)
    hours = max(0.25, min(float(hours or (requested or {}).get("hours") or DEFAULT_HOURS), MAX_HOURS))
    stop_local(session)
    others = [state for state in live_sessions() if str(state.get("session")) != str(session)]
    for state in others[: max(0, len(others) - (MAX_LIVE - 1))]:
        stop_local(state["session"])
    folder = HOME / str(session)
    work = folder / "code"
    clone(repo, branch, token, work)
    plan = detect(work, requested)
    project = work / plan.cwd
    log_path = folder / "jtest.log"
    log_path.write_text(f"JTest de {repo} ({branch}) : {plan.label}\n", encoding="utf-8")
    env = clean_env(os.environ, plan.env)
    run_install(plan, project, env, log_path)
    data = {"state": "online", "repo": repo, "branch": branch, "kind": plan.kind, "label": plan.label,
            "started": iso(now), "until": iso(now + timedelta(hours=hours)),
            "until_text": (now + timedelta(hours=hours)).strftime("%H:%M UTC")}
    pgids = []
    basic = ""
    if plan.kind == "terminal":
        ttyd = tool("ttyd", TTYD.format(arch="aarch64" if arch() == "arm64" else "x86_64"))
        user, password = "jtest", secrets.token_urlsafe(18)
        command = terminal_command(plan, ttyd, plan.port, user, password)
        basic = f"{user}:{password}"
    else:
        command = plan.command
    pgids.append(spawn(command, project, env, log_path, hours))
    if not wait_http(plan.port, pgid=pgids[0]):
        for pgid in pgids:
            kill_group(pgid)
        raise JTestError(f"le projet ne répond pas sur le port {plan.port} (`{plan.command}`).")
    # La porte : seul le lien de Claw Web (avec la clé) mène au projet.
    key = secrets.token_urlsafe(32)
    gate_port = free_port()
    config = folder / "gate.json"
    write_private(config, {"listen": gate_port, "upstream": plan.port, "key": key, "hours": hours,
                           "basic": basic})
    pgids.append(spawn(f"{shlex_quote(sys.executable)} {shlex_quote(str(GATE))} --config {shlex_quote(str(config))}",
                       folder, clean_env(os.environ), folder / "gate.log", hours))
    if not wait_http(gate_port, timeout=20, pgid=pgids[-1]):
        for pgid in pgids:
            kill_group(pgid)
        raise JTestError("la porte d'accès de JTest n'a pas démarré (voir gate.log sur le serveur).")
    cloudflared = tool("cloudflared", CLOUDFLARED.format(arch=arch()))
    tunnel_log = folder / "tunnel.log"
    tunnel_log.write_text("", encoding="utf-8")
    pgids.append(spawn(f"{cloudflared} tunnel --no-autoupdate --url http://127.0.0.1:{gate_port} "
                       "--http-host-header localhost", folder, clean_env(os.environ), tunnel_log, hours))
    url = tunnel_url(tunnel_log, pgid=pgids[-1])
    if not url:
        for pgid in pgids:
            kill_group(pgid)
        raise JTestError("le tunnel Cloudflare n'a pas donné d'adresse (réseau du serveur ?).")
    problem = tunnel_ready(url, tunnel_log, pgid=pgids[-1])
    if problem:
        for pgid in pgids:
            kill_group(pgid)
        raise JTestError(f"le tunnel Cloudflare ne mène pas au test : {problem}. Relancez dans une minute.")
    data["url"] = url.rstrip("/") + (plan.path if plan.path.startswith("/") else "/" + plan.path)
    data["key"] = key
    save_state(session, {"session": str(session), "repo": repo, "branch": branch, "pgids": pgids, "port": plan.port,
                         "gate": gate_port,
                         "started": data["started"], "until": data["until"], "url": data["url"], "kind": plan.kind})
    return data


def status_report(session):
    """`jtest-status-<n>` : l'état d'une JTest sur le serveur, dans le journal du job, sans rien
    changer ni rien écrire dans la session (ni la clé, ni l'adresse complète des journaux)."""
    state = load_state(session)
    if not state:
        return f"JTest {session} : aucune sur ce serveur."
    folder = HOME / str(session)
    lines = [f"JTest {session} : {state.get('url')} (jusqu'à {state.get('until')})"]
    names = ["projet", "porte", "tunnel"]
    for index, pgid in enumerate(state.get("pgids") or []):
        name = names[index] if index < len(names) else f"processus {index}"
        lines.append(f"- {name} (groupe {pgid}) : {'en marche' if alive(pgid) else 'arrêté'}")
    if state.get("url"):
        lines.append(f"- adresse publique : {public_answer(state['url'])}")
    for title, path in [("tunnel.log", folder / "tunnel.log"), ("gate.log", folder / "gate.log"),
                        ("jtest.log", folder / "jtest.log")]:
        lines.append(f"--- {title} ---")
        # Les requêtes de la porte portent la clé (`?key=…`) : jamais dans le journal du job.
        lines.append(re.sub(r"\?\S*", "?…", tail(path, 15)) or "(vide)")
    return "\n".join(lines)


def main():
    from common import GitHubError, api

    raw = os.environ.get("SESSION_INPUT", "").strip()
    found = re.fullmatch(r"jtest-(stop-|status-)?(\d+)", raw)
    if not found:
        print(f"::error::session JTest invalide : {raw!r}")
        return 2
    if found.group(1) == "status-":
        print(status_report(found.group(2)))
        return 0
    stopping, number = bool(found.group(1)), found.group(2)
    engine = os.environ["GITHUB_REPOSITORY"]
    repo = os.environ.get("REPO", "").strip()
    branch = os.environ.get("BRANCH", "").strip()
    session = Session(engine, number, os.environ["JOB_TOKEN"], api)
    if stopping:
        state = stop_local(number)
        try:
            session.post({"state": "stopped", "repo": state.get("repo") or repo, "branch": state.get("branch") or branch})
        except GitHubError as error:
            print(f"::warning::{error}")
        print("JTest arrêté" if state else "Aucun JTest en ligne pour cette session")
        return 0
    try:
        requested = json.loads(os.environ.get("OPTIONS") or "{}")
    except ValueError:
        requested = {}
    requested = requested if isinstance(requested, dict) else {}
    # La clé d'accès est écrite dans la session : seulement si personne d'autre ne la lit.
    try:
        private = bool((api("GET", f"/repos/{engine}", os.environ["JOB_TOKEN"]) or {}).get("private"))
    except GitHubError as error:
        print(f"::warning::{error}")
        private = False
    if not private:
        reason = (f"le dépôt des sessions ({engine}) est public, ou illisible : la clé d'accès du test y serait "
                  "lisible de tous. Rendez-le privé (GitHub, Settings, Danger Zone, Change visibility), puis relancez.")
        session.post({"state": "failed", "repo": repo, "branch": branch, "reason": reason})
        print(f"::error::{reason}")
        return 1
    posted = session.post({"state": "starting", "repo": repo, "branch": branch})
    try:
        data = start(number, repo, branch, requested, os.environ.get("REPO_TOKEN", "").strip())
    except (JTestError, OSError, subprocess.SubprocessError) as error:
        reason = str(error) if isinstance(error, JTestError) else f"erreur du serveur ({error})"
        session.edit(posted["id"], {"state": "failed", "repo": repo, "branch": branch, "reason": reason,
                                    "log": tail(HOME / number / "jtest.log")})
        print(f"::error::{reason}")
        return 1
    session.edit(posted["id"], data)
    print(f"JTest en ligne : {data['url']}, accès réservé à Claw Web ({data['label']}, jusqu'à {data['until_text']})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
