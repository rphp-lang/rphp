#!/bin/sh

set -eu

script_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
php_bin=${RPHP_PHPT_REFERENCE_PHP:-php}
if command -v "$php_bin" >/dev/null 2>&1; then
    php_bin=$(command -v "$php_bin")
fi
fixture_copy=$(mktemp -d "${TMPDIR:-/tmp}/rphp-phpt-runner-fixtures.XXXXXX")
wrapper_fixture=$(mktemp -d "${TMPDIR:-/tmp}/rphp-phpt-wrapper-fixtures.XXXXXX")
trap 'rm -rf -- "$fixture_copy" "$wrapper_fixture"' EXIT HUP INT TERM

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
    || classify_failure("Fatal error: Uncaught Error: broken in /tmp/type_declarations/default.php:7\nStack trace:\n#0 {main}", 255) !== "runtime"
    || expectation_profile(["EXPECT" => "Error is ordinary user data"]) !== "ordinary"
    || expectation_profile(["EXPECTF" => "Fatal error: broken in %s on line %d"]) !== "diagnostic"
    || expectation_profile(["EXPECT" => "prefix\nWarning: broken"]) !== "diagnostic"
) {
    fwrite(STDERR, "unexpected execution-phase classification\n");
    exit(1);
}
' "$script_root/scripts/phpt/expectation.php"

"$php_bin" -r '
require $argv[1];
require $argv[2];
require $argv[3];
$supported = "zend.assertions=0\nassert.exception=1";
$unsupported = "zend.assertions=1\nmemory_limit=64M";
if (unsupported_rphp_ini_directives($supported) !== []
    || unsupported_rphp_ini_directives($unsupported) !== ["memory_limit"]
    || target_command("/rphp", "rphp", "test.php", $supported, "") !== [
        "/rphp",
        "-d",
        "zend.assertions=0",
        "-d",
        "assert.exception=1",
        "test.php",
    ]
) {
    fwrite(STDERR, "unexpected RPHP CLI INI capability routing\n");
    exit(1);
}
' "$script_root/scripts/phpt/case.php" \
  "$script_root/scripts/phpt/process.php" \
  "$script_root/scripts/phpt/execution.php"

# Exercise the public wrapper as well as the underlying PHP runner. A supplied
# executable does not expose its Cargo features, so an unset label must match
# the documented default-feature contract build.
mkdir -p "$wrapper_fixture/Zend/tests" "$wrapper_fixture/tests/lang"
cp -R "$script_root/tests/php-src/runner-fixtures/." "$wrapper_fixture/Zend/tests/"
git -C "$wrapper_fixture" init -q
git -C "$wrapper_fixture" config user.name 'RPHP PHPT test'
git -C "$wrapper_fixture" config user.email 'phpt-test@example.invalid'
git -C "$wrapper_fixture" add Zend tests
git -C "$wrapper_fixture" -c commit.gpgsign=false commit -qm 'Create runner fixture'
wrapper_commit=$(git -C "$wrapper_fixture" rev-parse HEAD)
RPHP_PHPT_PHP_SRC_COMMIT=$wrapper_commit \
RPHP_PHPT_REFERENCE_PHP=$php_bin \
    "$script_root/scripts/run-php-src-phpt.sh" \
    "$wrapper_fixture" "$php_bin" "$wrapper_fixture/results" 1
"$php_bin" -r '
$summary = json_decode(file_get_contents($argv[1]), true, flags: JSON_THROW_ON_ERROR);
if ($summary["features"] !== "default") {
    fwrite(STDERR, "unexpected default PHPT feature label\n");
    exit(1);
}
' "$wrapper_fixture/results/summary.json"
