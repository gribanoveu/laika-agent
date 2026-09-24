"""Builds the nightly report from settings."""


def build(settings):
    if settings["debug"]:
        # Debug runs also audit what they did.
        settings["features"].append("audit")
    lines = [f"retries={settings['retries']}", f"timeout={settings['timeout']}"]
    lines += [f"feature={name}" for name in settings["features"]]
    return "\n".join(lines)
