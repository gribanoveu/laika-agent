#!/usr/bin/env bash
# Освобождает диск, не выбрасывая тёплый кеш сборки.
#
# cargo никогда не чистит target/ сам: каждая пересборка кладёт новый
# артефакт с новым хешем, старый остаётся навсегда. За неделю активной
# работы debug/deps дорастает до десятков ГБ из 200k+ файлов, и сборка
# падает с «No space left on device».
#
# Удаляется только производное:
#   debug/incremental, release/  — кеш целиком (пересоздаётся сборкой)
#   deps, build, .fingerprint    — файлы, к которым не обращались DAYS дней
#   Cursor sandbox cargo-target  — отдельный кеш на каждую sandbox-сессию
#                                  в /private/var/folders, вне home
#
# Прунинг по mtime оставляет граф текущей сборки живым: cargo сам
# пересоберёт то, чего недосчитается. Полный сброс — `cargo clean`.
#
#   scripts/clean-build-cache.sh          # прунить старше 1 дня
#   scripts/clean-build-cache.sh 7        # старше 7 дней (осторожнее)
#   scripts/clean-build-cache.sh --dry-run

set -euo pipefail

DAYS=1
DRY_RUN=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    [0-9]*) DAYS="$arg" ;;
    *) echo "usage: $0 [days] [--dry-run]" >&2; exit 2 ;;
  esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$REPO_ROOT/src-tauri/target"

free_space() { df -h /System/Volumes/Data 2>/dev/null | tail -1 | awk '{print $4}'; }
size_of() { du -sh "$1" 2>/dev/null | cut -f1; }

run() {
  if [[ $DRY_RUN -eq 1 ]]; then
    echo "  [dry-run] $*"
  else
    "$@"
  fi
}

echo "Свободно до: $(free_space)"

for dir in "$TARGET/debug/incremental" "$TARGET/release"; do
  [[ -d "$dir" ]] || continue
  echo "Удаляю ${dir#"$REPO_ROOT"/} ($(size_of "$dir"))"
  run rm -rf "$dir"
done

for dir in deps build .fingerprint; do
  path="$TARGET/debug/$dir"
  [[ -d "$path" ]] || continue
  count=$(find "$path" -type f -mtime "+$DAYS" 2>/dev/null | wc -l | tr -d ' ')
  echo "Пруню debug/$dir: $count файлов старше $DAYS дн."
  # -delete, а не rm: список в 200k путей не влезает в аргументы команды.
  [[ $DRY_RUN -eq 1 ]] || find "$path" -type f -mtime "+$DAYS" -delete 2>/dev/null || true
done

# Кеш sandbox-сессий Cursor — по отдельной копии cargo-target на сессию,
# вне репозитория и вне home, поэтому его не видно ни в одном обычном du.
shopt -s nullglob
# macOS отдаёт TMPDIR со слешем на конце, /tmp — нет: снимаем оба случая.
TMP_ROOT="${TMPDIR:-/tmp}"
for cache in "${TMP_ROOT%/}"/cursor-sandbox-cache/*/cargo-target; do
  echo "Удаляю Cursor sandbox cargo-target ($(size_of "$cache"))"
  run rm -rf "$cache"
done
shopt -u nullglob

echo "Свободно после: $(free_space)"
[[ $DRY_RUN -eq 1 ]] && echo "(ничего не удалено — dry-run)"
exit 0
