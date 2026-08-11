#!/bin/sh

# Candidate-only equivalence gate for the newly supported self:: turbofish
# call form. The exact baseline cannot execute that syntax, so regressions
# against it remain covered by run_generics_gate.sh on established workloads.

set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "usage: $0 CANDIDATE_BINARY [CPU]" >&2
    exit 2
fi

candidate=$1
cpu=${2-}
pairs=${RPHP_GENERICS_STATIC_GATE_PAIRS:-20}
warmups=${RPHP_GENERICS_STATIC_GATE_WARMUPS:-4}
max_regression=${RPHP_GENERICS_STATIC_GATE_MAX_REGRESSION:-5}

if [ ! -x "$candidate" ]; then
    echo "runtime executable not found: $candidate" >&2
    exit 2
fi
case $pairs in
    '' | *[!0-9]*)
        echo "RPHP_GENERICS_STATIC_GATE_PAIRS must be a positive even integer" >&2
        exit 2
        ;;
esac
if [ "$pairs" -le 0 ] || [ $((pairs % 2)) -ne 0 ]; then
    echo "RPHP_GENERICS_STATIC_GATE_PAIRS must be a positive even integer" >&2
    exit 2
fi
case $warmups in
    '' | *[!0-9]*)
        echo "RPHP_GENERICS_STATIC_GATE_WARMUPS must be a non-negative integer" >&2
        exit 2
        ;;
esac
if [ -n "$cpu" ] && ! command -v taskset >/dev/null 2>&1; then
    echo "CPU pinning requested, but taskset is unavailable" >&2
    exit 2
fi
if ! printf '%s\n' "$max_regression" | awk '
    /^[0-9]+([.][0-9]+)?$/ { valid = 1 }
    END { exit !valid }
'; then
    echo "RPHP_GENERICS_STATIC_GATE_MAX_REGRESSION must be a non-negative number" >&2
    exit 2
fi

script_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
self_workload="$script_root/benches/bench_generics_static_self_turbofish.php"
explicit_workload="$script_root/benches/bench_generics_static_explicit_turbofish.php"
results=$(mktemp "${TMPDIR:-/tmp}/rphp-generics-static-results.XXXXXX")
trap 'rm -f -- "$results"' EXIT HUP INT TERM

measure() {
    workload=$1
    if [ -n "$cpu" ]; then
        output=$(taskset -c "$cpu" "$candidate" "$workload" 2>&1)
    else
        output=$("$candidate" "$workload" 2>&1)
    fi
    result=$(printf '%s\n' "$output" | awk -F '|' 'NF > 1 { value = $1 } END { print value }')
    elapsed=$(printf '%s\n' "$output" | awk -F '|' 'NF > 1 { value = $NF } END { print value }')
    if [ "$result" != "5000000" ] || ! printf '%s\n' "$elapsed" | awk '
        /^[0-9]+([.][0-9]+)?$/ { valid = 1 }
        END { exit !valid }
    '; then
        printf '%s\n' "$output" >&2
        echo "invalid benchmark result for $workload" >&2
        return 1
    fi
    printf '%s\n' "$elapsed"
}

median() {
    order=$1
    column=$2
    awk -F '\t' -v order="$order" -v column="$column" \
        '$1 == order { print $column }' "$results" |
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
        measure "$self_workload" >/dev/null
        measure "$explicit_workload" >/dev/null
    else
        measure "$explicit_workload" >/dev/null
        measure "$self_workload" >/dev/null
    fi
    warmup=$((warmup + 1))
done

pair=1
while [ "$pair" -le "$pairs" ]; do
    if [ $((pair % 2)) -eq 1 ]; then
        self_time=$(measure "$self_workload")
        explicit_time=$(measure "$explicit_workload")
        order=self-first
    else
        explicit_time=$(measure "$explicit_workload")
        self_time=$(measure "$self_workload")
        order=explicit-first
    fi
    printf '%s\t%s\t%s\t%s\n' \
        "$order" "$pair" "$self_time" "$explicit_time" >>"$results"
    pair=$((pair + 1))
done

self_first_self=$(median self-first 3)
self_first_explicit=$(median self-first 4)
explicit_first_self=$(median explicit-first 3)
explicit_first_explicit=$(median explicit-first 4)
balanced=$(awk \
    -v sfs="$self_first_self" -v sfe="$self_first_explicit" \
    -v efs="$explicit_first_self" -v efe="$explicit_first_explicit" \
    'BEGIN { printf "%.6f", ((((sfs / sfe) - 1) + ((efs / efe) - 1)) / 2) * 100 }')

echo "balanced mean of order-specific self/explicit median ratios:"
printf '%-42s %+10.3f%%\n' "static self/explicit" "$balanced"
if ! awk -v ratio="$balanced" -v limit="$max_regression" \
    'BEGIN { exit !(ratio <= limit) }'; then
    echo "static self turbofish exceeded ${max_regression}% regression budget" >&2
    exit 1
fi
