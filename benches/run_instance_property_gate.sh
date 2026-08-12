#!/bin/sh

# Candidate-only equivalence gate for typed instance-property reads, writes,
# constructor initialization and property-method plans. Exact baseline
# regressions for the untyped control remain covered by run_runtime_gate.sh.

set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "usage: $0 CANDIDATE_BINARY [CPU]" >&2
    exit 2
fi

candidate=$1
cpu=${2-}
pairs=${RPHP_INSTANCE_PROPERTY_GATE_PAIRS:-20}
warmups=${RPHP_INSTANCE_PROPERTY_GATE_WARMUPS:-4}
max_regression=${RPHP_INSTANCE_PROPERTY_GATE_MAX_REGRESSION:-5}
only=${RPHP_INSTANCE_PROPERTY_GATE_ONLY:-}

if [ ! -x "$candidate" ]; then
    echo "runtime executable not found: $candidate" >&2
    exit 2
fi
case $pairs in
    '' | *[!0-9]*)
        echo "RPHP_INSTANCE_PROPERTY_GATE_PAIRS must be a positive even integer" >&2
        exit 2
        ;;
esac
if [ "$pairs" -le 0 ] || [ $((pairs % 2)) -ne 0 ]; then
    echo "RPHP_INSTANCE_PROPERTY_GATE_PAIRS must be a positive even integer" >&2
    exit 2
fi
case $warmups in
    '' | *[!0-9]*)
        echo "RPHP_INSTANCE_PROPERTY_GATE_WARMUPS must be a non-negative integer" >&2
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
    echo "RPHP_INSTANCE_PROPERTY_GATE_MAX_REGRESSION must be a non-negative number" >&2
    exit 2
fi
case $only in
    '' | read | write | method | constructor) ;;
    *)
        echo "RPHP_INSTANCE_PROPERTY_GATE_ONLY names an unknown lane" >&2
        exit 2
        ;;
esac

script_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
results=$(mktemp "${TMPDIR:-/tmp}/rphp-instance-property-results.XXXXXX")
trap 'rm -f -- "$results"' EXIT HUP INT TERM

measure() {
    workload=$1
    expected=$2
    if [ -n "$cpu" ]; then
        output=$(taskset -c "$cpu" "$candidate" "$workload" 2>&1)
    else
        output=$("$candidate" "$workload" 2>&1)
    fi
    result=$(printf '%s\n' "$output" | awk -F '|' 'NF > 1 { value = $1 } END { print value }')
    elapsed=$(printf '%s\n' "$output" | awk -F '|' 'NF > 1 { value = $NF } END { print value }')
    if [ "$result" != "$expected" ] || ! printf '%s\n' "$elapsed" | awk '
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

run_gate() {
    label=$1
    typed_workload=$2
    control_workload=$3
    expected=$4
    : >"$results"

    warmup=1
    while [ "$warmup" -le "$warmups" ]; do
        if [ $((warmup % 2)) -eq 1 ]; then
            measure "$typed_workload" "$expected" >/dev/null
            measure "$control_workload" "$expected" >/dev/null
        else
            measure "$control_workload" "$expected" >/dev/null
            measure "$typed_workload" "$expected" >/dev/null
        fi
        warmup=$((warmup + 1))
    done

    pair=1
    while [ "$pair" -le "$pairs" ]; do
        if [ $((pair % 2)) -eq 1 ]; then
            typed_time=$(measure "$typed_workload" "$expected")
            control_time=$(measure "$control_workload" "$expected")
            order=typed-first
        else
            control_time=$(measure "$control_workload" "$expected")
            typed_time=$(measure "$typed_workload" "$expected")
            order=control-first
        fi
        printf '%s\t%s\t%s\t%s\n' \
            "$order" "$pair" "$typed_time" "$control_time" >>"$results"
        pair=$((pair + 1))
    done

    typed_first_typed=$(median typed-first 3)
    typed_first_control=$(median typed-first 4)
    control_first_typed=$(median control-first 3)
    control_first_control=$(median control-first 4)
    balanced=$(awk \
        -v tft="$typed_first_typed" -v tfc="$typed_first_control" \
        -v cft="$control_first_typed" -v cfc="$control_first_control" \
        'BEGIN { printf "%.6f", ((((tft / tfc) - 1) + ((cft / cfc) - 1)) / 2) * 100 }')

    printf '%-42s %+10.3f%%\n' "$label" "$balanced"
    if ! awk -v ratio="$balanced" -v limit="$max_regression" \
        'BEGIN { exit !(ratio <= limit) }'; then
        echo "$label exceeded ${max_regression}% regression budget" >&2
        return 1
    fi
}

run_selected_gate() {
    lane=$1
    shift
    if [ -z "$only" ] || [ "$only" = "$lane" ]; then
        run_gate "$@"
    fi
}

echo "balanced mean of order-specific typed/untyped median ratios:"
run_selected_gate read "typed/untyped instance read" \
    "$script_root/benches/bench_instance_property_read_typed.php" \
    "$script_root/benches/bench_instance_property_read.php" 35000000
run_selected_gate write "typed/untyped instance write" \
    "$script_root/benches/bench_instance_property_write_typed.php" \
    "$script_root/benches/bench_instance_property_write.php" 4999999
run_selected_gate method "typed/untyped property method" \
    "$script_root/benches/bench_property_typed.php" \
    "$script_root/benches/bench_property.php" 12499997500000
run_selected_gate constructor "typed/untyped property constructor" \
    "$script_root/benches/bench_instance_property_constructor_typed.php" \
    "$script_root/benches/bench_instance_property_constructor.php" 999999
