"""Commentaire de progression : ce que Claw compte faire, puis l'étape en cours (run.Progress)."""

import os
import unittest

os.environ.setdefault("GITHUB_REPOSITORY", "o/claw-sessions")
os.environ.setdefault("JOB_TOKEN", "t")
os.environ.setdefault("SESSION", "1")

import run  # noqa: E402


def reply(text, *tools):
    content = ([{"type": "text", "text": text}] if text else []) + [
        {"type": "tool_use", "name": name, "input": {"path": path}} for name, path in tools
    ]
    return {"type": "assistant", "message": {"content": content}}


class ProgressTest(unittest.TestCase):
    def test_the_plan_and_the_current_step_are_shown(self):
        progress = run.Progress()
        progress.event(reply("Je lis les gabarits,   puis je refais la page d'accueil.", ("read_file", "templates/index.html")))
        progress.event(reply("", ("read_file", "app.py")))
        progress.event(reply("Les gabarits sont lus : j'écris le nouveau style.", ("write_file", "static/style.css")))
        body = progress.body("running")
        self.assertIn("**Ce que Claw compte faire** : Je lis les gabarits, puis je refais la page d'accueil.", body)
        self.assertIn("**En ce moment** : Les gabarits sont lus : j'écris le nouveau style.", body)
        # Fini : le plan reste, l'étape en cours disparaît.
        done = progress.body("done")
        self.assertIn("Ce que Claw compte faire", done)
        self.assertNotIn("En ce moment", done)

    def test_a_reply_without_tools_is_not_a_plan(self):
        progress = run.Progress()
        progress.event(reply("Voici la réponse à votre question."))
        self.assertNotIn("Ce que Claw compte faire", progress.body("done"))

    def test_a_long_plan_is_cut_on_a_word(self):
        self.assertEqual(run.shorten("un deux trois quatre", 12), "un deux…")
        self.assertEqual(run.shorten("court", 12), "court")


if __name__ == "__main__":
    unittest.main()
