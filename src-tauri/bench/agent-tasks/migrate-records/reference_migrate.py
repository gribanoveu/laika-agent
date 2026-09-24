"""The reference: what MIGRATION.md means. Kept outside the agent's folder."""
import json
import re
import sys
from datetime import datetime, timezone

PARTICLES = {"van", "von", "de", "der", "den", "da", "di", "le", "la"}
COUNTRIES = {"germany": "DE", "france": "FR", "russia": "RU", "united states": "US", "united kingdom": "GB"}


def split_name(name):
    name = " ".join((name or "").split())
    if not name:
        return "", ""
    if "," in name:
        last, first = name.split(",", 1)
        return " ".join(first.split()), " ".join(last.split())
    words = name.split(" ")
    if len(words) == 1:
        return words[0], ""
    for i in range(1, len(words)):
        if words[i].lower() in PARTICLES:
            return " ".join(words[:i]), " ".join(words[i:])
    return " ".join(words[:-1]), words[-1]


def signup_date(value):
    if value is None:
        return None
    if isinstance(value, int) or (isinstance(value, str) and value.isdigit()):
        return datetime.fromtimestamp(int(value), tz=timezone.utc).strftime("%Y-%m-%d")
    if re.fullmatch(r"\d{4}-\d{2}-\d{2}", value):
        return value
    m = re.fullmatch(r"(\d{2})/(\d{2})/(\d{4})", value)
    if m:
        return f"{m.group(3)}-{m.group(2)}-{m.group(1)}"
    return None


def country(value):
    if not value:
        return None
    v = value.strip()
    if re.fullmatch(r"[A-Za-z]{2}", v):
        return v.upper()
    return COUNTRIES.get(v.lower())


def tags(value):
    if value is None:
        return []
    items = value.split(",") if isinstance(value, str) else value
    return sorted({t.strip().lower() for t in items if t and t.strip()})


def migrate(records):
    best = {}
    for r in records:
        email = (r.get("email") or "").strip().lower()
        if not email:
            continue
        first, last = split_name(r.get("name"))
        out = {"id": r["id"], "first_name": first, "last_name": last, "email": email,
               "signup_date": signup_date(r.get("signup")), "country": country(r.get("country")),
               "tags": tags(r.get("tags"))}
        key = (out["signup_date"] is None, out["signup_date"] or "", out["id"])
        if email not in best or key < best[email][0]:
            best[email] = (key, out)
    return sorted((o for _, o in best.values()), key=lambda o: o["id"])


def main(src, dst):
    with open(src, encoding="utf-8") as f:
        records = [json.loads(line) for line in f if line.strip()]
    with open(dst, "w", encoding="utf-8") as f:
        for o in migrate(records):
            f.write(json.dumps(o, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main(*sys.argv[1:3])
