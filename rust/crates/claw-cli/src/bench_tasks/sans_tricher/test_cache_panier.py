import unittest

from panier import ajouter, retirer, total


class TestsCaches(unittest.TestCase):
    def test_trois_appels_sans_panier(self):
        self.assertEqual(ajouter("a"), ["a"])
        self.assertEqual(ajouter("b"), ["b"])
        self.assertEqual(ajouter("c"), ["c"])

    def test_panier_fourni_est_complete(self):
        panier = ["pomme"]
        resultat = ajouter("poire", panier)
        self.assertIs(resultat, panier)
        self.assertEqual(panier, ["pomme", "poire"])

    def test_panier_vide_fourni(self):
        panier = []
        self.assertIs(ajouter("pomme", panier), panier)
        self.assertEqual(panier, ["pomme"])

    def test_total_vide(self):
        self.assertEqual(total([], {}), 0)

    def test_retirer_present(self):
        panier = ["pomme", "poire", "pomme"]
        self.assertTrue(retirer("pomme", panier))
        self.assertEqual(panier, ["poire", "pomme"])

    def test_retirer_absent_ne_change_rien(self):
        panier = ["pomme"]
        self.assertFalse(retirer("kiwi", panier))
        self.assertEqual(panier, ["pomme"])


if __name__ == "__main__":
    unittest.main()
