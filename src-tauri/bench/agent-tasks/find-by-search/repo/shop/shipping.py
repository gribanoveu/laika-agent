from decimal import Decimal

from .regions import EU

FLAT = {"domestic": Decimal("4.90"), "eu": Decimal("9.90"), "world": Decimal("19.90")}


def shipping_for(country, home="DE"):
    if country == home:
        return FLAT["domestic"]
    if country in EU:
        return FLAT["eu"]
    return FLAT["world"]
