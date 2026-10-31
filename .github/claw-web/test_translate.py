"""Traduction en français des problèmes relevés sur les captures."""

import json
import os
import sys
import tempfile
import threading
import time
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import translate  # noqa: E402
from journal import Journal  # noqa: E402


class ParseTest(unittest.TestCase):
    def test_the_array_is_read_after_the_reasoning_and_without_long_dashes(self):
        answer = "<think>Je traduis [deux] textes.</think>\nVoici : [\"Le titre est coupé \u2014 en haut\", \"Bouton inactif\"]"
        self.assertEqual(translate.parse(answer, 2), ["Le titre est coupé - en haut", "Bouton inactif"])

    def test_a_wrapped_answer_is_read(self):
        # Un bloc de code, un objet autour du tableau, un raisonnement sans balise ouvrante.
        self.assertEqual(translate.parse('```json\n["un", "deux"]\n```', 2), ["un", "deux"])
        self.assertEqual(translate.parse('{"translations": ["un", "deux"]}', 2), ["un", "deux"])
        self.assertEqual(translate.parse('je traduis [x]</think>["un"]', 1), ["un"])

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

    def test_texts_go_in_small_batches_and_the_relay_takes_a_failed_one(self):
        """Vu le 29 octobre 2026 (essai C3) : chaque liste entière dépassait le délai."""
        nvidia, relay = [], []

        def first(texts):
            nvidia.append(list(texts))
            if "b7" in texts:
                raise TimeoutError()
            return [f"fr:{text}" for text in texts]

        def second(texts):
            relay.append(list(texts))
            return [f"gemini:{text}" for text in texts]

        translator = translate.Translator("cle", call=first, fallback=second)
        got = []
        texts = [f"b{number}" for number in range(9)]
        printed = []
        translator._report = printed.append
        translator(texts, got.append)
        translator.wait(5)
        self.assertEqual([len(batch) for batch in nvidia], [translate.BATCH, 9 - translate.BATCH])
        self.assertEqual(relay, [texts[translate.BATCH:]])
        self.assertEqual(got[0][:2], ["fr:b0", "fr:b1"])
        self.assertEqual(got[0][-1], "gemini:b8")
        self.assertEqual(printed, ["Traduction des problèmes impossible : TimeoutError"])

    def test_a_batch_nobody_translates_keeps_its_english_and_the_others_their_french(self):
        def call(texts):
            return None if "x" in texts else [f"fr:{text}" for text in texts]

        translator = translate.Translator("cle", call=call)
        translator._report = lambda line: None
        got = []
        texts = [f"t{number}" for number in range(translate.BATCH)] + ["x"]
        translator(texts, got.append)
        translator.wait(5)
        self.assertEqual(got, [[f"fr:t{number}" for number in range(translate.BATCH)] + ["x"]])

    def test_a_failed_translation_leaves_the_english(self):
        def call(texts):
            raise OSError("réseau")

        translator = translate.Translator("cle", call=call)
        got = []
        translator(["a"], got.append)
        translator.wait(5)
        self.assertEqual(got, [])


    def test_the_end_of_the_run_never_waits_for_a_queue_of_slow_translations(self):
        # Session #41 : onze traductions en échec en file, 13 minutes de plus à la fin.
        release = threading.Event()
        calls = []

        def call(texts):
            calls.append(list(texts))
            release.wait(5)
            return None

        translator = translate.Translator("cle", call=call)
        for index in range(6):
            translator([f"défaut {index}"], lambda _: None)
        started = time.monotonic()
        translator.wait(0.3)
        self.assertLess(time.monotonic() - started, 1)
        release.set()
        time.sleep(0.2)
        # Le premier appel était en cours ; les autres ne sont plus demandés.
        self.assertEqual(len(calls), 1)
        self.assertFalse(translator.worker is None or not translator.worker.daemon)


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
