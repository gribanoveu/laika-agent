#!/bin/sh
set -e
cmp "$ORIG/data/orders.csv" data/orders.csv
python3 validate.py data/orders.csv > /dev/null
PYTHONPATH=. python3 "$TASK/hidden_test.py" 2>&1
