mod common;

use common::{run_php, run_php_bytes};

#[test]
fn calendar_exports_all_php_85_global_signatures_and_extension_identity() {
    assert_eq!(
        run_php(
            r#"<?php
$functions = [
    'cal_days_in_month', 'cal_from_jd', 'cal_info', 'cal_to_jd',
    'easter_date', 'easter_days', 'frenchtojd', 'gregoriantojd',
    'jddayofweek', 'jdmonthname', 'jdtofrench', 'jdtogregorian',
    'jdtojewish', 'jdtojulian', 'jdtounix', 'jewishtojd',
    'juliantojd', 'unixtojd',
];
foreach ($functions as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getExtensionName(), ':',
        $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), ':', (string) $function->getReturnType(), '|';
}
"#,
        ),
        concat!(
            "cal_days_in_month:calendar:3/3:int|cal_from_jd:calendar:2/2:array|",
            "cal_info:calendar:0/1:array|cal_to_jd:calendar:4/4:int|",
            "easter_date:calendar:0/2:int|easter_days:calendar:0/2:int|",
            "frenchtojd:calendar:3/3:int|gregoriantojd:calendar:3/3:int|",
            "jddayofweek:calendar:1/2:string|int|jdmonthname:calendar:2/2:string|",
            "jdtofrench:calendar:1/1:string|jdtogregorian:calendar:1/1:string|",
            "jdtojewish:calendar:1/3:string|jdtojulian:calendar:1/1:string|",
            "jdtounix:calendar:1/1:int|jewishtojd:calendar:3/3:int|",
            "juliantojd:calendar:3/3:int|unixtojd:calendar:0/1:int|false|",
        )
    );
}

#[test]
fn calendar_constants_and_core_discovery_are_public_and_case_insensitive() {
    assert_eq!(
        run_php(
            r#"<?php
$names = [
    'CAL_GREGORIAN','CAL_JULIAN','CAL_JEWISH','CAL_FRENCH','CAL_NUM_CALS',
    'CAL_DOW_DAYNO','CAL_DOW_LONG','CAL_DOW_SHORT',
    'CAL_MONTH_GREGORIAN_SHORT','CAL_MONTH_GREGORIAN_LONG',
    'CAL_MONTH_JULIAN_SHORT','CAL_MONTH_JULIAN_LONG','CAL_MONTH_JEWISH','CAL_MONTH_FRENCH',
    'CAL_EASTER_DEFAULT','CAL_EASTER_ROMAN','CAL_EASTER_ALWAYS_GREGORIAN','CAL_EASTER_ALWAYS_JULIAN',
    'CAL_JEWISH_ADD_ALAFIM_GERESH','CAL_JEWISH_ADD_ALAFIM','CAL_JEWISH_ADD_GERESHAYIM',
];
foreach ($names as $name) echo $name, '=', constant($name), '|';
echo "\n", (int) extension_loaded('calendar'), (int) extension_loaded('Calendar'),
    (int) extension_loaded('standard'), '|', implode(',', get_loaded_extensions()), '|',
    count(get_loaded_extensions(true)), "\n";
"#,
        ),
        concat!(
            "CAL_GREGORIAN=0|CAL_JULIAN=1|CAL_JEWISH=2|CAL_FRENCH=3|CAL_NUM_CALS=4|",
            "CAL_DOW_DAYNO=0|CAL_DOW_LONG=1|CAL_DOW_SHORT=2|",
            "CAL_MONTH_GREGORIAN_SHORT=0|CAL_MONTH_GREGORIAN_LONG=1|",
            "CAL_MONTH_JULIAN_SHORT=2|CAL_MONTH_JULIAN_LONG=3|CAL_MONTH_JEWISH=4|CAL_MONTH_FRENCH=5|",
            "CAL_EASTER_DEFAULT=0|CAL_EASTER_ROMAN=1|CAL_EASTER_ALWAYS_GREGORIAN=2|",
            "CAL_EASTER_ALWAYS_JULIAN=3|CAL_JEWISH_ADD_ALAFIM_GERESH=2|",
            "CAL_JEWISH_ADD_ALAFIM=4|CAL_JEWISH_ADD_GERESHAYIM=8|\n",
            "110|calendar,gettext,iconv,tokenizer|0\n",
        )
    );
}

#[test]
fn gregorian_and_julian_conversions_share_bc_leap_and_overflow_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
$cases = [
    [1, 1, 1970], [2, 29, 2000], [2, 29, 1900], [10, 5, 1582], [11, 25, -4714],
];
foreach ($cases as [$month, $day, $year]) {
    $gregorian = gregoriantojd($month, $day, $year);
    $julian = juliantojd($month, $day, $year);
    echo $gregorian, ':', jdtogregorian($gregorian), '|',
        $julian, ':', jdtojulian($julian), "\n";
}
echo gregoriantojd(13, 1, 2025), ':', juliantojd(1, 0, 2025), ':',
    jdtogregorian(PHP_INT_MAX), ':', jdtojulian(PHP_INT_MAX), "\n";
"#,
        ),
        concat!(
            "2440588:1/1/1970|2440601:1/1/1970\n",
            "2451604:2/29/2000|2451617:2/29/2000\n",
            "2415080:3/1/1900|2415092:2/29/1900\n",
            "2299151:10/5/1582|2299161:10/5/1582\n",
            "1:11/25/-4714|0:0/0/0\n",
            "0:0:0/0/0:0/0/0\n",
        )
    );
}

#[test]
fn french_and_hebrew_calendars_round_trip_variable_years() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([[1,1,1],[13,5,14],[13,6,14],[1,1,15]] as [$m,$d,$y]) {
    $jd = frenchtojd($m,$d,$y);
    echo $jd, ':', jdtofrench($jd), '|';
}
echo "\n";
foreach ([[1,1,1],[1,1,5785],[6,1,5784],[6,1,5785],[13,29,5785]] as [$m,$d,$y]) {
    $jd = jewishtojd($m,$d,$y);
    echo $jd, ':', jdtojewish($jd), '|';
}
echo "\n";
"#,
        ),
        concat!(
            "2375840:1/1/1|2380952:13/5/14|2380953:0/0/0|0:0/0/0|\n",
            "347998:1/1/1|2460587:1/1/5785|2460351:6/1/5784|",
            "2460736:7/1/5785|2460941:13/29/5785|\n",
        )
    );
}

#[test]
fn calendar_day_counts_track_civil_and_hebrew_year_shapes() {
    assert_eq!(
        run_php(
            r#"<?php
$cases = [
    [CAL_GREGORIAN,2,2000], [CAL_GREGORIAN,2,1900],
    [CAL_JULIAN,2,1900], [CAL_FRENCH,13,3], [CAL_FRENCH,13,14],
    [CAL_JEWISH,2,5771], [CAL_JEWISH,3,5772],
    [CAL_JEWISH,6,5784], [CAL_JEWISH,6,5785],
];
foreach ($cases as $case) echo cal_days_in_month(...$case), '|';
try { cal_days_in_month(CAL_GREGORIAN, 13, 2025); }
catch (ValueError $error) { echo $error->getMessage(); }
echo "\n";
"#,
        ),
        "29|28|29|6|5|30|30|30|0|Invalid date\n"
    );
}

#[test]
fn generic_calendar_projection_has_canonical_keys_names_and_weekdays() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([CAL_GREGORIAN,CAL_JULIAN,CAL_JEWISH,CAL_FRENCH] as $calendar) {
    $value = cal_from_jd(2460587, $calendar);
    echo implode(',', array_keys($value)), '|', $value['date'], '|',
        $value['dow'], ':', $value['abbrevdayname'], ':', $value['dayname'], '|',
        $value['abbrevmonth'], ':', $value['monthname'], "\n";
}
foreach ([CAL_DOW_DAYNO,CAL_DOW_LONG,CAL_DOW_SHORT,99] as $mode) {
    echo jddayofweek(2440588, $mode), '|';
}
echo "\n";
"#,
        ),
        concat!(
            "date,month,day,year,dow,abbrevdayname,dayname,abbrevmonth,monthname|10/3/2024|4:Thu:Thursday|Oct:October\n",
            "date,month,day,year,dow,abbrevdayname,dayname,abbrevmonth,monthname|9/20/2024|4:Thu:Thursday|Sep:September\n",
            "date,month,day,year,dow,abbrevdayname,dayname,abbrevmonth,monthname|1/1/5785|4:Thu:Thursday|Tishri:Tishri\n",
            "date,month,day,year,dow,abbrevdayname,dayname,abbrevmonth,monthname|0/0/0|4:Thu:Thursday|:\n",
            "4|Thursday|Thu|4|\n",
        )
    );
}

#[test]
fn calendar_info_publishes_stable_month_tables_and_symbols() {
    assert_eq!(
        run_php(
            r#"<?php
$all = cal_info();
echo count($all), ':', implode(',', array_keys($all)), "\n";
foreach ($all as $id => $info) {
    echo $id, ':', implode(',', array_keys($info)), ':', count($info['months']), ':',
        $info['months'][1], ':', $info['months'][array_key_last($info['months'])], ':',
        $info['maxdaysinmonth'], ':', $info['calname'], ':', $info['calsymbol'], "\n";
}
"#,
        ),
        concat!(
            "4:0,1,2,3\n",
            "0:months,abbrevmonths,maxdaysinmonth,calname,calsymbol:12:January:December:31:Gregorian:CAL_GREGORIAN\n",
            "1:months,abbrevmonths,maxdaysinmonth,calname,calsymbol:12:January:December:31:Julian:CAL_JULIAN\n",
            "2:months,abbrevmonths,maxdaysinmonth,calname,calsymbol:13:Tishri:Elul:30:Jewish:CAL_JEWISH\n",
            "3:months,abbrevmonths,maxdaysinmonth,calname,calsymbol:13:Vendemiaire:Extra:30:French:CAL_FRENCH\n",
        )
    );
}

#[test]
fn easter_modes_cover_both_historical_cutovers_and_timestamp_projection() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([1582,1583,1752,1753,2025] as $year) {
    echo $year, ':';
    foreach ([CAL_EASTER_DEFAULT,CAL_EASTER_ROMAN,CAL_EASTER_ALWAYS_GREGORIAN,CAL_EASTER_ALWAYS_JULIAN] as $mode) {
        echo easter_days($year, $mode), '|';
    }
    echo "\n";
}
foreach ([2000,2001,2002,2047] as $year) echo gmdate('Y-m-d', easter_date($year)), '|';
echo "\n";
"#,
        ),
        concat!(
            "1582:25|25|28|25|\n1583:10|20|20|10|\n",
            "1752:8|12|12|8|\n1753:32|32|32|21|\n2025:30|30|30|17|\n",
            "2000-04-23|2001-04-15|2002-03-31|2047-04-14|\n",
        )
    );
}

#[test]
fn unix_julian_projection_preserves_ranges_and_nullable_defaults() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([0,1,40000,86400,1000000000,1152459009] as $timestamp) {
    $jd = unixtojd($timestamp);
    echo $timestamp, ':', $jd, ':', jdtounix($jd), '|';
}
echo "\n", (int) is_int(unixtojd(null)), '|';
foreach ([2440587,106751993607889] as $jd) {
    try { jdtounix($jd); }
    catch (ValueError $error) { echo $error->getMessage(), '|'; }
}
try { unixtojd(-1); }
catch (ValueError $error) { echo $error->getMessage(); }
echo "\n";
"#,
        ),
        concat!(
            "0:2440588:0|1:2440588:0|40000:2440588:0|86400:2440589:86400|",
            "1000000000:2452162:999993600|1152459009:2453926:1152403200|\n",
            "1|jday must be between 2440588 and 106751993607888|",
            "jday must be between 2440588 and 106751993607888|",
            "unixtojd(): Argument #1 ($timestamp) must be greater than or equal to 0\n",
        )
    );
}

#[test]
fn hebrew_rendering_keeps_php_iso_8859_8_bytes_and_punctuation_flags() {
    assert_eq!(
        run_php_bytes(
            r#"<?php
$jd = jewishtojd(1, 1, 5785);
foreach ([0, CAL_JEWISH_ADD_ALAFIM_GERESH | CAL_JEWISH_ADD_GERESHAYIM,
          CAL_JEWISH_ADD_ALAFIM | CAL_JEWISH_ADD_GERESHAYIM] as $flags) {
    echo bin2hex(jdtojewish($jd, true, $flags)), "\n";
}
"#,
        ),
        concat!(
            "e020faf9f8e920e4faf9f4e4\n",
            "e02720faf9f8e920e427faf9f422e4\n",
            "e02720faf9f8e920e420e0ecf4e9ed20faf9f422e4\n",
        )
        .as_bytes()
    );
}

#[test]
fn calendar_value_errors_and_strict_types_are_observable_before_arithmetic() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
$calls = [
    static fn() => cal_days_in_month(4, 1, 2025),
    static fn() => cal_from_jd(2440588, -1),
    static fn() => cal_info(4),
    static fn() => easter_date(1969),
    static fn() => cal_to_jd(CAL_GREGORIAN, '1', 1, 2025),
];
foreach ($calls as $call) {
    try { $call(); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "ValueError:cal_days_in_month(): Argument #1 ($calendar) must be a valid calendar ID\n",
            "ValueError:cal_from_jd(): Argument #2 ($calendar) must be a valid calendar ID\n",
            "ValueError:cal_info(): Argument #1 ($calendar) must be a valid calendar ID\n",
            "ValueError:easter_date(): Argument #1 ($year) must be a year after 1970 (inclusive)\n",
            "TypeError:cal_to_jd(): Argument #2 ($month) must be of type int, string given\n",
        )
    );
}
