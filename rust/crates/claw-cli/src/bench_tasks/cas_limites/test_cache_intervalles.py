import unittest

from intervalles import fusionner


class TestsCaches(unittest.TestCase):
    def test_vide(self):
        self.assertEqual(fusionner([]), [])

    def test_se_touchent(self):
        self.assertEqual(fusionner([(1, 3), (3, 5)]), [(1, 5)])

    def test_imbriques(self):
        self.assertEqual(fusionner([(1, 10), (2, 3), (4, 8)]), [(1, 10)])

    def test_ordre_quelconque(self):
        self.assertEqual(fusionner([(8, 9), (1, 2), (2, 4), (6, 7)]), [(1, 4), (6, 7), (8, 9)])

    def test_entree_non_modifiee(self):
        donnees = [(5, 6), (1, 2)]
        fusionner(donnees)
        self.assertEqual(donnees, [(5, 6), (1, 2)])

    def test_resultat_en_tuples(self):
        resultat = fusionner([[1, 2], [2, 3]])
        self.assertEqual(resultat, [(1, 3)])
        self.assertIsInstance(resultat[0], tuple)

    def test_invalide(self):
        with self.assertRaises(ValueError):
            fusionner([(1, 2), (5, 4)])

    def test_negatifs_et_points(self):
        self.assertEqual(fusionner([(-5, -1), (-1, -1), (0, 0)]), [(-5, -1), (0, 0)])


if __name__ == "__main__":
    unittest.main()
