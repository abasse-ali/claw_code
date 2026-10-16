"""Essais de on_github.py et de la commande `claw-github`, avec une fausse API GitHub.

    cd .github/claw-web && python -m unittest test_on_github
"""

import io
import json
import os
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

import claw_github
import on_github
from common import GitHubError


class FakeGitHub:
    """Réponses par (méthode, chemin) ; une exception est levée, une liste est consommée dans
    l'ordre. Garde les appels."""

    def __init__(self, routes):
        self.routes = routes
        self.calls = []

    def __call__(self, method, path, token, body=None):
        self.calls.append((method, path, body))
        key = (method, path.split("/repos/o/r", 1)[-1])
        if key not in self.routes:
            raise GitHubError(f"{method} {path} : HTTP 404", 404, "Not Found")
        answer = self.routes[key]
        if isinstance(answer, list):
            answer = answer.pop(0)
        if isinstance(answer, GitHubError):
            raise answer
        return answer


def refused(status, reason):
    return GitHubError(f"HTTP {status} {reason}", status, reason)


class StateTest(unittest.TestCase):
    def test_merged_pull_request_and_pages_off_on_a_private_repository(self):
        fake = FakeGitHub({
            ("GET", ""): {"private": True, "default_branch": "main"},
            ("GET", "/compare/main...claw%2Fsession-12"): {"ahead_by": 0},
            ("GET", "/pulls?state=all&head=o%3Aclaw%2Fsession-12&per_page=10"): [[
                {"number": 2, "state": "closed", "merged_at": "2026-10-16T02:19:31Z",
                 "html_url": "https://github.com/o/r/pull/2", "base": {"ref": "main"}}]],
        })
        with mock.patch.object(on_github, "api", fake):
            state = on_github.read_state("o/r", "main", "claw/session-12", "t")
        text = on_github.describe(state)
        self.assertIn("Pull request #2 (claw/session-12 into main): already merged into main on 2026-10-16", text)
        self.assertIn("GitHub Pages: not enabled", text)
        self.assertIn("private", text)
        self.assertIn("free GitHub account", text)

    def test_pages_settings_and_last_build_error(self):
        fake = FakeGitHub({
            ("GET", ""): {"private": False, "default_branch": "main"},
            ("GET", "/compare/main...claw%2Fx"): {"ahead_by": 3},
            ("GET", "/pulls?state=all&head=o%3Aclaw%2Fx&per_page=10"): [[
                {"number": 4, "state": "closed", "merged_at": None, "html_url": "u4", "base": {"ref": "main"}},
                {"number": 5, "state": "open", "merged_at": None, "html_url": "u5", "base": {"ref": "main"}}]],
            ("GET", "/pages"): {"html_url": "https://o.github.io/r/", "source": {"branch": "main", "path": "/docs"}},
            ("GET", "/pages/builds/latest"): {"status": "errored", "error": {"message": "Page build failed."}},
        })
        with mock.patch.object(on_github, "api", fake):
            text = on_github.describe(on_github.read_state("o/r", "main", "claw/x", "t"))
        self.assertIn("Branch claw/x: 3 commit(s) not in main", text)
        self.assertIn("Pull request #5 (claw/x into main): open, not merged yet. u5", text)
        self.assertIn("built from main, docs/ folder, at https://o.github.io/r/; last build: errored (Page build failed.)", text)
        self.assertNotIn("private", text)

    def test_unreadable_parts_are_said(self):
        fake = FakeGitHub({
            ("GET", ""): {"private": False, "default_branch": "main"},
            ("GET", "/pages"): refused(403, "Resource not accessible by personal access token"),
        })
        with mock.patch.object(on_github, "api", fake):
            state = on_github.read_state("o/r", "main", "claw/x", "t")
        text = on_github.describe(state)
        self.assertIn("GitHub Pages: unknown", text)
        self.assertIn("Not readable with the token: pages: HTTP 403", text)
        self.assertIn("Branch claw/x: not on GitHub yet", text)


class ActorTest(unittest.TestCase):
    def actor(self):
        return on_github.Actor("o/r", "main", "claw/x", "t", "budget", wait=0)

    def test_merge_opens_the_pull_request_then_retries_while_github_computes_it(self):
        fake = FakeGitHub({
            ("GET", "/pulls?state=open&head=o%3Aclaw%2Fx"): [[]],
            ("GET", "/compare/main...claw%2Fx"): {"ahead_by": 2},
            ("POST", "/pulls"): {"number": 7, "html_url": "u7"},
            ("PUT", "/pulls/7/merge"): [refused(405, "Pull Request is not mergeable"), {"merged": True}],
        })
        with mock.patch.object(on_github, "api", fake):
            notes = self.actor().carry_out({"merge": True})
        self.assertEqual(notes, ["Pull request #7 fusionnée dans `main`."])
        self.assertEqual(fake.calls[2][2]["head"], "claw/x")

    def test_nothing_to_merge(self):
        fake = FakeGitHub({("GET", "/pulls?state=open&head=o%3Aclaw%2Fx"): [[]],
                           ("GET", "/compare/main...claw%2Fx"): {"ahead_by": 0}})
        with mock.patch.object(on_github, "api", fake):
            notes = self.actor().carry_out({"merge": True})
        self.assertTrue(notes[0].startswith("Rien à fusionner"))

    def test_failed_merge_stops_the_publication(self):
        fake = FakeGitHub({
            ("GET", "/pulls?state=open&head=o%3Aclaw%2Fx"): [[{"number": 3, "html_url": "u3"}]],
            ("PUT", "/pulls/3/merge"): refused(409, "Head branch was modified"),
        })
        with mock.patch.object(on_github, "api", fake):
            notes = self.actor().carry_out({"merge": True, "pages": "docs"})
        self.assertIn("Fusion de la pull request #3 impossible : Head branch was modified", notes[0])
        self.assertEqual(notes[1], "Mise en ligne non faite : la fusion a échoué.")
        self.assertFalse(any(call[1].endswith("/pages") for call in fake.calls))

    def test_first_publication_from_docs(self):
        fake = FakeGitHub({
            ("GET", "/contents/docs/index.html?ref=main"): {"name": "index.html"},
            ("GET", "/pages"): [refused(404, "Not Found"), {"html_url": "https://o.github.io/r/"}],
            ("POST", "/pages"): {"status": None},
        })
        with mock.patch.object(on_github, "api", fake):
            notes = self.actor().carry_out({"pages": "docs"})
        self.assertEqual(notes, ["Site mis en ligne sur GitHub Pages depuis `main` (dossier `docs/`) : "
                                 "https://o.github.io/r/ (visible d'ici une à deux minutes)."])
        self.assertIn(("POST", "/repos/o/r/pages", {"source": {"branch": "main", "path": "/docs"}}), fake.calls)

    def test_publication_refusals_are_explained(self):
        for status, reason, expected in (
                (422, "Your current plan does not support GitHub Pages for this repository.", "dépôt privé"),
                (403, "Resource not accessible by personal access token", "autorisation Pages")):
            fake = FakeGitHub({
                ("GET", "/contents/index.html?ref=main"): {"name": "index.html"},
                ("POST", "/pages"): refused(status, reason),
            })
            with mock.patch.object(on_github, "api", fake):
                notes = self.actor().carry_out({"pages": "root"})
            self.assertIn(expected, notes[0])

    def test_site_missing_on_the_base_branch(self):
        with mock.patch.object(on_github, "api", FakeGitHub({})):
            notes = self.actor().carry_out({"pages": "docs"})
        self.assertIn("`docs/index.html` n'est pas sur `main`", notes[0])


class CommandTest(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        root = Path(self.folder.name)
        self.repo = root / "work"
        (self.repo / "docs").mkdir(parents=True)
        patches = [
            mock.patch.object(on_github, "REQUESTS_FILE", root / "requests.json"),
            mock.patch.object(claw_github, "STATE_FILE", root / "state.json"),
            mock.patch.object(claw_github, "REPO_DIR", self.repo),
        ]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)
        self.addCleanup(self.folder.cleanup)
        (root / "state.json").write_text(json.dumps({
            "repo": "o/r", "base": "main", "branch": "claw/x", "private": True, "default_branch": "main",
            "ahead": 0, "branch_exists": True, "pull": None, "pages": None, "problems": []}), encoding="utf-8")

    def run_command(self, *args):
        out = io.StringIO()
        with redirect_stdout(out):
            code = claw_github.main(list(args))
        return code, out.getvalue()

    def test_pages_needs_a_site(self):
        code, out = self.run_command("pages")
        self.assertEqual(code, 1)
        self.assertIn("Not recorded", out)
        self.assertEqual(on_github.load_requests(), {})

    def test_requests_are_recorded_and_cancelled(self):
        (self.repo / "docs" / "index.html").write_text("<!doctype html>", encoding="utf-8")
        code, out = self.run_command("pages")
        self.assertEqual(code, 0)
        self.assertIn("also run `claw-github merge`", out)
        self.assertIn("private", out)
        self.run_command("merge")
        self.assertEqual(on_github.load_requests(), {"pages": "docs", "merge": True})
        code, out = self.run_command("status")
        self.assertTrue(out.startswith("GitHub state of o/r"))
        self.assertIn("Pull request of claw/x: none", out)
        self.assertIn('"merge": true', out)
        self.run_command("cancel")
        self.assertEqual(on_github.load_requests(), {})


if __name__ == "__main__":
    os.environ.setdefault("RUNNER_TEMP", tempfile.gettempdir())
    unittest.main()
