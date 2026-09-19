mod common;

use common::run_php;

#[test]
fn legacy_strftime_uses_the_request_timezone_without_libc() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('Europe/Prague');
echo strftime('%Y-%m-%d %H:%M:%S %A %B %z %Z %% %s', 0), "\n";
echo gmstrftime('%Y-%m-%d %H:%M:%S %A %B %z %Z %% %s', 0), "\n";
"#,
        ),
        concat!(
            "\nDeprecated: Function strftime() is deprecated since 8.1, use IntlDateFormatter::format() instead in <main> on line 3\n",
            "1970-01-01 01:00:00 Thursday January +0100 CET % 0\n",
            "\nDeprecated: Function gmstrftime() is deprecated since 8.1, use IntlDateFormatter::format() instead in <main> on line 4\n",
            "1970-01-01 00:00:00 Thursday January +0000 GMT % -3600\n",
        )
    );
}

#[test]
fn solar_functions_share_the_independent_astronomy_core() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL & ~E_DEPRECATED);
date_default_timezone_set('Europe/Prague');
foreach ([SUNFUNCS_RET_TIMESTAMP, SUNFUNCS_RET_STRING, SUNFUNCS_RET_DOUBLE] as $format) {
    $rise = date_sunrise(1165881600, $format, 31.7667, 35.2333, 90.833333, 2.0);
    $set = date_sunset(1165881600, $format, 31.7667, 35.2333, 90.833333, 2.0);
    if ($format === SUNFUNCS_RET_DOUBLE) {
        printf("%.6f|%.6f\n", $rise, $set);
    } else {
        echo $rise, '|', $set, "\n";
    }
}
foreach (date_sun_info(1165881600, 31.7667, 35.2333) as $name => $value) {
    echo $name, '=', $value === false ? 'false' : $value, "\n";
}
"#,
        ),
        concat!(
            "1165897682|1165934239\n",
            "06:28|16:37\n",
            "6.467453|16.622174\n",
            "sunrise=1165897761\n",
            "sunset=1165934160\n",
            "transit=1165915961\n",
            "civil_twilight_begin=1165896156\n",
            "civil_twilight_end=1165935765\n",
            "nautical_twilight_begin=1165894334\n",
            "nautical_twilight_end=1165937588\n",
            "astronomical_twilight_begin=1165892551\n",
            "astronomical_twilight_end=1165939371\n",
        )
    );
}

#[test]
fn remaining_date_globals_publish_php_85_signatures() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'date_add', 'date_create_from_format', 'date_create_immutable_from_format',
    'date_diff', 'date_get_last_errors', 'date_interval_create_from_date_string',
    'date_interval_format', 'date_modify', 'date_parse', 'date_parse_from_format',
    'date_sub', 'date_sun_info', 'date_sunrise', 'date_sunset', 'gmstrftime',
    'strftime', 'strtotime',
] as $name) {
    $r = new ReflectionFunction($name);
    echo $name, ':', $r->getNumberOfRequiredParameters(), '/', $r->getNumberOfParameters(),
        ':', (string) $r->getReturnType(), ':', $r->getExtensionName(), "\n";
}
"#,
        ),
        concat!(
            "date_add:2/2:DateTime:date\n",
            "date_create_from_format:2/3:DateTime|false:date\n",
            "date_create_immutable_from_format:2/3:DateTimeImmutable|false:date\n",
            "date_diff:2/3:DateInterval:date\n",
            "date_get_last_errors:0/0:array|false:date\n",
            "date_interval_create_from_date_string:1/1:DateInterval|false:date\n",
            "date_interval_format:2/2:string:date\n",
            "date_modify:2/2:DateTime|false:date\n",
            "date_parse:1/1:array:date\n",
            "date_parse_from_format:2/2:array:date\n",
            "date_sub:2/2:DateTime:date\n",
            "date_sun_info:3/3:array:date\n",
            "date_sunrise:1/6:string|int|float|false:date\n",
            "date_sunset:1/6:string|int|float|false:date\n",
            "gmstrftime:1/2:string|false:date\n",
            "strftime:1/2:string|false:date\n",
            "strtotime:1/2:int|false:date\n",
        )
    );
}
