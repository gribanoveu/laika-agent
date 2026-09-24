import re


def slugify(text):
    """A URL-safe slug: lowercase words joined by single dashes."""
    return re.sub(r"[^a-z]+", "-", text.lower()).strip("-")
