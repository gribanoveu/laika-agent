import unittest

from calc import calc


class CalcTest(unittest.TestCase):
    def test_basics(self):
        self.assertEqual(calc("1 + 2 * 3"), 7)
        self.assertEqual(calc("(1 + 2) * 3"), 9)
        self.assertEqual(calc("7 / 2"), 3.5)

    def test_power_is_right_associative(self):
        self.assertEqual(calc("2**3**2"), 512)


if __name__ == "__main__":
    unittest.main()
