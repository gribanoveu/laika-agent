"""Order totals."""
from decimal import Decimal

RATES = {"EUR": Decimal("1"), "USD": Decimal("1.08"), "GBP": Decimal("0.86")}


def total(items, currency="EUR", coupon=None):
    """The order total in `currency`, rounded to cents."""
    subtotal = sum((Decimal(str(item["price"])) * item["qty"] for item in items), Decimal("0"))
    rate = RATES[currency]
    discount = coupon_discount(coupon, items, subtotal)
    return ((subtotal - discount) * rate).quantize(Decimal("0.01"))


def coupon_discount(coupon, items, subtotal):
    if not coupon:
        return Decimal("0")
    if coupon == "SAVE10":
        return subtotal * Decimal("0.10")
    if coupon == "BULK":
        count = sum(item["qty"] for item in items)
        average = subtotal / count
        percent = Decimal("0.10") if average < 10 else Decimal("0.05")
        return subtotal * percent
    raise ValueError(f"unknown coupon {coupon!r}")
