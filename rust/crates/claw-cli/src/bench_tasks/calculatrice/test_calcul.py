import unittest

from calcul import evaluer


class Tests(unittest.TestCase):
    def test_priorite(self):
        self.assertEqual(evaluer("2 + 3 * 4"), 14)

    def test_parentheses(self):
        self.assertEqual(evaluer("(2 + 3) * 4"), 20)

    def test_moins_unaire(self):
        self.assertEqual(evaluer("-3 + 2"), -1)


if __name__ == "__main__":
    unittest.main()
