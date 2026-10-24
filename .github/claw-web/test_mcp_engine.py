"""Essais du branchement des outils MCP dans le moteur : jeton passé par fichier puis effacé,
serveurs déclarés à Claw, messages des autres sessions et des routines, journal en français,
catalogue de l'appli.

    cd .github/claw-web && python -m unittest test_mcp_engine
"""

import json
import os
import stat
import tempfile
import unittest
from pathlib import Path
from unittest import mock

os.environ.setdefault("GITHUB_REPOSITORY", "o/claw-sessions")
os.environ.setdefault("JOB_TOKEN", "t")
os.environ.setdefault("SESSION", "7")

import journal  # noqa: E402
import mcp_server  # noqa: E402
import run  # noqa: E402
import setup  # noqa: E402
from mcp_server import Server  # noqa: E402

ISSUE = {"body": "Fais une appli de budget\n\n<!-- claw-web {\"repo\": \"o/r\"} -->"}
CATALOG = Path(__file__).resolve().parents[2] / "claw-web" / "mcp-tools.json"


class MessagesTest(unittest.TestCase):
    def test_peer_routine_and_follow_up_messages_are_not_the_users_words(self):
        peer = setup.incoming("<!-- claw-web:from-session 9 -->\n**Message de la session #9 de Claw**\n\nLe schéma a changé")
        self.assertTrue(peer.startswith("Message de la session #9 de Claw :\nLe schéma a changé"))
        self.assertIn("not typed by the user", peer)
        routine = setup.incoming("<!-- claw-web:routine trig_ab12 -->\n**Routine trig_ab12**\n\nFais le point")
        self.assertTrue(routine.startswith("Routine trig_ab12 :\nFais le point"))
        follow = setup.incoming("<!-- claw-web:watch sub_1 -->\n**Surveillance sub_1**\n\nActivity on the pull request")
        self.assertEqual(follow, "Suivi sub_1 :\nActivity on the pull request")
        self.assertEqual(setup.incoming("Un message ordinaire"), "Un message ordinaire")

    def test_conversation_labels_a_pending_peer_message(self):
        notes = [{"id": 5, "body": "<!-- claw-web:answer -->\n<!-- claw-web:answered 0 -->\nFait."},
                 {"id": 6, "body": "<!-- claw-web:from-session 9 -->\n**Message de la session #9 de Claw**\n\nRelis api.md"}]
        request, history, _, last, _ = setup.conversation(ISSUE, notes)
        self.assertTrue(request.startswith("Message de la session #9 de Claw :\nRelis api.md"))
        self.assertEqual(last, 6)
        self.assertIn("[Request]\nFais une appli de budget", history)

    def test_mcp_note(self):
        with mock.patch.object(setup, "REPO", "o/r"), mock.patch.object(setup, "BASE", "main"):
            note = setup.mcp_note("claw/session-3", True)
            self.assertIn("`mcp__github__*`: GitHub on o/r only", note)
            self.assertIn("never write files of claw/session-3 or main", note)
            self.assertIn("mcp__claw-remote__", note)
            self.assertIn("mcp__claw-docs__", note)
            self.assertNotIn("mcp__github__", setup.mcp_note("claw/session-3", False))


class TokenHandoffTest(unittest.TestCase):
    def test_token_file_is_private_then_read_and_deleted_before_the_agent(self):
        with tempfile.TemporaryDirectory() as folder:
            token_file = Path(folder) / ".mcp-token"
            state_file = Path(folder) / "github-state.json"
            state_file.write_text(json.dumps({"repo": "o/r", "base": "main", "branch": "claw/session-3"}))
            with mock.patch.object(setup, "MCP_TOKEN_FILE", token_file), mock.patch.object(setup, "REPO_TOKEN", "ghp_x"):
                setup.hand_token_to_mcp()
            self.assertEqual(stat.S_IMODE(token_file.stat().st_mode), 0o600)
            seen = {}

            def build(context):
                seen["context"] = context
                return [Server("github", []), Server("claw-remote", []), Server("claw-docs", [])]

            env = {"CLAW_CONFIG_HOME": str(Path(folder) / "home")}
            with mock.patch.object(run, "MCP_TOKEN_FILE", token_file), \
                    mock.patch.object(run, "STATE_FILE", state_file), mock.patch.object(run, "build", build), \
                    mock.patch.dict(os.environ, {"RUNNER": "claw-pc"}):
                host = run.start_mcp(env)
            try:
                self.assertFalse(token_file.exists())
                context = seen["context"]
                self.assertEqual((context.repo, context.base, context.branch, context.repo_token, context.runner),
                                 ("o/r", "main", "claw/session-3", "ghp_x", "claw-pc"))
                settings = json.loads((Path(folder) / "home" / "settings.json").read_text())
                self.assertEqual(sorted(settings["mcpServers"]), ["claw-docs", "claw-remote", "github"])
                server = settings["mcpServers"]["github"]
                self.assertEqual(server["type"], "http")
                self.assertTrue(server["url"].startswith("http://127.0.0.1:"))
                self.assertNotIn("ghp_x", json.dumps(settings))
            finally:
                host.stop()

    def test_runner_does_not_reach_the_agent(self):
        self.assertNotIn("RUNNER", run.agent_env({"RUNNER": "claw-pc", "PATH": "/bin"}))


class JournalTest(unittest.TestCase):
    def test_french_titles_for_mcp_calls(self):
        self.assertEqual(run.describe("mcp__claw-remote__send_message", {"session_id": "9", "message": "x"}),
                         "Sessions et routines : Écrire à une session #9")
        self.assertEqual(mcp_server.describe_call("mcp__github__issue_read", {"issue_number": 12, "method": "get"}),
                         "GitHub : " + mcp_server.tool_titles()[("github", "issue_read")] + " #12")
        self.assertIsNone(mcp_server.describe_call("mcp__other__thing", {}))
        entry = journal.Journal(lambda text: text)._input("mcp__claw-docs__create", {"object": "file"})
        self.assertTrue(entry["title"].startswith("Documents : "))


class CatalogTest(unittest.TestCase):
    def test_catalog_lists_the_three_servers(self):
        catalog = mcp_server.catalog()
        counts = {server["server"]: len(server["tools"]) for server in catalog["servers"]}
        self.assertEqual(counts, {"github": 54, "claw-remote": 28, "claw-docs": 8})
        for server in catalog["servers"]:
            for tool in server["tools"]:
                self.assertTrue(tool["title"] and tool["title"][0].isupper(), tool)

    def test_the_apps_catalog_is_up_to_date(self):
        # Après un changement d'outil : python3 mcp_server.py --catalog > ../../claw-web/mcp-tools.json
        self.assertEqual(json.loads(CATALOG.read_text(encoding="utf-8")), mcp_server.catalog())


if __name__ == "__main__":
    unittest.main()
