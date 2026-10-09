import unittest

from intervalles import fusionner


class Tests(unittest.TestCase):
    def test_chevauchement(self):
        self.assertEqual(fusionner([(1, 4), (2, 6)]), [(1, 6)])

    def test_disjoints(self):
        self.assertEqual(fusionner([(1, 2), (5, 7)]), [(1, 2), (5, 7)])


if __name__ == "__main__":
    unittest.main()
