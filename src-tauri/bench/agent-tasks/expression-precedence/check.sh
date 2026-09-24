#!/bin/sh
set -e
# The original tests, as they were, against the new code — adding tests is fine,
# changing or deleting the originals is not.
for original in "$ORIG"/tests/test_*.py; do
    test -f "tests/$(basename "$original")" || { echo "tests/$(basename "$original") is gone"; exit 1; }
    PYTHONPATH=. python3 "$original" 2>&1
done
python3 -m unittest discover -s tests 2>&1
PYTHONPATH=. python3 "$TASK/hidden_test.py" 2>&1
