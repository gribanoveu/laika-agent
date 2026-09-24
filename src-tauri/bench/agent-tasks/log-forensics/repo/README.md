# shop

`shop.api.handle_order(body)` prices an order: `(200, {"total": "12.34"})`, or `(400, {"error": ...})`
for a request the client got wrong. A 500 means a bug in this code — never the client's fault.

- `items`: `[{"sku": str, "price": number >= 0, "qty": int >= 0}, ...]`
- `currency`: ISO code — `EUR`, `USD`, `GBP` — case-insensitive, surrounding whitespace ignored.
  Anything else is a 400. Defaults to `EUR`.
- `coupon`: optional. `SAVE10` takes 10% off. `BULK` takes 5% off, and a further 5% when the
  average item price is under 10. An unknown coupon is a 400.

Tests: `python3 -m unittest discover -s tests`
