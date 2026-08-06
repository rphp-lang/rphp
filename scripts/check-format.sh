#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

EXPECTED_RUSTFMT_VERSION="1.8.0"
ACTUAL_RUSTFMT_VERSION="$(rustfmt --version | awk '{version=$2; sub(/-.*/, "", version); print version}')"
if [[ "$ACTUAL_RUSTFMT_VERSION" != "$EXPECTED_RUSTFMT_VERSION" ]]; then
    echo "Expected rustfmt $EXPECTED_RUSTFMT_VERSION, found $(rustfmt --version)" >&2
    exit 1
fi

cargo fmt --all -- --check
