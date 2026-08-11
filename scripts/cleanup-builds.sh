#!/usr/bin/env bash
# Keep regeneratable RPHP build artifacts from filling local or CI disks.
#
# The workspace target is cleaned only after crossing the configured limit.
# Task-scoped candidate directories are removed only after they are stale;
# exact baseline directories are deliberately never selected automatically.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="$ROOT_DIR/target"
TMP_ROOT="${TMPDIR:-/tmp}"
THRESHOLD_GIB="${RPHP_BUILD_CLEAN_THRESHOLD_GIB:-20}"
STALE_DAYS="${RPHP_BUILD_STALE_DAYS:-1}"
rphp_cargo_bin="${RPHP_CARGO_BIN:-}"

case "$THRESHOLD_GIB" in
    '' | *[!0-9]*)
        echo "RPHP_BUILD_CLEAN_THRESHOLD_GIB must be a non-negative integer" >&2
        exit 2
        ;;
esac
case "$STALE_DAYS" in
    '' | *[!0-9]*)
        echo "RPHP_BUILD_STALE_DAYS must be a non-negative integer" >&2
        exit 2
        ;;
esac

if [ -d "$TARGET_DIR" ]; then
    target_kib=$(du -sk "$TARGET_DIR" | awk '{ print $1 }')
    limit_kib=$((THRESHOLD_GIB * 1024 * 1024))
    if [ "$target_kib" -ge "$limit_kib" ]; then
        if [ -z "$rphp_cargo_bin" ]; then
            if command -v cargo >/dev/null 2>&1; then
                rphp_cargo_bin=$(command -v cargo)
            elif [ -x /root/.cargo/bin/cargo ]; then
                rphp_cargo_bin=/root/.cargo/bin/cargo
            else
                echo "cargo executable not found; set RPHP_CARGO_BIN" >&2
                exit 1
            fi
        fi
        echo "cleaning workspace target ($((target_kib / 1024 / 1024)) GiB)"
        (cd "$ROOT_DIR" && "$rphp_cargo_bin" clean)
    else
        echo "keeping workspace target ($((target_kib / 1024)) MiB; limit ${THRESHOLD_GIB} GiB)"
    fi
fi

# `find` supplies fully resolved entries directly beneath the selected temp
# root. Only RPHP candidate naming schemes are eligible; baselines are kept.
find "$TMP_ROOT" -maxdepth 1 -type d \
    \( -name 'rphp-candidate-*' -o -name 'rphp-runtime-candidate.*' \) \
    -mtime "+$STALE_DAYS" -print0 |
    while IFS= read -r -d '' candidate_dir; do
        case "$candidate_dir" in
            "$TMP_ROOT"/rphp-candidate-* | "$TMP_ROOT"/rphp-runtime-candidate.*)
                echo "removing stale candidate $candidate_dir"
                rm -rf -- "$candidate_dir"
                ;;
        esac
    done
