"""Who may call what, before and after: only export_reports × auditor may differ,
and every other handler's source is untouched."""
import importlib.util
import inspect
import os
import sys
from types import SimpleNamespace

# The original is imported from the fixture itself: leave no bytecode there.
sys.dont_write_bytecode = True


def load(root, name):
    sys.path.insert(0, root)
    try:
        for mod in [m for m in sys.modules if m == "api" or m.startswith("api.")]:
            del sys.modules[mod]
        module = importlib.import_module("api.handlers")
        return module
    finally:
        sys.path.remove(root)


def matrix(module):
    access, source = {}, {}
    for name, fn in inspect.getmembers(module, inspect.isfunction):
        if not name.startswith("handle_"):
            continue
        source[name] = inspect.getsource(fn)
        for role in ["admin", "manager", "auditor", "viewer"]:
            try:
                fn(SimpleNamespace(role=role), None)
                access[name, role] = True
            except Exception:
                access[name, role] = False
    return access, source


before, before_src = matrix(load(os.environ["ORIG"], "orig"))
after, after_src = matrix(load(os.environ["WORKSPACE"], "work"))
want = dict(before)
want["handle_export_reports", "auditor"] = True
problems = [f"{k}: {after.get(k)} (want {v})" for k, v in want.items() if after.get(k) != v]
problems += [f"{name} changed" for name in before_src
             if name != "handle_export_reports" and after_src.get(name) != before_src[name]]
problems += [f"{name} is new" for name in after_src if name not in before_src]
for p in problems:
    print(p)
sys.exit(1 if problems else 0)
