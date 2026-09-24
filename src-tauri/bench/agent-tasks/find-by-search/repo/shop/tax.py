from decimal import Decimal

RATES = {
    "DE": Decimal("0.19"),
    "FR": Decimal("0.20"),
    "NL": Decimal("0.21"),
    "US": Decimal("0"),
}


def rate_for(country):
    return RATES.get(country, Decimal("0"))


def apply_tax(amount, country):
    """`amount` with the country's VAT added."""
    return amount * (1 + rate_for(country))


def tax_on(amount, country):
    """Only the VAT on `amount`."""
    return amount * rate_for(country)
