import time
import unittest

from listes import paires_somme, sans_doublons


class Tests(unittest.TestCase):
    def test_sans_doublons(self):
        self.assertEqual(sans_doublons([3, 1, 3, 2, 1]), [3, 1, 2])

    def test_paires(self):
        self.assertEqual(paires_somme([1, 2, 3, 4], 5), 2)

    def test_rapide(self):
        debut = time.perf_counter()
        sans_doublons(list(range(15_000)) * 2)
        paires_somme(list(range(15_000)), 14_999)
        self.assertLess(time.perf_counter() - debut, 1.5, "trop lent sur 30 000 éléments")


if __name__ == "__main__":
    unittest.main()
