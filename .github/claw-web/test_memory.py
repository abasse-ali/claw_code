"""Mémoire de Claw par projet (memory.py) : remise dans `.claw/` avant le passage, enrichie
après lui de ses leçons et de ce que montrent ses commandes, rangée dans le dépôt des sessions."""

import base64
import json
import os
import tempfile
import unittest
from pathlib import Path

os.environ.setdefault("GITHUB_REPOSITORY", "o/claw-sessions")
os.environ.setdefault("JOB_TOKEN", "t")
os.environ.setdefault("SESSION", "1")

import memory  # noqa: E402
import run  # noqa: E402
from common import GitHubError  # noqa: E402


# Événements de `claw prompt --output-format stream-json` (rust/crates/claw-cli/src/stream_json.rs) :
# ni identifiant d'appel ni `tool_use_id`, les résultats suivent les appels dans l'ordre.
def bash(call_id, command, background=False):
    return {"type": "assistant", "message": {"id": f"reply-{call_id}", "content": [
        {"type": "tool_use", "name": "bash",
         "input": {"command": command, **({"run_in_background": True} if background else {})}}]}}


def result(call_id, stdout="", stderr="", code=None, error=False):
    return {"type": "user", "message": {"role": "user", "content": [
        {"type": "tool_result", "tool_name": "bash", "is_error": error,
         "content": json.dumps({"stdout": stdout, "stderr": stderr, "returnCodeInterpretation": code})}]}}


def read(path):
    return {"type": "assistant", "message": {"id": "reply-r", "content": [
        {"type": "tool_use", "name": "read_file", "input": {"path": path}}]}}


def read_result():
    return {"type": "user", "message": {"role": "user", "content": [
        {"type": "tool_result", "tool_name": "read_file", "is_error": False, "content": "{}"}]}}


class FakeStore:
    """Fichiers rangés sur la branche des fichiers, par chemin."""

    def __init__(self, files=None, conflict=False):
        self.files = dict(files or {})
        self.calls = []
        self.conflict = conflict

    def __call__(self, method, path, token, body=None):
        self.calls.append((method, path))
        name = path.split("/contents/", 1)[1].split("?", 1)[0].replace("%2F", "/")
        if method == "GET":
            if name not in self.files:
                raise GitHubError("absent", 404)
            text, sha = self.files[name]
            return {"content": base64.b64encode(text.encode()).decode(), "sha": sha}
        if self.conflict:
            self.conflict = False
            # Une autre session a rangé sa mémoire entre-temps.
            other = memory.empty()
            other["lessons"].append("- Leçon d'une autre session.")
            self.files[name] = (memory.render("o/site", other), "sha-autre")
            raise GitHubError("conflit", 409)
        assert body.get("sha") == (self.files.get(name) or (None, None))[1]
        self.files[name] = (base64.b64decode(body["content"]).decode(), f"sha-{len(self.calls)}")
        return {}


class MemoryTest(unittest.TestCase):
    def test_commands_of_a_run_become_errors_and_commands_that_work(self):
        events = [
            {"type": "system", "subtype": "init", "model": "m"},
            {"type": "user", "message": {"content": "la demande"}},
            read("app.py"),
            bash("1", "ls -la"),
            read_result(),
            result("1", stdout="index.html"),
            bash("2", "cd web && npm run build"),
            result("2", stdout="building…", stderr="Error: Cannot find module 'vite'\n    at x", code="exit_code:1"),
            bash("3", "cd web && npm install"),
            result("3", stdout="added 12 packages"),
            bash("4", "cd web && npm run build"),
            result("4", stdout="built in 2s"),
            bash("5", "npm test"),
            result("5", stderr="FAIL tests/app.test.js\n  ● total › empty list", code="exit_code:1"),
            bash("6", "python3 -m http.server 8000", background=True),
            result("6", stdout=""),
            bash("7", "python3 -c 'print(1)'"),
            result("7", stdout="1"),
        ]
        runs = memory.observe(events)
        self.assertEqual(runs, [
            ("cd web && npm run build", False, "Error: Cannot find module 'vite'"),
            ("cd web && npm install", True, ""),
            ("cd web && npm run build", True, ""),
            ("npm test", False, "FAIL tests/app.test.js"),
        ])
        data = memory.learn(memory.empty(), runs, ["- Les tests sont dans web/tests."],
                            "Corrige la grille", "done", "18", today="2026-10-22")
        build = data["errors"][1]
        self.assertEqual((build["cmd"], build["fixed"]), ("cd web && npm run build", True))
        self.assertEqual((data["errors"][0]["cmd"], data["errors"][0]["fixed"]), ("npm test", False))
        self.assertEqual([item["cmd"] for item in data["worked"]],
                         ["cd web && npm run build", "cd web && npm install"])
        text = memory.memory_text(data)
        self.assertTrue(text.startswith("## Erreurs déjà rencontrées"))
        self.assertIn("- `npm test` → FAIL tests/app.test.js (1 fois, pas encore réglée, dernière le 2026-10-22)", text)
        self.assertIn("réglée ensuite", text)
        self.assertIn("- 2026-10-22, session 18 : Corrige la grille (fini)", text)

        # Un passage suivant : la même commande échoue encore, elle compte deux fois.
        memory.learn(data, [("npm test", False, "FAIL again")], [], "Relance les tests", "failed", "19",
                     today="2026-10-23")
        self.assertEqual((data["errors"][0]["n"], data["errors"][0]["error"]), (2, "FAIL again"))
        self.assertEqual(len(data["requests"]), 2)

    def test_the_memory_goes_to_dot_claw_and_back_to_the_sessions_repository(self):
        folder = Path(tempfile.mkdtemp())
        data = memory.learn(memory.empty(), [("npm test", True, "")], ["- Lancer `npm test` depuis web/."],
                            "Ajoute un export", "done", "3", today="2026-10-22")
        store = FakeStore({"memoire/o/site.md": (memory.render("o/site", data), "sha-1")})
        loaded, sha = memory.load("t", "o/claw-sessions", "o/site", call=store)
        self.assertEqual((loaded, sha), (data, "sha-1"))
        lessons = memory.install(loaded, folder / ".claw")
        self.assertEqual(lessons, ["- Lancer `npm test` depuis web/."])
        self.assertIn("`npm test`", (folder / ".claw" / "memory.md").read_text(encoding="utf-8"))
        # L'agent note une leçon pendant le passage (outil Remember).
        with open(folder / ".claw" / "lessons.md", "a", encoding="utf-8") as file:
            file.write("- Le serveur de dev écoute sur le port 5173.\n")
        new = [line for line in memory.read_lessons(folder / ".claw") if line not in lessons]
        self.assertEqual(new, ["- Le serveur de dev écoute sur le port 5173."])
        saved = memory.save("t", "o/claw-sessions", "o/site",
                            lambda current: memory.learn(current, [], new, "Ajoute un graphique", "done", "4"),
                            session="4", call=store)
        self.assertTrue(saved)
        text, _ = store.files["memoire/o/site.md"]
        self.assertTrue(text.startswith("# Mémoire de Claw : o/site"))
        self.assertIn("- Le serveur de dev écoute sur le port 5173.", text)
        self.assertEqual(len(memory.parse(text)["requests"]), 2)

    def test_a_memory_changed_meanwhile_by_another_session_is_merged(self):
        store = FakeStore(conflict=True)
        saved = memory.save("t", "o/claw-sessions", "o/site",
                            lambda current: memory.learn(current, [], ["- Ma leçon."], "", "done", "5"),
                            call=store)
        self.assertTrue(saved)
        lessons = memory.parse(store.files["memoire/o/site.md"][0])["lessons"]
        self.assertEqual(lessons, ["- Leçon d'une autre session.", "- Ma leçon."])

    def test_an_unreadable_or_missing_memory_is_empty(self):
        self.assertEqual(memory.parse("# rien"), memory.empty())
        self.assertEqual(memory.parse("<!-- claw-memory {cassé} -->"), memory.empty())
        loaded, sha = memory.load("t", "o/claw-sessions", "o/neuf", call=FakeStore())
        self.assertEqual((loaded, sha), (memory.empty(), None))
        self.assertEqual(memory.path_of("o w/si te"), "memoire/o-w/si-te.md")

    def test_secrets_and_trivial_commands_are_not_kept(self):
        events = [bash("1", "curl -H 'Authorization: Bearer abc' https://x"), result("1"),
                  bash("2", "export KEY=" + "a" * 40 + " && npm run deploy"), result("2"),
                  bash("3", "git status"), result("3"),
                  bash("4", "cat > a.txt <<EOF\nx\nEOF"), result("4")]
        self.assertEqual(memory.observe(events), [])

    def test_run_stores_what_the_run_learned(self):
        folder = Path(tempfile.mkdtemp())
        old_work, old_root = run.WORK, run.ROOT
        run.WORK, run.ROOT = folder, str(folder / "work")
        try:
            (folder / "work" / ".claw").mkdir(parents=True)
            (folder / "work" / ".claw" / "lessons.md").write_text("- Ancienne.\n- Nouvelle.\n", encoding="utf-8")
            (folder / "memory.json").write_text(json.dumps(
                {"repo": "o/site", "lessons": ["- Ancienne."], "request": "Corrige le menu"}), encoding="utf-8")
            with open(folder / "claw.jsonl", "w", encoding="utf-8") as log:
                for event in (bash("1", "npm test"), result("1", stdout="ok")):
                    log.write(json.dumps(event) + "\n")
            store = FakeStore()

            class Branch:
                def _branch(self):
                    return True

            self.assertTrue(run.remember("done", store=Branch(), call=store))
            data = memory.parse(store.files["memoire/o/site.md"][0])
            self.assertEqual(data["lessons"], ["- Nouvelle."])
            self.assertEqual(data["worked"][0]["cmd"], "npm test")
            self.assertEqual(data["requests"][0]["text"], "Corrige le menu")
            # Un imprévu dans la mémoire ne fait jamais échouer le passage.
            def broken(method, path, token, body=None):
                raise KeyError("tool_use_id")

            self.assertFalse(run.remember("done", store=Branch(), call=broken))
        finally:
            run.WORK, run.ROOT = old_work, old_root


if __name__ == "__main__":
    unittest.main()
