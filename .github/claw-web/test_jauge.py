"""Essais de la jauge du juge de design (jauge.py) : pages trouvées, note lue dans la sortie de
`claw design-replay`, échec dit, tableau du résumé.

    cd .github/claw-web && python -m unittest test_jauge
"""

import json
import subprocess
import tempfile
import unittest
from pathlib import Path

import jauge


class Done:
    def __init__(self, returncode, stdout, stderr=""):
        self.returncode = returncode
        self.stdout = stdout
        self.stderr = stderr


class JaugeTest(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        for name in ("kit", "claude"):
            (self.root / "jauge" / name).mkdir(parents=True)
            (self.root / "jauge" / name / "index.html").write_text("<!doctype html>", encoding="utf-8")
        (self.root / "jauge" / "claude" / "demande.txt").write_text("Mon budget", encoding="utf-8")
        (self.root / "jauge" / "vide").mkdir()

    def test_the_pages_are_the_folders_with_an_index(self):
        self.assertEqual([page.parent.name for page in jauge.pages(self.root)], ["claude", "kit"])

    def test_a_page_is_graded_with_its_request_and_the_work_model(self):
        seen = {}

        def run(command, **_):
            seen["command"] = command
            verdict = {"mean": 8.3, "reached": True, "bar": 8, "scores": [["phone", 9], ["color", 8]],
                       "problems": 0, "judge": "nvidia:vl"}
            return Done(0, "Captures…\n" + json.dumps(verdict) + "\n")

        page = self.root / "jauge" / "claude" / "index.html"
        result = jauge.grade(page, "nvidia:nemotron", run)
        self.assertEqual(seen["command"], ["claw", "--model", "nvidia:nemotron", "design-replay", str(page),
                                           str(page.parent / "demande.txt")])
        self.assertEqual((result["page"], result["mean"], result["reached"]), ("claude", 8.3, True))

    def test_a_failure_is_said_with_the_end_of_its_message(self):
        failed = jauge.grade(self.root / "jauge" / "kit" / "index.html", "",
                             lambda *_, **__: Done(1, "", "aucun modèle gratuit ne lit les images"))
        self.assertEqual(failed["page"], "kit")
        self.assertIn("aucun modèle gratuit", failed["error"])

        def slow(*_, **__):
            raise subprocess.TimeoutExpired("claw", 1)

        self.assertIn("délai", jauge.grade(self.root / "jauge" / "kit" / "index.html", "", slow)["error"])

    def test_the_summary_shows_each_page_its_score_and_the_bar(self):
        table = jauge.summary([
            {"page": "kit", "mean": 8.43, "reached": True, "bar": 8, "scores": [["phone", 9]], "problems": 0,
             "judge": "vl"},
            {"page": "claw39", "mean": 5.7, "reached": False, "bar": 8, "scores": [["identity", 5]],
             "problems": 2, "judge": "vl"},
            {"page": "claude", "error": "juge indisponible"},
        ])
        self.assertIn("| kit | 8.4 | atteinte | phone=9 | 0 | vl |", table)
        self.assertIn("| claw39 | 5.7 | sous 8 | identity=5 | 2 | vl |", table)
        self.assertIn("| claude | échec |", table)


if __name__ == "__main__":
    unittest.main()
