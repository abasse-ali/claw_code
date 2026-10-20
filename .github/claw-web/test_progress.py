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

    def test_claw_s_separate_events_show_the_plan(self):
        # Le format réel de Claw : un événement par bloc (texte, puis chaque outil). Vu en
        # session : le plan n'était jamais montré.
        progress = run.Progress()
        progress.event(reply("Je lis le projet, puis j'ajoute la fonction."))
        # L'annonce se montre tout de suite, avant l'outil suivant.
        self.assertIn("**Ce que Claw compte faire** : Je lis le projet, puis j'ajoute la fonction.", progress.body("running"))
        progress.event(reply("", ("read_file", "stats.py")))
        progress.event(reply("", ("read_file", "app.py")))
        progress.event(reply("Le projet est lu : j'écris le test."))
        body = progress.body("running")
        self.assertIn("**Ce que Claw compte faire** : Je lis le projet, puis j'ajoute la fonction.", body)
        self.assertIn("**En ce moment** : Le projet est lu : j'écris le test.", body)
        progress.event(reply("", ("write_file", "test_stats.py")))
        progress.event(reply("La fonction et son test sont prêts."))
        # Fini : le dernier texte, sans outil après lui, est la réponse.
        done = progress.body("done")
        self.assertIn("Je lis le projet", done)
        self.assertNotIn("sont prêts", done)

    def test_a_reply_without_tools_is_not_a_plan(self):
        progress = run.Progress()
        progress.event(reply("Voici la réponse à votre question."))
        self.assertNotIn("Ce que Claw compte faire", progress.body("done"))

    def test_a_long_plan_is_cut_on_a_word(self):
        self.assertEqual(run.shorten("un deux trois quatre", 12), "un deux…")
        self.assertEqual(run.shorten("court", 12), "court")


if __name__ == "__main__":
    unittest.main()
