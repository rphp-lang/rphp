#!/bin/sh

set -eu

script_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
php_bin=${RPHP_PHPT_REFERENCE_PHP:-php}
if command -v "$php_bin" >/dev/null 2>&1; then
    php_bin=$(command -v "$php_bin")
fi
fixture_copy=$(mktemp -d "${TMPDIR:-/tmp}/rphp-phpt-runner-fixtures.XXXXXX")
trap 'rm -rf -- "$fixture_copy"' EXIT HUP INT TERM

cp -R "$script_root/tests/php-src/runner-fixtures/." "$fixture_copy/"
"$php_bin" "$script_root/scripts/phpt-runner.php" run \
    --suite-root "$fixture_copy" \
    --target "$php_bin" \
    --target-kind php \
    --timeout 3 \
    --manifest "$fixture_copy/shard.jsonl" \
    .
"$php_bin" "$script_root/scripts/phpt-runner.php" merge \
    --manifest "$fixture_copy/manifest.jsonl" \
    --summary "$fixture_copy/summary.json" \
    --target-label reference-php \
    "$fixture_copy/shard.jsonl"

"$php_bin" -r '
$summary = json_decode(file_get_contents($argv[1]), true, flags: JSON_THROW_ON_ERROR);
if ($summary["total"] !== 8
    || $summary["statuses"]["pass"] !== 7
    || $summary["statuses"]["skip"] !== 1
    || array_sum(array_intersect_key(
        $summary["statuses"],
        array_flip(["fail", "unsupported", "timeout", "crash"]),
    )) !== 0
) {
    fwrite(STDERR, "unexpected PHPT runner fixture summary\n");
    exit(1);
}
' "$fixture_copy/summary.json"
