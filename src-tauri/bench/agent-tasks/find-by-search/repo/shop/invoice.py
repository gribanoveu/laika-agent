from dataclasses import dataclass
from decimal import Decimal

from .money import round2
from .pricing import line_total
from .tax import tax_on


@dataclass(frozen=True)
class Invoice:
    subtotal: Decimal
    tax: Decimal
    total: Decimal


def build_invoice(cart, customer):
    subtotal = sum((line_total(item, customer) for item in cart.items), Decimal("0"))
    tax = tax_on(subtotal, customer.country)
    return Invoice(subtotal=round2(subtotal), tax=round2(tax), total=round2(subtotal + tax))
