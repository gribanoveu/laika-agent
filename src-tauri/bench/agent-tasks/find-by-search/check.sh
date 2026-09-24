#!/bin/sh
set -e
PYTHONPATH=. python3 "$TASK/hidden_test.py" 2>&1
