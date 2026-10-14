"""Lance l'agent dans le dépôt de la tâche et publie sa progression dans la session.

Entrées (environnement) : MODEL (vide : le meilleur modèle gratuit ; `mock` : faux modèle
d'essai), SESSION, JOB_TOKEN, OPENROUTER_API_KEY, CLAW_SRC (source de Claw, pour le faux
modèle). L'agent tourne dans `work/`, sans aucun jeton GitHub dans son environnement. Un
commentaire de progression est créé dans la session puis mis à jour au fil du travail.
"""

import json
import os
import subprocess
import sys
import threading
import time

from common import PROGRESS, WORK, GitHubError, edit_comment, output, post_comment, run_url

SESSION = os.environ["SESSION"]
ENGINE = os.environ["GITHUB_REPOSITORY"]
JOB_TOKEN = os.environ["JOB_TOKEN"]
MODEL = os.environ.get("MODEL", "").strip()
# Mise à jour du commentaire de progression au plus toutes les N secondes.
PROGRESS_EVERY = 12
SHOWN_ACTIONS = 8


def describe(name, data):
    """Une action de l'agent en quelques mots, pour l'utilisateur."""
    data = data if isinstance(data, dict) else {}
    path = data.get("path") or data.get("file_path") or data.get("notebook_path") or ""
    short = lambda text, size=90: (text if len(text) <= size else text[: size - 1] + "…")  # noqa: E731
    if name == "read_file":
        return f"Lecture de `{short(path)}`"
    if name == "write_file":
        return f"Écriture de `{short(path)}`"
    if name in ("edit_file", "NotebookEdit"):
        return f"Modification de `{short(path)}`"
    if name in ("bash", "PowerShell"):
        command = " ".join(str(data.get("command", "")).split())
        return f"Commande : `{short(command)}`"
    if name in ("grep_search", "glob_search"):
        return f"Recherche : `{short(str(data.get('pattern', '')))}`"
    if name in ("WebFetch", "WebSearch", "ViewPage"):
        return f"Web : {short(str(data.get('url') or data.get('query') or ''))}"
    if name == "TodoWrite":
        return "Liste de tâches mise à jour"
    if name == "Agent":
        return f"Sous-agent : {short(str(data.get('description', '')))}"
    return name


class Progress:
    """Commentaire de progression de la session, mis à jour au fil des événements."""

    def __init__(self):
        self.started = time.time()
        self.actions = []
        self.notices = []
        self.replies = 0
        self.comment_id = None
        self.dirty = True
        self.lock = threading.Lock()

    def event(self, event):
        with self.lock:
            kind = event.get("type")
            if kind == "assistant":
                for part in (event.get("message") or {}).get("content") or []:
                    if part.get("type") == "tool_use":
                        self.actions.append(describe(part.get("name", ""), part.get("input")))
                        self.dirty = True
                    elif part.get("type") == "text" and part.get("text", "").strip():
                        self.replies += 1
            elif kind == "system" and event.get("subtype") == "notice":
                self.notices.append(" ".join(str(event.get("text", "")).split()))
                self.dirty = True

    def body(self, state):
        elapsed = int(time.time() - self.started)
        duration = f"{elapsed // 60} min {elapsed % 60:02d} s" if elapsed >= 60 else f"{elapsed} s"
        head = {
            "running": f"**Claw travaille** depuis {duration}",
            "done": f"**Claw a fini** en {duration}",
            "failed": f"**Claw s'est arrêté sur une erreur** après {duration}",
        }[state]
        lines = [PROGRESS, f"{head} · {len(self.actions)} action(s)"]
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
        with self.lock:
            if not (self.dirty or force):
                return
            body = self.body(state)
            self.dirty = False
        try:
            if self.comment_id is None:
                self.comment_id = post_comment(JOB_TOKEN, ENGINE, SESSION, body)["id"]
            else:
                edit_comment(JOB_TOKEN, ENGINE, self.comment_id, body)
        except GitHubError as error:
            print(f"::warning::progression non publiée : {error}")


def start_mock():
    """Faux modèle local (`rust/scripts/mock-model.js`) pour essayer le moteur sans requête."""
    script = os.path.join(os.environ["CLAW_SRC"], "rust", "scripts", "mock-model.js")
    process = subprocess.Popen(["node", script, "--log", str(WORK / "mock.jsonl")],
                               stdout=subprocess.PIPE, text=True)
    line = process.stdout.readline()
    if not line.startswith("PORT "):
        sys.exit("le faux modèle n'a pas démarré")
    return process, f"http://127.0.0.1:{line.split()[1]}/v1"


def main():
    request = (WORK / "prompt.txt").read_text(encoding="utf-8")
    # Aucun jeton GitHub pour l'agent : il travaille, Claw publie après lui.
    env = {key: value for key, value in os.environ.items()
           if "TOKEN" not in key.upper() or key == "OPENROUTER_API_KEY"}
    env["CLAW_NO_BROWSER"] = "1"
    args = ["claw", "--permission-mode", "danger-full-access", "--output-format", "stream-json"]
    mock = None
    if MODEL == "mock":
        # Aucune clé réelle : rien ne peut partir vers un vrai modèle, relecteurs compris.
        env = {key: value for key, value in env.items() if not key.upper().endswith("_API_KEY")}
        env["CLAW_NO_USER_ENV"] = "1"
        mock, base_url = start_mock()
        env["OLLAMA_BASE_URL"] = base_url
        args += ["--model", "ollama:mock"]
    elif MODEL:
        args += ["--model", MODEL]
    args += ["prompt", request]

    progress = Progress()
    progress.publish(force=True)
    stop = threading.Event()

    def refresh():
        while not stop.wait(PROGRESS_EVERY):
            progress.publish()

    threading.Thread(target=refresh, daemon=True).start()
    result, error = "", ""
    with open(WORK / "claw.jsonl", "w", encoding="utf-8") as log, \
            open(WORK / "claw.stderr", "w", encoding="utf-8") as errors:
        process = subprocess.Popen(args, cwd="work", env=env, stdout=subprocess.PIPE, stderr=errors,
                                   text=True, encoding="utf-8", errors="replace")
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
        status = process.wait()
    stop.set()
    if mock:
        mock.terminate()
    if not result and not error:
        error = (WORK / "claw.stderr").read_text(encoding="utf-8", errors="replace")[-1500:].strip()
    progress.publish("done" if status == 0 and result else "failed", force=True)
    (WORK / "result.md").write_text(result or "", encoding="utf-8")
    (WORK / "error.txt").write_text(error or "", encoding="utf-8")
    output("status", str(status))
    sys.exit(status)


if __name__ == "__main__":
    main()
