"""Captures de l'outil Screenshot : vignette dans le journal, capture entière rangée dans le
dossier de la session sur la branche des fichiers (journal.shots, run.ShotStore)."""

import json
import os
import tempfile
import unittest

os.environ.setdefault("GITHUB_REPOSITORY", "o/claw-sessions")
os.environ.setdefault("JOB_TOKEN", "t")
os.environ.setdefault("SESSION", "1")

import run  # noqa: E402
from common import GitHubError  # noqa: E402
from journal import Journal  # noqa: E402

JPEG = b"\xff\xd8\xff\xe0" + b"0" * 200


class FakeGitHub:
    def __init__(self, branch=True, refuse_put=False):
        self.calls = []
        self.branch = branch
        self.refuse_put = refuse_put

    def __call__(self, method, path, token, body=None):
        self.calls.append((method, path, body))
        if path.endswith("/branches/claw-web-fichiers") and not self.branch:
            raise GitHubError("absente", 404)
        if path == "/repos/o/claw-sessions":
            return {"default_branch": "main"}
        if path.endswith("/git/ref/heads/main"):
            return {"object": {"sha": "abc"}}
        if method == "PUT" and self.refuse_put:
            raise GitHubError("refusé", 403)
        return {}


class ShotsTest(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.mkdtemp()
        self.full = os.path.join(self.folder, "1-phone-390x844.jpg")
        self.preview = os.path.join(self.folder, "1-phone-390x844-preview.jpg")
        for path in (self.full, self.preview):
            with open(path, "wb") as file:
                file.write(JPEG)

    def result(self):
        return json.dumps({"url": "file:///x/index.html", "problemsFound": 2, "captures": [
            {"screen": "phone 390x844", "problems": ["a", "b"], "file": self.full, "preview": self.preview}]})

    def entry(self, store):
        log = Journal(store=store)
        log.add({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "name": "Screenshot", "input": {"path": "index.html"}}]}})
        log.add({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_name": "Screenshot", "content": self.result()}]}})
        return log.events[-1]

    def test_a_capture_becomes_a_thumbnail_and_a_stored_picture(self):
        github = FakeGitHub()
        store = run.ShotStore(token="t", engine="o/claw-sessions", session="13", run_id="99", call=github)
        entry = self.entry(store)
        self.assertEqual(entry["i"], {"target": "index.html"})
        shot = entry["r"]["shots"][0]
        self.assertTrue(shot["t"].startswith("data:image/jpeg;base64,"))
        self.assertEqual(shot["p"], "uploads/session-13/99-1-phone-390x844.jpg")
        self.assertEqual((shot["screen"], shot["n"], entry["r"]["found"]), ("phone 390x844", 2, 2))
        # Le texte des problèmes, pour l'appli quand on touche la capture.
        self.assertEqual(shot["pb"], ["a", "b"])
        put = [call for call in github.calls if call[0] == "PUT"][0]
        self.assertEqual(put[1], "/repos/o/claw-sessions/contents/uploads/session-13/99-1-phone-390x844.jpg")
        self.assertEqual(put[2]["branch"], "claw-web-fichiers")

    def test_a_window_capture_is_stored_under_an_ascii_name(self):
        github = FakeGitHub()
        store = run.ShotStore(token="t", engine="o/claw-sessions", session="13", run_id="99", call=github)
        name = store(self.full, 'phone 390x844, window opened by button "Réserver"')
        self.assertEqual(name, "uploads/session-13/99-1-phone-390x844--window-opened-b.jpg")

    def test_the_files_branch_is_created_when_missing(self):
        github = FakeGitHub(branch=False)
        store = run.ShotStore(token="t", engine="o/claw-sessions", session="13", run_id="99", call=github)
        self.entry(store)
        created = [call for call in github.calls if call[0] == "POST"]
        self.assertEqual(created[0][2], {"ref": "refs/heads/claw-web-fichiers", "sha": "abc"})

    def test_without_write_access_the_thumbnail_stays(self):
        github = FakeGitHub(refuse_put=True)
        store = run.ShotStore(token="t", engine="o/claw-sessions", session="13", run_id="99", call=github)
        shot = self.entry(store)["r"]["shots"][0]
        self.assertEqual(shot["p"], "")
        self.assertTrue(shot["t"])
        # Refus noté : les captures suivantes ne réessaient pas.
        self.assertEqual(store(self.full, "desktop 1280x800"), "")
        self.assertEqual(len([call for call in github.calls if call[0] == "PUT"]), 1)

    def test_a_screenshot_is_named_in_a_few_words(self):
        self.assertEqual(run.describe("Screenshot", {"path": "docs/index.html"}), "Capture de la page : docs/index.html")


if __name__ == "__main__":
    unittest.main()
