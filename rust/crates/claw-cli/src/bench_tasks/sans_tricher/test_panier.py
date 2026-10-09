import unittest

from panier import ajouter, retirer, total


class Tests(unittest.TestCase):
    def test_paniers_independants(self):
        ajouter("pomme")
        self.assertEqual(ajouter("poire"), ["poire"])

    def test_total(self):
        self.assertEqual(total(["pomme", "pomme", "poire"], {"pomme": 2, "poire": 3}), 7)

    def test_prix_inconnu(self):
        with self.assertRaises(KeyError):
            total(["kiwi"], {"pomme": 2})

    def test_retirer_absent(self):
        self.assertFalse(retirer("kiwi", ["pomme"]))


if __name__ == "__main__":
    unittest.main()
