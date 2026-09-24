"""Checks orders.csv against SPEC.md, one line of output per row."""
import re
import sys

DAYS = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
CURRENCIES = {"EUR", "USD", "GBP"}


def is_leap(year):
    return year % 4 == 0 and year % 100 != 0


def valid_date(text):
    m = re.fullmatch(r"(\d{4})-(\d{2})-(\d{2})", text)
    if not m:
        return False
    year, month, day = map(int, m.groups())
    if not 1 <= month <= 12:
        return False
    last = 29 if month == 2 and is_leap(year) else DAYS[month - 1]
    return 1 <= day <= last


def valid_amount(text):
    return re.fullmatch(r"\d+\.\d{2}", text) is not None and float(text) > 0


def check_row(fields, seen):
    if len(fields) != 4:
        return "wrong number of fields"
    id_, date, amount, currency = fields
    if not id_.isdigit() or int(id_) <= 0:
        return "bad id"
    if id_ in seen:
        return "duplicate id"
    if not valid_date(date):
        return "bad date"
    if not valid_amount(amount):
        return "bad amount"
    if currency not in CURRENCIES:
        return "bad currency"
    return None


def main(path):
    seen, bad = set(), 0
    with open(path) as f:
        for n, line in enumerate(f, start=1):
            fields = line.rstrip("\n").split(",")
            problem = check_row(fields, seen)
            seen.add(fields[0])
            if problem:
                bad += 1
                print(f"row {n}: INVALID ({problem}): {line.strip()}")
            else:
                print(f"row {n}: ok")
    print(f"{n - bad} ok, {bad} invalid")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
