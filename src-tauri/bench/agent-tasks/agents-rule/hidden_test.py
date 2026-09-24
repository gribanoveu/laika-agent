import unittest

from slug import slugify


class Hidden(unittest.TestCase):
    def test_keeps_numbers(self):
        self.assertEqual(slugify("Top 10 tips"), "top-10-tips")

    def test_punctuation(self):
        self.assertEqual(slugify("Hello, World!"), "hello-world")

    def test_strips_dashes(self):
        self.assertEqual(slugify("  --2024 recap--  "), "2024-recap")


if __name__ == "__main__":
    unittest.main()
