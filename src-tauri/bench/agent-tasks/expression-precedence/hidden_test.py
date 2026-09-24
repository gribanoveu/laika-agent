"""calc(s) == eval(s), as the README promises — including what the visible tests never ask."""
import unittest

from calc import calc

CASES = [
    "2**3**2", "1-2-3", "10-4+3", "8/4/2", "2*3/4*5", "-2**2", "(-2)**2", "2**-1",
    "-2**-2", "2*-3", "-(-3)", "--3", "7-2*3", "2**2**-1", "1 - -1", "-(2+3)*4",
    "3**-1**2", "100/10/5-1", "2 * (3 + 4) ** 2", "-3**2*2",
]


class Hidden(unittest.TestCase):
    def test_matches_python(self):
        for text in CASES:
            with self.subTest(text=text):
                self.assertAlmostEqual(calc(text), eval(text))

    def test_rejects_garbage(self):
        for text in ["2 +", "(1", "1 2", "*3"]:
            with self.subTest(text=text), self.assertRaises(SyntaxError):
                calc(text)


if __name__ == "__main__":
    unittest.main()
