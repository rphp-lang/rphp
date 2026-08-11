#!/bin/sh

# Candidate-only equivalence gates for pseudo-static call forms that the exact
# baseline cannot parse. Established explicit-owner workloads remain covered
# against the prior binary by run_generics_gate.sh.

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
only=${RPHP_GENERICS_STATIC_GATE_ONLY:-}

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
case $only in
    '' | ordinary | property-read | property-write | generic | generic-control) ;;
    *)
        echo "RPHP_GENERICS_STATIC_GATE_ONLY names an unknown lane" >&2
        exit 2
        ;;
esac

script_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
ordinary_late_workload="$script_root/benches/bench_static_late_call.php"
ordinary_self_workload="$script_root/benches/bench_static_self_call.php"
late_property_workload="$script_root/benches/bench_static_late_property.php"
self_property_workload="$script_root/benches/bench_static_self_property.php"
late_property_write_workload="$script_root/benches/bench_static_late_property_write.php"
self_property_write_workload="$script_root/benches/bench_static_self_property_write.php"
generic_late_workload="$script_root/benches/bench_generics_static_late_turbofish.php"
generic_self_workload="$script_root/benches/bench_generics_static_self_turbofish.php"
generic_explicit_workload="$script_root/benches/bench_generics_static_explicit_turbofish.php"
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

run_gate() {
    label=$1
    candidate_workload=$2
    control_workload=$3
    : >"$results"

    warmup=1
    while [ "$warmup" -le "$warmups" ]; do
        if [ $((warmup % 2)) -eq 1 ]; then
            measure "$candidate_workload" >/dev/null
            measure "$control_workload" >/dev/null
        else
            measure "$control_workload" >/dev/null
            measure "$candidate_workload" >/dev/null
        fi
        warmup=$((warmup + 1))
    done

    pair=1
    while [ "$pair" -le "$pairs" ]; do
        if [ $((pair % 2)) -eq 1 ]; then
            candidate_time=$(measure "$candidate_workload")
            control_time=$(measure "$control_workload")
            order=candidate-first
        else
            control_time=$(measure "$control_workload")
            candidate_time=$(measure "$candidate_workload")
            order=control-first
        fi
        printf '%s\t%s\t%s\t%s\n' \
            "$order" "$pair" "$candidate_time" "$control_time" >>"$results"
        pair=$((pair + 1))
    done

    candidate_first_candidate=$(median candidate-first 3)
    candidate_first_control=$(median candidate-first 4)
    control_first_candidate=$(median control-first 3)
    control_first_control=$(median control-first 4)
    balanced=$(awk \
        -v cfc="$candidate_first_candidate" -v cfk="$candidate_first_control" \
        -v kfc="$control_first_candidate" -v kfk="$control_first_control" \
        'BEGIN { printf "%.6f", ((((cfc / cfk) - 1) + ((kfc / kfk) - 1)) / 2) * 100 }')

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

echo "balanced mean of order-specific candidate/control median ratios:"
run_selected_gate ordinary "ordinary static::/self::" "$ordinary_late_workload" "$ordinary_self_workload"
run_selected_gate property-read "property static::/self::" "$late_property_workload" "$self_property_workload"
run_selected_gate property-write "property write static::/self::" "$late_property_write_workload" "$self_property_write_workload"
run_selected_gate generic "generic static::/self::" "$generic_late_workload" "$generic_self_workload"
run_selected_gate generic-control "generic self::/explicit" "$generic_self_workload" "$generic_explicit_workload"
