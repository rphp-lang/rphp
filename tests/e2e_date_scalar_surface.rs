mod common;

use common::run_php;

#[test]
fn date_scalar_surface_exposes_php_85_signatures() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['checkdate', 'idate', 'localtime', 'getdate', 'gmmktime',
          'date_default_timezone_get', 'date_default_timezone_set',
          'date', 'gmdate', 'mktime', 'time'] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), ':', (string) $function->getReturnType(), ':',
        $function->getExtensionName();
    foreach ($function->getParameters() as $parameter) {
        echo ':$', $parameter->getName(), '=', (string) $parameter->getType(), '/',
            $parameter->isDefaultValueAvailable()
                ? var_export($parameter->getDefaultValue(), true)
                : 'REQ';
    }
    echo "\n";
}

"#,
        ),
        concat!(
            "checkdate:3/3:bool:date:$month=int/REQ:$day=int/REQ:$year=int/REQ\n",
            "idate:1/2:int|false:date:$format=string/REQ:$timestamp=?int/NULL\n",
            "localtime:0/2:array:date:$timestamp=?int/NULL:$associative=bool/false\n",
            "getdate:0/1:array:date:$timestamp=?int/NULL\n",
            "gmmktime:1/6:int|false:date:$hour=int/REQ:$minute=?int/NULL:$second=?int/NULL:$month=?int/NULL:$day=?int/NULL:$year=?int/NULL\n",
            "date_default_timezone_get:0/0:string:date\n",
            "date_default_timezone_set:1/1:bool:date:$timezoneId=string/REQ\n",
            "date:1/2:string:date:$format=string/REQ:$timestamp=?int/NULL\n",
            "gmdate:1/2:string:date:$format=string/REQ:$timestamp=?int/NULL\n",
            "mktime:1/6:int|false:date:$hour=int/REQ:$minute=?int/NULL:$second=?int/NULL:$month=?int/NULL:$day=?int/NULL:$year=?int/NULL\n",
            "time:0/0:int:date\n",
        )
    );
}

#[test]
fn complete_date_surface_is_advertised_as_a_loaded_extension() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(extension_loaded('date'));
var_dump(in_array('date', get_loaded_extensions(), true));
echo (new ReflectionFunction('checkdate'))->getExtensionName(), "\n";
"#,
        ),
        "bool(true)\nbool(true)\ndate\n"
    );
}

#[test]
fn checkdate_covers_calendar_and_php_year_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([[1,0,2006], [1,32,2006], [0,1,2006], [13,1,2006],
          [2,29,2024], [2,29,2023], [1,1,1], [1,1,32767], [1,1,32768]] as $date) {
    echo (int) checkdate(...$date);
}
"#,
        ),
        "000010110"
    );
}

#[test]
fn gmmktime_normalizes_components_like_php() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
foreach ([[0,0,0,1,0,2006], [0,0,0,13,1,2005], [25,61,61,1,1,1970],
          [8,8,8,8,8,2008]] as $parts) {
    echo gmmktime(...$parts), '|';
}
"#,
        ),
        "1135987200|1136073600|93721|1218182888|"
    );
}

#[test]
fn mktime_two_digit_year_window_matches_php() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
foreach ([0, 1, 69, 70, 99, 100, 101, -1] as $year) {
    $timestamp = gmmktime(0, 0, 0, 1, 1, $year);
    echo $year, '=', $timestamp, "\n";
}
"#,
        ),
        concat!(
            "0=946684800\n",
            "1=978307200\n",
            "69=3124224000\n",
            "70=0\n",
            "99=915148800\n",
            "100=946684800\n",
            "101=-58979923200\n",
            "-1=-62198755200\n",
        )
    );
}

#[test]
fn localtime_and_getdate_project_fixed_timestamp_fields() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
$timestamp = gmmktime(0, 0, 0, 1, 1, 1970);
echo implode(',', localtime($timestamp)), "\n";
echo implode(',', localtime($timestamp, true)), "\n";
echo implode('|', getdate($timestamp)), "\n";
"#,
        ),
        concat!(
            "0,0,0,1,0,70,4,0,0\n",
            "0,0,0,1,0,70,4,0,0\n",
            "0|0|0|1|4|1|1970|0|Thursday|January|0\n",
        )
    );
}

#[test]
fn idate_supports_every_php_85_integer_format() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
foreach (str_split('BdhHiILmNosUtUwWYo yzZ') as $format) {
    if ($format !== ' ') echo $format, '=', idate($format, 0), '|';
}
"#,
        ),
        "B=41|d=1|h=12|H=0|i=0|I=0|L=0|m=1|N=4|o=1970|s=0|U=0|t=31|U=0|w=4|W=1|Y=1970|o=1970|y=70|z=0|Z=0|"
    );
}

#[test]
fn idate_invalid_formats_warn_and_return_false() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function (int $severity, string $message): bool {
    echo $severity, ':', $message, "\n";
    return true;
});
foreach (['', 'YY', 'q'] as $format) var_dump(idate($format, 0));
"#,
        ),
        concat!(
            "2:idate(): idate format is one char\n",
            "bool(false)\n",
            "2:idate(): idate format is one char\n",
            "bool(false)\n",
            "2:idate(): Unrecognized date format token\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn date_formats_utc_and_supported_fixed_offset_zones() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
echo date('jS e T O P Z c r W o B', 0), "\n";
date_default_timezone_set('Asia/Kolkata');
echo date('Y-m-d H:i:s e T O P Z', 0), "\n";
echo gmdate('Y-m-d H:i:s e T O P Z', 0), "\n";
"#,
        ),
        concat!(
            "1st UTC UTC +0000 +00:00 0 1970-01-01T00:00:00+00:00 Thu, 01 Jan 1970 00:00:00 +0000 01 1970 041\n",
            "1970-01-01 05:30:00 Asia/Kolkata IST +0530 +05:30 19800\n",
            "1970-01-01 00:00:00 UTC GMT +0000 +00:00 0\n",
        )
    );
}

#[test]
fn timezone_set_rejects_unknown_ids_without_mutating_state() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
set_error_handler(static function (int $severity, string $message): bool {
    echo $severity, ':', $message, "\n";
    return true;
});
var_dump(date_default_timezone_set('No/Such'), date_default_timezone_get());
restore_error_handler();
var_dump(date_default_timezone_set('GMT'), date_default_timezone_get());
"#,
        ),
        concat!(
            "8:date_default_timezone_set(): Timezone ID 'No/Such' is invalid\n",
            "bool(false)\n",
            "string(3) \"UTC\"\n",
            "bool(true)\n",
            "string(3) \"GMT\"\n",
        )
    );
}

#[test]
fn date_scalar_types_follow_weak_and_strict_internal_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(checkdate('2', '29', '2024'), date(89, 0));
try {
    eval('declare(strict_types=1); checkdate("2", 29, 2024);');
} catch (Throwable $error) {
    echo $error::class, ':', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "bool(true)\n",
            "string(2) \"89\"\n",
            "TypeError:checkdate(): Argument #1 ($month) must be of type int, string given\n",
        )
    );
}
