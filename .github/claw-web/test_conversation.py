"""Essais de la conversation donnée à l'agent (setup.py), relance comprise.

    cd .github/claw-web && python -m unittest test_conversation
"""

import os
import unittest

os.environ.setdefault("GITHUB_REPOSITORY", "o/claw-sessions")
os.environ.setdefault("JOB_TOKEN", "t")

import setup  # noqa: E402

ISSUE = {"body": "Fais une appli de budget\n\n<!-- claw-web {\"repo\": \"o/r\"} -->"}


def message(number, text):
    return {"id": number, "body": text}


def answer(number, mark, text):
    return {"id": number, "body": f"<!-- claw-web:answer -->\n<!-- claw-web:answered {mark} -->\n\n{text}\n\n---\n- [Job](u)"}


def progress(number):
    return {"id": number, "body": "<!-- claw-web:progress -->\n**Claw a fini**"}


class ConversationTest(unittest.TestCase):
    def test_first_run_is_the_task(self):
        request, history, _, last, retry = setup.conversation(ISSUE, [])
        self.assertEqual((request, history, last, retry), ("Fais une appli de budget", "", 0, False))

    def test_new_message(self):
        notes = [progress(1), answer(2, 0, "Appli faite."), message(10, "Ajoute un export CSV")]
        request, history, _, last, retry = setup.conversation(ISSUE, notes)
        self.assertEqual((request, last, retry), ("Ajoute un export CSV", 10, False))
        self.assertIn("[Your answer]\nAppli faite.", history)

    def test_retry_answers_the_last_message_again_without_the_earlier_attempts(self):
        notes = [progress(1), answer(2, 0, "Appli faite."), message(10, "Mets en ligne"),
                 progress(11), answer(12, 10, "Erreur du fournisseur."),
                 progress(13), answer(14, 10, "Je ne peux pas.")]
        request, history, _, last, retry = setup.conversation(ISSUE, notes)
        self.assertEqual((request, last, retry), ("Mets en ligne", 10, True))
        self.assertIn("Appli faite.", history)
        self.assertNotIn("Erreur du fournisseur.", history)
        self.assertNotIn("Je ne peux pas.", history)
        self.assertNotIn("Mets en ligne", history)

    def test_retry_of_the_task(self):
        notes = [progress(1), answer(2, 0, "Erreur du fournisseur.")]
        request, history, _, last, retry = setup.conversation(ISSUE, notes)
        self.assertEqual((request, history, last, retry), ("Fais une appli de budget", "", 0, True))

    def test_retry_of_a_batch_of_messages(self):
        notes = [answer(2, 0, "Appli faite."), message(10, "Ajoute un export"), message(11, "en CSV"),
                 answer(12, 11, "Export ajouté.")]
        request, history, _, last, retry = setup.conversation(ISSUE, notes)
        self.assertEqual((request, last, retry), ("Ajoute un export\n\nen CSV", 11, True))
        self.assertNotIn("Export ajouté.", history)

    def test_a_task_that_could_not_start_is_not_a_retry_nor_history(self):
        notes = [message(10, "Mets en ligne"),
                 {"id": 11, "body": "<!-- claw-web:answer -->\n<!-- claw-web:answered keep -->\n**La tâche n'a pas pu commencer.**"}]
        request, history, _, last, retry = setup.conversation(ISSUE, notes)
        self.assertEqual((request, last, retry), ("Mets en ligne", 10, False))
        self.assertNotIn("pas pu commencer", history)


if __name__ == "__main__":
    unittest.main()
