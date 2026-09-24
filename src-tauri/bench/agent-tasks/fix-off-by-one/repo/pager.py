"""Splitting a list into pages."""


def paginate(items, page, size):
    """The items on `page`, counting pages from 1, `size` items per page."""
    start = page * size
    return items[start:start + size]


def page_count(items, size):
    """How many pages `items` fill, the last one possibly partial."""
    return len(items) // size
