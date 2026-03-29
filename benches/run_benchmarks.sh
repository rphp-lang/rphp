#!/bin/bash
# Performance benchmark: rphp vs PHP
# Usage: ./benches/run_benchmarks.sh
#
# Each benchmark outputs "result|elapsed_seconds" via internal microtime().
# This eliminates process startup from measurements.

set -e
export LC_ALL=C

cd "$(dirname "$0")/.."

# Build release (skip with --no-build for PGO builds)
if [ "$1" != "--no-build" ]; then
    echo "=== Building rphp (release) ==="
    cargo build --release 2>&1 | tail -1
else
    echo "=== Skipping build (--no-build) ==="
fi

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
    "bench_string.php:String concat 200K"
    "bench_array.php:Array build+sum 500K"
    "bench_foreach.php:Foreach sum 500K"
    "bench_nested_loops.php:Nested 1500x1500"
    "bench_fib_method.php:Method fib(35) \$this"
    "bench_property.php:Property R/W 5M"
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
