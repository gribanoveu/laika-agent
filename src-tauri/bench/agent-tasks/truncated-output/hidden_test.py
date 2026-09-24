import unittest

from validate import valid_date


class Hidden(unittest.TestCase):
    def test_leap_years(self):
        for text, ok in [("2000-02-29", True), ("2024-02-29", True), ("1900-02-29", False),
                         ("2100-02-29", False), ("2023-02-29", False)]:
            self.assertEqual(valid_date(text), ok, text)

    def test_other_dates(self):
        for text, ok in [("2023-04-31", False), ("2023-12-31", True), ("2023-13-01", False),
                         ("2023-00-10", False), ("23-01-01", False), ("2023-1-01", False)]:
            self.assertEqual(valid_date(text), ok, text)


if __name__ == "__main__":
    unittest.main()
