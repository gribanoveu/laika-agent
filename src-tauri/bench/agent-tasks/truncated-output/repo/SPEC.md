# orders.csv

One order per line: `id,date,amount,currency`.

- `id` — a positive integer, unique in the file.
- `date` — an ISO 8601 calendar date, `YYYY-MM-DD`, in the Gregorian calendar.
  Orders imported from the old system go back to 1998.
- `amount` — a decimal with exactly two places, greater than zero.
- `currency` — `EUR`, `USD` or `GBP`.
