# Users: v1 → v2

Both formats are JSON Lines: one object per line, UTF-8.

## v1

```json
{"id": 17, "name": "Doe, Jane", "email": " Jane.Doe@Example.COM ", "signup": "04/03/2021", "country": "de", "tags": "b, a"}
```

Any field but `id` may be missing or `null`.

## v2

```json
{"id": 17, "first_name": "Jane", "last_name": "Doe", "email": "jane.doe@example.com", "signup_date": "2021-03-04", "country": "DE", "tags": ["a", "b"]}
```

Keys in exactly this order. Records sorted by `id`. Non-ASCII written as is, not escaped.

## Rules

1. **name** → `first_name`, `last_name`.
   - `"Last, First"` (a comma): split at the first comma; both parts trimmed.
   - Otherwise the last word is the last name and everything before it the first name —
     except that the particles `van`, `von`, `de`, `der`, `den`, `da`, `di`, `le`, `la`
     (any case) belong to the last name, together with every word after them:
     `"Jane van der Berg"` → `"Jane"`, `"van der Berg"`. The first word is always part of
     the first name, even if it is one of those particles.
   - One word: it is the first name; `last_name` is `""`.
   - Missing, `null` or blank: both `""`.
   - Runs of whitespace count as one space; the result is trimmed.
2. **email**: trimmed and lowercased. A record without an email (missing, `null`, blank
   after trimming) is dropped.
3. Two records with the same email (after rule 2) are one user: keep the one with the
   earliest `signup_date`; if that is equal too, the lower `id`. A record without a
   `signup_date` loses to any record that has one.
4. **signup** → `signup_date`, `YYYY-MM-DD`:
   - already `YYYY-MM-DD`: as is;
   - `DD/MM/YYYY`: day first;
   - a Unix timestamp in seconds, as a number or as a string of digits: the UTC date;
   - missing or `null`: `null`.
5. **country**: a two-letter code in any case → upper case. One of these names, in any
   case → its code: Germany `DE`, France `FR`, Russia `RU`, United States `US`,
   United Kingdom `GB`. Anything else, missing or `null` → `null`.
6. **tags**: a comma-separated string or a list of strings → a list, each tag trimmed
   and lowercased, empty ones dropped, duplicates removed, sorted. Missing or `null` → `[]`.
