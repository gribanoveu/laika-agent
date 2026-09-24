"""The rule from AGENTS.md: a new `- ` line under `## Unreleased`."""
import sys

lines = open("CHANGELOG.md", encoding="utf-8").read().splitlines()
start = lines.index("## Unreleased") + 1
end = next((i for i in range(start, len(lines)) if lines[i].startswith("## ")), len(lines))
entries = [l for l in lines[start:end] if l.startswith("- ")]
if not entries:
    print("CHANGELOG.md has no entry under ## Unreleased")
    sys.exit(1)
