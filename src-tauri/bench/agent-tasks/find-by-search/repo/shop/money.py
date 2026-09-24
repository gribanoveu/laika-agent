from decimal import ROUND_HALF_UP, Decimal


def round2(amount):
    """Rounded to cents, halves away from zero, as the accountants want it."""
    return amount.quantize(Decimal("0.01"), rounding=ROUND_HALF_UP)
