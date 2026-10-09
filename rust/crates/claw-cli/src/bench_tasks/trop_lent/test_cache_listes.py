import time
import unittest

from listes import paires_somme, sans_doublons


class TestsCaches(unittest.TestCase):
    def test_ordre_conserve(self):
        self.assertEqual(sans_doublons(["b", "a", "b", "c", "a"]), ["b", "a", "c"])
        self.assertEqual(sans_doublons([]), [])

    def test_paires_avec_doublons(self):
        self.assertEqual(paires_somme([1, 1, 1], 2), 3)
        self.assertEqual(paires_somme([2, 2, 2, 2], 4), 6)
        self.assertEqual(paires_somme([0, 0], 0), 1)

    def test_paires_negatifs(self):
        self.assertEqual(paires_somme([-2, 2, 0, 0, 5], 0), 2)
        self.assertEqual(paires_somme([], 3), 0)
        self.assertEqual(paires_somme([3], 6), 0)

    def test_grandes_listes(self):
        debut = time.perf_counter()
        resultat = sans_doublons(list(range(200_000)) * 2)
        self.assertEqual(len(resultat), 200_000)
        self.assertEqual(resultat[:3], [0, 1, 2])
        self.assertEqual(paires_somme(list(range(200_000)), 199_999), 100_000)
        self.assertEqual(paires_somme([1] * 100_000, 2), 100_000 * 99_999 // 2)
        self.assertLess(time.perf_counter() - debut, 4.0)


if __name__ == "__main__":
    unittest.main()
