#!/bin/bash
# Performance benchmark: rphp vs PHP
# Usage: ./benches/run_benchmarks.sh [--no-pgo]
#
# Default: PGO build (instrument → profile → rebuild).
# --no-pgo: plain release build (faster, for quick checks).
#
# Each benchmark outputs "result|elapsed_seconds" via internal microtime().
# This eliminates process startup from measurements.

set -e
set -o pipefail
export LC_ALL=C

cd "$(dirname "$0")/.."

PROFDATA_DIR="/tmp/pgo-data"
PROFDATA_MERGED="/tmp/pgo-merged.profdata"

if [ "$1" = "--no-pgo" ]; then
    echo "=== Building rphp (release, no PGO) ==="
    cargo build --release 2>&1 | tail -1
else
    RPHP_RUST_SYSROOT=$(rustc --print sysroot)
    LLVM_PROFDATA=$(find "$RPHP_RUST_SYSROOT/lib/rustlib" -type f -name llvm-profdata -print -quit 2>/dev/null || true)
    echo "=== PGO Step 1/3: Instrumented build ==="
    rm -rf "$PROFDATA_DIR" && mkdir -p "$PROFDATA_DIR"
    RUSTFLAGS="-Cprofile-generate=$PROFDATA_DIR" cargo build --release 2>&1 | tail -1

    echo "=== PGO Step 2/3: Collecting profiles (3 passes) ==="
    for pass in 1 2 3; do
        for f in benches/bench_*.php; do
            ./target/release/rphp "$f" > /dev/null 2>&1 || true
        done
    done
    echo "  Collected $(ls "$PROFDATA_DIR"/*.profraw 2>/dev/null | wc -l | tr -d ' ') profile(s)"

    echo "=== PGO Step 3/3: Optimized rebuild ==="
    if [ -z "$LLVM_PROFDATA" ]; then
        echo "ERROR: llvm-profdata not found. Run: rustup component add llvm-tools-preview"
        exit 1
    fi
    "$LLVM_PROFDATA" merge -o "$PROFDATA_MERGED" "$PROFDATA_DIR/" 2>&1
    cargo clean -p rphp 2>/dev/null || true
    RUSTFLAGS="-Cprofile-use=$PROFDATA_MERGED" cargo build --release 2>&1 | tail -1
    echo "  PGO build ready."
fi

echo ""
RUSTPHP="./target/release/rphp"
PHP="php -n"

# Verify PHP has xdebug disabled
echo ""
echo "PHP version: $(php -r 'echo PHP_VERSION;')"
echo "Xdebug: $($PHP -r 'echo extension_loaded("xdebug") ? "ACTIVE (warning!)" : "disabled";')"
echo ""

BENCHMARKS=(
    "bench_fib.php:Fibonacci(35) recursive"
    "bench_fib39.php:Fibonacci(39) recursive"
    "bench_calls.php:Call-heavy 5M iterations"
    "bench_loop.php:Loop 10M iterations"
    "bench_scalar_loop.php:Scalar quick loop 50M"
    "bench_scalar_call_loop.php:Scalar call quick loop 10M"
    "bench_scalar_function_tree.php:Scalar function tree 5M"
    "bench_scalar_call_branch_standalone.php:Standalone scalar branch calls 10M"
    "bench_branch_loop.php:Branched scalar loop 10M"
    "bench_modulo_branch_loop.php:Modulo branch loop 10M"
    "bench_binary_assign_loop.php:Binary assign loop 10M"
    "bench_scalar_expression_chain_loop.php:Scalar expression chain 10M"
    "bench_variable_multiply_add_loop.php:Variable multiply-add recurrence 100M"
    "bench_variable_multiply_add_independent_loop.php:Variable multiply-add independent 100M"
    "bench_variable_multiply_add_dual_loop.php:Dual variable multiply-add 100M"
    "bench_scalar_overlapping_lifetimes_loop.php:Overlapping scalar lifetimes 10M"
    "bench_scalar_recurrence_loop.php:Scalar recurrences 10M"
    "bench_conditional_recurrence_loop.php:Conditional recurrences 10M"
    "bench_carried_condition_recurrence_loop.php:Carried-condition recurrences 10M"
    "bench_conditional_composed_recurrence_loop.php:Conditional composed recurrence 10M"
    "bench_composed_recurrence_loop.php:Composed recurrence 10M"
    "bench_dependent_recurrence_loop.php:Dependent recurrence 10M"
    "bench_reverse_dependent_recurrence_loop.php:Reverse dependent recurrence 10M"
    "bench_packed_array_loop.php:Packed array reads 1M"
    "bench_packed_array_materialized_loop.php:Packed materialized reads 1M"
    "bench_hash_int_array_loop.php:Hash integer-key reads 1M"
    "bench_hash_int_array_materialized_loop.php:Hash materialized reads 1M"
    "bench_hash_constant_int_array_materialized_loop.php:Hash constant-int materialized 10M"
    "bench_hash_sparse_int_array_loop.php:Sparse hash integer reads 1M"
    "bench_hash_sparse_int_array_transform.php:Sparse hash transform 1M"
    "bench_hash_sparse_int_array_filter.php:Sparse hash filter 1M"
    "bench_hash_irregular_prefix_int_array_transform.php:Irregular-prefix hash transform 1M"
    "bench_hash_string_array_loop.php:Hash string-key reads 10M"
    "bench_hash_string_array_materialized_loop.php:Hash string materialized 10M"
    "bench_jit_mixed_string_hash_update.php:Mixed String/hash update 1M"
    "bench_string.php:String concat 200K"
    "bench_array.php:Array build+sum 500K"
    "bench_foreach.php:Foreach sum 500K"
    "bench_hash_foreach.php:Hash foreach sum 500K"
    "bench_nested_loops.php:Nested 1500x1500"
    "bench_nested_dependent_loops.php:Nested dependent bound 3000"
    "bench_nested_scalar_call.php:Nested scalar call 1500x1500"
    "bench_fib_method.php:Method fib(35) \$this"
    "bench_property.php:Property R/W 5M"
    "bench_method_chain.php:Method chain 5M"
    "bench_scalar_method.php:Scalar method 5M"
    "bench_scalar_method_native_loop.php:Scalar method native loop 10M"
    "bench_scalar_method_branch.php:Scalar method branch 10M"
    "bench_obj_dispatch.php:App-like obj dispatch 5M"
)

printf "%-30s %12s %12s %10s\n" "Benchmark" "rphp" "PHP" "Ratio"
printf "%-30s %12s %12s %10s\n" "------------------------------" "------------" "------------" "----------"

for bench_info in "${BENCHMARKS[@]}"; do
    IFS=: read -r file label <<< "$bench_info"

    # rphp: 3 runs, take best internal time
    best_rust=999999
    rust_result=""
    for i in 1 2 3; do
        raw=$($RUSTPHP "benches/$file" 2>/dev/null || echo "ERROR|0")
        result="${raw%%|*}"
        elapsed="${raw##*|}"
        rust_result="$result"
        if (( $(echo "$elapsed < $best_rust" | bc -l 2>/dev/null || echo 0) )); then
            best_rust=$elapsed
        fi
    done

    # PHP: 3 runs, take best internal time
    best_php=999999
    php_result=""
    for i in 1 2 3; do
        raw=$($PHP "benches/$file" 2>/dev/null || echo "ERROR|0")
        result="${raw%%|*}"
        elapsed="${raw##*|}"
        php_result="$result"
        if (( $(echo "$elapsed < $best_php" | bc -l 2>/dev/null || echo 0) )); then
            best_php=$elapsed
        fi
    done

    # Check correctness
    if [ "$rust_result" != "$php_result" ]; then
        printf "%-30s  OUTPUT MISMATCH: rphp='%s' php='%s'\n" "$label" "$rust_result" "$php_result"
        continue
    fi

    # Calculate ratio
    ratio=$(python3 -c "print(f'{float(\"$best_rust\") / float(\"$best_php\"):.2f}x')")

    printf "%-30s %11.4fs %11.4fs %10s\n" "$label" "$best_rust" "$best_php" "$ratio"
done

echo ""
echo "Ratio < 1.00x = rphp faster, > 1.00x = PHP faster"
echo "Times are internal (microtime), excluding process startup."
