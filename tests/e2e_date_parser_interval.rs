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
var_dump(DateInterval::createFromDateString('2 weeks 3 days 4 hours')->__serialize());
"#,
        ),
        concat!(
            "+01-02-03 04:05:06.000000 (unknown)\n",
            "2001-03-04 04:05:06.500000|2000-01-01 00:00:00.500000\n",
            "+1/2/3 4:5:6.250000 428\n",
            "array(2) {\n",
            "  [\"from_string\"]=>\n  bool(true)\n",
            "  [\"date_string\"]=>\n  string(22) \"2 weeks 3 days 4 hours\"\n",
            "}\n",
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
