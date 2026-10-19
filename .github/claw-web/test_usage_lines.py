"""Pied d'une réponse : les requêtes du passage selon le fournisseur (common.usage_lines)."""

import unittest

from common import provider_of, usage_lines

QUOTA = {"used": 7, "limit": 1000, "remaining": 993, "run": 0}


class UsageLinesTest(unittest.TestCase):
    def test_provider_of_a_model(self):
        self.assertEqual(provider_of("gemini:gemini-3.8-flash"), "gemini")
        self.assertEqual(provider_of("nvidia:nvidia/nemotron-3-super-120b-a12b"), "nvidia")
        # Le même modèle chez OpenRouter, et « Auto » (vide).
        self.assertEqual(provider_of("nvidia/nemotron-3-super-120b-a12b:free"), "openrouter")
        self.assertEqual(provider_of(""), "openrouter")
        self.assertEqual(provider_of(None), "openrouter")

    def test_openrouter_keeps_its_daily_count(self):
        self.assertEqual(usage_lines("", 12, {**QUOTA, "run": 12}),
                         ["Requêtes gratuites : 12 pour ce passage · 7/1000 utilisées aujourd'hui (993 restantes)"])

    def test_nvidia_says_its_own_limits_not_openrouter(self):
        lines = usage_lines("nvidia:nvidia/nemotron-3-super-120b-a12b", 23, QUOTA)
        self.assertEqual(lines, ["NVIDIA (nvidia/nemotron-3-super-120b-a12b) : 23 requêtes dans ce passage · "
                                 "sans quota par jour, environ 40 requêtes par minute"])

    def test_gemini_gives_the_daily_limit_of_the_model(self):
        lines = usage_lines("gemini:gemini-3.8-flash", 9, QUOTA)
        self.assertEqual(len(lines), 1)
        self.assertIn("Google Gemini (gemini-3.8-flash) : 9 requêtes dans ce passage", lines[0])
        self.assertIn("20 requêtes par jour pour ce modèle", lines[0])
        # Modèle dont le quota n'a pas été relevé : rien d'inventé.
        self.assertIn("quota par jour et par modèle", usage_lines("gemini:gemini-2.5-pro", 3, None)[0])

    def test_openrouter_line_too_when_a_fallback_spent_its_requests(self):
        lines = usage_lines("gemini:gemini-3.8-flash", 30, {**QUOTA, "run": 11})
        self.assertEqual(len(lines), 2)
        self.assertTrue(lines[1].startswith("Requêtes gratuites : 11 pour ce passage"))

    def test_unknown_request_count_is_left_out(self):
        self.assertEqual(usage_lines("nvidia:openai/gpt-oss-20b", None, None),
                         ["NVIDIA (openai/gpt-oss-20b) : sans quota par jour, environ 40 requêtes par minute"])


if __name__ == "__main__":
    unittest.main()
