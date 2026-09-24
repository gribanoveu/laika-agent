"""A thin client for the upstream API."""

from . import settings


def request_timeout(override=None):
    """The timeout for one request, in seconds."""
    return override if override is not None else settings.DEFAULT_TIMEOUT


def health_every():
    return settings.HEALTH_INTERVAL
