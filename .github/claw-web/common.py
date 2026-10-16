"""Outils communs du moteur de l'appli web de Claw (workflow claw-web.yml).

Les sessions sont des issues du dépôt du moteur (étiquette `claw-web`) : le corps porte la
tâche et ses réglages, chaque message de suivi est un commentaire, et le moteur y publie sa
progression puis sa réponse, marquées pour que l'appli et la session suivante les
reconnaissent.
"""

import json
import os
import re
import signal
import time
import urllib.error
import urllib.request
from pathlib import Path

API = "https://api.github.com"
ANSWER = "<!-- claw-web:answer -->"
PROGRESS = "<!-- claw-web:progress -->"
LOG = "<!-- claw-web:log"
# Dans une réponse : le dernier message traité (identifiant de commentaire), ou « keep »
# (réponse qui ne traite rien : échec au démarrage).
ANSWERED = re.compile(r"<!--\s*claw-web:answered\s+(\d+|keep)\s*-->")


def kind_of(body):
    """« engine » (progression, journal), « answer » ou « message » (de l'utilisateur)."""
    body = (body or "").strip()
    if PROGRESS in body or body.startswith(LOG):
        return "engine"
    return "answer" if ANSWER in body else "message"


def answer_marks(notes):
    """Chaque réponse, dans l'ordre, avec l'identifiant du dernier message qu'elle a traité (0 :
    la tâche seule ; None : « keep », la tâche n'a pas pu commencer et ne traite rien). Une
    réponse sans marque (plus ancienne) a traité les messages qui la précèdent."""
    marks, last_message = [], 0
    for note in notes:
        body = note.get("body") or ""
        kind = kind_of(body)
        if kind == "message":
            last_message = note["id"]
        elif kind == "answer":
            mark = ANSWERED.search(body)
            if mark is None:
                marks.append((note, last_message))
            else:
                marks.append((note, None if mark.group(1) == "keep" else int(mark.group(1))))
    return marks


def answered_up_to(notes):
    """Identifiant du dernier message traité par une réponse."""
    return max((mark for _, mark in answer_marks(notes) if mark is not None), default=0)
SETTINGS = re.compile(r"<!--\s*claw-web\s+(\{.*?\})\s*-->", re.S)
# Images jointes à un message par l'appli : chemins dans la branche FILES_BRANCH du moteur.
IMAGES = re.compile(r"<!--\s*claw-web:images\s+(\[.*?\])\s*-->", re.S)
FILES_BRANCH = "claw-web-fichiers"
IMAGE_PATH = re.compile(r"uploads/[A-Za-z0-9._-]+/[A-Za-z0-9._-]+\.(?:png|jpe?g|gif|webp)")

# Fichiers échangés entre les étapes du job.
WORK = Path(os.environ.get("RUNNER_TEMP", ".")) / "claw-web"


class GitHubError(Exception):
    """Réponse d'erreur de l'API : `status` est le code HTTP, `reason` le message de GitHub."""

    def __init__(self, message, status=0, reason=""):
        super().__init__(message)
        self.status = status
        self.reason = reason


def api(method, path, token, body=None):
    """Appel de l'API GitHub ; renvoie le JSON décodé (ou None)."""
    data = None if body is None else json.dumps(body).encode("utf-8")
    request = urllib.request.Request(API + path, method=method, data=data)
    request.add_header("Authorization", f"Bearer {token}")
    request.add_header("Accept", "application/vnd.github+json")
    request.add_header("X-GitHub-Api-Version", "2022-11-28")
    request.add_header("User-Agent", "claw-web")
    if data is not None:
        request.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            raw = response.read()
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", "replace")[:300]
        try:
            reason = str(json.loads(detail).get("message") or "")
        except (ValueError, AttributeError):
            reason = ""
        raise GitHubError(f"{method} {path} : HTTP {error.code} {detail}", error.code, reason or detail) from None
    return json.loads(raw) if raw else None


def comments(token, repo, issue):
    """Tous les commentaires d'une issue, dans l'ordre."""
    found, page = [], 1
    while True:
        batch = api("GET", f"/repos/{repo}/issues/{issue}/comments?per_page=100&page={page}", token)
        found.extend(batch)
        if len(batch) < 100:
            return found
        page += 1


def post_comment(token, repo, issue, body):
    return api("POST", f"/repos/{repo}/issues/{issue}/comments", token, {"body": body})


def edit_comment(token, repo, comment_id, body):
    return api("PATCH", f"/repos/{repo}/issues/comments/{comment_id}", token, {"body": body})


def settings(body):
    """Réglages de la session écrits par l'appli dans le corps de l'issue."""
    found = SETTINGS.search(body or "")
    if not found:
        return {}
    try:
        value = json.loads(found.group(1))
    except json.JSONDecodeError:
        return {}
    return value if isinstance(value, dict) else {}


def images_of(text):
    """Le texte sans ses blocs d'images (remplacés par « (n image(s) jointe(s)) ») et les
    chemins des images, vérifiés."""
    found = []

    def replace(match):
        try:
            paths = json.loads(match.group(1))
        except json.JSONDecodeError:
            return ""
        paths = [path for path in paths if isinstance(path, str) and IMAGE_PATH.fullmatch(path)]
        found.extend(paths)
        return f"({len(paths)} image(s) jointe(s))" if paths else ""

    return IMAGES.sub(replace, text or "").strip(), found


def task_text(body):
    """La tâche, sans le bloc de réglages."""
    return SETTINGS.sub("", body or "").strip()


def output(name, value):
    """Sortie d'étape (texte sur plusieurs lignes accepté)."""
    path = os.environ.get("GITHUB_OUTPUT")
    if not path:
        print(f"{name}={value}")
        return
    delimiter = f"CLAW_WEB_{os.urandom(8).hex()}"
    with open(path, "a", encoding="utf-8") as handle:
        handle.write(f"{name}<<{delimiter}\n{value}\n{delimiter}\n")


def process_tree(root):
    """Le processus et tous ses descendants (Linux : /proc), lus avant de l'arrêter : mort,
    ses enfants changeraient de parent."""
    children = {}
    if os.path.isdir("/proc"):
        for entry in os.listdir("/proc"):
            if not entry.isdigit():
                continue
            try:
                with open(f"/proc/{entry}/stat", encoding="utf-8", errors="replace") as handle:
                    parent = int(handle.read().rsplit(")", 1)[1].split()[1])
            except (OSError, ValueError, IndexError):
                continue
            children.setdefault(parent, []).append(int(entry))
    tree, todo = [], [root]
    while todo:
        pid = todo.pop()
        tree.append(pid)
        todo.extend(children.get(pid, []))
    return tree


def stop_tree(root, alive=lambda: True, grace=5):
    """Arrête l'agent et les commandes qu'il a lancées (serveur, tests...) : rien ne doit plus
    écrire dans le dépôt pendant la publication. SIGTERM, puis SIGKILL après `grace` s."""
    tree = process_tree(root)

    def send(sig):
        for pid in tree:
            try:
                os.kill(pid, sig)
            except (ProcessLookupError, PermissionError):
                pass

    send(signal.SIGTERM)
    deadline = time.time() + grace
    while time.time() < deadline and alive():
        time.sleep(0.1)
    send(signal.SIGKILL)


def run_url():
    server = os.environ.get("GITHUB_SERVER_URL", "https://github.com")
    return f"{server}/{os.environ.get('GITHUB_REPOSITORY', '')}/actions/runs/{os.environ.get('GITHUB_RUN_ID', '')}"
