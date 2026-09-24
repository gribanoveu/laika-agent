"""The order endpoint."""
import logging

from . import pricing

log = logging.getLogger("shop.api")


def validate(body):
    items = body.get("items")
    if not isinstance(items, list):
        raise ValueError("items must be a list")
    for item in items:
        if not isinstance(item.get("qty"), int) or item["qty"] < 0:
            raise ValueError("qty must be a non-negative integer")
        if not isinstance(item.get("price"), (int, float)) or item["price"] < 0:
            raise ValueError("price must be a non-negative number")
    return items


def handle_order(body):
    try:
        items = validate(body)
        amount = pricing.total(items, body.get("currency", "EUR"), body.get("coupon"))
        return 200, {"total": str(amount)}
    except ValueError as e:
        return 400, {"error": str(e)}
    except Exception:
        log.exception("order failed")
        return 500, {"error": "internal error"}
