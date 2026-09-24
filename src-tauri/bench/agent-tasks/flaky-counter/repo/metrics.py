"""In-process request metrics, shared by the worker threads."""
import time
from collections import defaultdict


class Metrics:
    def __init__(self):
        self._counts = defaultdict(int)
        self._last_seen = {}
        self._flushed = {}

    def hit(self, route):
        """Counts one request to `route`."""
        count = self._counts[route]
        self._touch(route)
        self._counts[route] = count + 1

    def _touch(self, route):
        self._last_seen[route] = time.monotonic()

    def count(self, route):
        return self._counts[route]

    def last_seen(self, route):
        return self._last_seen.get(route)

    def take(self, route):
        """The route's count, reset to zero in the same step — the exporter calls
        this every few seconds, and no hit may be lost or counted twice."""
        count = self._counts.get(route, 0)
        self._mark_flushed(route)
        self._counts[route] = 0
        return count

    def _mark_flushed(self, route):
        self._flushed[route] = time.monotonic()

    def snapshot(self):
        """A copy of every count, safe to read while workers keep counting."""
        return dict(self._counts)

    def reset(self, route):
        self._counts.pop(route, None)
        self._last_seen.pop(route, None)
