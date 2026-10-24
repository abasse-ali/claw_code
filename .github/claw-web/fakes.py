"""Fausse API GitHub en mémoire pour les essais des serveurs MCP et des routines : issues et
commentaires du dépôt des sessions, fichiers d'une branche (API Contents), passages d'Actions,
pull requests. Garde chaque appel (`calls`)."""

import base64
import hashlib
import json
import re
import urllib.parse

from common import GitHubError


def refused(status, reason="refused"):
    return GitHubError(f"HTTP {status} {reason}", status, reason)


class FakeGitHub:
    def __init__(self, engine="o/claw-sessions", default_branch="main"):
        self.engine = engine
        self.default_branch = default_branch
        self.calls = []
        self.files = {}          # (dépôt, branche, chemin) -> (texte en octets, sha)
        self.branches = {(engine, default_branch), (engine, "claw-web-fichiers")}
        self.issues = {}         # numéro -> issue
        self.comments = {}       # numéro -> [commentaire]
        self.runs = []
        self.dispatches = []
        self.cancelled = []
        self.pulls = {}          # (dépôt, numéro) -> données
        self.pull_events = {}    # (dépôt, numéro) -> {"comments": [], "review_comments": [], "reviews": [], "checks": []}
        self.user_repos = []
        self.next_id = 1000
        self.extra = {}          # (méthode, chemin) -> réponse

    # --- Aides des essais ----------------------------------------------------------------------

    def add_session(self, number, text, values, state="open", labels=("claw-web",), title=None):
        body = f"{text}\n\n<!-- claw-web {json.dumps(values)} -->"
        self.issues[number] = {"number": number, "title": title or text.splitlines()[0], "body": body, "state": state,
                               "labels": [{"name": name} for name in labels], "created_at": "2026-10-20T10:00:00Z",
                               "updated_at": "2026-10-20T10:00:00Z", "html_url": f"https://github.com/{self.engine}/issues/{number}"}
        self.comments.setdefault(number, [])
        return self.issues[number]

    def add_comment(self, number, body, created_at="2026-10-20T11:00:00Z"):
        self.next_id += 1
        comment = {"id": self.next_id, "body": body, "created_at": created_at, "user": {"login": "u"}}
        self.comments.setdefault(number, []).append(comment)
        return comment

    def file(self, path, branch="claw-web-fichiers", repo=None):
        found = self.files.get((repo or self.engine, branch, path))
        return None if found is None else found[0].decode("utf-8")

    def json_file(self, path):
        text = self.file(path)
        return None if text is None else json.loads(text)

    # --- L'API ---------------------------------------------------------------------------------

    def __call__(self, method, path, token, body=None):
        self.calls.append((method, path, body))
        if (method, path) in self.extra:
            answer = self.extra[(method, path)]
            if isinstance(answer, GitHubError):
                raise answer
            return answer
        parsed = urllib.parse.urlsplit(path)
        route, query = parsed.path, dict(urllib.parse.parse_qsl(parsed.query))
        found = re.fullmatch(r"/repos/([^/]+/[^/]+)(/.*)?", route)
        if route == "/user/repos":
            page = int(query.get("page", 1))
            return self.user_repos[(page - 1) * 100: page * 100]
        if not found:
            raise refused(404, "Not Found")
        repo, rest = found.group(1), found.group(2) or ""
        if rest == "" and method == "GET":
            return {"full_name": repo, "default_branch": self.default_branch, "private": True}
        if rest.startswith("/branches/") and method == "GET":
            branch = urllib.parse.unquote(rest[len("/branches/"):])
            if (repo, branch) in self.branches:
                return {"name": branch}
            raise refused(404, "Branch not found")
        if rest.startswith("/git/ref/heads/"):
            return {"object": {"sha": "abc"}}
        if rest == "/git/refs" and method == "POST":
            self.branches.add((repo, body["ref"][len("refs/heads/"):]))
            return {}
        if rest.startswith("/contents/"):
            return self._contents(method, repo, urllib.parse.unquote(rest[len("/contents/"):]), query, body)
        if rest.startswith("/issues"):
            return self._issues(method, repo, rest, query, body)
        if rest.startswith("/actions/"):
            return self._actions(method, rest, query, body)
        if rest.startswith("/pulls/"):
            return self._pulls(method, repo, rest, query)
        if rest.startswith("/commits/") and rest.endswith("/check-runs"):
            sha = rest.split("/")[2]
            checks = [check for (r, _), events in self.pull_events.items() if r == repo
                      for check in events.get("checks", []) if check.get("head_sha") == sha]
            return {"check_runs": checks}
        raise refused(404, "Not Found")

    def _contents(self, method, repo, path, query, body):
        if method == "GET":
            branch = query.get("ref", self.default_branch)
            key = (repo, branch, path)
            if key in self.files:
                data, sha = self.files[key]
                return {"type": "file", "path": path, "sha": sha, "content": base64.b64encode(data).decode()}
            prefix = path.rstrip("/") + "/"
            entries = {}
            for (r, b, p) in self.files:
                if r == repo and b == branch and p.startswith(prefix):
                    head = p[len(prefix):].split("/", 1)
                    entries[head[0]] = "dir" if len(head) > 1 else "file"
            if entries:
                return [{"name": name, "path": prefix + name, "type": kind} for name, kind in sorted(entries.items())]
            raise refused(404, "Not Found")
        branch = body.get("branch", self.default_branch)
        key = (repo, branch, path)
        current = self.files.get(key)
        if method == "PUT":
            if current and body.get("sha") != current[1]:
                raise refused(409, "sha mismatch")
            if not current and body.get("sha"):
                raise refused(422, "no such file")
            data = base64.b64decode(body["content"])
            sha = hashlib.sha1(data + path.encode()).hexdigest()
            self.files[key] = (data, sha)
            return {"content": {"sha": sha, "path": path}}
        if method == "DELETE":
            if not current or body.get("sha") != current[1]:
                raise refused(409, "sha mismatch")
            del self.files[key]
            return {}
        raise refused(405, "Method not allowed")

    def _issues(self, method, repo, rest, query, body):
        if rest == "/issues" and method == "GET":
            labels = set(filter(None, query.get("labels", "").split(",")))
            state = query.get("state", "open")
            found = [issue for issue in self.issues.values()
                     if labels <= {label["name"] for label in issue["labels"]} and state in ("all", issue["state"])]
            found.sort(key=lambda issue: issue["updated_at"], reverse=True)
            per_page, page = int(query.get("per_page", 30)), int(query.get("page", 1))
            return found[(page - 1) * per_page: page * per_page]
        if rest == "/issues" and method == "POST":
            number = max(self.issues, default=0) + 1
            issue = self.add_session(number, body["title"], {}, labels=tuple(body.get("labels") or ()), title=body["title"])
            issue["body"] = body["body"]
            return issue
        match = re.fullmatch(r"/issues/(\d+)(/.*)?", rest)
        if not match:
            raise refused(404, "Not Found")
        number, sub = int(match.group(1)), match.group(2) or ""
        if (repo, number) in self.pulls and sub == "/comments" and method == "GET":
            return self.pull_events.get((repo, number), {}).get("comments", [])
        if number not in self.issues:
            raise refused(404, "Not Found")
        issue = self.issues[number]
        if sub == "" and method == "GET":
            return issue
        if sub == "" and method == "PATCH":
            issue.update(body)
            return issue
        if sub == "/comments" and method == "GET":
            per_page, page = int(query.get("per_page", 30)), int(query.get("page", 1))
            return self.comments.get(number, [])[(page - 1) * per_page: page * per_page]
        if sub == "/comments" and method == "POST":
            return self.add_comment(number, body["body"], "2026-10-23T12:00:00Z")
        if sub == "/labels" and method == "POST":
            for name in body["labels"]:
                if name not in [label["name"] for label in issue["labels"]]:
                    issue["labels"].append({"name": name})
            return issue["labels"]
        if sub.startswith("/labels/") and method == "DELETE":
            name = urllib.parse.unquote(sub[len("/labels/"):])
            before = len(issue["labels"])
            issue["labels"] = [label for label in issue["labels"] if label["name"] != name]
            if len(issue["labels"]) == before:
                raise refused(404, "Label not found")
            return issue["labels"]
        raise refused(404, "Not Found")

    def _actions(self, method, rest, query, body):
        if rest.endswith("/runs") and method == "GET":
            return {"workflow_runs": self.runs}
        if rest.endswith("/dispatches") and method == "POST":
            self.dispatches.append(body)
            return None
        match = re.fullmatch(r"/actions/runs/(\d+)/cancel", rest)
        if match and method == "POST":
            self.cancelled.append(int(match.group(1)))
            return None
        if rest.startswith("/actions/runners"):
            return {"runners": [{"name": "pc", "status": "online", "busy": False, "labels": [{"name": "claw-pc"}]}]}
        raise refused(404, "Not Found")

    def _pulls(self, method, repo, rest, query):
        match = re.fullmatch(r"/pulls/(\d+)(/.*)?", rest)
        number, sub = int(match.group(1)), match.group(2) or ""
        if (repo, number) not in self.pulls:
            raise refused(404, "Not Found")
        events = self.pull_events.get((repo, number), {})
        if sub == "":
            return self.pulls[(repo, number)]
        if sub == "/comments":
            return events.get("review_comments", [])
        if sub == "/reviews":
            return events.get("reviews", [])
        raise refused(404, "Not Found")
