# calc

A small arithmetic evaluator: numbers, `+ - * / **`, unary minus and parentheses.

Precedence and associativity are Python's, exactly: `calc(s)` must equal `eval(s)`
for every expression it accepts. `/` is true division.

```sh
python3 -m unittest discover -s tests
```
