#!/usr/bin/env bash
# Reproduce the current baseline/typed-region/JIT scorecard on one host.
#
# The baseline lane uses the same optimized binary as the typed lane with
# quick-region planning disabled. This isolates region value without changing
# the compiler, profile, target CPU, or unrelated runtime fast paths.

set -euo pipefail

export LC_ALL=C

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNS="${RPHP_SCORECARD_RUNS:-15}"
COOLDOWN_SECONDS="${RPHP_SCORECARD_COOLDOWN_SECONDS:-60}"

case "$RUNS" in
    '' | *[!0-9]*)
        echo "RPHP_SCORECARD_RUNS must be a positive integer" >&2
        exit 2
        ;;
esac
if [ "$RUNS" -le 0 ]; then
    echo "RPHP_SCORECARD_RUNS must be a positive integer" >&2
    exit 2
fi
case "$COOLDOWN_SECONDS" in
    '' | *[!0-9]*)
        echo "RPHP_SCORECARD_COOLDOWN_SECONDS must be a non-negative integer" >&2
        exit 2
        ;;
esac

typed_target=''
jit_target=''
typed_stats_target=''
jit_stats_target=''
stats_stdout=''
stats_stderr=''
resource_stderr=''

cleanup() {
    if [ -n "$typed_target" ]; then rm -rf -- "$typed_target"; fi
    if [ -n "$jit_target" ]; then rm -rf -- "$jit_target"; fi
    if [ -n "$typed_stats_target" ]; then rm -rf -- "$typed_stats_target"; fi
    if [ -n "$jit_stats_target" ]; then rm -rf -- "$jit_stats_target"; fi
    if [ -n "$stats_stdout" ]; then rm -f -- "$stats_stdout"; fi
    if [ -n "$stats_stderr" ]; then rm -f -- "$stats_stderr"; fi
    if [ -n "$resource_stderr" ]; then rm -f -- "$resource_stderr"; fi
    "$ROOT_DIR/scripts/cleanup-builds.sh"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

typed_target="$(mktemp -d "${TMPDIR:-/tmp}/rphp-candidate-scorecard-typed.XXXXXX")"
jit_target="$(mktemp -d "${TMPDIR:-/tmp}/rphp-candidate-scorecard-jit.XXXXXX")"
typed_stats_target="$(mktemp -d "${TMPDIR:-/tmp}/rphp-candidate-scorecard-typed-stats.XXXXXX")"
jit_stats_target="$(mktemp -d "${TMPDIR:-/tmp}/rphp-candidate-scorecard-jit-stats.XXXXXX")"
stats_stdout="$(mktemp "${TMPDIR:-/tmp}/rphp-scorecard-stdout.XXXXXX")"
stats_stderr="$(mktemp "${TMPDIR:-/tmp}/rphp-scorecard-stderr.XXXXXX")"
resource_stderr="$(mktemp "${TMPDIR:-/tmp}/rphp-scorecard-resource.XXXXXX")"

cd "$ROOT_DIR"
./scripts/cleanup-builds.sh

build() {
    local target_dir=$1
    shift
    RUSTFLAGS="-C target-cpu=native" CARGO_TARGET_DIR="$target_dir" \
        cargo build --profile max-perf "$@" >&2
}

SECONDS=0
build "$typed_target"
typed_build_seconds=$SECONDS
SECONDS=0
build "$jit_target" --features jit-prototype
jit_build_seconds=$SECONDS

typed_binary="$typed_target/max-perf/rphp"
jit_binary="$jit_target/max-perf/rphp"
typed_stats_binary="$typed_stats_target/max-perf/rphp"
jit_stats_binary="$jit_stats_target/max-perf/rphp"

php_jit_status=$(php \
    -dopcache.enable_cli=1 \
    -dopcache.jit_buffer_size=100M \
    -dopcache.jit=tracing \
    -r '$status=opcache_get_status(false); echo ($status["jit"]["enabled"] && $status["jit"]["on"]) ? "on" : "off";')
if [ "$php_jit_status" != on ]; then
    echo "requested PHP tracing JIT is not active" >&2
    exit 1
fi

if [ "$COOLDOWN_SECONDS" -gt 0 ]; then
    sleep "$COOLDOWN_SECONDS"
fi

set_mode_command() {
    local mode=$1
    local workload=$2
    case "$mode" in
        baseline)
            scorecard_command=(env RPHP_DISABLE_QUICK_LOOPS=1 "$typed_binary" "$workload")
            ;;
        typed) scorecard_command=("$typed_binary" "$workload") ;;
        jit) scorecard_command=("$jit_binary" "$workload") ;;
        php_nojit) scorecard_command=(php -n "$workload") ;;
        php_jit)
            scorecard_command=(
                php -dopcache.enable_cli=1 -dopcache.jit_buffer_size=100M
                -dopcache.jit=tracing "$workload"
            )
            ;;
        *)
            echo "unknown scorecard mode: $mode" >&2
            return 2
            ;;
    esac
}

run_mode() {
    set_mode_command "$1" "$2"
    "${scorecard_command[@]}"
}

php_modules() {
    "$@" -m | awk '
        /^\[PHP Modules\]$/ { section = "php"; next }
        /^\[Zend Modules\]$/ { section = "zend"; next }
        NF {
            if (seen++) printf ","
            printf "%s:%s", section, $0
        }
        END { print "" }
    '
}

measure_peak_rss() {
    local mode=$1
    local workload=$2
    local label=$3
    local expected=$4
    local raw result peak_rss

    if [ ! -x /usr/bin/time ]; then
        printf 'resource\t%s\t%s\tpeak_rss_bytes\tunavailable\n' "$label" "$mode"
        return
    fi
    set_mode_command "$mode" "$workload"
    case "$(uname -s)" in
        Darwin)
            raw=$(/usr/bin/time -l "${scorecard_command[@]}" 2>"$resource_stderr")
            peak_rss=$(awk '/maximum resident set size/ { print $1; exit }' "$resource_stderr")
            ;;
        Linux)
            raw=$(/usr/bin/time -v "${scorecard_command[@]}" 2>"$resource_stderr")
            peak_rss=$(awk '
                /Maximum resident set size/ { printf "%.0f", $NF * 1024; exit }
            ' "$resource_stderr")
            ;;
        *)
            printf 'resource\t%s\t%s\tpeak_rss_bytes\tunavailable\n' "$label" "$mode"
            return
            ;;
    esac
    result=${raw%%|*}
    if [ "$result" != "$expected" ]; then
        echo "resource-run output mismatch for $label in $mode" >&2
        return 1
    fi
    printf 'resource\t%s\t%s\tpeak_rss_bytes\t%s\n' \
        "$label" "$mode" "${peak_rss:-unavailable}"
}

run_stats_mode() {
    local mode=$1
    local workload=$2
    case "$mode" in
        baseline)
            RPHP_DISABLE_QUICK_LOOPS=1 RPHP_VM_STATS=1 \
                "$typed_stats_binary" "$workload"
            ;;
        typed) RPHP_VM_STATS=1 "$typed_stats_binary" "$workload" ;;
        jit) RPHP_VM_STATS=1 "$jit_stats_binary" "$workload" ;;
        *)
            echo "unknown stats mode: $mode" >&2
            return 2
            ;;
    esac
}

emit_stats() {
    local label=$1
    local mode=$2
    awk -v workload="$label" -v mode="$mode" '
        /^-- value\.clone by type --$/ { section = "clone"; next }
        /^-- value\.drop by type --$/ { section = "drop"; next }
        /^-- admitted regions by shape --$/ ||
        /^-- executed optimized regions by shape --$/ ||
        /^-- native JIT executions by shape --$/ ||
        /^-- rejected loops by dominant gap --$/ ||
        /^-- rejected backedge executions by dominant gap --$/ {
            section = (mode == "jit") ? "coverage" : ""
            if (section == "coverage") print "stats\t" workload "\t" mode "\t" $0
            next
        }
        /^-- / { section = ""; next }
        section == "clone" && /^[a-z_]+=[0-9]+$/ {
            split($0, fields, "="); clones += fields[2]; next
        }
        section == "drop" && /^[a-z_]+=[0-9]+$/ {
            split($0, fields, "="); drops += fields[2]; next
        }
        section == "coverage" && /^[a-z_]+=[0-9]+(,side_exits=[0-9]+)?$/ {
            print "stats\t" workload "\t" mode "\t" $0; next
        }
        /^(push_call_frame|cleanup_frame|write_val|write_frame_slot|do_fcall|return|quick_loop|quick_packed_array_|quick_property_shadow_|array_owner_|closure_payload_|declared_object_|declared_property_|newobj_|resolved_virtual_aggregate_|jit_loop|jit_straight_)[a-z_]*=[0-9]+$/ {
            print "stats\t" workload "\t" mode "\t" $0
        }
        END {
            print "stats\t" workload "\t" mode "\tvalue_clones_total=" clones + 0
            print "stats\t" workload "\t" mode "\tvalue_drops_total=" drops + 0
        }
    ' "$stats_stderr"
}

summarize() {
    printf '%s\n' "$@" | sort -n | awk '
        { values[NR] = $1 }
        END {
            count = NR
            if (count % 2) median = values[(count + 1) / 2]
            else median = (values[count / 2] + values[count / 2 + 1]) / 2
            p10 = values[int((count - 1) * 0.10) + 1]
            p90 = values[int((count - 1) * 0.90) + 1]
            printf "%.9f\t%.9f\t%.9f", median, p10, p90
        }
    '
}

commit=$(git rev-parse HEAD)
if [ -n "$(git status --porcelain)" ]; then
    tree_state=dirty
else
    tree_state=clean
fi

printf 'metadata\tcommit\t%s\n' "$commit"
printf 'metadata\ttree\t%s\n' "$tree_state"
printf 'metadata\truns\t%s\n' "$RUNS"
printf 'metadata\trustc\t%s\n' "$(rustc --version)"
printf 'metadata\tphp\t%s\n' "$(php -r 'echo PHP_VERSION;')"
printf 'metadata\tphp_jit\t%s\n' "$php_jit_status"
printf 'metadata\tprofile\tmax-perf; target-cpu=native\n'
printf 'metadata\tfeatures\tbaseline=quick-loops runtime-disabled; typed=quick-loops; jit=quick-loops,jit-prototype\n'
printf 'metadata\ttyped_build_elapsed_s\t%s\n' "$typed_build_seconds"
printf 'metadata\tjit_build_elapsed_s\t%s\n' "$jit_build_seconds"
printf 'metadata\ttyped_binary_bytes\t%s\n' "$(wc -c <"$typed_binary" | tr -d ' ')"
printf 'metadata\tjit_binary_bytes\t%s\n' "$(wc -c <"$jit_binary" | tr -d ' ')"
printf 'metadata\tphp_nojit_modules\t%s\n' "$(php_modules php -n)"
printf 'metadata\tphp_jit_modules\t%s\n' "$(php_modules php)"
printf 'metadata\tordering\trotated five-mode order; all samples retained\n'
printf 'metadata\twarmup\tone untimed validation; every measured process starts fresh and includes region/JIT admission\n'
printf 'metadata\tpost_build_cooldown_s\t%s\n' "$COOLDOWN_SECONDS"
printf 'metadata\tisolation\tno affinity or explicit power-mode override\n'
printf 'sample\tworkload\tmode\trun\telapsed_s\n'

workloads=(
    benches/corpus_order_pipeline.php
    benches/corpus_typed_order_pipeline.php
    benches/corpus_ledger_pipeline.php
    benches/corpus_typed_ledger_pipeline.php
    benches/holdout_routing_pipeline.php
    benches/bench_hash_dynamic_string_array_loop.php
    benches/bench_hash_dynamic_string_cv_array_loop.php
    benches/bench_jit_mixed_string_hash_update.php
    benches/bench_foreach.php
    benches/bench_hash_foreach.php
    benches/bench_string.php
    benches/bench_array.php
    benches/bench_closure_copy.php
    benches/bench_closure_storage.php
    benches/bench_declared_object_lifecycle.php
)
labels=(
    order typed_order ledger typed_ledger routing_holdout
    dynamic_string_read dynamic_string_cv_read mixed_string_update
    packed_foreach hash_foreach string_append array_build_read
    closure_copy
    closure_storage
    declared_object_lifecycle
)
modes=(baseline typed jit php_nojit php_jit)
expected_results=()

for workload_index in "${!workloads[@]}"; do
    workload=${workloads[$workload_index]}
    label=${labels[$workload_index]}
    expected=''

    for mode in "${modes[@]}"; do
        raw=$(run_mode "$mode" "$workload")
        result=${raw%%|*}
        if [ -z "$expected" ]; then
            expected=$result
        elif [ "$result" != "$expected" ]; then
            echo "output mismatch for $label in $mode" >&2
            exit 1
        fi
    done
    expected_results[$workload_index]=$expected

    baseline_times=()
    typed_times=()
    jit_times=()
    php_nojit_times=()
    php_jit_times=()

    for ((run = 0; run < RUNS; run++)); do
        offset=$((run % ${#modes[@]}))
        for ((mode_index = 0; mode_index < ${#modes[@]}; mode_index++)); do
            mode=${modes[$(((offset + mode_index) % ${#modes[@]}))]}
            raw=$(run_mode "$mode" "$workload")
            result=${raw%%|*}
            elapsed=${raw##*|}
            if [ "$result" != "$expected" ]; then
                echo "output mismatch for $label in $mode run $((run + 1))" >&2
                exit 1
            fi
            printf 'sample\t%s\t%s\t%d\t%s\n' \
                "$label" "$mode" "$((run + 1))" "$elapsed"
            case "$mode" in
                baseline) baseline_times+=("$elapsed") ;;
                typed) typed_times+=("$elapsed") ;;
                jit) jit_times+=("$elapsed") ;;
                php_nojit) php_nojit_times+=("$elapsed") ;;
                php_jit) php_jit_times+=("$elapsed") ;;
            esac
        done
    done

    printf 'summary\tworkload\tmode\tmedian_s\tp10_s\tp90_s\n'
    for mode in "${modes[@]}"; do
        case "$mode" in
            baseline) summary=$(summarize "${baseline_times[@]}") ;;
            typed) summary=$(summarize "${typed_times[@]}") ;;
            jit) summary=$(summarize "${jit_times[@]}") ;;
            php_nojit) summary=$(summarize "${php_nojit_times[@]}") ;;
            php_jit) summary=$(summarize "${php_jit_times[@]}") ;;
        esac
        printf 'summary\t%s\t%s\t%s\n' "$label" "$mode" "$summary"
    done

done

# Keep extra resource runs and instrumented builds after the timing cycle so
# they cannot heat or perturb the retained distributions above.
for workload_index in "${!workloads[@]}"; do
    workload=${workloads[$workload_index]}
    label=${labels[$workload_index]}
    expected=${expected_results[$workload_index]}
    for mode in baseline typed jit; do
        measure_peak_rss "$mode" "$workload" "$label" "$expected"
    done
done

build "$typed_stats_target" --features vm-stats
build "$jit_stats_target" --features jit-prototype,vm-stats

for workload_index in "${!workloads[@]}"; do
    workload=${workloads[$workload_index]}
    label=${labels[$workload_index]}
    expected=${expected_results[$workload_index]}
    for mode in baseline typed jit; do
        run_stats_mode "$mode" "$workload" >"$stats_stdout" 2>"$stats_stderr"
        stats_result=$(cut -d '|' -f 1 <"$stats_stdout")
        if [ "$stats_result" != "$expected" ]; then
            echo "vm-stats output mismatch for $label in $mode" >&2
            exit 1
        fi
        emit_stats "$label" "$mode"
    done
done
