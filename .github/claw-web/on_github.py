"""Ce que Claw sait et fait sur GitHub pour le dépôt de la tâche, avec le jeton du dépôt, que
l'agent n'a jamais : l'état lu au début du passage (setup.py, donné à l'agent), les demandes de
l'agent (commande `claw-github`, claw_github.py) et leur exécution après la poussée (publish.py).
"""

import json
import time
import urllib.parse

from common import WORK, GitHubError, api

STATE_FILE = WORK / "github-state.json"
REQUESTS_FILE = WORK / "github-requests.json"
# Fusion juste après l'ouverture de la pull request : GitHub calcule encore si elle est
# fusionnable et répond 405 quelques secondes.
MERGE_TRIES = 3
MERGE_WAIT = 5


def quote(ref):
    return urllib.parse.quote(ref, safe="")


def read_state(repo, base, branch, token):
    """État GitHub du dépôt pour cette session. Une partie illisible (droits du jeton) reste
    à None et `problems` dit pourquoi."""
    state = {"repo": repo, "base": base, "branch": branch, "private": None, "default_branch": None,
             "ahead": None, "branch_exists": None, "pull": None, "pages": None, "problems": []}

    def part(name, call):
        try:
            return call()
        except GitHubError as error:
            if error.status == 404:
                return None
            state["problems"].append(f"{name}: HTTP {error.status} {error.reason[:120]}")
            return "unknown"

    info = part("repository", lambda: api("GET", f"/repos/{repo}", token))
    if isinstance(info, dict):
        state["private"] = bool(info.get("private"))
        state["default_branch"] = info.get("default_branch")
    compare = part("branch", lambda: api("GET", f"/repos/{repo}/compare/{quote(base)}...{quote(branch)}", token))
    if isinstance(compare, dict):
        state["branch_exists"] = True
        state["ahead"] = compare.get("ahead_by")
    elif compare is None:
        state["branch_exists"] = False
        state["ahead"] = 0
    owner = repo.split("/")[0]
    pulls = part("pull request", lambda: api(
        "GET", f"/repos/{repo}/pulls?state=all&head={quote(owner + ':' + branch)}&per_page=10", token))
    if isinstance(pulls, list) and pulls:
        pull = next((item for item in pulls if item.get("state") == "open"), pulls[0])
        state["pull"] = {
            "number": pull.get("number"),
            "state": "open" if pull.get("state") == "open" else ("merged" if pull.get("merged_at") else "closed"),
            "url": pull.get("html_url"),
            "base": (pull.get("base") or {}).get("ref"),
            "merged_at": (pull.get("merged_at") or "")[:10],
        }
    pages = part("pages", lambda: api("GET", f"/repos/{repo}/pages", token))
    if isinstance(pages, dict):
        source = pages.get("source") or {}
        found = {"url": pages.get("html_url"), "branch": source.get("branch"), "path": source.get("path"),
                 "build_type": pages.get("build_type"), "status": pages.get("status"), "error": ""}
        build = part("pages build", lambda: api("GET", f"/repos/{repo}/pages/builds/latest", token))
        if isinstance(build, dict):
            found["status"] = build.get("status") or found["status"]
            found["error"] = ((build.get("error") or {}).get("message") or "")[:200]
        state["pages"] = found
    elif pages == "unknown":
        state["pages"] = "unknown"
    return state


def describe(state):
    """L'état pour l'agent (en anglais, comme ses autres notes)."""
    repo, base, branch = state["repo"], state["base"], state["branch"]
    lines = [f"[Claw] GitHub state of {repo}, read at the start of this task. It is current: trust it "
             "over anything said earlier in the conversation."]
    if state["private"] is not None:
        lines.append(f"- Repository: {'private' if state['private'] else 'public'}, default branch "
                     f"{state['default_branch']}.")
    if state["branch_exists"] is False:
        lines.append(f"- Branch {branch}: not on GitHub yet (created from {base} for this task).")
    elif state["ahead"] is not None:
        lines.append(f"- Branch {branch}: {state['ahead']} commit(s) not in {base}.")
    pull = state["pull"]
    if pull is None:
        lines.append(f"- Pull request of {branch}: none.")
    else:
        status = {"open": "open, not merged yet", "merged": f"already merged into {pull['base']} on {pull['merged_at']}",
                  "closed": "closed without merging"}[pull["state"]]
        lines.append(f"- Pull request #{pull['number']} ({branch} into {pull['base']}): {status}. {pull['url']}")
    pages = state["pages"]
    if pages == "unknown":
        lines.append("- GitHub Pages: unknown (the token cannot read the Pages settings).")
    elif pages is None:
        lines.append("- GitHub Pages: not enabled, so the site address does not exist yet.")
    else:
        folder = "docs/ folder" if pages["path"] == "/docs" else "repository root"
        error = f" ({pages['error']})" if pages["error"] else ""
        lines.append(f"- GitHub Pages: enabled, built from {pages['branch']}, {folder}, at {pages['url']}; "
                     f"last build: {pages['status'] or 'unknown'}{error}.")
        # Vu le 21 octobre 2026 : « des captures de la nouvelle version » prises sur le site
        # en ligne, qui montrait l'ancienne.
        if pages["branch"] != branch:
            lines.append(f"- What is online: the site shows {pages['branch']} as last published. The work of "
                         f"this session ({branch}, with your changes in this pass) is not online until it is "
                         f"merged into {pages['branch']}. To show the user this session's version "
                         "(screenshots, a review), use your working copy: Screenshot with `path`, or a local "
                         "server, not the site address. Say in your answer which version each screenshot "
                         "shows; if the user's words could mean either version, ask them.")
    if state["private"] and not isinstance(pages, dict):
        lines.append("- The repository is private: GitHub Pages refuses private repositories on a free "
                     "GitHub account (GitHub Pro allows them). Only the user can make the repository "
                     "public (Settings > General > Danger Zone > Change visibility).")
    for problem in state["problems"]:
        lines.append(f"- Not readable with the token: {problem}.")
    return "\n".join(lines)


def load_requests():
    try:
        value = json.loads(REQUESTS_FILE.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    return value if isinstance(value, dict) else {}


def save_requests(requests):
    REQUESTS_FILE.parent.mkdir(parents=True, exist_ok=True)
    REQUESTS_FILE.write_text(json.dumps(requests), encoding="utf-8")


class Actor:
    """Exécute les demandes de l'agent avec le jeton du dépôt ; chaque méthode renvoie
    (réussite, phrase pour l'utilisateur)."""

    def __init__(self, repo, base, branch, token, title, wait=MERGE_WAIT):
        self.repo, self.base, self.branch, self.token, self.title = repo, base, branch, token, title
        self.wait = wait

    def call(self, method, path, body=None):
        return api(method, f"/repos/{self.repo}{path}", self.token, body)

    def session_pull(self):
        """La pull request ouverte de la branche ; ouverte s'il le faut quand la branche a des
        commits absents de la base ; None s'il n'y a rien à fusionner."""
        owner = self.repo.split("/")[0]
        found = self.call("GET", f"/pulls?state=open&head={quote(owner + ':' + self.branch)}")
        if found:
            return found[0]
        try:
            compare = self.call("GET", f"/compare/{quote(self.base)}...{quote(self.branch)}")
        except GitHubError as error:
            if error.status == 404:
                return None
            raise
        if not compare or not compare.get("ahead_by"):
            return None
        return self.call("POST", "/pulls", {"title": f"Claw : {self.title}", "head": self.branch,
                                            "base": self.base, "body": "Proposé et fusionné par Claw depuis l'appli web."})

    def merge(self):
        try:
            pull = self.session_pull()
        except GitHubError as error:
            return False, f"Fusion non faite : {error.reason or error}"
        if pull is None:
            return True, (f"Rien à fusionner : `{self.branch}` n'a aucun changement absent de "
                          f"`{self.base}` (déjà fusionnée, ou aucun changement).")
        number = pull["number"]
        for attempt in range(MERGE_TRIES):
            try:
                self.call("PUT", f"/pulls/{number}/merge", {"merge_method": "merge"})
                return True, f"Pull request #{number} fusionnée dans `{self.base}`."
            except GitHubError as error:
                if error.status == 405 and attempt + 1 < MERGE_TRIES:
                    time.sleep(self.wait)
                    continue
                if error.status in (403, 404):
                    wanted = permissions_text(error.accepted) or "Contents (écriture) et Pull requests (écriture)"
                    reason = ("le jeton du moteur (secret CLAW_REPO_TOKEN, le jeton de l'appli) n'a pas "
                              f"le droit de fusionner : il lui faut {wanted}")
                elif error.status in (405, 409):
                    reason = f"{error.reason} (conflit avec `{self.base}` ou vérifications obligatoires)"
                else:
                    reason = error.reason or str(error)
                return False, f"Fusion de la pull request #{number} impossible : {reason}. {pull.get('html_url', '')}".strip()
        return False, f"Fusion de la pull request #{number} impossible."

    def pages(self, folder):
        path = "/docs" if folder == "docs" else "/"
        index = "docs/index.html" if folder == "docs" else "index.html"
        try:
            self.call("GET", f"/contents/{index}?ref={quote(self.base)}")
        except GitHubError as error:
            if error.status == 404:
                return False, (f"Mise en ligne non faite : `{index}` n'est pas sur `{self.base}`, d'où "
                               "GitHub Pages publie (la pull request qui l'ajoute n'est pas fusionnée ?).")
            return False, f"Mise en ligne non faite : {error.reason or error}"
        source = {"branch": self.base, "path": path}
        try:
            try:
                current = self.call("GET", "/pages")
            except GitHubError as error:
                if error.status != 404:
                    raise
                current = None
            if current is None:
                self.call("POST", "/pages", {"source": source})
            elif (current.get("source") or {}) != source:
                self.call("PUT", "/pages", {"source": source})
            else:
                # Déjà publié d'ici : une nouvelle construction prend les derniers changements
                # (une fusion en lance une aussi ; un site construit par un workflow la refuse).
                try:
                    self.call("POST", "/pages/builds")
                except GitHubError:
                    pass
            current = self.call("GET", "/pages") or {}
        except GitHubError as error:
            return False, "Mise en ligne non faite : " + pages_problem(error, self.repo, self.base, folder)
        owner, name = self.repo.split("/")
        url = current.get("html_url") or f"https://{owner}.github.io/{name}/"
        where = "dossier `docs/`" if folder == "docs" else "racine du dépôt"
        return True, f"Site mis en ligne sur GitHub Pages depuis `{self.base}` ({where}) : {url} (visible d'ici une à deux minutes)."

    def carry_out(self, requests):
        """Les demandes dans l'ordre : fusion, puis mise en ligne (qui publie la base)."""
        notes = []
        if requests.get("merge"):
            merged, note = self.merge()
            notes.append(note)
            if not merged and requests.get("pages"):
                notes.append("Mise en ligne non faite : la fusion a échoué.")
                return notes
        if requests.get("pages"):
            notes.append(self.pages(requests["pages"])[1])
        return notes


PERMISSIONS = {"administration": "Administration", "pages": "Pages", "contents": "Contents",
               "pull_requests": "Pull requests", "metadata": "Metadata", "workflows": "Workflows"}
LEVELS = {"write": "écriture", "read": "lecture"}


def permissions_text(accepted):
    """En-tête X-Accepted-GitHub-Permissions en mots : « pages=write,administration=write »
    devient « Pages (écriture) et Administration (écriture) » ; « ; » sépare des choix."""
    choices = []
    for group in (accepted or "").split(";"):
        names = []
        for item in group.split(","):
            name, _, level = item.strip().partition("=")
            if name:
                names.append(f"{PERMISSIONS.get(name, name)} ({LEVELS.get(level, level)})")
        if names:
            choices.append(" et ".join(names))
    return " ou ".join(choices)


def pages_problem(error, repo, base, folder):
    said = f" (GitHub : « {error.reason[:160]} »)" if error.reason else ""
    if error.status == 422 and any(word in error.reason.lower() for word in ("plan", "private")):
        return ("GitHub Pages refuse ce dépôt privé avec un compte GitHub gratuit. Rendez-le public "
                "(Settings > General > Danger Zone > Change visibility), puis redemandez la mise en ligne.")
    if error.status in (403, 404):
        # Activer ou changer Pages demande Administration ET Pages en écriture (documentation
        # des jetons à autorisations fines) ; l'en-tête de GitHub le dit quand il est là.
        wanted = permissions_text(error.accepted) or "Administration (écriture) et Pages (écriture)"
        return ("le jeton du moteur (secret CLAW_REPO_TOKEN, le jeton de l'appli) n'a pas les autorisations "
                f"que GitHub demande pour activer Pages : {wanted}. Ajoutez-les dans les réglages du jeton "
                "(https://github.com/settings/personal-access-tokens, « Repository permissions ») ; sa "
                f"valeur ne change pas{said}. Sans Administration : activez Pages une fois vous-même "
                f"(https://github.com/{repo}/settings/pages, « Deploy from a branch », `{base}`, "
                f"`{'/docs' if folder == 'docs' else '/ (root)'}`) ; Claw republiera ensuite avec la seule "
                "autorisation Pages.")
    return f"GitHub Pages : {error.reason or error}"
