import unittest

from commande import total_commande
from export import exporter
from facture import ligne_total
from tarifs import prix_ttc


class TestsCaches(unittest.TestCase):
    def test_defaut_inchange(self):
        self.assertEqual(prix_ttc(0.99, 0.2), 1.19)
        self.assertEqual(total_commande([(10.0, 2), (0.99, 1)]), 25.19)
        self.assertEqual(ligne_total([(10.0, 1)]), "Total : 12.00 EUR")
        self.assertEqual(exporter([(0.99, 3)]), ["0.99;3;1.19"])

    def test_prix_ttc_decimales(self):
        self.assertEqual(prix_ttc(0.99, 0.2, decimales=1), 1.2)
        self.assertEqual(prix_ttc(104.5, 0.2, decimales=0), 125.0)

    def test_total_decimales(self):
        # 1.2 x 3 = 3.6 avec une décimale (et non 1.19 x 3 = 3.57).
        self.assertAlmostEqual(total_commande([(0.99, 3)], decimales=1), 3.6)
        self.assertAlmostEqual(total_commande([(104.5, 2)], decimales=0), 250.0)

    def test_facture_decimales(self):
        self.assertEqual(ligne_total([(104.5, 2)], "JPY", decimales=0), "Total : 250 JPY")
        self.assertEqual(ligne_total([(0.99, 3)], decimales=1), "Total : 3.6 EUR")

    def test_export_decimales(self):
        self.assertEqual(exporter([(104.5, 2)], decimales=0), ["104.5;2;125.0"])
        self.assertEqual(exporter([(0.99, 3)], decimales=1), ["0.99;3;1.2"])


if __name__ == "__main__":
    unittest.main()
