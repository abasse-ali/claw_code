"""Fichiers rangés par le moteur sur la branche des fichiers du dépôt des sessions
(`claw-web-fichiers`, privé) : routines, abonnements, documents. Lus et écrits par l'API
Contents de GitHub, avec le jeton du moteur, jamais celui de l'agent.
"""

import base64
import json
import urllib.parse

from common import FILES_BRANCH, GitHubError, api


def quote_path(path):
    return urllib.parse.quote(path, safe="/")


class BranchStore:
    """Une branche d'un dépôt vue comme un petit stockage de fichiers. `call` est l'appel de
    l'API (remplacé dans les essais)."""

    def __init__(self, token, repo, branch=FILES_BRANCH, call=api):
        self.token, self.repo, self.branch, self.call = token, repo, branch, call
        self.ready = None

    def ensure_branch(self):
        """La branche existe (créée depuis la branche par défaut au besoin)."""
        if self.ready:
            return True
        try:
            self.call("GET", f"/repos/{self.repo}/branches/{urllib.parse.quote(self.branch, safe='')}", self.token)
            self.ready = True
            return True
        except GitHubError as error:
            if error.status != 404:
                raise
        default = self.call("GET", f"/repos/{self.repo}", self.token)["default_branch"]
        head = self.call("GET", f"/repos/{self.repo}/git/ref/heads/{urllib.parse.quote(default, safe='')}", self.token)
        try:
            self.call("POST", f"/repos/{self.repo}/git/refs", self.token,
                      {"ref": f"refs/heads/{self.branch}", "sha": head["object"]["sha"]})
        except GitHubError as error:
            # Créée entre-temps par une autre session.
            if error.status != 422:
                raise
        self.ready = True
        return True

    def read(self, path):
        """(texte, sha) ; (None, None) si le fichier ou la branche manque."""
        try:
            found = self.call("GET", f"/repos/{self.repo}/contents/{quote_path(path)}?ref="
                                     f"{urllib.parse.quote(self.branch, safe='')}", self.token)
        except GitHubError as error:
            if error.status == 404:
                return None, None
            raise
        if not isinstance(found, dict) or found.get("type", "file") != "file":
            return None, None
        try:
            text = base64.b64decode(found.get("content") or "").decode("utf-8", "replace")
        except (ValueError, TypeError):
            text = ""
        return text, found.get("sha")

    def read_json(self, path, default):
        text, sha = self.read(path)
        if text is None:
            return default, None
        try:
            return json.loads(text), sha
        except ValueError:
            return default, sha

    def write(self, path, text, message, sha=None):
        """Écrit le fichier ; `sha` : la version lue (sans lui, un fichier existant est refusé)."""
        self.ensure_branch()
        body = {"message": message, "content": base64.b64encode(text.encode("utf-8")).decode("ascii"),
                "branch": self.branch}
        if sha:
            body["sha"] = sha
        return self.call("PUT", f"/repos/{self.repo}/contents/{quote_path(path)}", self.token, body)

    def write_bytes(self, path, data, message, sha=None):
        self.ensure_branch()
        body = {"message": message, "content": base64.b64encode(data).decode("ascii"), "branch": self.branch}
        if sha:
            body["sha"] = sha
        return self.call("PUT", f"/repos/{self.repo}/contents/{quote_path(path)}", self.token, body)

    def update_json(self, path, update, message, default):
        """Relit le fichier JSON, y applique `update` (qui change la valeur et peut rendre un
        résultat), le range, et recommence une fois si une autre session l'a changé entre-temps.
        Rend le résultat de `update`."""
        last = None
        for _ in range(3):
            value, sha = self.read_json(path, default() if callable(default) else default)
            result = update(value)
            try:
                self.write(path, json.dumps(value, ensure_ascii=False, indent=1) + "\n", message, sha)
                return result
            except GitHubError as error:
                if error.status not in (409, 422):
                    raise
                last = error
        raise last

    def delete(self, path, message):
        """Efface le fichier ; faux s'il n'existait pas."""
        _, sha = self.read(path)
        if not sha:
            return False
        self.call("DELETE", f"/repos/{self.repo}/contents/{quote_path(path)}", self.token,
                  {"message": message, "sha": sha, "branch": self.branch})
        return True

    def list(self, folder):
        """Les entrées d'un dossier (nom, chemin, type) ; [] s'il manque."""
        try:
            found = self.call("GET", f"/repos/{self.repo}/contents/{quote_path(folder)}?ref="
                                     f"{urllib.parse.quote(self.branch, safe='')}", self.token)
        except GitHubError as error:
            if error.status == 404:
                return []
            raise
        return [{"name": item.get("name"), "path": item.get("path"), "type": item.get("type")}
                for item in found or [] if isinstance(item, dict)] if isinstance(found, list) else []
