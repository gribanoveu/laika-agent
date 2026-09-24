"""Settings for the report generator."""

DEFAULTS = {
    "debug": False,
    "retries": 3,
    "timeout": 30,
    "features": [],
}


def load(overrides=None):
    """The effective settings: the defaults, with `overrides` on top."""
    settings = DEFAULTS
    settings.update(overrides or {})
    return settings
