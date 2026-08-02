#!/bin/bash
# Integer type-declaration matrix: identical code with parameter/return hints.

set -e
set -o pipefail
export LC_ALL=C

cd "$(dirname "$0")/.."

echo "=== Building rphp (release, no PGO) ==="
cargo build --release 2>&1 | tail -1
echo ""

RPHP="./target/release/rphp"
PHP="php -n"
RUNS=5

BENCHMARKS=(
    "bench_type_function_untyped.php:Function untyped"
    "bench_type_function_params.php:Function int params"
    "bench_type_function_return.php:Function int return"
    "bench_type_function_typed.php:Function params+return"
    "bench_type_function_strict.php:Function strict typed"
    "bench_type_method_untyped.php:Method untyped"
    "bench_type_method_typed.php:Method params+return"
    "bench_type_scalar_function_untyped.php:Scalar function untyped"
    "bench_type_scalar_function_typed.php:Scalar function typed"
    "bench_type_scalar_method_untyped.php:Scalar method untyped"
    "bench_type_scalar_method_typed.php:Scalar method typed"
)

measure_best() {
    local engine="$1"
    local file="$2"
    local best=999999
    local result=""
    local raw elapsed run

    for ((run = 0; run < RUNS; run++)); do
        if [ "$engine" = "rphp" ]; then
            raw=$($RPHP "benches/$file")
        else
            raw=$($PHP "benches/$file")
        fi
        result="${raw%%|*}"
        elapsed="${raw##*|}"
        if awk "BEGIN { exit !(($elapsed) < ($best)) }"; then
            best="$elapsed"
        fi
    done

    MEASURED_RESULT="$result"
    MEASURED_BEST="$best"
}

printf "%-28s %12s %12s %10s\n" "Variant" "rphp" "PHP" "Ratio"
printf "%-28s %12s %12s %10s\n" "----------------------------" "------------" "------------" "----------"

for benchmark in "${BENCHMARKS[@]}"; do
    IFS=: read -r file label <<< "$benchmark"
    measure_best rphp "$file"
    rphp_result="$MEASURED_RESULT"
    rphp_time="$MEASURED_BEST"
    measure_best php "$file"
    php_result="$MEASURED_RESULT"
    php_time="$MEASURED_BEST"

    if [ "$rphp_result" != "$php_result" ]; then
        printf "%-28s OUTPUT MISMATCH: rphp='%s' php='%s'\n" "$label" "$rphp_result" "$php_result"
        exit 1
    fi

    ratio=$(awk "BEGIN { printf \"%.2fx\", ($rphp_time) / ($php_time) }")
    printf "%-28s %11.4fs %11.4fs %10s\n" "$label" "$rphp_time" "$php_time" "$ratio"
done

echo ""
echo "Ratio < 1.00x = rphp faster, > 1.00x = PHP faster"
echo "Best of $RUNS internal times; parsing and process startup are excluded."
