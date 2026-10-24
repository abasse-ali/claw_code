"""Lance l'agent dans le dépôt de la tâche et publie sa progression dans la session.

Entrées (environnement) : MODEL (vide : le meilleur modèle gratuit ; `mock` : faux modèle
d'essai), SESSION, JOB_TOKEN, OPENROUTER_API_KEY, CLAW_SRC (source de Claw, pour le faux
modèle). L'agent tourne dans `work/`, sans aucun jeton GitHub dans son environnement. Un
commentaire de progression est créé dans la session puis mis à jour au fil du travail ; le
journal détaillé du passage (journal.py) suit dans d'autres commentaires, lus par l'appli ; les
actions et les avis de Claw vont aussi au journal du job. Un job annulé (SIGINT puis SIGTERM,
reçus grâce à `exec` dans le workflow) arrête l'agent et le dit dans la progression.
"""

import base64
import json
import os
import signal
import subprocess
import sys
import threading
import time
import urllib.request
from pathlib import Path

from common import (FILES_BRANCH, MCP_TOKEN_FILE, PROGRESS, WORK, GitHubError, api, clean_notice, edit_comment,
                    output, post_comment, run_url, stop_tree)
from journal import Journal
from mcp_server import Context, Host, build, describe_call
from on_github import REQUESTS_FILE, STATE_FILE
import memory

SESSION = os.environ["SESSION"]
ENGINE = os.environ["GITHUB_REPOSITORY"]
JOB_TOKEN = os.environ["JOB_TOKEN"]
# « Auto » (aucun modèle choisi) : NVIDIA dès que le moteur a sa clé, son modèle le plus fort
# pour le code (77/78 au banc réel, contre 61 pour le plus petit) ; sans clé, le meilleur
# modèle gratuit d'OpenRouter. Demandé le 24 octobre 2026 : « Les sessions doivent se faire
# avec Nvidia ».
NVIDIA_DEFAULT = "nvidia:nvidia/nemotron-3-ultra-550b-a55b"


def session_model(environ):
    chosen = environ.get("MODEL", "").strip()
    if not chosen and environ.get("NVIDIA_API_KEY", "").strip():
        return NVIDIA_DEFAULT
    return chosen


MODEL = session_model(os.environ)
# Mise à jour du commentaire de progression au plus toutes les N secondes, du journal
# détaillé (plus lourd) au plus toutes les LOG_EVERY secondes.
PROGRESS_EVERY = 12
# 12 s : l'appli fait paraître les étapes reçues une par une entre deux publications (24 s
# donnaient des paquets d'une dizaine de lignes d'un coup, vu le 22 octobre 2026).
LOG_EVERY = 12
RUN_ID = int(os.environ.get("GITHUB_RUN_ID") or 0)
SHOWN_ACTIONS = 8
# Racine du dépôt de la tâche : les chemins sont montrés depuis elle, pas depuis le disque du job.
ROOT = os.path.abspath("work")


class Stopped(Exception):
    """Le job est annulé."""


def stop(signum, frame):
    # Un seul arrêt : le SIGTERM qui suit le SIGINT ne doit pas interrompre le rangement.
    signal.signal(signal.SIGINT, signal.SIG_IGN)
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    raise Stopped()


def relative(text):
    """`/home/runner/.../work/app/main.py` devient `app/main.py`."""
    return str(text).replace(ROOT + os.sep, "").replace(ROOT, ".")


class ShotStore:
    """Range les captures entières de l'outil Screenshot sur la branche des fichiers du moteur,
    dans le dossier de la session (`uploads/session-<numéro>/`, effacé avec elle par l'appli) :
    l'appli les montre en grand d'un toucher sur leur vignette. Sans droit d'écriture (ancien
    workflow de claw-sessions), rien n'est rangé et seules les vignettes restent."""

    MAX_BYTES = 3_000_000

    def __init__(self, token=JOB_TOKEN, engine=ENGINE, session=SESSION, run_id=None, call=api):
        self.token, self.engine, self.session, self.call = token, engine, session, call
        self.run_id = run_id or os.environ.get("GITHUB_RUN_ID", "local")
        self.count = 0
        self.ready = None

    def _branch(self):
        """La branche des fichiers existe (créée depuis la branche par défaut au besoin)."""
        if self.ready is None:
            try:
                self.call("GET", f"/repos/{self.engine}/branches/{FILES_BRANCH}", self.token)
                self.ready = True
            except GitHubError as error:
                if error.status != 404:
                    self.ready = False
                    return False
                try:
                    default = self.call("GET", f"/repos/{self.engine}", self.token)["default_branch"]
                    head = self.call("GET", f"/repos/{self.engine}/git/ref/heads/{default}", self.token)
                    self.call("POST", f"/repos/{self.engine}/git/refs", self.token,
                              {"ref": f"refs/heads/{FILES_BRANCH}", "sha": head["object"]["sha"]})
                    self.ready = True
                except (GitHubError, KeyError, TypeError):
                    self.ready = False
        return self.ready

    def __call__(self, local, screen):
        try:
            with open(local, "rb") as file:
                data = file.read(self.MAX_BYTES + 1)
        except OSError:
            return ""
        if not data.startswith(b"\xff\xd8\xff") or len(data) > self.MAX_BYTES or not self._branch():
            return ""
        self.count += 1
        label = "".join(c if c.isalnum() else "-" for c in screen.lower()).strip("-")[:30] or "page"
        name = f"uploads/session-{self.session}/{self.run_id}-{self.count}-{label}.jpg"
        try:
            self.call("PUT", f"/repos/{self.engine}/contents/{name}", self.token, {
                "message": f"Capture de la session {self.session}",
                "content": base64.b64encode(data).decode("ascii"),
                "branch": FILES_BRANCH,
            })
        except GitHubError as error:
            print(f"Capture non rangée : {error}", flush=True)
            # Écriture refusée : inutile de réessayer pour chaque capture.
            if error.status in (401, 403, 404):
                self.ready = False
            return ""
        return name


def describe(name, data):
    """Une action de l'agent en quelques mots, pour l'utilisateur."""
    data = data if isinstance(data, dict) else {}
    mcp = describe_call(name, data)
    if mcp:
        return mcp
    path = relative(data.get("path") or data.get("file_path") or data.get("notebook_path") or "")
    short = lambda text, size=90: (text if len(text) <= size else text[: size - 1] + "…")  # noqa: E731
    if name == "read_file":
        return f"Lecture de `{short(path)}`"
    if name == "write_file":
        return f"Écriture de `{short(path)}`"
    if name in ("edit_file", "NotebookEdit", "MultiEdit"):
        return f"Modification de `{short(path)}`"
    if name == "apply_patch":
        text = str(data.get("patch") or data.get("input") or "")
        paths = [relative(line.split(":", 1)[1].strip()) for line in text.splitlines()
                 if line.startswith(("*** Add File:", "*** Update File:", "*** Delete File:"))]
        if len(paths) == 1:
            return f"Correctif de `{short(paths[0])}`"
        return f"Correctif de {len(paths)} fichiers" if paths else "Correctif"
    if name == "delete_file":
        return f"Suppression de `{short(path)}`"
    if name == "move_file":
        source = relative(str(data.get("source") or ""))
        target = relative(str(data.get("destination") or ""))
        return f"Déplacement de `{short(source, 60)}` vers `{short(target, 60)}`"
    if name == "list_directory":
        return f"Dossier `{short(path or '.')}`"
    if name == "Wireframe":
        return f"Wireframe : {short(str(data.get('title') or 'écran'))}"
    if name == "file_outline":
        return f"Plan de `{short(path)}`"
    if name == "read_many_files":
        paths = [relative(str(item)) for item in (data.get("paths") or [])]
        return f"Lecture de {len(paths)} fichiers" if len(paths) != 1 else f"Lecture de `{short(paths[0])}`"
    if name == "replace_in_files":
        return f"Remplacement de `{short(str(data.get('pattern', '')), 50)}` dans le projet"
    if name == "run_tests":
        return f"Tests : `{short(str(data.get('command') or data.get('filter') or 'tests du projet'))}`"
    if name == "git":
        return f"Git : {short(str(data.get('action', '')))}"
    if name == "git_commit":
        return f"Commit : {short(str(data.get('message', '')))}"
    if name == "http_request":
        return f"Requête : {short(str(data.get('method') or 'GET'))} {short(str(data.get('url', '')), 70)}"
    if name in ("bash", "PowerShell"):
        command = relative(" ".join(str(data.get("command", "")).split()))
        return f"Commande : `{short(command)}`"
    if name in ("grep_search", "glob_search"):
        return f"Recherche : `{short(str(data.get('pattern', '')))}`"
    if name in ("WebFetch", "WebSearch", "ViewPage"):
        return f"Web : {short(str(data.get('url') or data.get('query') or ''))}"
    if name == "TodoWrite":
        return "Liste de tâches mise à jour"
    if name == "Agent":
        return f"Sous-agent : {short(str(data.get('description', '')))}"
    # Outils de design : ce qu'ils cherchent ou tracent, pas leur seul nom.
    names = lambda: ", ".join(str(item) for item in (data.get("names") or [data.get("name") or ""]))  # noqa: E731
    if name == "Photo":
        return f"Photos : « {short(str(data.get('search', '')))} »"
    if name == "Icon":
        return f"Icônes : {short(names())}"
    if name == "Ornament":
        return f"Ornement : {short(str(data.get('kind', '')))}"
    if name == "Component":
        return f"Composants : {short(names())}"
    if name == "Screenshot":
        return f"Capture de la page : {short(relative(str(data.get('url') or data.get('path') or '')))}"
    return name


def shorten(text, size):
    """Une ligne de `size` caractères au plus, coupée sur un mot."""
    text = " ".join(str(text).split())
    if len(text) <= size:
        return text
    return text[: size - 1].rsplit(" ", 1)[0] + "…"


class Progress:
    """Commentaire de progression de la session, mis à jour au fil des événements."""

    def __init__(self):
        self.started = time.time()
        self.actions = []
        self.notices = []
        self.replies = 0
        # Ce que Claw annonce : son plan (le texte qui précède ses premiers outils), puis
        # l'étape en cours (les suivants). Claw envoie le texte et chaque appel d'outil dans
        # des événements séparés : le dernier texte attend l'outil qui le suit (`said`).
        self.plan = ""
        self.step = ""
        self.said = ""
        # Questions de Claw à l'utilisateur (AskUserQuestion), publiées avec sa réponse.
        self.questions = []
        self.comment_id = None
        self.dirty = True
        self.lock = threading.Lock()
        self.journal = Journal(relative, store=ShotStore())
        self.journal_at = 0.0
        self.log_comments = {}
        # Un envoi à la fois (le fil de rafraîchissement et la fin du passage) : jamais deux
        # commentaires pour la même partie.
        self.sending = threading.Lock()

    def event(self, event):
        with self.lock:
            self.journal.add(event)
            kind = event.get("type")
            if kind == "assistant":
                parts = (event.get("message") or {}).get("content") or []
                for part in parts:
                    if part.get("type") == "tool_use":
                        action = describe(part.get("name", ""), part.get("input"))
                        self.actions.append(action)
                        print(f"[action] {action}", flush=True)
                        # Un texte suivi d'outils annonce ce que Claw fait.
                        if self.said:
                            if self.plan:
                                self.step = self.said
                            else:
                                self.plan = self.said
                            self.said = ""
                        self.dirty = True
                    elif part.get("type") == "text" and part.get("text", "").strip():
                        self.replies += 1
                        self.said = " ".join(part["text"].split())
                        self.dirty = True
            elif kind == "system" and event.get("subtype") == "notice":
                notice = clean_notice(event.get("text", ""))
                print(f"[avis] {notice}", flush=True)
                # Le même avis deux fois de suite ne se répète pas dans la progression.
                if notice and (not self.notices or self.notices[-1] != notice):
                    self.notices.append(notice)
                    self.dirty = True
            elif kind == "system" and event.get("subtype") == "answer_superseded":
                # Réponse que Claw fait reprendre : ni un plan ni l'étape en cours.
                self.said = ""
                self.dirty = True
            elif kind == "system" and event.get("subtype") == "questions":
                # Le passage finit sur ces questions : l'utilisateur y répond d'un toucher.
                self.questions = event.get("questions") or []
                print(f"[questions] {len(self.questions)} question(s) à l'utilisateur", flush=True)

    def body(self, state):
        elapsed = int(time.time() - self.started)
        duration = f"{elapsed // 60} min {elapsed % 60:02d} s" if elapsed >= 60 else f"{elapsed} s"
        head = {
            "running": f"**Claw travaille** depuis {duration}",
            "done": f"**Claw a fini** en {duration}",
            "failed": f"**Claw s'est arrêté sur une erreur** après {duration}",
            "stopped": f"**Claw a été arrêté** après {duration} (passage annulé)",
            "unfinished": f"**Claw s'est arrêté sans conclure** après {duration}",
        }[state]
        lines = [PROGRESS, f"{head} · {len(self.actions)} action(s)"]
        plan, step = self.plan, self.step
        # Pendant le travail, le dernier texte se montre sans attendre l'outil suivant (une
        # annonce demandée par Claw arrive seule). Fini, un texte sans outil après lui est la
        # réponse, pas un plan.
        if self.said and state == "running":
            if plan:
                step = self.said
            else:
                plan = self.said
        if plan:
            lines.append("")
            lines.append(f"**Ce que Claw compte faire** : {shorten(plan, 500)}")
            if step and state == "running":
                lines.append(f"**En ce moment** : {shorten(step, 300)}")
        if self.actions:
            lines.append("")
            hidden = len(self.actions) - SHOWN_ACTIONS
            if hidden > 0:
                lines.append(f"- … ({hidden} action(s) plus tôt)")
            lines.extend(f"- {action}" for action in self.actions[-SHOWN_ACTIONS:])
        if self.notices:
            lines.append("")
            lines.extend(f"> {notice}" for notice in self.notices[-3:])
        lines.append("")
        lines.append(f"[Détail du job]({run_url()})")
        return "\n".join(lines)

    def publish(self, state="running", force=False):
        # Calculé sous le verrou d'envoi : un état plus ancien n'écrase jamais un plus récent.
        with self.sending:
            with self.lock:
                if not (self.dirty or force):
                    return
                body = self.body(state)
                self.dirty = False
            self._send_progress(body)

    def _send_progress(self, body):
        try:
            if self.comment_id is None:
                self.comment_id = post_comment(JOB_TOKEN, ENGINE, SESSION, body)["id"]
            else:
                edit_comment(JOB_TOKEN, ENGINE, self.comment_id, body)
        except GitHubError as error:
            print(f"::warning::progression non publiée : {error}")

    def publish_journal(self, force=False):
        """Les parties du journal détaillé qui ont changé, au plus toutes les LOG_EVERY s."""
        with self.sending:
            with self.lock:
                if not (self.journal.dirty or force):
                    return
                if not force and time.time() - self.journal_at < LOG_EVERY:
                    return
                pages = self.journal.bodies(RUN_ID)
                self.journal.dirty = False
                self.journal_at = time.time()
            self._send_journal(pages)

    def _send_journal(self, pages):
        for number, body, digest in pages:
            if self.journal.sent.get(number) == digest:
                continue
            try:
                if number in self.log_comments:
                    edit_comment(JOB_TOKEN, ENGINE, self.log_comments[number], body)
                else:
                    self.log_comments[number] = post_comment(JOB_TOKEN, ENGINE, SESSION, body)["id"]
                self.journal.sent[number] = digest
            except GitHubError as error:
                print(f"::warning::journal non publié : {error}")


def remember(state, store=None, call=api):
    """Ajoute à la mémoire du dépôt (memory.py) ce que ce passage a appris : les leçons notées
    par l'agent, les commandes qui ont marché ou échoué, la demande et son issue. Jamais d'échec
    du passage pour elle : un imprévu est dit dans le journal du job (le 22 octobre 2026, une
    erreur ici faisait passer pour inachevé un passage fini)."""
    try:
        return _remember(state, store, call)
    except Exception as error:  # noqa: BLE001 - la mémoire ne doit jamais faire échouer le passage
        print(f"::warning::mémoire du projet non rangée : {error!r}", flush=True)
        return False


def _remember(state, store, call):
    try:
        seen = json.loads((WORK / "memory.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return False
    repo = seen.get("repo") if isinstance(seen, dict) else None
    if not repo or MODEL == "mock":
        return False
    before = set(seen.get("lessons") or [])
    lessons = [line for line in memory.read_lessons(Path(ROOT) / ".claw") if line not in before]
    events = []
    try:
        with open(WORK / "claw.jsonl", encoding="utf-8") as log:
            for line in log:
                try:
                    events.append(json.loads(line))
                except ValueError:
                    continue
    except OSError:
        pass
    runs = memory.observe(events, relative)
    store = store or ShotStore()
    try:
        saved = memory.save(
            JOB_TOKEN, ENGINE, repo,
            lambda data: memory.learn(data, runs, lessons, seen.get("request") or "", state, SESSION,
                                      files=memory.edited_files(events, relative)),
            session=SESSION, call=call, ensure_branch=store._branch)
    except GitHubError as error:
        print(f"::warning::mémoire du projet non rangée : {error}")
        return False
    if saved:
        failed = sum(1 for run in runs if not run[1])
        print(f"Mémoire du projet rangée : {len(lessons)} leçon(s) nouvelle(s), {len(runs)} commande(s) "
              f"relevée(s) dont {failed} en échec", flush=True)
    return saved


def free_quota(key):
    """Requêtes gratuites du jour chez OpenRouter ({used, limit, remaining}), ou None. Une
    lecture de la clé : aucune requête de modèle, rien du quota."""
    if not key:
        return None
    request = urllib.request.Request("https://openrouter.ai/api/v1/key")
    request.add_header("Authorization", f"Bearer {key}")
    try:
        with urllib.request.urlopen(request, timeout=15) as response:
            data = json.loads(response.read()).get("data") or {}
    except (OSError, ValueError):
        return None
    quota = data.get("free_model_daily_requests")
    if not isinstance(quota, dict) or not all(isinstance(quota.get(name), int) for name in ("used", "limit", "remaining")):
        return None
    return quota


def start_mock():
    """Faux modèle local (`rust/scripts/mock-model.js`) pour essayer le moteur sans requête."""
    script = os.path.join(os.environ["CLAW_SRC"], "rust", "scripts", "mock-model.js")
    process = subprocess.Popen(["node", script, "--log", str(WORK / "mock.jsonl")],
                               stdout=subprocess.PIPE, text=True)
    line = process.stdout.readline()
    if not line.startswith("PORT "):
        sys.exit("le faux modèle n'a pas démarré")
    return process, f"http://127.0.0.1:{line.split()[1]}/v1"


def start_mcp(env):
    """Les serveurs MCP du moteur (mcp_server.py : `github`, `claw-remote`, `claw-docs`) sur un
    port local, déclarés à Claw dans ses réglages (`mcpServers` de CLAW_CONFIG_HOME). Le jeton du
    dépôt est lu dans le fichier que setup.py a posé, puis effacé : il ne reste que dans ce
    processus. None si les serveurs ne démarrent pas (Claw travaille alors sans eux)."""
    try:
        repo_token = MCP_TOKEN_FILE.read_text(encoding="utf-8").strip()
    except OSError:
        repo_token = ""
    MCP_TOKEN_FILE.unlink(missing_ok=True)
    try:
        state = json.loads(STATE_FILE.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        state = {}
    context = Context(
        repo=os.environ.get("REPO", "").strip() or state.get("repo") or "",
        base=os.environ.get("BASE", "").strip() or state.get("base") or "",
        branch=os.environ.get("BRANCH", "").strip() or state.get("branch") or "",
        session=SESSION, engine=ENGINE, job_token=JOB_TOKEN, repo_token=repo_token, run_id=str(RUN_ID),
        work=Path(ROOT), requests_file=REQUESTS_FILE, model="" if MODEL == "mock" else MODEL,
        runner=os.environ.get("RUNNER", "").strip(), repos_dir=WORK / "repos",
        claw_src=Path(os.environ["CLAW_SRC"]) if os.environ.get("CLAW_SRC") else None,
    )
    try:
        host = Host(build(context)).start()
    except Exception as error:  # noqa: BLE001 - sans eux, la tâche se fait quand même
        print(f"::warning::outils MCP du moteur non démarrés : {error!r}", flush=True)
        return None
    home = Path(env.get("CLAW_CONFIG_HOME") or WORK / "claw-home")
    env["CLAW_CONFIG_HOME"] = str(home)
    home.mkdir(parents=True, exist_ok=True)
    path = home / "settings.json"
    try:
        current = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        current = {}
    if not isinstance(current, dict):
        current = {}
    current.setdefault("mcpServers", {}).update(host.config())
    path.write_text(json.dumps(current, ensure_ascii=False, indent=1), encoding="utf-8")
    tools = sum(len(server.tools) for server in host.servers.values())
    print(f"Outils MCP du moteur : {', '.join(host.servers)} ({tools} outils)"
          + ("" if repo_token else " ; GitHub sans jeton du dépôt"), flush=True)
    return host


def agent_env(environ, mock=False):
    """Environnement de Claw. Aucun jeton GitHub : il travaille, Claw publie après lui. Les clés
    des fournisseurs passent (OPENROUTER_API_KEY, NVIDIA_API_KEY…), sauf pour le faux modèle :
    rien ne doit pouvoir partir vers un vrai modèle, relecteurs compris."""
    env = {key: value for key, value in environ.items()
           if ("TOKEN" not in key.upper() or key == "OPENROUTER_API_KEY") and key != "RUNNER"}
    if mock:
        env = {key: value for key, value in env.items() if not key.upper().endswith("_API_KEY")}
    return env


def main():
    request = (WORK / "prompt.txt").read_text(encoding="utf-8")
    env = agent_env(os.environ, mock=MODEL == "mock")
    env["CLAW_NO_BROWSER"] = "1"
    # Questions de Claw (AskUserQuestion) montrées dans l'appli, avec des réponses à toucher :
    # le passage s'arrête sur elles, la réponse de l'utilisateur lance le suivant.
    env["CLAW_DEFERRED_QUESTIONS"] = "1"
    # L'appli est en français, et le moteur entoure la demande de notes en anglais : la
    # langue des réponses est fixée (annonces, étapes, réponse finale).
    env.setdefault("CLAW_LANGUAGE", "français")
    # `claw-github` : l'agent demande une fusion ou une mise en ligne, faite après la poussée
    # par publish.py avec le jeton du dépôt (claw_github.py n'en a aucun).
    tools = WORK / "bin"
    tools.mkdir(parents=True, exist_ok=True)
    helper = tools / "claw-github"
    script = os.path.join(os.path.dirname(os.path.abspath(__file__)), "claw_github.py")
    helper.write_text(f'#!/bin/sh\nexec python3 "{script}" "$@"\n', encoding="utf-8")
    helper.chmod(0o755)
    env["PATH"] = f"{tools}{os.pathsep}{env.get('PATH', '')}"
    env["CLAW_WEB_REPO_DIR"] = ROOT
    # Options choisies dans l'appli : mode équipe, Ultracode (comme les boutons de l'interface
    # locale de Claw).
    for choice, variable, label in (("TEAM", "CLAW_TEAM", "mode équipe"), ("ULTRACODE", "CLAW_ULTRACODE", "Ultracode")):
        env.pop(choice, None)
        if os.environ.get(choice, "").strip() == "1":
            env[variable] = "1"
            print(f"Option : {label}", flush=True)
    # Mode projet : choisi dans l'appli, coupé par défaut. Coupé, un SPEC.md à la racine ne le
    # rouvre plus (une simple question repartait sur le jalon suivant du projet, 22 octobre 2026).
    env.pop("PROJECT", None)
    if os.environ.get("PROJECT", "").strip() == "1":
        env["CLAW_PROJECT_MODE"] = "start"
        print("Option : mode projet", flush=True)
    else:
        env["CLAW_PROJECT_MODE"] = "0"
    args = ["claw", "--permission-mode", "danger-full-access", "--output-format", "stream-json"]
    mock = None
    if MODEL == "mock":
        # Aucune clé réelle (retirées par agent_env) ni réglage de l'utilisateur.
        env["CLAW_NO_USER_ENV"] = "1"
        mock, base_url = start_mock()
        env["OLLAMA_BASE_URL"] = base_url
        args += ["--model", "ollama:mock"]
    elif MODEL:
        args += ["--model", MODEL]
    # La demande (avec la conversation) sur l'entrée standard, pas dans la ligne de commande :
    # un `pkill -f <texte>` lancé par l'agent y trouvait Claw et l'arrêtait (session 12).
    args += ["prompt", "-"]

    host = start_mcp(env)
    quota_before = free_quota(env.get("OPENROUTER_API_KEY", ""))
    progress = Progress()
    progress.publish(force=True)
    stop_refresh = threading.Event()

    def refresh():
        while not stop_refresh.wait(PROGRESS_EVERY):
            progress.publish()
            progress.publish_journal()

    threading.Thread(target=refresh, daemon=True).start()
    result, error, stopped = "", "", False
    # Requêtes du passage, d'après la fin de Claw (`num_turns`) ; inconnues s'il s'arrête avant.
    requests = None
    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)
    with open(WORK / "claw.jsonl", "w", encoding="utf-8") as log, \
            open(WORK / "claw.stderr", "w", encoding="utf-8") as errors:
        process = subprocess.Popen(args, cwd="work", env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=errors, text=True, encoding="utf-8", errors="replace")
        process.stdin.write(request)
        process.stdin.close()
        # L'étape de publication arrête un agent qui aurait survécu à la sienne.
        (WORK / "claw.pid").write_text(str(process.pid), encoding="utf-8")
        try:
            for line in process.stdout:
                log.write(line)
                log.flush()
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    continue
                progress.event(event)
                if event.get("type") == "result":
                    result = event.get("result") or ""
                    error = event.get("error") or ""
                    if isinstance(event.get("num_turns"), int):
                        requests = event["num_turns"]
            status = process.wait()
        except Stopped:
            stopped = True
            print("Job annulé : arrêt de Claw et des commandes qu'il a lancées", flush=True)
            stop_tree(process.pid, alive=lambda: process.poll() is None)
            status = process.wait()
    (WORK / "claw.pid").unlink(missing_ok=True)
    stop_refresh.set()
    if host:
        host.stop()
    if mock:
        mock.terminate()
    if stopped:
        error = "Passage annulé : Claw a été arrêté."
    elif not result and not error and status != 0:
        error = (WORK / "claw.stderr").read_text(encoding="utf-8", errors="replace")[-1500:].strip()
    if error:
        print(f"Erreur : {error[-800:]}", flush=True)
    # Fini sans réponse ni erreur : le tour s'est arrêté avant la conclusion (limite de
    # requêtes du tour, par exemple) ; ce n'est pas une erreur.
    if stopped:
        state = "stopped"
    elif status == 0 and (result or progress.questions):
        state = "done"
    elif status == 0 and not error:
        state = "unfinished"
    else:
        state = "failed"
    quota = free_quota(env.get("OPENROUTER_API_KEY", ""))
    if quota:
        spent = quota["used"] - quota_before["used"] if quota_before else None
        quota["run"] = spent if spent is not None and spent >= 0 else None
        with progress.lock:
            progress.journal.events.append({"k": "quota", **quota})
            progress.journal.dirty = True
        (WORK / "quota.json").write_text(json.dumps(quota), encoding="utf-8")
        print(f"Requêtes gratuites : {quota['used']}/{quota['limit']} utilisées aujourd'hui", flush=True)
    # Modèle choisi et requêtes du passage : le pied de la réponse suit son fournisseur
    # (Gemini et NVIDIA n'ont pas de compte à lire comme OpenRouter).
    (WORK / "usage.json").write_text(json.dumps({"model": MODEL, "requests": requests}), encoding="utf-8")
    progress.publish(state, force=True)
    progress.publish_journal(force=True)
    (WORK / "notices.txt").write_text("\n".join(progress.notices[-2:]), encoding="utf-8")
    (WORK / "result.md").write_text(result or "", encoding="utf-8")
    (WORK / "questions.json").write_text(json.dumps(progress.questions, ensure_ascii=False), encoding="utf-8")
    (WORK / "error.txt").write_text(error or "", encoding="utf-8")
    remember(state)
    output("status", str(status))
    sys.exit(130 if stopped else status)


if __name__ == "__main__":
    main()
