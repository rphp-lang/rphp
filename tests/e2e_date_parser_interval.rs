mod common;

use common::run_php;

#[test]
fn interval_arithmetic_formatting_and_diff_share_civil_state() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
$i = new DateInterval('P1Y2M3DT4H5M6S');
echo $i->format('%R%Y-%M-%D %H:%I:%S.%F %a'), "\n";
$a = new DateTimeImmutable('2000-01-01 00:00:00.500000 UTC');
$b = $a->add($i);
$c = $b->sub($i);
echo $b->format('Y-m-d H:i:s.u'), '|', $c->format('Y-m-d H:i:s.u'), "\n";
$d = $a->diff(new DateTimeImmutable('2001-03-04 04:05:06.750000 UTC'));
echo $d->format('%R%y/%m/%d %h:%i:%s.%F %a'), "\n";
$mutable = new DateInterval('P7D');
var_dump($mutable->invert = true, $mutable->invert);
echo (new DateTimeImmutable('2009-01-14 UTC'))->add($mutable)->format('Y-m-d'), "\n";
var_dump(DateInterval::createFromDateString('2 weeks 3 days 4 hours')->__serialize());
"#,
        ),
        concat!(
            "+01-02-03 04:05:06.000000 (unknown)\n",
            "2001-03-04 04:05:06.500000|2000-01-01 00:00:00.500000\n",
            "+1/2/3 4:5:6.250000 428\n",
            "bool(true)\nint(1)\n",
            "2009-01-07\n",
            "array(2) {\n",
            "  [\"from_string\"]=>\n  bool(true)\n",
            "  [\"date_string\"]=>\n  string(22) \"2 weeks 3 days 4 hours\"\n",
            "}\n",
        )
    );
}

#[test]
fn iso_interval_time_components_cross_dst_as_elapsed_time() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('America/New_York');
foreach ([
    ['2006-04-02 01:30:00', 'PT2H'],
    ['2006-04-01 23:00:00', 'P1DT4H'],
    ['2006-10-29 00:30:00', 'PT1H'],
    ['2006-10-29 00:30:00', 'PT2H'],
    ['2006-10-29 00:30:00', 'PT3H'],
] as [$start, $interval]) {
    $date = new DateTime($start);
    $date->add(new DateInterval($interval));
    echo $date->format('Y-m-d H:i:s T U'), "\n";
}
$date = new DateTime('2006-04-02 04:00:00');
while ($date > new DateTime('2006-04-02 01:00:00')) {
    $date->sub(new DateInterval('PT1H'));
    echo $date->format('Y-m-d H:i T'), "\n";
}
$before = new DateTime('2010-11-07 01:59:59 EDT');
$after = new DateTime('2010-11-07 01:00:00 EST');
echo $before->diff($after)->format('%R%yY%mM%dDT%hH%iM%sS'), "\n";
"#,
        ),
        concat!(
            "2006-04-02 04:30:00 EDT 1143966600\n",
            "2006-04-03 03:00:00 EDT 1144047600\n",
            "2006-10-29 01:30:00 EDT 1162099800\n",
            "2006-10-29 01:30:00 EST 1162103400\n",
            "2006-10-29 02:30:00 EST 1162107000\n",
            "2006-04-02 03:00 EDT\n",
            "2006-04-02 01:00 EST\n",
            "+0Y0M0DT0H0M1S\n",
        )
    );
}

#[test]
fn relative_parser_handles_units_weekdays_and_epoch_bases() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
$d = new DateTime('2009-01-31 14:28:41');
foreach (['+1 day', '+1 week 2 days 4 hours 2 seconds', 'next Thursday', 'last Sunday'] as $m) {
    $d->modify($m);
    echo $d->format('Y-m-d H:i:s'), "\n";
}
echo strtotime('+1 week 2 days', 0), ':', strtotime('tomorrow', 0), "\n";
"#,
        ),
        concat!(
            "2009-02-01 14:28:41\n",
            "2009-02-10 18:28:43\n",
            "2009-02-12 00:00:00\n",
            "2009-02-08 00:00:00\n",
            "777600:86400\n",
        )
    );
}

#[test]
fn parser_accepts_php_textual_numeric_and_subsecond_grammar() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
foreach ([
    '2015-2-1',
    '18-01-2009 00:00:00',
    '7.8.2010',
    '0099-01',
    '1985-102',
    '2012-02-02T10',
    '28-July-2008',
    '1pm Aug 1 GMT 2007',
] as $input) {
    echo (new DateTimeImmutable($input))->format('Y-m-d H:i:s.u T'), "\n";
}
$base = new DateTimeImmutable('2016-10-07 13:25:50.000000 UTC');
foreach (['+1 ms', '-2 msec', '+7 usecs', '-10 µs', '+8 msec -2 µsec'] as $input) {
    echo $base->modify($input)->format('Y-m-d H:i:s.u'), "\n";
}
echo (new DateTimeImmutable('2008-01-17 last Monday'))->format('Y-m-d H:i:s'), "\n";
echo (new DateTimeImmutable('Monday next week 13:00'))->format('l H:i:s'), "\n";
echo (new DateTimeImmutable('first day of January 2011'))->format('Y-m-d H:i:s.u'), "\n";
$special = DateInterval::createFromDateString('third Tuesday of next month');
echo (new DateTimeImmutable('2010-03-07 13:21:38 UTC'))->add($special)->format('c'), "\n";
"#,
        ),
        concat!(
            "2015-02-01 00:00:00.000000 UTC\n",
            "2009-01-18 00:00:00.000000 UTC\n",
            "2010-08-07 00:00:00.000000 UTC\n",
            "0099-01-01 00:00:00.000000 UTC\n",
            "1985-04-12 00:00:00.000000 UTC\n",
            "2012-02-02 10:00:00.000000 UTC\n",
            "2008-07-28 00:00:00.000000 UTC\n",
            "2007-08-01 13:00:00.000000 GMT\n",
            "2016-10-07 13:25:50.001000\n",
            "2016-10-07 13:25:49.998000\n",
            "2016-10-07 13:25:50.000007\n",
            "2016-10-07 13:25:49.999990\n",
            "2016-10-07 13:25:50.007998\n",
            "2008-01-14 00:00:00\n",
            "Monday 13:00:00\n",
            "2011-01-01 00:00:00.000000\n",
            "2010-04-20T13:21:38+00:00\n",
        )
    );
}

#[test]
fn format_parser_preserves_offsets_fractions_and_last_diagnostics() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
$d = DateTimeImmutable::createFromFormat(
    '!Y-m-d H:i:s.u P',
    '2024-03-05 06:07:08.123 +02:30',
);
echo $d->format('Y-m-d H:i:s.u P U'), "\n";
var_dump(DateTimeImmutable::getLastErrors());
$x = DateTime::createFromFormat('!Y-m-d', '2024-02-31');
echo $x->format('Y-m-d'), "\n";
var_export(DateTime::getLastErrors());
echo "\n";
"#,
        ),
        concat!(
            "2024-03-05 06:07:08.123000 +02:30 1709609828\n",
            "bool(false)\n",
            "2024-03-02\n",
            "array (\n",
            "  'warning_count' => 1,\n",
            "  'warnings' => \n  array (\n    10 => 'The parsed date was invalid',\n  ),\n",
            "  'error_count' => 0,\n",
            "  'errors' => \n  array (\n  ),\n",
            ")\n",
        )
    );
}

#[test]
fn date_parse_exposes_absolute_relative_and_error_shapes() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
foreach (['2006-12-12 10:00:00.5 Europe/Prague', '+1 week 2 days', 'bad'] as $input) {
    $parsed = date_parse($input);
    echo json_encode($parsed, JSON_UNESCAPED_SLASHES), "\n";
}
"#,
        ),
        concat!(
            "{\"year\":2006,\"month\":12,\"day\":12,\"hour\":10,\"minute\":0,\"second\":0,\"fraction\":0.5,\"warning_count\":0,\"warnings\":[],\"error_count\":0,\"errors\":[],\"is_localtime\":true,\"zone_type\":3,\"tz_id\":\"Europe/Prague\"}\n",
            "{\"year\":false,\"month\":false,\"day\":false,\"hour\":false,\"minute\":false,\"second\":false,\"fraction\":false,\"warning_count\":0,\"warnings\":[],\"error_count\":0,\"errors\":[],\"is_localtime\":false,\"relative\":{\"year\":0,\"month\":0,\"day\":9,\"hour\":0,\"minute\":0,\"second\":0}}\n",
            "{\"year\":false,\"month\":false,\"day\":false,\"hour\":false,\"minute\":false,\"second\":false,\"fraction\":false,\"warning_count\":0,\"warnings\":[],\"error_count\":1,\"errors\":[\"The timezone could not be found in the database\"],\"is_localtime\":true,\"zone_type\":0}\n",
        )
    );
}

#[test]
fn parser_keeps_relative_signs_offsets_and_clock_reset_rules() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
echo date('r', strtotime('Mon, 08 May 2006 13:06:44 -0400 +30 days')), "\n";
$base = new DateTimeImmutable('2016-10-03 12:47:18.081921 UTC');
foreach (['noon', '10 weekday', '-3 months'] as $modifier) {
    echo $base->modify($modifier)->format('Y-m-d H:i:s.u'), "\n";
}
foreach (['28 Feb 2008 12:00:00 +1460000 days', '28 Feb 2008 12:00:00 -1460000 days'] as $input) {
    echo (new DateTimeImmutable($input))->format('Y-m-d H:i:s'), "\n";
}
"#,
        ),
        concat!(
            "Wed, 07 Jun 2006 17:06:44 +0000\n",
            "2016-10-03 12:00:00.000000\n",
            "2016-10-17 12:47:18.081921\n",
            "2016-07-03 12:47:18.081921\n",
            "6005-07-03 12:00:00\n",
            "-1990-10-25 12:00:00\n",
        )
    );
}

#[test]
fn format_parser_supports_unix_fractions_day_of_year_and_trailing_warnings() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
$date = DateTimeImmutable::createFromFormat('U.u', '1696617500.123456');
echo $date->format('Y-m-d H:i:s.u e P'), "\n";
var_export(date_parse_from_format('!Y-z H:i:s +', '2024-59 06:07:08 trailing'));
echo "\n";
"#,
        ),
        concat!(
            "2023-10-06 18:38:20.123456 +00:00 +00:00\n",
            "array (\n",
            "  'year' => 2024,\n  'month' => 2,\n  'day' => 29,\n",
            "  'hour' => 6,\n  'minute' => 7,\n  'second' => 8,\n",
            "  'fraction' => 0.0,\n",
            "  'warning_count' => 1,\n",
            "  'warnings' => \n  array (\n    17 => 'Trailing data',\n  ),\n",
            "  'error_count' => 0,\n",
            "  'errors' => \n  array (\n  ),\n",
            "  'is_localtime' => false,\n",
            ")\n",
        )
    );
}
