#!/usr/bin/env python3
"""Прогон мутаций: ломаем свой код по одной правке и смотрим, кто это заметит.

Метод описан в docs/11-mutation-testing.md — там же про то, что считать
мутацией и что делать с выжившей. Здесь — механика прогона.

    python3 scripts/mutate.py план.json [-k] [--only N]

План (запускать из корня репозитория):

    {
      "test": "cargo test resolve",          // команда, без оболочки
      "cwd": "src-tauri",                    // откуда её запускать
      "timeout": 600,                        // секунд на мутанта
      "mutants": [
        {"file": "src-tauri/src/services/ai_tools/resolve.rs",
         "find":  "if !p.starts_with(root)",
         "replace": "if false",
         "note": "снятая проверка выхода за корень"}
      ]
    }

`find` должен встречаться в файле ровно один раз — иначе прогон не начнётся.
Оригиналы лежат вне репозитория; путь печатается в начале, restore из них идёт
`cp`-ом, а не `git checkout` (в дереве бывают и другие правки).
"""

import json
import os
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.stdout.reconfigure(line_buffering=True)

CAUGHT, SURVIVED, HUNG, BROKEN = "пойман", "ВЫЖИЛ", "пойман (завис)", "НЕ СОБРАЛСЯ"


def run(cmd: list[str], cwd: Path, timeout: float | None) -> tuple[int, str]:
    """Запускает тесты своей группой процессов: по таймауту убивается всё дерево.

    Мутант умеет не только ронять тест, но и подвешивать его навсегда —
    достаточно испортить ожидание дочернего процесса."""
    p = subprocess.Popen(
        cmd, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        text=True, errors="replace", start_new_session=True,
    )
    try:
        out, _ = p.communicate(timeout=timeout)
        return p.returncode, out
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        out, _ = p.communicate()
        return -signal.SIGKILL, out


def classify(code: int, out: str) -> str:
    if code == -signal.SIGKILL:
        return HUNG
    # Несобравшийся мутант — не убитый: тесты его даже не видели. Пустой вывод
    # красных тестов легко принять за поимку, поэтому смотрим на текст сборки.
    if any(k in out for k in ("error[E", "error: could not compile", "Unhandled error between tests")):
        return BROKEN
    return CAUGHT if code != 0 else SURVIVED


def first_failure(out: str) -> str:
    """Строка провалившегося теста — «поймано», под которым не падает ни один
    тест, это баг прогона, а не убийство."""
    for line in out.splitlines():
        if "(fail)" in line or line.strip().endswith("FAILED") or "panicked at" in line:
            return line.strip()[:120]
    return ""


def main() -> int:
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    plan = json.loads(Path(sys.argv[1]).read_text())
    keep_going = "-k" in sys.argv
    only = int(sys.argv[sys.argv.index("--only") + 1]) if "--only" in sys.argv else None

    cmd = shlex.split(plan["test"])
    cwd = Path(plan.get("cwd", ".")).resolve()
    timeout = plan.get("timeout", 600)
    mutants = plan["mutants"]
    if only is not None:
        mutants = [mutants[only - 1]]

    for m in mutants:
        src = Path(m["file"])
        hits = src.read_text().count(m["find"])
        if hits != 1:
            sys.exit(f"{src}: «{m['find'][:60]}» встречается {hits} раз, нужно ровно один")

    backups = Path(tempfile.mkdtemp(prefix="mutate-"))
    print(f"оригиналы: {backups}")

    print(f"базовый прогон: {' '.join(cmd)} (в {cwd})")
    t = time.monotonic()
    code, out = run(cmd, cwd, timeout)
    if code != 0:
        print(out[-4000:])
        sys.exit("дерево красное до мутаций — сначала зелёный базовый прогон")
    print(f"базовый прогон зелёный, {time.monotonic() - t:.0f} c\n")

    results = []
    for i, m in enumerate(mutants, 1):
        src = Path(m["file"])
        bak = backups / f"{i}-{src.name}"
        shutil.copy2(src, bak)
        print(f"[{i}/{len(mutants)}] {m.get('note') or m['find'][:60]}  ({src})")
        try:
            src.write_text(src.read_text().replace(m["find"], m["replace"], 1))
            t = time.monotonic()
            verdict = classify(*(r := run(cmd, cwd, timeout)))
            why = first_failure(r[1]) if verdict == CAUGHT else ""
        finally:
            # Contents back, but the time now: copy2 restores the original's
            # mtime, older than the last mutant's build, and cargo would go on
            # testing that mutant after the run.
            shutil.copy2(bak, src)
            os.utime(src)
        print(f"    {verdict}, {time.monotonic() - t:.0f} c" + (f" — {why}" if why else ""))
        results.append((i, verdict, m.get("note", "")))
        if verdict != CAUGHT and not keep_going:
            print("\nостановлен на первом не-убийстве (-k чтобы дойти до конца)")
            break

    print("\n| # | итог | мутация |")
    print("|---|---|---|")
    for i, verdict, note in results:
        print(f"| {i} | {verdict} | {note} |")
    bad = [r for r in results if r[1] != CAUGHT]
    print(f"\nвсего {len(results)}, не поймано {len(bad)}")
    if not bad:
        shutil.rmtree(backups, ignore_errors=True)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
