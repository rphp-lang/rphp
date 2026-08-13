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
"$php_bin" "$script_root/scripts/phpt-coverage-map.php" \
    "$fixture_copy/manifest.jsonl" \
    "$fixture_copy/coverage-map.json"

"$php_bin" -r '
$summary = json_decode(file_get_contents($argv[1]), true, flags: JSON_THROW_ON_ERROR);
if ($summary["schema_version"] !== 5
    || $summary["total"] !== 14
    || $summary["statuses"]["pass"] !== 12
    || $summary["statuses"]["skip"] !== 1
    || $summary["statuses"]["xfail"] !== 1
    || array_sum(array_intersect_key(
        $summary["statuses"],
        array_flip(["fail", "unsupported", "timeout", "crash"]),
    )) !== 0
    || $summary["execution_profile"] !== [
        "attempted" => 12,
        "pre_execution_failed" => 0,
        "front_end_rejected" => 2,
        "runtime_reached" => 10,
        "runtime_reach_rate" => 10 / 12,
    ]
    || $summary["expectation_profiles"] !== [
        "diagnostic" => [
            "pass" => 2,
            "fail" => 0,
            "skip" => 0,
            "xfail" => 0,
            "unsupported" => 0,
            "timeout" => 0,
            "crash" => 0,
            "total" => 2,
            "headline_pass_rate" => 1,
            "attempted_pass_rate" => 1,
        ],
        "ordinary" => [
            "pass" => 10,
            "fail" => 0,
            "skip" => 1,
            "xfail" => 1,
            "unsupported" => 0,
            "timeout" => 0,
            "crash" => 0,
            "total" => 12,
            "headline_pass_rate" => 1,
            "attempted_pass_rate" => 1,
        ],
    ]
) {
    fwrite(STDERR, "unexpected PHPT runner fixture summary\n");
    exit(1);
}
' "$fixture_copy/summary.json"

"$php_bin" -r '
$map = json_decode(file_get_contents($argv[1]), true, flags: JSON_THROW_ON_ERROR);
$groupTotal = array_sum(array_column($map["groups"], "total"));
if ($map["schema_version"] !== 1
    || $map["total"] !== 14
    || $groupTotal !== 14
    || $map["hazards"] !== []
    || $map["manifest_sha256"] !== hash_file("sha256", $argv[2])
) {
    fwrite(STDERR, "unexpected PHPT coverage map\n");
    exit(1);
}
' "$fixture_copy/coverage-map.json" "$fixture_copy/manifest.jsonl"

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
    3,
    1,
);
if ($profile !== [
    "attempted" => 10,
    "pre_execution_failed" => 1,
    "front_end_rejected" => 3,
    "runtime_reached" => 6,
    "runtime_reach_rate" => 0.6,
]) {
    fwrite(STDERR, "unexpected synthetic execution profile\n");
    exit(1);
}
' "$script_root/scripts/phpt/report.php"

"$php_bin" -r '
require $argv[1];
if (classify_failure("Parse error: emitted by user code", 0) !== "output"
    || classify_failure("Parse error: emitted by the parser", 1) !== "parse"
    || expectation_profile(["EXPECT" => "Error is ordinary user data"]) !== "ordinary"
    || expectation_profile(["EXPECTF" => "Fatal error: broken in %s on line %d"]) !== "diagnostic"
    || expectation_profile(["EXPECT" => "prefix\nWarning: broken"]) !== "diagnostic"
) {
    fwrite(STDERR, "unexpected execution-phase classification\n");
    exit(1);
}
' "$script_root/scripts/phpt/expectation.php"
