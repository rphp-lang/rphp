#!/bin/bash
# Performance benchmark: rphp vs PHP
# Usage: ./benches/run_benchmarks.sh

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
    "bench_fib.php:Fibonacci(30) recursive"
    "bench_fib39.php:Fibonacci(39) recursive"
    "bench_calls.php:Call-heavy 1M iterations"
    "bench_loop.php:Loop 1M iterations"
    "bench_string.php:String concat 10K"
    "bench_array.php:Array build+sum 50K"
    "bench_foreach.php:Foreach sum 50K"
    "bench_nested_loops.php:Nested loops 500x500"
    "bench_fib_method.php:Method fib(30) \$this"
    "bench_property.php:Property R/W 1M"
)

printf "%-30s %12s %12s %10s\n" "Benchmark" "rphp" "PHP 8.4" "Ratio"
printf "%-30s %12s %12s %10s\n" "------------------------------" "------------" "------------" "----------"

for bench_info in "${BENCHMARKS[@]}"; do
    IFS=: read -r file label <<< "$bench_info"

    # rphp timing (3 runs, take best)
    best_rust=999999
    rust_output=""
    for i in 1 2 3; do
        start=$(python3 -c "import time; print(time.time())")
        rust_output=$($RUSTPHP < "benches/$file" 2>/dev/null || echo "ERROR")
        end=$(python3 -c "import time; print(time.time())")
        elapsed=$(python3 -c "print(f'{$end - $start:.4f}')")
        if (( $(echo "$elapsed < $best_rust" | bc -l) )); then
            best_rust=$elapsed
        fi
    done

    # PHP timing (3 runs, take best)
    best_php=999999
    php_output=""
    for i in 1 2 3; do
        start=$(python3 -c "import time; print(time.time())")
        php_output=$($PHP "benches/$file" 2>/dev/null || echo "ERROR")
        end=$(python3 -c "import time; print(time.time())")
        elapsed=$(python3 -c "print(f'{$end - $start:.4f}')")
        if (( $(echo "$elapsed < $best_php" | bc -l) )); then
            best_php=$elapsed
        fi
    done

    # Check correctness
    if [ "$rust_output" != "$php_output" ]; then
        printf "%-30s  OUTPUT MISMATCH: rust='%s' php='%s'\n" "$label" "$rust_output" "$php_output"
        continue
    fi

    # Calculate ratio
    ratio=$(python3 -c "print(f'{$best_rust / $best_php:.2f}x')")

    printf "%-30s %10.4fs %10.4fs %10s\n" "$label" "$best_rust" "$best_php" "$ratio"
done

echo ""
echo "Ratio < 1.00x = rphp faster, > 1.00x = PHP faster"
