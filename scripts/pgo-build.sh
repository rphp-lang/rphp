#!/usr/bin/env bash
# PGO (Profile-Guided Optimization) build pipeline for rphp.
#
# Usage:
#   ./scripts/pgo-build.sh
#
# Produces an optimized release binary at target/release/rphp.
# Requires Xcode Command Line Tools (for llvm-profdata).
set -euo pipefail

PROFDATA_DIR="/tmp/rphp-pgo-data"
LLVM_PROFDATA="$(xcrun --find llvm-profdata 2>/dev/null || echo llvm-profdata)"

echo "=== Step 1/4: Clean ==="
rm -rf "$PROFDATA_DIR"
mkdir -p "$PROFDATA_DIR"

echo "=== Step 2/4: Instrumented build ==="
RUSTFLAGS="-Cprofile-generate=$PROFDATA_DIR" cargo build --release 2>&1 | tail -1

echo "=== Step 3/4: Training runs ==="
# Diverse workload corpus — don't over-fit to one benchmark.
# Recursive call-heavy
./target/release/rphp benches/bench_fib.php > /dev/null
# Method dispatch
./target/release/rphp benches/bench_fib_method.php > /dev/null
# Run the full test suite as broad coverage
cargo test --release 2>&1 | tail -1
echo "Training complete."

echo "=== Step 4/4: Optimized build ==="
"$LLVM_PROFDATA" merge -o "$PROFDATA_DIR/merged.profdata" "$PROFDATA_DIR"/*.profraw
RUSTFLAGS="-Cprofile-use=$PROFDATA_DIR/merged.profdata -Ctarget-cpu=native" cargo build --release 2>&1 | tail -1

echo ""
echo "=== Done ==="
echo "PGO-optimized binary: target/release/rphp"
