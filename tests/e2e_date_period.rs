mod common;

use common::run_php;

#[test]
fn period_constructors_flags_getters_and_iteration_share_native_state() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
$start = new DateTimeImmutable('2024-01-01 12:00:00 UTC');
$interval = new DateInterval('P1D');
foreach ([[2, 0], [2, DatePeriod::EXCLUDE_START_DATE], [2, DatePeriod::INCLUDE_END_DATE]] as [$n, $o]) {
    $p = new DatePeriod($start, $interval, $n, $o);
    echo implode(',', array_map(fn($d) => $d->format('Y-m-d'), iterator_to_array($p))),
        '|', $p->getRecurrences(), '|', count($p->__serialize()), "\n";
}
$p = new DatePeriod(
    $start,
    $interval,
    new DateTimeImmutable('2024-01-04 12:00:00 UTC'),
    DatePeriod::INCLUDE_END_DATE,
);
echo implode(',', array_map(fn($d) => $d->format('Y-m-d'), iterator_to_array($p))),
    '|', get_class($p->getStartDate()), '|', get_class($p->getDateInterval()), "\n";
$p = DatePeriod::createFromISO8601String('R2/2024-01-01T00:00:00Z/P1D');
echo implode(',', array_map(fn($d) => $d->format('c'), iterator_to_array($p))), "\n";
"#,
        ),
        concat!(
            "2024-01-01,2024-01-02,2024-01-03|2|7\n",
            "2024-01-02,2024-01-03|2|7\n",
            "2024-01-01,2024-01-02,2024-01-03,2024-01-04|2|7\n",
            "2024-01-01,2024-01-02,2024-01-03,2024-01-04|DateTimeImmutable|DateInterval\n",
            "2024-01-01T00:00:00+00:00,2024-01-02T00:00:00+00:00,2024-01-03T00:00:00+00:00\n",
        )
    );
}

#[test]
fn period_reflection_matches_php_85_call_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
$r = new ReflectionClass(DatePeriod::class);
echo implode(',', $r->getInterfaceNames()), "\n";
foreach ([
    'createFromISO8601String', '__construct', 'getStartDate', 'getEndDate',
    'getDateInterval', 'getRecurrences', '__serialize', 'getIterator',
] as $name) {
    $m = $r->getMethod($name);
    echo $name, ':', (int) $m->isStatic(), ':', $m->getNumberOfRequiredParameters(),
        '/', $m->getNumberOfParameters(), ':', $m->hasReturnType() ? (string) $m->getReturnType() : '-', "\n";
}
"#,
        ),
        concat!(
            "IteratorAggregate,Traversable\n",
            "createFromISO8601String:1:1/2:static\n",
            "__construct:0:1/4:-\n",
            "getStartDate:0:0/0:-\n",
            "getEndDate:0:0/0:-\n",
            "getDateInterval:0:0/0:-\n",
            "getRecurrences:0:0/0:-\n",
            "__serialize:0:0/0:array\n",
            "getIterator:0:0/0:Iterator\n",
        )
    );
}
