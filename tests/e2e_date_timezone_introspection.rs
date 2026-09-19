mod common;

use common::run_php;

#[test]
fn timezone_globals_expose_php_85_call_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['timezone_version_get', 'timezone_identifiers_list',
          'timezone_abbreviations_list', 'timezone_name_from_abbr',
          'timezone_open', 'timezone_name_get', 'timezone_location_get',
          'timezone_transitions_get'] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), ':', (string) $function->getReturnType(), ':',
        $function->getExtensionName(), "\n";
}
"#,
        ),
        concat!(
            "timezone_version_get:0/0:string:date\n",
            "timezone_identifiers_list:0/2:array:date\n",
            "timezone_abbreviations_list:0/0:array:date\n",
            "timezone_name_from_abbr:1/3:string|false:date\n",
            "timezone_open:1/1:DateTimeZone|false:date\n",
            "timezone_name_get:1/1:string:date\n",
            "timezone_location_get:1/1:array|false:date\n",
            "timezone_transitions_get:1/3:array|false:date\n",
        )
    );
}

#[test]
fn datetimezone_constants_cover_every_php_group() {
    assert_eq!(
        run_php(
            r#"<?php
$reflection = new ReflectionClass(DateTimeZone::class);
foreach ($reflection->getConstants() as $name => $value) echo "$name=$value|";
"#,
        ),
        concat!(
            "AFRICA=1|AMERICA=2|ANTARCTICA=4|ARCTIC=8|ASIA=16|ATLANTIC=32|",
            "AUSTRALIA=64|EUROPE=128|INDIAN=256|PACIFIC=512|UTC=1024|",
            "ALL=2047|ALL_WITH_BC=4095|PER_COUNTRY=4096|",
        )
    );
}

#[test]
fn identifier_groups_come_from_zone_tab_and_the_full_iana_inventory() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([DateTimeZone::AFRICA, DateTimeZone::AMERICA, DateTimeZone::ANTARCTICA,
          DateTimeZone::ARCTIC, DateTimeZone::ASIA, DateTimeZone::ATLANTIC,
          DateTimeZone::AUSTRALIA, DateTimeZone::EUROPE, DateTimeZone::INDIAN,
          DateTimeZone::PACIFIC, DateTimeZone::UTC, DateTimeZone::ALL,
          DateTimeZone::ALL_WITH_BC] as $group) {
    echo $group, '=', count(timezone_identifiers_list($group)), "\n";
}
echo implode(',', timezone_identifiers_list(DateTimeZone::PER_COUNTRY, 'CZ')), "\n";
echo (int) in_array('US/Eastern', timezone_identifiers_list(DateTimeZone::ALL_WITH_BC), true), "\n";
"#,
        ),
        concat!(
            "1=52\n2=144\n4=11\n8=1\n16=82\n32=10\n64=11\n128=58\n",
            "256=11\n512=38\n1024=1\n2047=419\n4095=598\n",
            "Europe/Prague\n1\n",
        )
    );
}

#[test]
fn datetimezone_preserves_region_abbreviation_and_offset_kinds() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['Europe/Prague', 'UTC', 'CET', 'GMT', '+05:30', '-0430'] as $name) {
    $zone = new DateTimeZone($name);
    $state = $zone->__serialize();
    echo $name, '=', $zone->getName(), ':', $state['timezone_type'], ':',
        $state['timezone'], ':', var_export($zone->getTransitions(0, 1), true), "\n";
}
echo (new DateTimeZone('UTC'))->getTransitions()[0]['time'], "\n";
"#,
        ),
        concat!(
            "Europe/Prague=Europe/Prague:3:Europe/Prague:array (\n  0 => \n  array (\n    'ts' => 0,\n    'time' => '1970-01-01T00:00:00+00:00',\n    'offset' => 3600,\n    'isdst' => false,\n    'abbr' => 'CET',\n  ),\n)\n",
            "UTC=UTC:3:UTC:array (\n  0 => \n  array (\n    'ts' => 0,\n    'time' => '1970-01-01T00:00:00+00:00',\n    'offset' => 0,\n    'isdst' => false,\n    'abbr' => 'UTC',\n  ),\n)\n",
            "CET=CET:2:CET:false\nGMT=GMT:2:GMT:false\n",
            "+05:30=+05:30:1:+05:30:false\n-0430=-04:30:1:-04:30:false\n",
            "-292277022657-01-27T08:29:52+00:00\n",
        )
    );
}

#[test]
fn prague_location_and_transitions_are_byte_exact() {
    assert_eq!(
        run_php(
            r#"<?php
$zone = timezone_open('Europe/Prague');
echo json_encode(timezone_location_get($zone), JSON_PRESERVE_ZERO_FRACTION), "\n";
foreach (timezone_transitions_get($zone, 1704067200, 1735689600) as $transition) {
    echo implode('|', [$transition['ts'], $transition['time'], $transition['offset'],
        (int) $transition['isdst'], $transition['abbr']]), "\n";
}
"#,
        ),
        concat!(
            "{\"country_code\":\"CZ\",\"latitude\":50.08333,\"longitude\":14.43333,\"comments\":\"\"}\n",
            "1704067200|2024-01-01T00:00:00+00:00|3600|0|CET\n",
            "1711846800|2024-03-31T01:00:00+00:00|7200|1|CEST\n",
            "1729990800|2024-10-27T01:00:00+00:00|3600|0|CET\n",
        )
    );
}

#[test]
fn abbreviation_inventory_and_name_resolution_match_php_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
$abbreviations = timezone_abbreviations_list();
echo count($abbreviations), "\n";
foreach ($abbreviations['acst'] as $entry) {
    echo (int) $entry['dst'], ':', $entry['offset'], ':', $entry['timezone_id'], "\n";
}
foreach ([['GMT', -1, -1], ['CET', -1, -1], ['EDT', -1, -1],
          ['ADT', 14400, 0], ['', -14400, 1], ['NOPE', -1, -1]] as $case) {
    var_dump(timezone_name_from_abbr(...$case));
}
"#,
        ),
        concat!(
            "144\n",
            "0:34200:Australia/Adelaide\n0:34200:Australia/Broken_Hill\n",
            "0:34200:Australia/Darwin\n0:34200:Australia/North\n",
            "0:34200:Australia/South\n0:34200:Australia/Yancowinna\n",
            "string(3) \"UTC\"\nstring(13) \"Europe/Berlin\"\n",
            "string(16) \"America/New_York\"\nstring(15) \"America/Halifax\"\n",
            "string(16) \"America/New_York\"\nbool(false)\n",
        )
    );
}

#[test]
fn invalid_constructor_and_procedural_open_keep_distinct_php_diagnostics() {
    assert_eq!(
        run_php(
            r#"<?php
try { new DateTimeZone('No/Such'); } catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
set_error_handler(static function (int $severity, string $message): bool {
    echo $severity, ':', $message, "\n";
    return true;
});
var_dump(timezone_open('No/Such'));
"#,
        ),
        concat!(
            "DateInvalidTimeZoneException:DateTimeZone::__construct(): Unknown or bad timezone (No/Such)\n",
            "2:timezone_open(): Unknown or bad timezone (No/Such)\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn cli_date_timezone_accepts_valid_values_and_warns_before_invalid_requests() {
    let binary = env!("CARGO_BIN_EXE_rphp");
    let valid = std::process::Command::new(binary)
        .args([
            "-ddate.timezone=Europe/Prague",
            "-r",
            "echo date_default_timezone_get();",
        ])
        .output()
        .unwrap();
    assert!(valid.status.success());
    assert_eq!(String::from_utf8(valid.stdout).unwrap(), "Europe/Prague");
    assert_eq!(String::from_utf8(valid.stderr).unwrap(), "");

    let invalid = std::process::Command::new(binary)
        .args([
            "-ddate.timezone=",
            "-r",
            "echo date_default_timezone_get();",
        ])
        .output()
        .unwrap();
    assert!(invalid.status.success());
    assert_eq!(String::from_utf8(invalid.stdout).unwrap(), "UTC");
    assert_eq!(
        String::from_utf8(invalid.stderr).unwrap(),
        "PHP Warning:  PHP Startup: Invalid date.timezone value '', using 'UTC' instead in Unknown on line 0\n"
    );
}
