import unittest

from devise import arrondir, convertir
from rapport import total_en_euros


class TestsCaches(unittest.TestCase):
    def test_arrondi_au_plus_proche(self):
        self.assertEqual(arrondir(2.678), 2.68)
        self.assertEqual(arrondir(2.671), 2.67)

    def test_negatif(self):
        self.assertEqual(arrondir(-1.239), -1.24)

    def test_decimales(self):
        self.assertEqual(arrondir(3.14159, 3), 3.142)
        self.assertEqual(arrondir(7.6, 0), 8.0)

    def test_convertir(self):
        self.assertEqual(convertir(12.34, 0.917), 11.32)

    def test_total_plusieurs_lignes(self):
        self.assertEqual(total_en_euros([12.34, 1.99, 5.55], 0.917), 18.23)


if __name__ == "__main__":
    unittest.main()
