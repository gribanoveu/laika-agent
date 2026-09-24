"""A recursive-descent evaluator for arithmetic expressions."""
import re

TOKEN = re.compile(r"\s*(?:(\d+\.\d*|\d*\.\d+|\d+)|(\*\*|[-+*/()]))")


def tokenize(text):
    tokens, pos = [], 0
    text = text.rstrip()
    while pos < len(text):
        m = TOKEN.match(text, pos)
        if not m:
            raise SyntaxError(f"unexpected {text[pos:]!r}")
        number, op = m.groups()
        tokens.append(float(number) if number and "." in number else int(number) if number else op)
        pos = m.end()
    return tokens


class Parser:
    def __init__(self, tokens):
        self.tokens, self.i = tokens, 0

    def peek(self):
        return self.tokens[self.i] if self.i < len(self.tokens) else None

    def take(self):
        tok = self.peek()
        self.i += 1
        return tok

    # expr := term (('+' | '-') term)*
    def expr(self):
        left = self.term()
        while self.peek() in ("+", "-"):
            op = self.take()
            right = self.expr()
            left = left + right if op == "+" else left - right
        return left

    # term := unary (('*' | '/') unary)*
    def term(self):
        left = self.unary()
        while self.peek() in ("*", "/"):
            op = self.take()
            right = self.unary()
            left = left * right if op == "*" else left / right
        return left

    # unary := '-' unary | power
    def unary(self):
        if self.peek() == "-":
            self.take()
            return -self.unary()
        return self.power()

    # power := atom ('**' atom)*
    def power(self):
        left = self.atom()
        while self.peek() == "**":
            self.take()
            left = left ** self.atom()
        return left

    def atom(self):
        tok = self.take()
        if tok == "(":
            value = self.expr()
            if self.take() != ")":
                raise SyntaxError("expected )")
            return value
        if tok == "-":
            return -self.atom()
        if isinstance(tok, (int, float)):
            return tok
        raise SyntaxError(f"unexpected {tok!r}")


def calc(text):
    parser = Parser(tokenize(text))
    value = parser.expr()
    if parser.peek() is not None:
        raise SyntaxError(f"unexpected {parser.peek()!r}")
    return value
