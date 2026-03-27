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
# Diverse workload corpus — exercises hot production paths.
# Each script runs multiple times to build strong profile data.
# Priority: hot loops > call dispatch > value operations > cold paths.

RPHP="./target/release/rphp"

# 1. Hot loops & arithmetic (dominant in real workloads)
for i in 1 2 3; do
    $RPHP benches/bench_loop.php > /dev/null
    $RPHP benches/bench_nested_loops.php > /dev/null
done

# 2. Function/method dispatch (call path profiling)
for i in 1 2 3; do
    $RPHP benches/bench_fib.php > /dev/null
    $RPHP benches/bench_fib_method.php > /dev/null
done

# 3. String operations (COW, concat, append)
for i in 1 2 3; do
    $RPHP benches/bench_string.php > /dev/null
done

# 4. Array operations (COW, push, foreach)
for i in 1 2 3; do
    $RPHP benches/bench_array.php > /dev/null
    $RPHP benches/bench_foreach.php > /dev/null
done

# 5. JSON (stdlib mix — string parsing, array building)
for i in 1 2; do
    $RPHP benches/bench_json.php > /dev/null
done

# 6. Broad test suite coverage (cold/error paths — single pass)
cargo test --release 2>&1 | tail -1

echo "Training complete ($(ls "$PROFDATA_DIR"/*.profraw 2>/dev/null | wc -l | tr -d ' ') profiles)."

echo "=== Step 4/4: Optimized build ==="
"$LLVM_PROFDATA" merge -o "$PROFDATA_DIR/merged.profdata" "$PROFDATA_DIR"/*.profraw
RUSTFLAGS="-Cprofile-use=$PROFDATA_DIR/merged.profdata -Ctarget-cpu=native" cargo build --release 2>&1 | tail -1

echo ""
echo "=== Done ==="
echo "PGO-optimized binary: target/release/rphp"
