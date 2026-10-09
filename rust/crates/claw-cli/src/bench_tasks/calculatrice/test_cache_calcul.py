import unittest

from calcul import evaluer


class TestsCaches(unittest.TestCase):
    def test_gauche_a_droite(self):
        self.assertEqual(evaluer("10 - 4 - 3"), 3)
        self.assertEqual(evaluer("24 / 4 / 2"), 3)

    def test_priorites_melangees(self):
        self.assertEqual(evaluer("1 + 2 * 3 - 8 / 4"), 5)
        self.assertEqual(evaluer("2 * 3 + 4 * 5"), 26)

    def test_parentheses_imbriquees(self):
        self.assertEqual(evaluer("((1 + 2) * (3 + 4)) / 7"), 3)
        self.assertEqual(evaluer("2 * (3 + (4 - 1)) * 2"), 24)

    def test_moins_unaire(self):
        self.assertEqual(evaluer("2 * -(1 + 1)"), -4)
        self.assertEqual(evaluer("--3"), 3)
        self.assertEqual(evaluer("4 - -2"), 6)
        self.assertEqual(evaluer("-(-(2))"), 2)

    def test_decimaux(self):
        self.assertAlmostEqual(evaluer("0.5 + 1.25 * 2"), 3.0)

    def test_nombre_seul(self):
        self.assertEqual(evaluer(" 42 "), 42)

    def test_mal_forme(self):
        for texte in ["", "   ", "2 +", "(2 + 3", "2 + 3)", "2 3", "* 2", "()", "2 + * 3"]:
            with self.subTest(texte=texte):
                with self.assertRaises(ValueError):
                    evaluer(texte)

    def test_division_par_zero(self):
        with self.assertRaises(ZeroDivisionError):
            evaluer("1 / (2 - 2)")


if __name__ == "__main__":
    unittest.main()
