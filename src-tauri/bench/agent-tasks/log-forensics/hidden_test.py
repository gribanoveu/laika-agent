"""No 500 for anything in the logs, and the README's contract for what caused them."""
import json
import os
import re
import unittest

from shop.api import handle_order

LOGS = os.path.join(os.environ["ORIG"], "logs")


def item(price, qty):
    return {"sku": "a", "price": price, "qty": qty}


class Hidden(unittest.TestCase):
    def test_nothing_from_the_logs_is_a_500(self):
        bodies = []
        for name in sorted(os.listdir(LOGS)):
            for line in open(os.path.join(LOGS, name)):
                m = re.search(r"POST /api/orders body=(.*)$", line)
                if m:
                    bodies.append(json.loads(m.group(1)))
        self.assertGreater(len(bodies), 1000)
        failed = [b for b in bodies if handle_order(b)[0] == 500]
        self.assertEqual(failed, [])

    def test_currency_is_case_and_space_insensitive(self):
        for code, total in [("eur", "20.00"), (" EUR ", "20.00"), ("Usd", "21.60"), ("gbp ", "17.20")]:
            with self.subTest(code=code):
                self.assertEqual(handle_order({"items": [item(10, 2)], "currency": code}), (200, {"total": total}))

    def test_unknown_currency_is_a_client_error(self):
        self.assertEqual(handle_order({"items": [item(10, 2)], "currency": "XYZ"})[0], 400)

    def test_bulk_on_nothing_is_not_a_500(self):
        """The README does not say which: nothing to price is either a 400 or a total of 0.00."""
        for items in ([item(10, 0), item(3, 0)], []):
            with self.subTest(items=items):
                status, body = handle_order({"items": items, "coupon": "BULK"})
                self.assertIn((status, body.get("total", "0.00")), [(400, "0.00"), (200, "0.00")])

    def test_bulk_still_prices(self):
        self.assertEqual(handle_order({"items": [item(5, 4)], "coupon": "BULK"}), (200, {"total": "18.00"}))
        self.assertEqual(handle_order({"items": [item(50, 2)], "coupon": "BULK"}), (200, {"total": "95.00"}))


if __name__ == "__main__":
    unittest.main()
