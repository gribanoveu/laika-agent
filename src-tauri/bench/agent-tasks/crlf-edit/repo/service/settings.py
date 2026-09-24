"""Settings for the sync service."""

# Seconds to wait for the upstream API before giving up.
DEFAULT_TIMEOUT = 30

# Seconds between health checks. Not a timeout.
HEALTH_INTERVAL = 30

# Retry policy for failed syncs.
RETRIES = {"attempts": 3, "backoff_seconds": 30}
