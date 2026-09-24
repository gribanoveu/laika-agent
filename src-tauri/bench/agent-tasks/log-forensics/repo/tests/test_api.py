import unittest

from shop.api import handle_order


class ApiTest(unittest.TestCase):
    def test_simple_order(self):
        body = {"items": [{"sku": "a", "price": 10, "qty": 2}], "currency": "EUR"}
        self.assertEqual(handle_order(body), (200, {"total": "20.00"}))

    def test_save10(self):
        body = {"items": [{"sku": "a", "price": 50, "qty": 1}], "currency": "USD", "coupon": "SAVE10"}
        self.assertEqual(handle_order(body), (200, {"total": "48.60"}))

    def test_bad_qty_is_a_client_error(self):
        status, _ = handle_order({"items": [{"sku": "a", "price": 1, "qty": -1}]})
        self.assertEqual(status, 400)


if __name__ == "__main__":
    unittest.main()
