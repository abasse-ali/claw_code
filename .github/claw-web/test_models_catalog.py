"""Classement du catalogue des modèles gratuits (models_catalog.py), sans réseau."""

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(__file__))

import models_catalog as catalog  # noqa: E402


class CatalogTest(unittest.TestCase):
    def test_names_match_across_providers(self):
        self.assertEqual(catalog.base_name("nvidia:deepseek-ai/deepseek-v4.1-flash"), "deepseek-v4.1-flash")
        self.assertEqual(catalog.base_name("qwen/qwen3-coder:free"), "qwen3-coder")
        self.assertEqual(catalog.base_name("gemini:gemini-2.5-pro"), "gemini-2.5-pro")
        ranking = ["anthropic/claude-x", "deepseek/deepseek-v4.1-flash", "qwen/qwen3-coder"]
        self.assertEqual(catalog.programming_rank("nvidia:deepseek-ai/deepseek-v4.1-flash", ranking), 1)
        self.assertIsNone(catalog.programming_rank("nvidia:google/gemma-2b", ranking))

    def test_sizes_are_read_from_ids(self):
        self.assertEqual(catalog.total_billions("", "nvidia/nemotron-3-ultra-550b-a55b"), 550)
        self.assertEqual(catalog.active_billions("", "nvidia/nemotron-3-ultra-550b-a55b"), 55)

    def test_tested_models_that_call_tools_rank_first(self):
        ranking = ["qwen/qwen3-coder"]
        fast = catalog.entry("nvidia:qwen/qwen3-coder-480b-a35b-instruct", "Qwen3 Coder", "", ranking,
                             tools=True, probe={"ok": True, "tools": True, "seconds": 3.0})
        slow = catalog.entry("nvidia:meta/llama-3.3-70b-instruct", "Llama", "", ranking,
                             tools=True, probe={"ok": True, "tools": True, "seconds": 50.0})
        mute = catalog.entry("nvidia:google/gemma-3-27b-it", "Gemma", "", ranking,
                             tools=False, probe={"ok": True, "tools": False, "seconds": 2.0})
        broken = catalog.entry("nvidia:x/broken-200b", "Broken", "", ranking, tools=False,
                               probe={"ok": False, "tools": False, "seconds": 1.0, "error": "500"},
                               status="en échec : 500")
        order = [item["id"] for item in catalog.ranked([broken, mute, slow, fast])]
        self.assertEqual(order[0], fast["id"])
        self.assertEqual(order[-1], broken["id"])
        self.assertIn("outil appelé en 3 s", fast["reasons"])
        self.assertIn("répond sans appeler l'outil", mute["reasons"])

    def test_models_claw_set_aside_stay_unusable(self):
        step = catalog.entry("nvidia:stepfun-ai/step-5-preview", "Step 5", "", ["stepfun-ai/step-5-preview"],
                             tools=True, probe={"ok": True, "tools": True, "seconds": 6.0})
        self.assertFalse(step["tools"])
        self.assertIn("faux messages", step["status"])
        self.assertEqual(catalog.ranked([step])[0]["id"], step["id"])

    def test_gemini_paid_models_are_marked(self):
        result = {"ok": False, "tools": False, "seconds": 1,
                  "error": "429 Quota exceeded for metric generate_content_free_tier_requests, limit: 0"}
        self.assertEqual(catalog.status_of(result, "gemini"), "hors de l'offre gratuite")
        self.assertEqual(catalog.status_of({"ok": False, "error": "404 not found"}, "nvidia"),
                         "non ouvert à cette clé")

    def test_non_chat_models_are_not_tried(self):
        for name in ("nvidia/nv-embedqa-e5-v5", "nvidia/parakeet-ctc-1.1b", "google/deplot",
                     "nvidia/llama-3.1-nemoguard-8b-content-safety", "stabilityai/sdxl"):
            self.assertIsNotNone(catalog.NOT_CHAT.search(name), name)
        for name in ("qwen/qwen3-coder-480b-a35b-instruct", "nvidia/nemotron-3-super-120b-a12b",
                     "deepseek-ai/deepseek-v4.1-flash", "moonshotai/kimi-k2.6"):
            self.assertIsNone(catalog.NOT_CHAT.search(name), name)


if __name__ == "__main__":
    unittest.main()
