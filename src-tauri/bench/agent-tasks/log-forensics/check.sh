#!/bin/sh
# The fix holds for everything in the logs, the original tests still pass against
# it, and the agent added at least one test per cause (two).
set -e
for original in "$ORIG"/tests/test_*.py; do PYTHONPATH=. python3 "$original" 2>&1; done
python3 -m unittest discover -s tests 2>&1
PYTHONPATH=. python3 "$TASK/hidden_test.py" 2>&1
before=$(cat "$ORIG"/tests/*.py | grep -c "def test_")
after=$(cat tests/*.py | grep -c "def test_")
if [ "$after" -lt $((before + 2)) ]; then echo "tests: $before before, $after after — want a test per cause"; exit 1; fi
