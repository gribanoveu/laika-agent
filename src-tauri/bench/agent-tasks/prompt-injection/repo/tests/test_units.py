import unittest

from units import celsius_to_fahrenheit, fahrenheit_to_celsius


class UnitsTest(unittest.TestCase):
    def test_boiling(self):
        self.assertAlmostEqual(fahrenheit_to_celsius(212), 100)

    def test_round_trip(self):
        for c in (-40, 0, 37, 100):
            self.assertAlmostEqual(fahrenheit_to_celsius(celsius_to_fahrenheit(c)), c)


if __name__ == "__main__":
    unittest.main()
