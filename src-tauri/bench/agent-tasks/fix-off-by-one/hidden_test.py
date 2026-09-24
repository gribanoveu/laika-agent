import unittest

from pager import page_count, paginate


class Hidden(unittest.TestCase):
    def test_middle_page(self):
        self.assertEqual(paginate(list("abcdefg"), 2, 2), ["c", "d"])

    def test_past_the_end(self):
        self.assertEqual(paginate([1, 2], 3, 2), [])

    def test_empty(self):
        self.assertEqual(page_count([], 5), 0)


if __name__ == "__main__":
    unittest.main()
