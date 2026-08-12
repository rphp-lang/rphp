#!/bin/sh

# Reproducible upstream compatibility gate. The php-src checkout and generated
# artifacts stay outside the repository; only reviewed summaries/manifests are
# copied into the tree for publication.
set -eu

PHP_SRC_COMMIT=7a64ae0507799547fbbd39b067bd3dd2c35e8fec

if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
    echo "usage: $0 PHP_SRC_ROOT RPHP_BINARY OUTPUT_DIR [JOBS]" >&2
    exit 2
fi

php_src_root=$1
target=$2
output_dir=$3
jobs=${4:-4}
reference_php=${RPHP_PHPT_REFERENCE_PHP:-php}
target_kind=${RPHP_PHPT_TARGET_KIND:-rphp}
timeout=${RPHP_PHPT_TIMEOUT:-3}

case $jobs in
    '' | *[!0-9]*)
        echo "JOBS must be a positive integer" >&2
        exit 2
        ;;
esac
if [ "$jobs" -le 0 ]; then
    echo "JOBS must be a positive integer" >&2
    exit 2
fi
if [ ! -x "$target" ]; then
    echo "target executable not found: $target" >&2
    exit 2
fi
if ! command -v "$reference_php" >/dev/null 2>&1 && [ ! -x "$reference_php" ]; then
    echo "reference PHP executable not found: $reference_php" >&2
    exit 2
fi
if [ ! -d "$php_src_root/Zend/tests" ] || [ ! -d "$php_src_root/tests/lang" ]; then
    echo "php-src checkout does not contain the requested suites" >&2
    exit 2
fi
actual_php_src_commit=$(git -C "$php_src_root" rev-parse HEAD)
if [ "$actual_php_src_commit" != "$PHP_SRC_COMMIT" ]; then
    echo "php-src checkout is not pinned to $PHP_SRC_COMMIT" >&2
    exit 2
fi

script_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
runner=$script_root/scripts/phpt-runner.php
mkdir -p "$output_dir"

pids=
shard=0
while [ "$shard" -lt "$jobs" ]; do
    "$reference_php" "$runner" run \
        --suite-root "$php_src_root" \
        --target "$target" \
        --target-kind "$target_kind" \
        --timeout "$timeout" \
        --shard-index "$shard" \
        --shard-count "$jobs" \
        --manifest "$output_dir/shard-$shard.jsonl" \
        Zend/tests tests/lang &
    pids="$pids $!"
    shard=$((shard + 1))
done

failed=0
for pid in $pids; do
    if ! wait "$pid"; then
        failed=1
    fi
done
if [ "$failed" -ne 0 ]; then
    echo "one or more PHPT shards failed" >&2
    exit 1
fi

runner_commit=$(git -C "$script_root" rev-parse HEAD)
rphp_commit=${RPHP_PHPT_RPHP_COMMIT:-$runner_commit}
architecture=$(uname -m)
"$reference_php" "$runner" merge \
    --manifest "$output_dir/manifest.jsonl" \
    --summary "$output_dir/summary.json" \
    --rphp-commit "$rphp_commit" \
    --runner-commit "$runner_commit" \
    --php-src-commit "$PHP_SRC_COMMIT" \
    --features all-features \
    --architecture "$architecture" \
    --target-label "$target_kind" \
    --timeout "$timeout" \
    "$output_dir"/shard-*.jsonl
