#!/bin/sh

# Compare a generics-enabled release candidate with an exact prior binary.
# Both binaries execute this tree's workloads so benchmark-source changes
# cannot be mistaken for runtime changes.

set -eu

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    echo "usage: $0 CANDIDATE_BINARY BASELINE_BINARY [CPU]" >&2
    exit 2
fi

candidate=$1
baseline=$2
cpu=${3-}
pairs=${RPHP_GENERICS_GATE_PAIRS:-20}
warmups=${RPHP_GENERICS_GATE_WARMUPS:-4}
only=${RPHP_GENERICS_GATE_ONLY:-}

for binary in "$candidate" "$baseline"; do
    if [ ! -x "$binary" ]; then
        echo "runtime executable not found: $binary" >&2
        exit 2
    fi
done
case $pairs in
    '' | *[!0-9]*)
        echo "RPHP_GENERICS_GATE_PAIRS must be a positive even integer" >&2
        exit 2
        ;;
esac
if [ "$pairs" -le 0 ] || [ $((pairs % 2)) -ne 0 ]; then
    echo "RPHP_GENERICS_GATE_PAIRS must be a positive even integer" >&2
    exit 2
fi
case $warmups in
    '' | *[!0-9]*)
        echo "RPHP_GENERICS_GATE_WARMUPS must be a non-negative integer" >&2
        exit 2
        ;;
esac
if [ -n "$cpu" ] && ! command -v taskset >/dev/null 2>&1; then
    echo "CPU pinning requested, but taskset is unavailable" >&2
    exit 2
fi
case $only in
    '' | bench_generics_method.php | bench_generics_method_native_loop.php | \
        bench_generics_method_nested_native_loop.php | \
        bench_generics_method_double_native_loop.php | \
        bench_generics_method_double_nested_native_loop.php | \
        bench_scalar_method_native_loop.php | \
        bench_scalar_method_nested_native_loop.php | \
        bench_scalar_method_double_native_loop.php | \
        bench_scalar_method_double_nested_native_loop.php | \
        bench_generics_method_turbofish.php | \
        bench_generics_default_omitted.php | bench_generics_default_explicit.php | \
        bench_generics_default_manual.php) ;;
    *)
        echo "RPHP_GENERICS_GATE_ONLY names an unknown workload" >&2
        exit 2
        ;;
esac

script_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
results=$(mktemp "${TMPDIR:-/tmp}/rphp-generics-results.XXXXXX")
trap 'rm -f -- "$results"' EXIT HUP INT TERM

measure() {
    measure_binary=$1
    measure_workload=$2
    if [ -n "$cpu" ]; then
        measure_output=$(taskset -c "$cpu" "$measure_binary" "$measure_workload" 2>&1)
    else
        measure_output=$("$measure_binary" "$measure_workload" 2>&1)
    fi
    measure_value=$(printf '%s\n' "$measure_output" | awk -F '|' 'NF > 1 { value = $NF } END { print value }')
    if ! printf '%s\n' "$measure_value" | awk '
        /^[0-9]+([.][0-9]+)?$/ { valid = 1 }
        END { exit !valid }
    '; then
        printf '%s\n' "$measure_output" >&2
        echo "failed to parse elapsed time for $measure_workload" >&2
        return 1
    fi
    printf '%s\n' "$measure_value"
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

median_candidate() {
    workload=$1
    awk -F '\t' -v workload="$workload" '$1 == workload { print $4 }' "$results" |
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

while read -r workload; do
    if [ -n "$only" ] && [ "$workload" != "$only" ]; then
        continue
    fi
    workload_path="$script_root/benches/$workload"
    warmup=1
    while [ "$warmup" -le "$warmups" ]; do
        if [ $((warmup % 2)) -eq 1 ]; then
            measure "$candidate" "$workload_path" >/dev/null
            measure "$baseline" "$workload_path" >/dev/null
        else
            measure "$baseline" "$workload_path" >/dev/null
            measure "$candidate" "$workload_path" >/dev/null
        fi
        warmup=$((warmup + 1))
    done
    pair=1
    while [ "$pair" -le "$pairs" ]; do
        if [ $((pair % 2)) -eq 1 ]; then
            candidate_value=$(measure "$candidate" "$workload_path")
            baseline_value=$(measure "$baseline" "$workload_path")
            order=candidate-first
        else
            baseline_value=$(measure "$baseline" "$workload_path")
            candidate_value=$(measure "$candidate" "$workload_path")
            order=baseline-first
        fi
        printf '%s\t%s\t%s\t%s\t%s\n' \
            "$workload" "$pair" "$order" "$candidate_value" "$baseline_value" >>"$results"
        pair=$((pair + 1))
    done
done <<'WORKLOADS'
bench_generics_method.php
bench_generics_method_native_loop.php
bench_generics_method_nested_native_loop.php
bench_generics_method_double_native_loop.php
bench_generics_method_double_nested_native_loop.php
bench_scalar_method_native_loop.php
bench_scalar_method_nested_native_loop.php
bench_scalar_method_double_native_loop.php
bench_scalar_method_double_nested_native_loop.php
bench_generics_method_turbofish.php
bench_generics_default_omitted.php
bench_generics_default_explicit.php
bench_generics_default_manual.php
WORKLOADS

echo "balanced mean of order-specific candidate/baseline median ratios:"
for workload in \
    bench_generics_method.php \
    bench_generics_method_native_loop.php \
    bench_generics_method_nested_native_loop.php \
    bench_generics_method_double_native_loop.php \
    bench_generics_method_double_nested_native_loop.php \
    bench_scalar_method_native_loop.php \
    bench_scalar_method_nested_native_loop.php \
    bench_scalar_method_double_native_loop.php \
    bench_scalar_method_double_nested_native_loop.php \
    bench_generics_method_turbofish.php \
    bench_generics_default_omitted.php \
    bench_generics_default_explicit.php \
    bench_generics_default_manual.php
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
    printf '%-42s %+10.3f%%\n' "$workload" "$balanced"
done

if [ -z "$only" ]; then
    omitted=$(median_candidate bench_generics_default_omitted.php)
    explicit=$(median_candidate bench_generics_default_explicit.php)
    manual=$(median_candidate bench_generics_default_manual.php)
    omitted_explicit=$(awk -v omitted="$omitted" -v explicit="$explicit" \
        'BEGIN { printf "%.3f", ((omitted / explicit) - 1) * 100 }')
    omitted_manual=$(awk -v omitted="$omitted" -v manual="$manual" \
        'BEGIN { printf "%.3f", ((omitted / manual) - 1) * 100 }')
    echo "candidate default-path median ratios:"
    printf '%-42s %+10.3f%%\n' "omitted/explicit" "$omitted_explicit"
    printf '%-42s %+10.3f%%\n' "omitted/manual" "$omitted_manual"
fi
