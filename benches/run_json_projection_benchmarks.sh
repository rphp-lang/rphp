#!/bin/bash
# Four-mode benchmark for the guarded invariant json_decode projection slice.
# Usage: ./benches/run_json_projection_benchmarks.sh [runs]

set -e
set -o pipefail
export LC_ALL=C

cd "$(dirname "$0")/.."

RUNS="${1:-7}"
NO_JIT_TARGET="target/json-projection-no-jit"
JIT_TARGET="target/json-projection-jit"

echo "=== Building max-perf RPHP without native JIT ==="
RUSTFLAGS="-C target-cpu=native" cargo build --profile max-perf --target-dir "$NO_JIT_TARGET" 2>&1 | tail -1
echo "=== Building max-perf RPHP with native JIT ==="
RUSTFLAGS="-C target-cpu=native" cargo build --profile max-perf --features jit-prototype --target-dir "$JIT_TARGET" 2>&1 | tail -1

RPHP_NO_JIT="./$NO_JIT_TARGET/max-perf/rphp"
RPHP_JIT="./$JIT_TARGET/max-perf/rphp"
PHP_NO_JIT=(php -dopcache.enable_cli=0 -dopcache.jit_buffer_size=0 -dopcache.jit=off)
PHP_JIT=(php -dopcache.enable_cli=1 -dopcache.jit_buffer_size=100M -dopcache.jit=tracing)

median() {
    printf '%s\n' "$@" | sort -n | awk '
        { values[NR] = $1 }
        END {
            middle = int((NR + 1) / 2)
            if (NR % 2) print values[middle]
            else print (values[middle] + values[middle + 1]) / 2
        }'
}

run_mode() {
    local mode="$1"
    local workload="$2"
    case "$mode" in
        rphp-jit) "$RPHP_JIT" "$workload" ;;
        rphp-no-jit) "$RPHP_NO_JIT" "$workload" ;;
        php-jit) "${PHP_JIT[@]}" "$workload" ;;
        php-no-jit) "${PHP_NO_JIT[@]}" "$workload" ;;
    esac
}

benchmark_workload() {
    local label="$1"
    local workload="$2"
    local modes=(rphp-jit php-jit rphp-no-jit php-no-jit)
    local rphp_jit_times=()
    local php_jit_times=()
    local rphp_no_jit_times=()
    local php_no_jit_times=()
    local expected=""
    local run offset index mode raw result elapsed

    for ((run = 0; run < RUNS; run++)); do
        offset=$((run % 4))
        for ((index = 0; index < 4; index++)); do
            mode="${modes[$(((offset + index) % 4))]}"
            raw=$(run_mode "$mode" "$workload")
            result="${raw%%|*}"
            elapsed="${raw##*|}"
            if [ -z "$expected" ]; then
                expected="$result"
            elif [ "$result" != "$expected" ]; then
                echo "OUTPUT MISMATCH for $label: expected '$expected', $mode returned '$result'"
                exit 1
            fi
            case "$mode" in
                rphp-jit) rphp_jit_times+=("$elapsed") ;;
                rphp-no-jit) rphp_no_jit_times+=("$elapsed") ;;
                php-jit) php_jit_times+=("$elapsed") ;;
                php-no-jit) php_no_jit_times+=("$elapsed") ;;
            esac
        done
    done

    local rphp_jit php_jit rphp_no_jit php_no_jit
    rphp_jit=$(median "${rphp_jit_times[@]}")
    php_jit=$(median "${php_jit_times[@]}")
    rphp_no_jit=$(median "${rphp_no_jit_times[@]}")
    php_no_jit=$(median "${php_no_jit_times[@]}")
    printf "%-30s %11.6fs %11.6fs %11.6fs %11.6fs\n" \
        "$label" "$rphp_jit" "$rphp_no_jit" "$php_jit" "$php_no_jit"
}

echo ""
echo "PHP version: $(php -r 'echo PHP_VERSION;')"
PHP_JIT_STATUS=$("${PHP_JIT[@]}" -r '$status=opcache_get_status(false); echo $status["jit"]["on"] ? "on" : "off";')
if [ "$PHP_JIT_STATUS" != "on" ]; then
    echo "ERROR: requested PHP tracing JIT is not active"
    exit 1
fi
echo "PHP tracing JIT: active"
printf "%-30s %12s %12s %12s %12s\n" \
    "Benchmark" "RPHP JIT" "RPHP no JIT" "PHP JIT" "PHP no JIT"
printf "%-30s %12s %12s %12s %12s\n" \
    "------------------------------" "------------" "------------" "------------" "------------"

benchmark_workload "Invariant Long projections" benches/bench_json_projection.php
benchmark_workload "Invariant Double projection" benches/bench_json_projection_double.php
benchmark_workload "Invariant String strlen" benches/bench_json_projection_string.php
benchmark_workload "Changing-input control" benches/bench_json_projection_dynamic.php
benchmark_workload "Changing typed control" benches/bench_json_projection_typed_dynamic.php
benchmark_workload "Changing object control" benches/bench_json_decode_object_dynamic.php
benchmark_workload "Assoc decode width 4" benches/bench_json_decode_assoc_width4.php
benchmark_workload "Assoc decode width 8" benches/bench_json_decode_assoc_width8.php
benchmark_workload "Assoc decode width 12" benches/bench_json_decode_assoc_width12.php

echo ""
echo "Medians of $RUNS order-rotated internal timings; process startup is excluded."
