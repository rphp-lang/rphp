#!/usr/bin/env bash
# Keep regeneratable RPHP build artifacts from filling local or CI disks.
#
# The workspace target is cleaned after crossing its size limit or consuming
# the configured free-space reserve. Task-scoped candidate directories are
# removed only after they are stale; exact baseline directories are never
# selected automatically.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="$ROOT_DIR/target"
THRESHOLD_GIB="${RPHP_BUILD_CLEAN_THRESHOLD_GIB:-20}"
MIN_FREE_GIB="${RPHP_BUILD_MIN_FREE_GIB:-20}"
STALE_DAYS="${RPHP_BUILD_STALE_DAYS:-1}"
rphp_cargo_bin="${RPHP_CARGO_BIN:-}"

usage() {
    echo "usage: $0 [--threshold-gib N] [--min-free-gib N] [--stale-days N]" >&2
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --threshold-gib | --min-free-gib | --stale-days)
            if [ "$#" -lt 2 ]; then
                usage
                exit 2
            fi
            case "$1" in
                --threshold-gib) THRESHOLD_GIB=$2 ;;
                --min-free-gib) MIN_FREE_GIB=$2 ;;
                --stale-days) STALE_DAYS=$2 ;;
            esac
            shift 2
            ;;
        --help)
            usage
            exit 0
            ;;
        *)
            usage
            exit 2
            ;;
    esac
done

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
case "$MIN_FREE_GIB" in
    '' | *[!0-9]*)
        echo "RPHP_BUILD_MIN_FREE_GIB must be a non-negative integer" >&2
        exit 2
        ;;
esac

if [ -d "$TARGET_DIR" ]; then
    target_kib=$(du -sk "$TARGET_DIR" | awk '{ print $1 }')
    limit_kib=$((THRESHOLD_GIB * 1024 * 1024))
    available_kib=$(df -Pk "$ROOT_DIR" | awk 'NR == 2 { print $4 }')
    minimum_free_kib=$((MIN_FREE_GIB * 1024 * 1024))
    if [ "$target_kib" -ge "$limit_kib" ] || [ "$available_kib" -lt "$minimum_free_kib" ]; then
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
        echo "cleaning workspace target ($((target_kib / 1024)) MiB; $((available_kib / 1024)) MiB free)"
        (cd "$ROOT_DIR" && "$rphp_cargo_bin" clean)
    else
        echo "keeping workspace target ($((target_kib / 1024)) MiB; $((available_kib / 1024)) MiB free)"
    fi
fi

cleanup_candidates() {
    local tmp_root=${1%/}
    [ -d "$tmp_root" ] || return 0

    # `find` supplies resolved entries directly beneath a known temporary
    # root. Only RPHP candidate naming schemes are eligible; baselines stay.
    find "$tmp_root" -maxdepth 1 -type d \
        \( -name 'rphp-candidate-*' -o -name 'rphp-runtime-candidate.*' \
        -o -name 'rphp-coroutine-candidate.*' \) \
        -mtime "+$STALE_DAYS" -print0 |
        while IFS= read -r -d '' candidate_dir; do
            case "$candidate_dir" in
                "$tmp_root"/rphp-candidate-* | \
                    "$tmp_root"/rphp-runtime-candidate.* | \
                    "$tmp_root"/rphp-coroutine-candidate.*)
                    echo "removing stale candidate $candidate_dir"
                    rm -rf -- "$candidate_dir"
                    ;;
            esac
        done
}

cleanup_candidates /tmp
if [ -n "${TMPDIR:-}" ] && [ "${TMPDIR%/}" != /tmp ]; then
    cleanup_candidates "$TMPDIR"
fi
