#!/bin/sh

# Build two source trees from scratch and compare representative default-runtime
# workloads. Both binaries execute the candidate tree's workload files so this
# gate measures runtime changes, not incidental benchmark-source differences.

set -eu

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    echo "usage: $0 CANDIDATE_ROOT BASELINE_ROOT [CPU]" >&2
    exit 2
fi

candidate_root=$(cd "$1" && pwd)
baseline_root=$(cd "$2" && pwd)
cpu=${3-}
pairs=${RPHP_RUNTIME_GATE_PAIRS:-20}
limit=${RPHP_RUNTIME_GATE_LIMIT:-1}
warmups=${RPHP_RUNTIME_GATE_WARMUPS:-4}
only=${RPHP_RUNTIME_GATE_ONLY:-}

case $pairs in
    '' | *[!0-9]*)
        echo "RPHP_RUNTIME_GATE_PAIRS must be a positive even integer" >&2
        exit 2
        ;;
esac
if [ "$pairs" -le 0 ] || [ $((pairs % 2)) -ne 0 ]; then
    echo "RPHP_RUNTIME_GATE_PAIRS must be a positive even integer" >&2
    exit 2
fi
case $warmups in
    '' | *[!0-9]*)
        echo "RPHP_RUNTIME_GATE_WARMUPS must be a non-negative integer" >&2
        exit 2
        ;;
esac
if [ -n "$cpu" ] && ! command -v taskset >/dev/null 2>&1; then
    echo "CPU pinning requested, but taskset is unavailable" >&2
    exit 2
fi
case $only in
    '' | bench_scalar_loop.php | bench_array.php | bench_string.php | \
        corpus_order_pipeline.php | corpus_ledger_pipeline.php) ;;
    *)
        echo "RPHP_RUNTIME_GATE_ONLY names an unknown workload" >&2
        exit 2
        ;;
esac

candidate_target=$(mktemp -d "${TMPDIR:-/tmp}/rphp-runtime-candidate.XXXXXX")
baseline_target=$(mktemp -d "${TMPDIR:-/tmp}/rphp-runtime-baseline.XXXXXX")
results=$(mktemp "${TMPDIR:-/tmp}/rphp-runtime-results.XXXXXX")

cleanup() {
    rm -rf -- "$candidate_target" "$baseline_target"
    rm -f -- "$results"
}
trap cleanup EXIT HUP INT TERM

build_runtime() {
    source_root=$1
    target_root=$2
    (
        cd "$source_root"
        CARGO_TARGET_DIR="$target_root" cargo build --release 1>&2
    )
    binary="$target_root/release/rphp"
    if [ ! -x "$binary" ]; then
        echo "runtime executable not found in $target_root" >&2
        return 1
    fi
    printf '%s\n' "$binary"
}

measure_once() {
    binary=$1
    workload=$2
    if [ -n "$cpu" ]; then
        output=$(taskset -c "$cpu" "$binary" "$workload" 2>&1)
    else
        output=$("$binary" "$workload" 2>&1)
    fi
    value=$(printf '%s\n' "$output" | awk -F '|' 'NF > 1 { value = $NF } END { print value }')
    if ! printf '%s\n' "$value" | awk '
        /^[0-9]+([.][0-9]+)?$/ { valid = 1 }
        END { exit !valid }
    '; then
        printf '%s\n' "$output" >&2
        echo "failed to parse elapsed time for $workload" >&2
        return 1
    fi
    printf '%s\n' "$value"
}

measure() {
    measure_binary=$1
    measure_workload_path=$2
    measure_repetitions=$3
    measure_repetition=1
    measure_total=0
    while [ "$measure_repetition" -le "$measure_repetitions" ]; do
        measure_value=$(measure_once "$measure_binary" "$measure_workload_path")
        measure_total=$(awk -v total="$measure_total" -v value="$measure_value" \
            'BEGIN { printf "%.12f", total + value }')
        measure_repetition=$((measure_repetition + 1))
    done
    printf '%s' "$measure_total"
}

median() {
    workload=$1
    order=$2
    column=$3
    awk -F '\t' -v workload="$workload" -v order="$order" -v column="$column" \
        '$1 == workload && $3 == order { print $column }' "$results" |
        sort -n |
        awk '
            { values[NR] = $1 }
            END {
                if (NR == 0) exit 1
                if (NR % 2 == 1) print values[(NR + 1) / 2]
                else printf "%.12f\n", (values[NR / 2] + values[NR / 2 + 1]) / 2
            }
        '
}

echo "building candidate from $candidate_root" >&2
candidate=$(build_runtime "$candidate_root" "$candidate_target")
echo "building baseline from $baseline_root" >&2
baseline=$(build_runtime "$baseline_root" "$baseline_target")

while read -r workload repetitions; do
    if [ -n "$only" ] && [ "$workload" != "$only" ]; then
        continue
    fi
    workload_path="$candidate_root/benches/$workload"
    warmup=1
    while [ "$warmup" -le "$warmups" ]; do
        if [ $((warmup % 2)) -eq 1 ]; then
            measure "$candidate" "$workload_path" "$repetitions" >/dev/null
            measure "$baseline" "$workload_path" "$repetitions" >/dev/null
        else
            measure "$baseline" "$workload_path" "$repetitions" >/dev/null
            measure "$candidate" "$workload_path" "$repetitions" >/dev/null
        fi
        warmup=$((warmup + 1))
    done
    pair=1
    while [ "$pair" -le "$pairs" ]; do
        if [ $((pair % 2)) -eq 1 ]; then
            candidate_value=$(measure "$candidate" "$workload_path" "$repetitions")
            baseline_value=$(measure "$baseline" "$workload_path" "$repetitions")
            order=candidate-first
        else
            baseline_value=$(measure "$baseline" "$workload_path" "$repetitions")
            candidate_value=$(measure "$candidate" "$workload_path" "$repetitions")
            order=baseline-first
        fi
        printf '%s\t%s\t%s\t%s\t%s\n' \
            "$workload" "$pair" "$order" "$candidate_value" "$baseline_value" |
            tee -a "$results"
        pair=$((pair + 1))
    done
done <<'WORKLOADS'
bench_scalar_loop.php 1
bench_array.php 8
bench_string.php 20
corpus_order_pipeline.php 1
corpus_ledger_pipeline.php 2
WORKLOADS

echo
echo "balanced mean of order-specific median ratios:"
failed=0
for workload in \
    bench_scalar_loop.php \
    bench_array.php \
    bench_string.php \
    corpus_order_pipeline.php \
    corpus_ledger_pipeline.php
do
    if [ -n "$only" ] && [ "$workload" != "$only" ]; then
        continue
    fi
    candidate_first_candidate=$(median "$workload" candidate-first 4)
    candidate_first_baseline=$(median "$workload" candidate-first 5)
    baseline_first_candidate=$(median "$workload" baseline-first 4)
    baseline_first_baseline=$(median "$workload" baseline-first 5)
    balanced=$(awk -v cfc="$candidate_first_candidate" -v cfb="$candidate_first_baseline" \
        -v bfc="$baseline_first_candidate" -v bfb="$baseline_first_baseline" \
        'BEGIN { printf "%.6f", ((((cfc / cfb) - 1) + ((bfc / bfb) - 1)) / 2) * 100 }')
    printf '%-54s %+10.3f%%\n' "$workload" "$balanced"
    if awk -v value="$balanced" -v limit="$limit" 'BEGIN { exit !(value > limit) }'; then
        failed=1
    fi
done

if [ "$failed" -ne 0 ]; then
    echo "regression limit exceeded (+${limit}%)" >&2
    exit 1
fi
