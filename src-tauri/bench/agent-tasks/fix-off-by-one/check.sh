#!/bin/sh
# The tests the agent saw, untouched and passing, and a few it did not see.
set -e
diff -r -x __pycache__ "$ORIG/tests" tests
python3 -m unittest discover -s tests 2>&1
PYTHONPATH=. python3 "$TASK/hidden_test.py" 2>&1
