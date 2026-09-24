"""Byte for byte: one number changed, every line still ends in CRLF, nothing else touched."""
import os
import sys

orig, work = os.environ["ORIG"], os.environ["WORKSPACE"]
failed = False
for name in ["settings.py", "client.py", "__init__.py"]:
    want = open(os.path.join(orig, "service", name), "rb").read()
    if name == "settings.py":
        want = want.replace(b"DEFAULT_TIMEOUT = 30\r\n", b"DEFAULT_TIMEOUT = 45\r\n")
    got = open(os.path.join(work, "service", name), "rb").read()
    if got != want:
        failed = True
        lf_only = got.count(b"\n") - got.count(b"\r\n")
        print(f"service/{name} differs from the expected bytes ({lf_only} bare LF line endings)")
sys.exit(1 if failed else 0)
