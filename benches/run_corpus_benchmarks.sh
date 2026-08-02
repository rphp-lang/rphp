#!/bin/bash
# Representative application workloads: rphp vs PHP without JIT.
# Kept separate from run_benchmarks.sh so microbenchmark history remains stable.

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

measure_best() {
    local engine="$1"
    local file="$2"
    local best=999999
    local result=""
    local raw elapsed run

    for ((run = 0; run < RUNS; run++)); do
        if [ "$engine" = "rphp" ]; then
            raw=$($RPHP "$file")
        else
            raw=$($PHP "$file")
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

printf "%-30s %12s %12s %10s\n" "Corpus" "rphp" "PHP" "Ratio"
printf "%-30s %12s %12s %10s\n" "------------------------------" "------------" "------------" "----------"

benchmark_corpus() {
    local label="$1"
    local file="$2"
    local rphp_result rphp_time php_result php_time ratio

    measure_best rphp "$file"
    rphp_result="$MEASURED_RESULT"
    rphp_time="$MEASURED_BEST"
    measure_best php "$file"
    php_result="$MEASURED_RESULT"
    php_time="$MEASURED_BEST"

    if [ "$rphp_result" != "$php_result" ]; then
        printf "%-30s  OUTPUT MISMATCH: rphp='%s' php='%s'\n" "$label" "$rphp_result" "$php_result"
        exit 1
    fi

    ratio=$(awk "BEGIN { printf \"%.2fx\", ($rphp_time) / ($php_time) }")
    printf "%-30s %11.4fs %11.4fs %10s\n" "$label" "$rphp_time" "$php_time" "$ratio"
}

benchmark_corpus "Order/service pipeline" benches/corpus_order_pipeline.php
benchmark_corpus "Typed order/service pipeline" benches/corpus_typed_order_pipeline.php
benchmark_corpus "Stateful ledger pipeline" benches/corpus_ledger_pipeline.php
benchmark_corpus "Typed stateful ledger" benches/corpus_typed_ledger_pipeline.php

echo ""
echo "Ratio < 1.00x = rphp faster, > 1.00x = PHP faster"
echo "Best of $RUNS internal times; parsing and process startup are excluded."
