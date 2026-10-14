"""Outils communs du moteur de l'appli web de Claw (workflow claw-web.yml).

Les sessions sont des issues du dépôt du moteur (étiquette `claw-web`) : le corps porte la
tâche et ses réglages, chaque message de suivi est un commentaire, et le moteur y publie sa
progression puis sa réponse, marquées pour que l'appli et la session suivante les
reconnaissent.
"""

import json
import os
import re
import urllib.error
import urllib.request
from pathlib import Path

API = "https://api.github.com"
ANSWER = "<!-- claw-web:answer -->"
PROGRESS = "<!-- claw-web:progress -->"
SETTINGS = re.compile(r"<!--\s*claw-web\s+(\{.*?\})\s*-->", re.S)

# Fichiers échangés entre les étapes du job.
WORK = Path(os.environ.get("RUNNER_TEMP", ".")) / "claw-web"


class GitHubError(Exception):
    pass


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
        raise GitHubError(f"{method} {path} : HTTP {error.code} {detail}") from None
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


def run_url():
    server = os.environ.get("GITHUB_SERVER_URL", "https://github.com")
    return f"{server}/{os.environ.get('GITHUB_REPOSITORY', '')}/actions/runs/{os.environ.get('GITHUB_RUN_ID', '')}"
