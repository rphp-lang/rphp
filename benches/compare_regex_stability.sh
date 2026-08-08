#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 || $# -gt 5 ]]; then
    echo "usage: $0 BASELINE CANDIDATE [PAIRS] [WARMUPS] [CPU]" >&2
    exit 2
fi

baseline=$1
candidate=$2
pairs=${3:-1003}
warmups=${4:-50}
cpu=${5:-}

for binary in "$baseline" "$candidate"; do
    if [[ ! -x "$binary" ]]; then
        echo "not an executable: $binary" >&2
        exit 2
    fi
done

if [[ -n "$cpu" ]] && ! command -v taskset >/dev/null 2>&1; then
    echo "CPU pinning requested but taskset is unavailable" >&2
    exit 2
fi

run_workload() {
    local binary=$1
    local workload=$2
    if [[ -n "$cpu" ]]; then
        taskset -c "$cpu" "$binary" "$workload"
    else
        "$binary" "$workload"
    fi
}

measure() {
    local label=$1
    local workload=$2
    local total=$((pairs + warmups))

    for ((iteration = 1; iteration <= total; iteration++)); do
        local before
        local after
        if ((iteration % 2 == 1)); then
            before=$(run_workload "$baseline" "$workload")
            after=$(run_workload "$candidate" "$workload")
        else
            after=$(run_workload "$candidate" "$workload")
            before=$(run_workload "$baseline" "$workload")
        fi
        if ((iteration > warmups)); then
            printf '%s %s\n' "${before#*|}" "${after#*|}"
        fi
    done | awk -v label="$label" '
        {
            before += $1
            after += $2
            measured++
        }
        END {
            printf "%-28s pairs=%d baseline=%.6fms candidate=%.6fms delta=%+.2f%%\n",
                label, measured, before / measured * 1000, after / measured * 1000,
                (after / before - 1) * 100
        }
    '
}

measure "callback" benches/bench_regex_repeated_callback.php
measure "callback mutates" benches/bench_regex_repeated_callback_mutates.php
measure "callback retains" benches/bench_regex_repeated_callback_retains.php
measure "callback grouped" benches/bench_regex_repeated_callback_groups.php
measure "count fixed prefix" benches/bench_regex_match_all_count.php
measure "count no literal" benches/bench_regex_match_all_count_no_literal.php
measure "count UTF-8" benches/bench_regex_match_all_count_utf8.php
measure "match-all output" benches/bench_regex_match_all.php
measure "preg_match no groups" benches/bench_regex_match_no_captures.php
measure "preg_match group miss" benches/bench_regex_match_unused_group_miss.php
