from dataclasses import dataclass, field
from decimal import Decimal


@dataclass(frozen=True)
class Customer:
    name: str
    country: str
    discount: Decimal = Decimal("0")


@dataclass(frozen=True)
class Item:
    sku: str
    unit_price: Decimal
    qty: int = 1


@dataclass
class Cart:
    items: list = field(default_factory=list)

    def add(self, item):
        self.items.append(item)
