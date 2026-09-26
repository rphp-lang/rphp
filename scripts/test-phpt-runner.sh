#!/bin/sh

set -eu

script_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
php_bin=${RPHP_PHPT_REFERENCE_PHP:-php}
if command -v "$php_bin" >/dev/null 2>&1; then
    php_bin=$(command -v "$php_bin")
fi
"$php_bin" "$script_root/tests/php-src/runner-process.php"
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
    || $summary["total"] !== 15
    || $summary["statuses"]["pass"] !== 13
    || $summary["statuses"]["skip"] !== 1
    || $summary["statuses"]["xfail"] !== 1
    || array_sum(array_intersect_key(
        $summary["statuses"],
        array_flip(["fail", "unsupported", "timeout", "crash"]),
    )) !== 0
    || $summary["execution_profile"] !== [
        "attempted" => 13,
        "pre_execution_failed" => 0,
        "front_end_rejected" => 2,
        "runtime_reached" => 11,
        "runtime_reach_rate" => 11 / 13,
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
            "pass" => 11,
            "fail" => 0,
            "skip" => 1,
            "xfail" => 1,
            "unsupported" => 0,
            "timeout" => 0,
            "crash" => 0,
            "total" => 13,
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
    || $map["total"] !== 15
    || $groupTotal !== 15
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
    || preg_match("~\\A" . expectf_pattern("literal %% %0") . "\\z~sD", "literal %% \x00") !== 1
    || preg_match("~\\A" . expectf_pattern("literal %% %0") . "\\z~sD", "literal % \x00") !== 0
) {
    fwrite(STDERR, "unexpected execution-phase classification\n");
    exit(1);
}
' "$script_root/scripts/phpt/expectation.php"

"$php_bin" -r '
require $argv[1];
require $argv[2];
require $argv[3];
$supported = "zend.assertions=0\nassert.exception=1\ndate.timezone=UTC\nerror_reporting=E_ALL\nprecision=17\nserialize_precision=17\nzend.exception_ignore_args=1\nzend.exception_string_param_max_len=23";
$highlightSupported = "highlight.string=#DD0000\nhighlight.comment=#FF8000\nhighlight.keyword=#007700\nhighlight.default=#0000BB\nhighlight.html=#000000";
$unsupported = "zend.assertions=1\nmemory_limit=64M";
$diagnostics = "display_errors=stderr\nlog_errors=1\nhtml_errors=1\nignore_repeated_errors=1\nignore_repeated_source=1\nfatal_error_backtraces=1\ndocref_root=/manual/\ndocref_ext=.php\nerror_log=\n";
$binaryRecord = json_decode(
    encode_manifest_record(["actual_excerpt" => "\xFF"]),
    true,
    flags: JSON_THROW_ON_ERROR,
);
if (unsupported_rphp_ini_directives($supported) !== []
    || unsupported_rphp_ini_directives($diagnostics) !== []
    || unsupported_rphp_ini_directives("include_path=first:second\n") !== []
    || unsupported_rphp_ini_directives("include_path=.\nauto_prepend_file=pre.php\nauto_append_file=post.php\n") !== []
    || unsupported_rphp_ini_directives("auto_prepend_file=pre.php\nmemory_limit=64M\n") !== ["memory_limit"]
    || target_command("/rphp", "rphp", "test.php", "include_path=first:second\n", "") !== [
        "/rphp", "-d", "fatal_error_backtraces=0", "-d", "docref_ext=.html", "-d", "include_path=first:second", "test.php",
    ]
    || unsupported_rphp_ini_directives("error_log=relative.log\n") !== []
    || unsupported_rphp_ini_directives("error_log=syslog\n") !== ["error_log"]
    || unsupported_rphp_ini_directives("error_log=\"syslog\"\n") !== ["error_log"]
    || target_command("/rphp", "rphp", "test.php", "fatal_error_backtraces=1\ndocref_ext=.php\n", "") !== [
        "/rphp", "-d", "fatal_error_backtraces=0", "-d", "docref_ext=.html",
        "-d", "fatal_error_backtraces=1", "-d", "docref_ext=.php", "test.php",
    ]
    || unsupported_rphp_ini_directives("output_handler=\n") !== []
    || unsupported_rphp_ini_directives("output_handler=htmlspecialchars\n") !== []
    || unsupported_rphp_ini_directives("output_handler=\nfilter.default=special_chars\n") !== ["filter.default"]
    || target_command("/rphp", "rphp", "test.php", "output_handler=htmlspecialchars\n", "") !== [
        "/rphp", "-d", "fatal_error_backtraces=0", "-d", "docref_ext=.html", "-d", "output_handler=htmlspecialchars", "test.php",
    ]
    || unsupported_rphp_ini_directives("disable_functions=strlen,count\nvariables_order=EGPCS\n") !== []
    || target_command("/rphp", "rphp", "test.php", "disable_functions=strlen,count\nvariables_order=EGPCS\n", "") !== [
        "/rphp", "-d", "fatal_error_backtraces=0", "-d", "docref_ext=.html", "-d", "disable_functions=strlen,count", "-d", "variables_order=EGPCS", "test.php",
    ]
    || unsupported_rphp_ini_directives("zend.enable_gc=0\n") !== []
    || unsupported_rphp_ini_directives("zend.enable_gc=1\n") !== []
    || target_command("/rphp", "rphp", "test.php", "zend.enable_gc=0\n", "") !== [
        "/rphp", "-d", "fatal_error_backtraces=0", "-d", "docref_ext=.html", "-d", "zend.enable_gc=0", "test.php",
    ]
    || unsupported_rphp_ini_directives("allow_url_fopen=0\n") !== []
    || unsupported_rphp_ini_directives("allow_url_include=1\n") !== ["allow_url_include"]
    || unsupported_rphp_ini_directives($highlightSupported) !== []
    || unsupported_rphp_ini_directives($unsupported) !== ["memory_limit"]
    || $binaryRecord !== ["actual_excerpt" => "\u{FFFD}"]
    || target_command("/rphp", "rphp", "test.php", $supported, "") !== [
        "/rphp",
        "-d", "fatal_error_backtraces=0", "-d", "docref_ext=.html",
        "-d",
        "zend.assertions=0",
        "-d",
        "assert.exception=1",
        "-d",
        "date.timezone=UTC",
        "-d",
        "error_reporting=E_ALL",
        "-d",
        "precision=17",
        "-d",
        "serialize_precision=17",
        "-d",
        "zend.exception_ignore_args=1",
        "-d",
        "zend.exception_string_param_max_len=23",
        "test.php",
    ]
) {
    fwrite(STDERR, "unexpected RPHP CLI INI capability routing\n");
    exit(1);
}
' "$script_root/scripts/phpt/case.php" \
  "$script_root/scripts/phpt/process.php" \
  "$script_root/scripts/phpt/execution.php"

# Optional optimizer preferences must not pretend that its extension exists,
# admit arbitrary optimizer directives, or rewrite a test's INI arguments.
"$php_bin" -r '
require $argv[1];
require $argv[2];
require $argv[3];
$preferences = "opcache.enable=1\nopcache.enable_cli=0\nopcache.optimization_level=-1\nopcache.save_comments=1\n";
if (unsupported_rphp_ini_directives($preferences) !== []
    || target_command("/rphp", "rphp", "unit.php", $preferences, "") !== [
        "/rphp", "-d", "fatal_error_backtraces=0", "-d", "docref_ext=.html",
        "-d", "opcache.enable=1", "-d", "opcache.enable_cli=0",
        "-d", "opcache.optimization_level=-1", "-d", "opcache.save_comments=1",
        "unit.php",
    ]
) {
    fwrite(STDERR, "unexpected optional optimizer preference routing\n");
    exit(1);
}
foreach (["opcache.preload", "opcache.file_cache", "opcache.jit", "opcache.unknown", "opcache.save_comments_extra"] as $key) {
    if (unsupported_rphp_ini_directives($preferences . "$key=1\n") !== [$key]) {
        fwrite(STDERR, "unsupported optimizer directive was admitted\n");
        exit(1);
    }
}
' "$script_root/scripts/phpt/case.php" \
  "$script_root/scripts/phpt/process.php" \
  "$script_root/scripts/phpt/execution.php"

"$php_bin" -r '
const RPHP_PHPT_SUPPORTED_SECTIONS = ["TEST", "INI", "EXTENSIONS", "FILE", "EXPECT"];
require $argv[1];
require $argv[2];
require $argv[3];
require $argv[4];
$optional = "--TEST--\nOriginal optional optimizer preference\n--INI--\nopcache.enable_cli=0\n--FILE--\n<?php echo 11 + 7; ?>\n--EXPECT--\n18\n";
$file = $argv[5] . "/optional-optimizer.phpt";
file_put_contents($file, $optional);
$result = run_test($file, "optional-optimizer.phpt", $argv[6], "rphp", 3, []);
if ($result["status"] !== "pass") {
    fwrite(STDERR, "ordinary core body with optimizer preference did not run\n");
    exit(1);
}
$required = str_replace("--INI--", "--EXTENSIONS--\nOPcache\n--INI--", $optional);
file_put_contents($file, $required);
$result = run_test($file, "required-optimizer.phpt", "/not-invoked", "rphp", 3, []);
if ($result["status"] !== "skip"
    || $result["category"] !== "extension"
    || $result["reason"] !== "required extension unavailable: opcache"
) {
    fwrite(STDERR, "optimizer extension requirement was bypassed\n");
    exit(1);
}
unlink($file);
echo "Optional optimizer admission tests passed\n";
' "$script_root/scripts/phpt/case.php" \
  "$script_root/scripts/phpt/process.php" \
  "$script_root/scripts/phpt/expectation.php" \
  "$script_root/scripts/phpt/execution.php" "$fixture_copy" "$php_bin"

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
