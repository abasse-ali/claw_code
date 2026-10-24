"""Les sessions de Claw Web vues du moteur : des issues du dépôt des sessions (étiquette
`claw-web`), dont le corps porte la tâche et ses réglages (`<!-- claw-web {...} -->`), chaque
message étant un commentaire. Utilisé par le serveur `claw-remote` (mcp_remote.py) et par les
routines (routines.py) : lister, créer une session, y écrire, lancer ou arrêter son passage.

Un message envoyé par une autre session porte `<!-- claw-web:from-session N -->` ; celui d'une
routine `<!-- claw-web:routine trig_… -->`. L'appli les montre comme des messages, et setup.py
les donne à l'agent comme des données venues d'ailleurs, pas comme les mots de l'utilisateur.
"""

import json
import os
import re
import urllib.parse

from common import ANSWER, SETTINGS, GitHubError, api, kind_of, settings, task_text

WORKFLOW = "claw-web.yml"
LABEL = "claw-web"
TAG_PREFIX = "tag:"
FROM_SESSION = re.compile(r"<!--\s*claw-web:from-session\s+(\d+)\s*-->")
FROM_ROUTINE = re.compile(r"<!--\s*claw-web:routine\s+(trig_[A-Za-z0-9]+)\s*-->")
FROM_WATCH = re.compile(r"<!--\s*claw-web:watch\s+([A-Za-z0-9_:-]+)\s*-->")
APP_URL = os.environ.get("CLAW_WEB_URL", "").strip() or "https://abasse-ali.github.io/claw_code/"


def app_link(fragment):
    return APP_URL.rstrip("/") + "/#/" + fragment.lstrip("/")


def run_title(repo, number):
    """Nom des passages d'une session dans Actions (claw-web.yml : « Claw <dépôt> #<session> »)."""
    return f"Claw {repo} #{number}"


def session_number(value):
    """« 12 », « #12 », « session-12 », « session_12 » : le numéro d'issue de la session."""
    found = re.fullmatch(r"\s*(?:#|session[-_ ]?)?(\d+)\s*", str(value or ""), re.I)
    return found.group(1) if found else ""


def origin_of(body):
    """D'où vient un message : (« session », numéro), (« routine », id), (« watch », id) ou None."""
    for kind, pattern in (("session", FROM_SESSION), ("routine", FROM_ROUTINE), ("watch", FROM_WATCH)):
        found = pattern.search(body or "")
        if found:
            return kind, found.group(1)
    return None


class SessionError(Exception):
    """Refus dit au modèle (en anglais)."""


class Sessions:
    """Les sessions du dépôt `engine`. `job_token` : jeton du job (issues, contenu) ;
    `user_token` : celui de l'utilisateur (CLAW_REPO_TOKEN), pour lancer ou arrêter un passage
    et lister les dépôts, ce que le jeton du job ne peut pas."""

    def __init__(self, engine, job_token, user_token="", call=api):
        self.engine, self.job_token, self.user_token, self.call = engine, job_token, user_token, call
        self._ref = None
        self._runs = None

    def _job(self, method, path, body=None):
        return self.call(method, path, self.job_token or self.user_token, body)

    def _user(self, method, path, body=None):
        token = self.user_token or self.job_token
        return self.call(method, path, token, body)

    # --- Lecture -------------------------------------------------------------------------------

    def list(self, state="all", tags=(), page=1, per_page=50):
        labels = ",".join([LABEL, *(TAG_PREFIX + tag for tag in tags)])
        issues = self._job("GET", f"/repos/{self.engine}/issues?labels={urllib.parse.quote(labels)}&state={state}"
                                  f"&sort=updated&direction=desc&per_page={per_page}&page={page}")
        return [issue for issue in issues or [] if not issue.get("pull_request")]

    def issue(self, number):
        try:
            found = self._job("GET", f"/repos/{self.engine}/issues/{number}")
        except GitHubError as error:
            if error.status == 404:
                raise SessionError(f"Session {number} not found in Claw Web.") from None
            raise
        if not any(label.get("name") == LABEL for label in found.get("labels") or []) or found.get("pull_request"):
            raise SessionError(f"#{number} is not a Claw Web session.")
        return found

    def comments(self, number):
        found, page = [], 1
        while True:
            batch = self._job("GET", f"/repos/{self.engine}/issues/{number}/comments?per_page=100&page={page}")
            found.extend(batch or [])
            if not batch or len(batch) < 100:
                return found
            page += 1

    def runs(self, refresh=False):
        """Les derniers passages du workflow des sessions (plus récents d'abord)."""
        if self._runs is None or refresh:
            try:
                data = self._job("GET", f"/repos/{self.engine}/actions/workflows/{WORKFLOW}/runs?per_page=60")
                self._runs = (data or {}).get("workflow_runs") or []
            except GitHubError:
                self._runs = []
        return self._runs

    def runs_of(self, number, repo):
        title = run_title(repo, number)
        return [run for run in self.runs() if run.get("display_title") == title]

    def status(self, issue):
        """archived, queued, working, idle (fini), stopped, failed, ou new (aucun passage)."""
        if issue.get("state") == "closed":
            return "archived"
        repo = settings(issue.get("body")).get("repo") or ""
        runs = self.runs_of(issue["number"], repo)
        if not runs:
            return "new"
        run = runs[0]
        if run.get("status") in ("queued", "waiting", "pending", "requested"):
            return "queued"
        if run.get("status") == "in_progress":
            return "working"
        return {"success": "idle", "cancelled": "stopped"}.get(run.get("conclusion"), "failed")

    def summary(self, issue):
        values = settings(issue.get("body"))
        tags = sorted(label["name"][len(TAG_PREFIX):] for label in issue.get("labels") or []
                      if str(label.get("name", "")).startswith(TAG_PREFIX))
        repo = values.get("repo") or ""
        runs = self.runs_of(issue["number"], repo)
        return {
            "session_id": str(issue["number"]),
            "n": values.get("n"),
            "title": issue.get("title"),
            "status": self.status(issue),
            "repo": repo,
            "base": values.get("base") or "",
            "branch": values.get("branch") or f"claw/session-{issue['number']}",
            "model": values.get("model") or "",
            "tags": tags,
            "created_at": issue.get("created_at"),
            "updated_at": issue.get("updated_at"),
            "last_run": ({"status": runs[0].get("status"), "conclusion": runs[0].get("conclusion"),
                          "url": runs[0].get("html_url"), "started_at": runs[0].get("run_started_at")}
                         if runs else None),
            "url": app_link(f"session/{issue['number']}"),
        }

    def next_number(self):
        """Numéro suivant de l'appli (comme `nextNumber` de l'appli)."""
        most = 0
        for page in range(1, 6):
            batch = self.list(page=page, per_page=100)
            for issue in batch:
                try:
                    most = max(most, int(settings(issue.get("body")).get("n") or 0))
                except (TypeError, ValueError):
                    pass
            if len(batch) < 100:
                break
        return most + 1

    def engine_ref(self):
        if self._ref is None:
            self._ref = (self._job("GET", f"/repos/{self.engine}") or {}).get("default_branch") or "main"
        return self._ref

    # --- Écriture ------------------------------------------------------------------------------

    def create(self, prompt, repo, base, model="", title="", origin="", tags=(), branch_exists=None):
        """Nouvelle session, comme l'appli : titre (première ligne), tâche et réglages."""
        n = self.next_number()
        branch = f"claw/session-{n}"
        for suffix in range(2, 20):
            if branch_exists is None or not branch_exists(repo, branch):
                break
            branch = f"claw/session-{n}-{suffix}"
        first = next((line.strip() for line in prompt.splitlines() if line.strip()), "Session")
        title = (title or (first if len(first) <= 72 else first[:71] + "…")).strip()
        values = {"repo": repo, "base": base, "model": model or "", "n": n, "branch": branch}
        if origin:
            values["from"] = str(origin)
        block = f"<!-- claw-web {json.dumps(values, ensure_ascii=False)} -->"
        labels = [LABEL, *(TAG_PREFIX + tag for tag in tags)]
        return self._job("POST", f"/repos/{self.engine}/issues",
                         {"title": title, "body": f"{prompt.strip()}\n\n{block}", "labels": labels})

    def post(self, number, text, origin=None):
        """Un message dans la session ; `origin` : (« session », n), (« routine », id) ou
        (« watch », id)."""
        header = ""
        if origin:
            kind, ident = origin
            mark = {"session": "from-session", "routine": "routine", "watch": "watch"}[kind]
            label = {"session": f"Message de la session #{ident} de Claw",
                     "routine": f"Routine {ident}", "watch": f"Surveillance {ident}"}[kind]
            header = f"<!-- claw-web:{mark} {ident} -->\n**{label}**\n\n"
        return self._job("POST", f"/repos/{self.engine}/issues/{number}/comments", {"body": header + text.strip()})

    def dispatch(self, issue, model=None, runner="", options=None):
        """Lance un passage de la session (workflow des sessions, jeton de l'utilisateur)."""
        values = settings(issue.get("body"))
        inputs = {"repo": values.get("repo") or "", "base": values.get("base") or "main",
                  "session": str(issue["number"]),
                  "model": (values.get("model") or "") if model is None else model}
        for key, value in (options or {}).items():
            if value:
                inputs[key] = value
        if runner:
            inputs["runner"] = runner
        try:
            self._user("POST", f"/repos/{self.engine}/actions/workflows/{WORKFLOW}/dispatches",
                       {"ref": self.engine_ref(), "inputs": inputs})
        except GitHubError as error:
            if error.status == 422 and "runner" in inputs and "runner" in (error.reason or ""):
                inputs.pop("runner")
                self._user("POST", f"/repos/{self.engine}/actions/workflows/{WORKFLOW}/dispatches",
                           {"ref": self.engine_ref(), "inputs": inputs})
            else:
                raise
        return inputs

    def active_runs(self, issue):
        repo = settings(issue.get("body")).get("repo") or ""
        return [run for run in self.runs_of(issue["number"], repo)
                if run.get("status") in ("queued", "in_progress", "waiting", "pending", "requested")]

    def cancel(self, run_id):
        self._user("POST", f"/repos/{self.engine}/actions/runs/{run_id}/cancel")

    def set_state(self, number, state):
        return self._job("PATCH", f"/repos/{self.engine}/issues/{number}", {"state": state})

    def set_title(self, number, title):
        return self._job("PATCH", f"/repos/{self.engine}/issues/{number}", {"title": title})

    def add_labels(self, number, labels):
        return self._job("POST", f"/repos/{self.engine}/issues/{number}/labels", {"labels": labels})

    def remove_label(self, number, label):
        try:
            self._job("DELETE", f"/repos/{self.engine}/issues/{number}/labels/{urllib.parse.quote(label, safe='')}")
        except GitHubError as error:
            if error.status != 404:
                raise


def event_of(comment):
    """Un commentaire de session comme événement de la conversation."""
    body = comment.get("body") or ""
    kind = kind_of(body)
    origin = origin_of(body)
    if kind == "engine":
        event_kind = "progress" if "claw-web:progress" in body else "journal"
    elif kind == "answer":
        event_kind = "assistant"
    else:
        event_kind = "peer" if origin else "user"
    text = re.sub(r"<!--[\s\S]*?-->", "", body.replace(ANSWER, "")).strip()
    event = {"event_id": str(comment.get("id")), "kind": event_kind, "created_at": comment.get("created_at")}
    if origin:
        event["from"] = {"kind": origin[0], "id": origin[1]}
    if event_kind == "journal":
        event["text"] = "(detailed run journal, compressed: open the session in Claw Web to read it)"
    else:
        event["text"] = text
    return event


def task_event(issue):
    return {"event_id": "task", "kind": "user", "created_at": issue.get("created_at"),
            "text": SETTINGS.sub("", task_text(issue.get("body"))).strip()}
