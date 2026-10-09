import unittest

from commande import total_commande
from export import exporter
from facture import ligne_total
from tarifs import prix_ttc


class Tests(unittest.TestCase):
    def test_prix_ttc(self):
        self.assertEqual(prix_ttc(10.0, 0.2), 12.0)

    def test_total(self):
        self.assertEqual(total_commande([(10.0, 2), (0.99, 1)]), 25.19)

    def test_facture(self):
        self.assertEqual(ligne_total([(10.0, 1)]), "Total : 12.00 EUR")

    def test_export(self):
        self.assertEqual(exporter([(0.99, 3)]), ["0.99;3;1.19"])

    def test_decimales(self):
        self.assertEqual(prix_ttc(104.5, 0.2, decimales=0), 125.0)


if __name__ == "__main__":
    unittest.main()
