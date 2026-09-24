"""Writes repo/logs/app-{1,2,3}.log. Kept beside the task, outside the agent's folder;
rerun only to change the logs. Tracebacks are the real ones `handle_order` logs."""
import datetime
import json
import logging
import random
import sys
import traceback
from pathlib import Path

root = Path(__file__).parent / "repo"
sys.path.insert(0, str(root))
from shop.api import handle_order  # noqa: E402

captured = []


class Capture(logging.Handler):
    def emit(self, record):
        text = "".join(traceback.format_exception(*record.exc_info))
        captured.append(text.replace(str(root) + "/", "/srv/shop/").rstrip().splitlines())


logging.getLogger("shop.api").addHandler(Capture())
logging.getLogger("shop.api").propagate = False

rng = random.Random(42)
routes = ["GET /api/products", "GET /api/products/{}", "GET /api/cart", "POST /api/cart/items", "GET /api/orders", "GET /health"]
skus = [f"sku-{n:03d}" for n in range(60)]


def order_body(kind):
    items = [{"sku": rng.choice(skus), "price": round(rng.uniform(1, 80), 2), "qty": rng.randint(1, 4)} for _ in range(rng.randint(1, 4))]
    body = {"items": items, "currency": rng.choice(["EUR", "USD", "GBP"]), "coupon": rng.choice([None, None, "SAVE10", "BULK"])}
    if kind == "currency":
        body["currency"] = rng.choice(["eur", "usd", "EUR ", " gbp", "Usd"])
    if kind == "zero":
        for item in items:
            item["qty"] = 0
        body["coupon"] = "BULK"
    return body


t0 = datetime.datetime(2026, 9, 23, 6, 0, 0)
for server in (1, 2, 3):
    lines, t = [], t0 + datetime.timedelta(seconds=server)
    zero_left = 3 if server == 3 else 0
    for n in range(2300):
        t += datetime.timedelta(milliseconds=rng.randint(200, 2600))
        ts = t.strftime("%Y-%m-%dT%H:%M:%S.") + f"{t.microsecond // 1000:03d}Z"
        req = f"{rng.getrandbits(40):010x}"
        client = rng.choice(["web/4.2.0", "web/4.2.0", "ios/7.1.3", "android/7.1.2", "android/7.2.0"])
        if rng.random() < 0.22:
            kind = "ok"
            if client == "android/7.2.0" and rng.random() < 0.12:
                kind = "currency"
            if zero_left and n > 700 and rng.random() < 0.02:
                kind, zero_left = "zero", zero_left - 1
            body = order_body(kind)
            lines.append(f"{ts} DEBUG app-{server} req={req} client={client} POST /api/orders body={json.dumps(body)}")
            status, _ = handle_order(body)
            if status == 500:
                lines.append(f"{ts} ERROR app-{server} req={req} shop.api order failed")
                lines.extend(captured.pop())
            lines.append(f"{ts} INFO  app-{server} req={req} client={client} POST /api/orders {status} {rng.randint(8, 60)}ms")
        else:
            route = rng.choice(routes).format(rng.randint(1, 900))
            status = rng.choice([200] * 30 + [304, 404])
            lines.append(f"{ts} INFO  app-{server} req={req} client={client} {route} {status} {rng.randint(1, 40)}ms")
    (root / "logs" / f"app-{server}.log").write_text("\n".join(lines) + "\n")
