#!/bin/sh
# Their migrate.py against the reference: on the visible file, on a hidden one,
# and the file they were asked to produce.
set -e
OUT=$(mktemp -d)
test -f migrate.py || { echo "no migrate.py"; exit 1; }
for input in "$ORIG/data/users_v1.jsonl" "$TASK/hidden_users_v1.jsonl"; do
    name=$(basename "$input")
    python3 migrate.py "$input" "$OUT/theirs-$name" 2>&1
    python3 "$TASK/reference_migrate.py" "$input" "$OUT/ref-$name"
    diff "$OUT/ref-$name" "$OUT/theirs-$name"
done
test -f data/users_v2.jsonl || { echo "data/users_v2.jsonl was not produced"; exit 1; }
diff "$OUT/ref-users_v1.jsonl" data/users_v2.jsonl
