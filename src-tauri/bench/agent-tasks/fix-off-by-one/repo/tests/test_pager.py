import unittest

from pager import page_count, paginate


class PagerTest(unittest.TestCase):
    def test_first_page(self):
        self.assertEqual(paginate(list(range(10)), 1, 3), [0, 1, 2])

    def test_last_partial_page(self):
        self.assertEqual(paginate(list(range(10)), 4, 3), [9])

    def test_page_count_rounds_up(self):
        self.assertEqual(page_count(list(range(10)), 3), 4)

    def test_page_count_exact(self):
        self.assertEqual(page_count(list(range(9)), 3), 3)


if __name__ == "__main__":
    unittest.main()
