"""Traduction en français des problèmes relevés sur les captures."""

import json
import os
import sys
import tempfile
import threading
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import translate  # noqa: E402
from journal import Journal  # noqa: E402


class ParseTest(unittest.TestCase):
    def test_the_array_is_read_after_the_reasoning_and_without_long_dashes(self):
        answer = "<think>Je traduis [deux] textes.</think>\nVoici : [\"Le titre est coupé \u2014 en haut\", \"Bouton inactif\"]"
        self.assertEqual(translate.parse(answer, 2), ["Le titre est coupé - en haut", "Bouton inactif"])

    def test_a_wrong_count_or_no_array_gives_nothing(self):
        self.assertIsNone(translate.parse('["un seul"]', 2))
        self.assertIsNone(translate.parse("Je ne sais pas.", 1))
        self.assertIsNone(translate.parse("[1, 2]", 2))


class TranslatorTest(unittest.TestCase):
    def test_each_text_is_translated_once_and_given_in_order(self):
        calls = []

        def call(texts):
            calls.append(list(texts))
            return [f"fr:{text}" for text in texts]

        translator = translate.Translator("cle", call=call)
        got = []
        translator(["a", "b", "a"], got.append)
        translator.wait(5)
        translator(["b", "c"], got.append)
        translator.wait(5)
        self.assertEqual(got, [["fr:a", "fr:b", "fr:a"], ["fr:b", "fr:c"]])
        self.assertEqual(calls, [["a", "b"], ["c"]])

    def test_a_failed_translation_leaves_the_english(self):
        def call(texts):
            raise OSError("réseau")

        translator = translate.Translator("cle", call=call)
        got = []
        translator(["a"], got.append)
        translator.wait(5)
        self.assertEqual(got, [])


class JournalTest(unittest.TestCase):
    def test_a_capture_gets_its_problems_in_french_and_the_journal_is_republished(self):
        with tempfile.TemporaryDirectory() as folder:
            preview = os.path.join(folder, "apercu.jpg")
            with open(preview, "wb") as file:
                file.write(b"\xff\xd8\xff" + b"0" * 100)
            # La traduction n'arrive qu'après la publication du journal (le test la libère).
            published = threading.Event()

            def call(texts):
                published.wait(5)
                return [f"en français : {text}" for text in texts]

            translator = translate.Translator("cle", call=call)
            log = Journal(translate=translator)
            log.add({"type": "assistant", "message": {"content": [
                {"type": "tool_use", "name": "Screenshot", "input": {"path": "index.html"}}]}})
            log.add({"type": "user", "message": {"content": [{"type": "tool_result", "tool_name": "Screenshot",
                     "content": json.dumps({"captures": [{"screen": "phone 390x844", "problems": ["Text is cut"],
                                                          "file": preview, "preview": preview}]})}]}})
            log.dirty = False
            self.assertNotIn("pf", log.events[-1]["r"]["shots"][0])
            published.set()
            translator.wait(5)
            shot = log.events[-1]["r"]["shots"][0]
            self.assertEqual(shot["pb"], ["Text is cut"])
            self.assertEqual(shot["pf"], ["en français : Text is cut"])
            self.assertTrue(log.dirty)


if __name__ == "__main__":
    unittest.main()
