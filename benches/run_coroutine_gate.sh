#!/bin/sh

# Build two source trees from scratch and compare the permanent coroutine
# release benchmarks without reusing a possibly stale integration-test image.

set -eu

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    echo "usage: $0 CANDIDATE_ROOT BASELINE_ROOT [CPU]" >&2
    exit 2
fi

candidate_root=$(cd "$1" && pwd)
baseline_root=$(cd "$2" && pwd)
cpu=${3-}
pairs=${RPHP_COROUTINE_GATE_PAIRS:-20}
limit=${RPHP_COROUTINE_GATE_LIMIT:-1}

case $pairs in
    '' | *[!0-9]*)
        echo "RPHP_COROUTINE_GATE_PAIRS must be a positive even integer" >&2
        exit 2
        ;;
esac
if [ "$pairs" -le 0 ] || [ $((pairs % 2)) -ne 0 ]; then
    echo "RPHP_COROUTINE_GATE_PAIRS must be a positive even integer" >&2
    exit 2
fi
if [ -n "$cpu" ] && ! command -v taskset >/dev/null 2>&1; then
    echo "CPU pinning requested, but taskset is unavailable" >&2
    exit 2
fi

candidate_target=$(mktemp -d "${TMPDIR:-/tmp}/rphp-coroutine-candidate.XXXXXX")
baseline_target=$(mktemp -d "${TMPDIR:-/tmp}/rphp-coroutine-baseline.XXXXXX")
results=$(mktemp "${TMPDIR:-/tmp}/rphp-coroutine-results.XXXXXX")

cleanup() {
    rm -rf -- "$candidate_target" "$baseline_target"
    rm -f -- "$results"
}
trap cleanup EXIT HUP INT TERM

build_benchmark() {
    source_root=$1
    target_root=$2
    (
        cd "$source_root"
        CARGO_TARGET_DIR="$target_root" cargo test \
            --release \
            --features coroutines \
            --test e2e_coroutines \
            --no-run 1>&2
    )

    binary=
    for artifact in "$target_root"/release/deps/e2e_coroutines-*; do
        if [ -f "$artifact" ] && [ -x "$artifact" ]; then
            if [ -n "$binary" ]; then
                echo "multiple coroutine benchmark executables found in $target_root" >&2
                return 1
            fi
            binary=$artifact
        fi
    done
    if [ -z "$binary" ]; then
        echo "coroutine benchmark executable not found in $target_root" >&2
        return 1
    fi
    printf '%s\n' "$binary"
}

measure() {
    binary=$1
    test_name=$2
    if [ -n "$cpu" ]; then
        output=$(taskset -c "$cpu" "$binary" --ignored --exact "$test_name" --nocapture 2>&1)
    else
        output=$("$binary" --ignored --exact "$test_name" --nocapture 2>&1)
    fi
    value=$(printf '%s\n' "$output" | sed -n 's/.*(\([0-9][0-9.]*\) ns\/[^)]*).*/\1/p')
    if [ -z "$value" ]; then
        printf '%s\n' "$output" >&2
        echo "failed to parse $test_name measurement" >&2
        return 1
    fi
    printf '%s' "$value"
}

median() {
    test_name=$1
    order=$2
    column=$3
    awk -F '\t' -v test_name="$test_name" -v order="$order" -v column="$column" \
        '$1 == test_name && $3 == order { print $column }' "$results" |
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
candidate=$(build_benchmark "$candidate_root" "$candidate_target")
echo "building baseline from $baseline_root" >&2
baseline=$(build_benchmark "$baseline_root" "$baseline_target")

for test_name in \
    benchmark_one_million_php_suspend_resume_cycles \
    benchmark_one_million_bounded_channel_values \
    benchmark_stream_readiness_ping_pong
do
    pair=1
    while [ "$pair" -le "$pairs" ]; do
        if [ $((pair % 2)) -eq 1 ]; then
            candidate_value=$(measure "$candidate" "$test_name")
            baseline_value=$(measure "$baseline" "$test_name")
            order=candidate-first
        else
            baseline_value=$(measure "$baseline" "$test_name")
            candidate_value=$(measure "$candidate" "$test_name")
            order=baseline-first
        fi
        printf '%s\t%s\t%s\t%s\t%s\n' \
            "$test_name" "$pair" "$order" "$candidate_value" "$baseline_value" |
            tee -a "$results"
        pair=$((pair + 1))
    done
done

echo
echo "balanced mean of order-specific median ratios:"
failed=0
for test_name in \
    benchmark_one_million_php_suspend_resume_cycles \
    benchmark_one_million_bounded_channel_values \
    benchmark_stream_readiness_ping_pong
do
    candidate_first_candidate=$(median "$test_name" candidate-first 4)
    candidate_first_baseline=$(median "$test_name" candidate-first 5)
    baseline_first_candidate=$(median "$test_name" baseline-first 4)
    baseline_first_baseline=$(median "$test_name" baseline-first 5)
    balanced=$(awk -v cfc="$candidate_first_candidate" -v cfb="$candidate_first_baseline" \
        -v bfc="$baseline_first_candidate" -v bfb="$baseline_first_baseline" \
        'BEGIN { printf "%.6f", ((((cfc / cfb) - 1) + ((bfc / bfb) - 1)) / 2) * 100 }')
    printf '%-54s %+10.3f%%\n' "$test_name" "$balanced"
    if awk -v value="$balanced" -v limit="$limit" 'BEGIN { exit !(value > limit) }'; then
        failed=1
    fi
done

if [ "$failed" -ne 0 ]; then
    echo "regression limit exceeded (+${limit}%)" >&2
    exit 1
fi
