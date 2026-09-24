import unittest
from decimal import Decimal as D

from shop.cart import Cart, Customer, Item
from shop.invoice import build_invoice


def invoice(country, *prices, discount="0"):
    cart = Cart()
    for n, price in enumerate(prices):
        cart.add(Item(f"sku{n}", D(price)))
    return build_invoice(cart, Customer("c", country, D(discount)))


class Hidden(unittest.TestCase):
    def test_germany(self):
        inv = invoice("DE", "100.00")
        self.assertEqual((inv.subtotal, inv.tax, inv.total), (D("100.00"), D("19.00"), D("119.00")))

    def test_france_two_items(self):
        inv = invoice("FR", "10.00", "15.00")
        self.assertEqual((inv.subtotal, inv.tax, inv.total), (D("25.00"), D("5.00"), D("30.00")))

    def test_us_unchanged(self):
        inv = invoice("US", "100.00")
        self.assertEqual((inv.subtotal, inv.tax, inv.total), (D("100.00"), D("0.00"), D("100.00")))

    def test_discount_before_tax(self):
        inv = invoice("DE", "100.00", discount="0.10")
        self.assertEqual(inv.total, D("107.10"))


if __name__ == "__main__":
    unittest.main()
