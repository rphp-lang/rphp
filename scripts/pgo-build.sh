#!/usr/bin/env bash
# Maximum-performance PGO build pipeline for rphp.
#
# Usage:
#   ./scripts/pgo-build.sh
#
# Produces an optimized, machine-specific binary at target/max-perf/rphp.
# The ordinary target/release/rphp binary is not changed.
set -euo pipefail

cd "$(dirname "$0")/.."

RUST_HOST="$(rustc -vV | awk '/^host:/{print $2}')"
RUST_LLVM_MAJOR="$(rustc -vV | awk -F'[:.]' '/^LLVM version:/{gsub(/ /, "", $2); print $2}')"
RUST_SYSROOT="$(rustc --print sysroot)"

# PGO data is compiler-version specific, so every build gets a fresh, private
# directory. It is removed only after the final binary has been linked.
PROFDATA_DIR="$(mktemp -d "${TMPDIR:-/tmp}/rphp-max-perf-pgo.XXXXXX")"
cleanup() {
    rm -rf -- "$PROFDATA_DIR"
}
trap cleanup EXIT

# Find llvm-profdata matching rustc's LLVM major version. A mismatched Apple
# LLVM tool can reject or misread profiles emitted by a Homebrew/rustup rustc.
LLVM_PROFDATA=""
find_llvm_profdata() {
    local candidate version
    for candidate in \
        "$RUST_SYSROOT/lib/rustlib/$RUST_HOST/bin/llvm-profdata" \
        "$(command -v llvm-profdata 2>/dev/null || true)" \
        "/opt/homebrew/opt/llvm/bin/llvm-profdata" \
        "/usr/local/opt/llvm/bin/llvm-profdata" \
        "$(xcrun --find llvm-profdata 2>/dev/null || true)"; do
        [ -n "$candidate" ] && [ -x "$candidate" ] || continue
        version="$($candidate --version 2>/dev/null || true)"
        if [[ "$version" == *"version $RUST_LLVM_MAJOR."* ]]; then
            LLVM_PROFDATA="$candidate"
            return
        fi
    done
}
find_llvm_profdata

if [ -z "$LLVM_PROFDATA" ]; then
    echo "ERROR: llvm-profdata matching rustc LLVM $RUST_LLVM_MAJOR was not found." >&2
    echo "Install Rust's llvm-tools component or a matching LLVM package." >&2
    exit 1
fi

BASE_RUSTFLAGS="-Ctarget-cpu=native"
RPHP="./target/max-perf/rphp"

echo "=== Maximum-performance RPHP build ==="
echo "rustc:          $(rustc --version)"
echo "llvm-profdata:  $LLVM_PROFDATA"
echo "CPU target:     native"
echo "Cargo profile:  max-perf (fat LTO, one codegen unit)"
echo ""

echo "=== Step 1/3: Instrumented max-perf build ==="
RUSTFLAGS="$BASE_RUSTFLAGS -Cprofile-generate=$PROFDATA_DIR" \
    cargo build --locked --profile max-perf

echo "=== Step 2/3: Representative training ==="
shopt -s nullglob
WORKLOADS=()
for workload in benches/bench_*.php benches/corpus_*.php; do
    # These diagnostic variants intentionally call the currently unsupported
    # gc_disable() function; they are not part of the supported benchmark set.
    [[ "$workload" == *_nogc.php ]] && continue
    WORKLOADS+=("$workload")
done
if [ "${#WORKLOADS[@]}" -eq 0 ]; then
    echo "ERROR: no PGO training workloads found." >&2
    exit 1
fi

for workload in "${WORKLOADS[@]}"; do
    echo "  $workload"
    "$RPHP" "$workload" > /dev/null
done

RAW_PROFILES=("$PROFDATA_DIR"/*.profraw)
if [ "${#RAW_PROFILES[@]}" -eq 0 ]; then
    echo "ERROR: instrumented workloads produced no PGO profiles." >&2
    exit 1
fi
echo "Training complete (${#WORKLOADS[@]} workloads, ${#RAW_PROFILES[@]} raw profiles)."

echo "=== Step 3/3: Profile-use max-perf build ==="
"$LLVM_PROFDATA" merge \
    -o "$PROFDATA_DIR/merged.profdata" \
    "${RAW_PROFILES[@]}"
RUSTFLAGS="$BASE_RUSTFLAGS -Cprofile-use=$PROFDATA_DIR/merged.profdata -Cllvm-args=-pgo-warn-missing-function" \
    cargo build --locked --profile max-perf

echo ""
echo "=== Done ==="
echo "PGO-optimized binary: target/max-perf/rphp"
