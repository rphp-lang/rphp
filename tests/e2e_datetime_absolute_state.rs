mod common;

use common::run_php;

#[test]
fn datetime_absolute_construction_and_timezone_projection_match_php() {
    assert_eq!(
        run_php(
            r#"<?php
$date = new DateTime(
    '1970-01-01 00:00:00.125000',
    new DateTimeZone('Europe/Prague'),
);
echo $date->format('Y-m-d H:i:s.u e T O P Z U X x v p'), "\n";
var_dump(
    $date->getTimestamp(),
    $date->getMicrosecond(),
    $date->getOffset(),
    $date->getTimezone()->getName(),
);
"#,
        ),
        concat!(
            "1970-01-01 00:00:00.125000 Europe/Prague CET +0100 +01:00 3600 -3600 +1970 1970 125 +01:00\n",
            "int(-3600)\nint(125000)\nint(3600)\nstring(13) \"Europe/Prague\"\n",
        )
    );
}

#[test]
fn mutable_and_immutable_setters_preserve_their_distinct_identity_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
$mutable = new DateTime('2024-01-31 12:34:56.123456', new DateTimeZone('UTC'));
$same = $mutable
    ->setDate(2024, 2, 31)
    ->setTime(25, 61, 62, 999999)
    ->setISODate(2020, 53, 7)
    ->setTimestamp(0)
    ->setMicrosecond(42);
echo (int) ($mutable === $same), ':', $mutable->format('Y-m-d H:i:s.u'), "\n";

$immutable = new DateTimeImmutable('2024-01-31 12:34:56.123456', new DateTimeZone('UTC'));
$changed = $immutable->setDate(2024, 2, 31)->setTime(1, 2, 3, 4);
echo (int) ($immutable === $changed), ':',
    $immutable->format('Y-m-d H:i:s.u'), ':',
    $changed->format('Y-m-d H:i:s.u'), "\n";
"#,
        ),
        concat!(
            "1:1970-01-01 00:00:00.000042\n",
            "0:2024-01-31 12:34:56.123456:2024-03-02 01:02:03.000004\n",
        )
    );
}

#[test]
fn timestamp_factories_preserve_fractional_and_negative_instants() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([0, 0.125, -0.125, 1700000000.999999] as $timestamp) {
    echo DateTimeImmutable::createFromTimestamp($timestamp)
        ->format('U.u e T P'), '|';
}
"#,
        ),
        concat!(
            "0.000000 +00:00 GMT+0000 +00:00|",
            "0.125000 +00:00 GMT+0000 +00:00|",
            "-1.875000 +00:00 GMT+0000 +00:00|",
            "1700000000.999999 +00:00 GMT+0000 +00:00|",
        )
    );
}

#[test]
fn procedural_datetime_api_shares_object_state() {
    assert_eq!(
        run_php(
            r#"<?php
$date = date_create('2001-02-03 04:05:06.007008', timezone_open('+05:30'));
date_date_set($date, 2002, 3, 4);
date_time_set($date, 5, 6, 7, 8);
date_isodate_set($date, 2009, 1, 1);
date_timestamp_set($date, 0);
date_timezone_set($date, timezone_open('Europe/Prague'));
echo date_format($date, 'Y-m-d H:i:s.u e'), ':',
    date_timestamp_get($date), ':', date_offset_get($date), ':',
    date_timezone_get($date)->getName(), ':',
    timezone_offset_get(timezone_open('Asia/Kolkata'), $date), "\n";
"#,
        ),
        "1970-01-01 01:00:00.000000 Europe/Prague:0:3600:Europe/Prague:19800\n"
    );
}

#[test]
fn datetime_native_state_is_hidden_from_properties_but_used_by_debug_casts() {
    assert_eq!(
        run_php(
            r#"<?php
$date = new DateTimeImmutable(
    '1970-01-01 00:00:00.123456',
    new DateTimeZone('UTC'),
);
var_dump(get_object_vars($date), (array) $date, $date->__serialize());
"#,
        ),
        concat!(
            "array(0) {\n}\n",
            "array(3) {\n",
            "  [\"date\"]=>\n  string(26) \"1970-01-01 00:00:00.123456\"\n",
            "  [\"timezone_type\"]=>\n  int(3)\n",
            "  [\"timezone\"]=>\n  string(3) \"UTC\"\n}\n",
            "array(3) {\n",
            "  [\"date\"]=>\n  string(26) \"1970-01-01 00:00:00.123456\"\n",
            "  [\"timezone_type\"]=>\n  int(3)\n",
            "  [\"timezone\"]=>\n  string(3) \"UTC\"\n}\n",
        )
    );
}

#[test]
fn absolute_datetime_functions_are_reflected_as_date_extension_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'date_create', 'date_create_immutable', 'date_format',
    'date_timestamp_get', 'date_offset_get', 'date_timezone_get',
    'date_timestamp_set', 'date_timezone_set', 'date_date_set',
    'date_time_set', 'date_isodate_set', 'timezone_offset_get',
] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), ':', (string) $function->getReturnType(), ':',
        $function->getExtensionName(), "\n";
}

foreach (['DateTime', 'DateTimeImmutable'] as $class) {
    foreach (['format', 'getTimestamp', 'getMicrosecond', 'getOffset', 'getTimezone',
              'setTimestamp', 'setTimezone', 'setDate', 'setTime', 'setMicrosecond',
              'setISODate', 'createFromTimestamp'] as $method) {
        $reflection = new ReflectionMethod($class, $method);
        echo $class, '::', $method, ':',
            $reflection->getNumberOfRequiredParameters(), '/',
            $reflection->getNumberOfParameters(), ':',
            (string) $reflection->getReturnType(), "\n";
    }
}
"#,
        ),
        concat!(
            "date_create:0/2:DateTime|false:date\n",
            "date_create_immutable:0/2:DateTimeImmutable|false:date\n",
            "date_format:2/2:string:date\n",
            "date_timestamp_get:1/1:int:date\n",
            "date_offset_get:1/1:int:date\n",
            "date_timezone_get:1/1:DateTimeZone|false:date\n",
            "date_timestamp_set:2/2:DateTime:date\n",
            "date_timezone_set:2/2:DateTime:date\n",
            "date_date_set:4/4:DateTime:date\n",
            "date_time_set:3/5:DateTime:date\n",
            "date_isodate_set:3/4:DateTime:date\n",
            "timezone_offset_get:2/2:int:date\n",
            "DateTime::format:1/1:string\n",
            "DateTime::getTimestamp:0/0:int\n",
            "DateTime::getMicrosecond:0/0:int\n",
            "DateTime::getOffset:0/0:int\n",
            "DateTime::getTimezone:0/0:DateTimeZone|false\n",
            "DateTime::setTimestamp:1/1:DateTime\n",
            "DateTime::setTimezone:1/1:DateTime\n",
            "DateTime::setDate:3/3:DateTime\n",
            "DateTime::setTime:2/4:DateTime\n",
            "DateTime::setMicrosecond:1/1:static\n",
            "DateTime::setISODate:2/3:DateTime\n",
            "DateTime::createFromTimestamp:1/1:static\n",
            "DateTimeImmutable::format:1/1:string\n",
            "DateTimeImmutable::getTimestamp:0/0:int\n",
            "DateTimeImmutable::getMicrosecond:0/0:int\n",
            "DateTimeImmutable::getOffset:0/0:int\n",
            "DateTimeImmutable::getTimezone:0/0:DateTimeZone|false\n",
            "DateTimeImmutable::setTimestamp:1/1:DateTimeImmutable\n",
            "DateTimeImmutable::setTimezone:1/1:DateTimeImmutable\n",
            "DateTimeImmutable::setDate:3/3:DateTimeImmutable\n",
            "DateTimeImmutable::setTime:2/4:DateTimeImmutable\n",
            "DateTimeImmutable::setMicrosecond:1/1:static\n",
            "DateTimeImmutable::setISODate:2/3:DateTimeImmutable\n",
            "DateTimeImmutable::createFromTimestamp:1/1:static\n",
        )
    );
}

#[test]
fn date_format_constants_share_php_interface_values() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'ATOM', 'COOKIE', 'ISO8601', 'ISO8601_EXPANDED', 'RFC822', 'RFC850',
    'RFC1036', 'RFC1123', 'RFC7231', 'RFC2822', 'RFC3339',
    'RFC3339_EXTENDED', 'RSS', 'W3C',
] as $name) {
    $global = constant("DATE_$name");
    $interface = constant("DateTimeInterface::$name");
    $mutable = constant("DateTime::$name");
    $immutable = constant("DateTimeImmutable::$name");
    echo $name, '=', $global, ':', (int) ($global === $interface),
        (int) ($global === $mutable), (int) ($global === $immutable), "\n";
}
echo DatePeriod::EXCLUDE_START_DATE, ':', DatePeriod::INCLUDE_END_DATE, "\n";
"#,
        ),
        concat!(
            "ATOM=Y-m-d\\TH:i:sP:111\n",
            "COOKIE=l, d-M-Y H:i:s T:111\n",
            "ISO8601=Y-m-d\\TH:i:sO:111\n",
            "ISO8601_EXPANDED=X-m-d\\TH:i:sP:111\n",
            "RFC822=D, d M y H:i:s O:111\n",
            "RFC850=l, d-M-y H:i:s T:111\n",
            "RFC1036=D, d M y H:i:s O:111\n",
            "RFC1123=D, d M Y H:i:s O:111\n",
            "\nDeprecated: Constant DATE_RFC7231 is deprecated since 8.5, as this format ignores the associated timezone and always uses GMT in <main> on line 7\n",
            "\nDeprecated: Constant DateTimeInterface::RFC7231 is deprecated since 8.5, as this format ignores the associated timezone and always uses GMT in <main> on line 8\n",
            "\nDeprecated: Constant DateTimeInterface::RFC7231 is deprecated since 8.5, as this format ignores the associated timezone and always uses GMT in <main> on line 9\n",
            "\nDeprecated: Constant DateTimeInterface::RFC7231 is deprecated since 8.5, as this format ignores the associated timezone and always uses GMT in <main> on line 10\n",
            "RFC7231=D, d M Y H:i:s \\G\\M\\T:111\n",
            "RFC2822=D, d M Y H:i:s O:111\n",
            "RFC3339=Y-m-d\\TH:i:sP:111\n",
            "RFC3339_EXTENDED=Y-m-d\\TH:i:s.vP:111\n",
            "RSS=D, d M Y H:i:s O:111\n",
            "W3C=Y-m-d\\TH:i:sP:111\n",
            "1:2\n",
        )
    );
}
