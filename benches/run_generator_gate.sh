#!/bin/sh

# Balanced comparison for the detached generator resume lifecycle. Both
# binaries execute the candidate tree's workload so source changes cannot be
# mistaken for runtime changes.

set -eu

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    echo "usage: $0 CANDIDATE_BINARY BASELINE_BINARY [CPU]" >&2
    exit 2
fi

candidate=$1
baseline=$2
cpu=${3-}
pairs=${RPHP_GENERATOR_GATE_PAIRS:-20}
warmups=${RPHP_GENERATOR_GATE_WARMUPS:-4}

for binary in "$candidate" "$baseline"; do
    if [ ! -x "$binary" ]; then
        echo "runtime executable not found: $binary" >&2
        exit 2
    fi
done
case $pairs in
    '' | *[!0-9]*)
        echo "RPHP_GENERATOR_GATE_PAIRS must be a positive even integer" >&2
        exit 2
        ;;
esac
if [ "$pairs" -le 0 ] || [ $((pairs % 2)) -ne 0 ]; then
    echo "RPHP_GENERATOR_GATE_PAIRS must be a positive even integer" >&2
    exit 2
fi
case $warmups in
    '' | *[!0-9]*)
        echo "RPHP_GENERATOR_GATE_WARMUPS must be a non-negative integer" >&2
        exit 2
        ;;
esac
if [ -n "$cpu" ] && ! command -v taskset >/dev/null 2>&1; then
    echo "CPU pinning requested, but taskset is unavailable" >&2
    exit 2
fi

script_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
workload="$script_root/benches/bench_generator_resume.php"
expected_sum=19999900000
results=$(mktemp "${TMPDIR:-/tmp}/rphp-generator-results.XXXXXX")
trap 'rm -f -- "$results"' EXIT HUP INT TERM

measure() {
    measure_binary=$1
    if [ -n "$cpu" ]; then
        measure_output=$(taskset -c "$cpu" "$measure_binary" "$workload" 2>&1)
    else
        measure_output=$("$measure_binary" "$workload" 2>&1)
    fi
    measure_sum=$(printf '%s\n' "$measure_output" | awk -F '|' 'NF > 1 { value = $1 } END { print value }')
    measure_value=$(printf '%s\n' "$measure_output" | awk -F '|' 'NF > 1 { value = $NF } END { print value }')
    if [ "$measure_sum" != "$expected_sum" ]; then
        printf '%s\n' "$measure_output" >&2
        echo "unexpected generator checksum: $measure_sum" >&2
        return 1
    fi
    if ! printf '%s\n' "$measure_value" | awk '
        /^[0-9]+([.][0-9]+)?$/ { valid = 1 }
        END { exit !valid }
    '; then
        printf '%s\n' "$measure_output" >&2
        echo "failed to parse generator elapsed time" >&2
        return 1
    fi
    printf '%s\n' "$measure_value"
}

median() {
    order=$1
    column=$2
    awk -F '\t' -v order="$order" -v column="$column" '$2 == order { print $column }' "$results" |
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

warmup=1
while [ "$warmup" -le "$warmups" ]; do
    if [ $((warmup % 2)) -eq 1 ]; then
        measure "$candidate" >/dev/null
        measure "$baseline" >/dev/null
    else
        measure "$baseline" >/dev/null
        measure "$candidate" >/dev/null
    fi
    warmup=$((warmup + 1))
done

pair=1
while [ "$pair" -le "$pairs" ]; do
    if [ $((pair % 2)) -eq 1 ]; then
        candidate_value=$(measure "$candidate")
        baseline_value=$(measure "$baseline")
        order=candidate-first
    else
        baseline_value=$(measure "$baseline")
        candidate_value=$(measure "$candidate")
        order=baseline-first
    fi
    printf '%s\t%s\t%s\t%s\n' \
        "$pair" "$order" "$candidate_value" "$baseline_value" >>"$results"
    pair=$((pair + 1))
done

candidate_first_candidate=$(median candidate-first 3)
candidate_first_baseline=$(median candidate-first 4)
baseline_first_candidate=$(median baseline-first 3)
baseline_first_baseline=$(median baseline-first 4)
balanced=$(awk -v cfc="$candidate_first_candidate" -v cfb="$candidate_first_baseline" \
    -v bfc="$baseline_first_candidate" -v bfb="$baseline_first_baseline" \
    'BEGIN { printf "%.6f", ((((cfc / cfb) - 1) + ((bfc / bfb) - 1)) / 2) * 100 }')

echo "balanced mean of order-specific candidate/baseline median ratios:"
printf '%-42s %+10.3f%%\n' "bench_generator_resume.php" "$balanced"
printf 'candidate medians: %.6f s / %.6f s\n' \
    "$candidate_first_candidate" "$baseline_first_candidate"
printf 'baseline medians:  %.6f s / %.6f s\n' \
    "$candidate_first_baseline" "$baseline_first_baseline"
