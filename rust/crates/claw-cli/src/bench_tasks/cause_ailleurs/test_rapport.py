import unittest

from rapport import resume, total_en_euros


class Tests(unittest.TestCase):
    def test_total(self):
        self.assertEqual(total_en_euros([12.34], 0.917), 11.32)

    def test_resume(self):
        self.assertEqual(resume([12.34, 20.0], 0.917), "2 ligne(s), total 29.66 EUR")


if __name__ == "__main__":
    unittest.main()
