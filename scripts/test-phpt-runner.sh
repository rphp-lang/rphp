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
if ($summary["schema_version"] !== 3
    || $summary["total"] !== 13
    || $summary["statuses"]["pass"] !== 11
    || $summary["statuses"]["skip"] !== 1
    || $summary["statuses"]["xfail"] !== 1
    || array_sum(array_intersect_key(
        $summary["statuses"],
        array_flip(["fail", "unsupported", "timeout", "crash"]),
    )) !== 0
    || $summary["execution_profile"] !== [
        "attempted" => 11,
        "front_end_rejected" => 0,
        "runtime_reached" => 11,
        "runtime_reach_rate" => 1,
    ]
) {
    fwrite(STDERR, "unexpected PHPT runner fixture summary\n");
    exit(1);
}
' "$fixture_copy/summary.json"

"$php_bin" -r '
require $argv[1];
$profile = execution_profile(
    [
        "pass" => 2,
        "fail" => 6,
        "skip" => 1,
        "xfail" => 1,
        "unsupported" => 1,
        "timeout" => 1,
        "crash" => 1,
    ],
    ["parse" => 2, "compile" => 1, "runtime" => 7],
);
if ($profile !== [
    "attempted" => 10,
    "front_end_rejected" => 3,
    "runtime_reached" => 7,
    "runtime_reach_rate" => 0.7,
]) {
    fwrite(STDERR, "unexpected synthetic execution profile\n");
    exit(1);
}
' "$script_root/scripts/phpt/report.php"
