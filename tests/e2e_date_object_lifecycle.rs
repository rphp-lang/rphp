mod common;

use common::run_php;

#[test]
fn native_date_objects_round_trip_through_php_serialization() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
$objects = [
    new DateTime('2024-03-04 05:06:07.123456 Europe/Prague'),
    new DateTimeImmutable('2024-03-04 UTC'),
    new DateTimeZone('Europe/Prague'),
    new DateInterval('P1Y2D'),
    new DatePeriod(new DateTimeImmutable('2024-01-01'), new DateInterval('P1D'), 2),
];
foreach ($objects as $object) {
    $serialized = serialize($object);
    $value = unserialize($serialized);
    echo get_class($value), '|', serialize($value) === $serialized ? 'same' : 'different', '|';
    if ($value instanceof DateTimeInterface) {
        echo $value->format('Y-m-d H:i:s.u e');
    } elseif ($value instanceof DateTimeZone) {
        echo $value->getName();
    } elseif ($value instanceof DateInterval) {
        echo $value->format('%y/%m/%d');
    } else {
        echo implode(',', array_map(fn($date) => $date->format('Y-m-d'), iterator_to_array($value)));
    }
    echo "\n";
}
"#,
        ),
        concat!(
            "DateTime|same|2024-03-04 05:06:07.123456 Europe/Prague\n",
            "DateTimeImmutable|same|2024-03-04 00:00:00.000000 UTC\n",
            "DateTimeZone|same|Europe/Prague\n",
            "DateInterval|same|1/0/2\n",
            "DatePeriod|same|2024-01-01,2024-01-02,2024-01-03\n",
        )
    );
}

#[test]
fn set_state_wakeup_and_sparse_interval_state_match_php() {
    assert_eq!(
        run_php(
            r#"<?php
$cases = [
    ['DateTime', ['date' => '2024-01-02 03:04:05.123456', 'timezone_type' => 3, 'timezone' => 'UTC']],
    ['DateTimeImmutable', ['date' => '2024-01-02 03:04:05.123456', 'timezone_type' => 3, 'timezone' => 'UTC']],
    ['DateTimeZone', ['timezone_type' => 3, 'timezone' => 'UTC']],
    ['DateInterval', ['y' => 1, 'm' => 2, 'd' => 3, 'h' => 4, 'i' => 5, 's' => 6, 'f' => 0.25, 'invert' => 0, 'days' => false, 'from_string' => false]],
];
foreach ($cases as [$class, $state]) {
    echo $class, ':', serialize($class::__set_state($state)), "\n";
}
foreach (['DateTime', 'DateTimeImmutable', 'DateTimeZone', 'DatePeriod'] as $class) {
    $object = (new ReflectionClass($class))->newInstanceWithoutConstructor();
    try {
        $object->__wakeup();
    } catch (Throwable $error) {
        echo $class, ':', get_class($error), ':', $error->getMessage(), "\n";
    }
}
$interval = (new ReflectionClass(DateInterval::class))->newInstanceWithoutConstructor();
$interval->__unserialize([]);
echo $interval->format('%y/%m/%d %h:%i:%s %a'), "\n";
"#,
        ),
        concat!(
            "DateTime:O:8:\"DateTime\":3:{s:4:\"date\";s:26:\"2024-01-02 03:04:05.123456\";s:13:\"timezone_type\";i:3;s:8:\"timezone\";s:3:\"UTC\";}\n",
            "DateTimeImmutable:O:17:\"DateTimeImmutable\":3:{s:4:\"date\";s:26:\"2024-01-02 03:04:05.123456\";s:13:\"timezone_type\";i:3;s:8:\"timezone\";s:3:\"UTC\";}\n",
            "DateTimeZone:O:12:\"DateTimeZone\":2:{s:13:\"timezone_type\";i:3;s:8:\"timezone\";s:3:\"UTC\";}\n",
            "DateInterval:O:12:\"DateInterval\":10:{s:1:\"y\";i:1;s:1:\"m\";i:2;s:1:\"d\";i:3;s:1:\"h\";i:4;s:1:\"i\";i:5;s:1:\"s\";i:6;s:1:\"f\";d:0.25;s:6:\"invert\";i:0;s:4:\"days\";b:0;s:11:\"from_string\";b:0;}\n",
            "DateTime:Error:Invalid serialization data for DateTime object\n",
            "DateTimeImmutable:Error:Invalid serialization data for DateTimeImmutable object\n",
            "DateTimeZone:Error:Invalid serialization data for DateTimeZone object\n",
            "DatePeriod:Error:Invalid serialization data for DatePeriod object\n",
            "-1/-1/-1 -1:-1:-1 -1\n",
        )
    );
}

#[test]
fn datetime_comparison_uses_the_absolute_instant_across_mutability_and_zones() {
    assert_eq!(
        run_php(
            r#"<?php
$a = new DateTimeImmutable('2024-01-01 UTC');
$b = new DateTimeImmutable('2024-01-02 UTC');
$c = new DateTime('2024-01-01 Europe/Prague');
$d = new DateTimeImmutable('2023-12-31 23:00:00 UTC');
foreach ([[$a, $b], [$b, $a], [$c, $d], [$a, clone $a]] as [$left, $right]) {
    echo (int) ($left == $right), ':', (int) ($left < $right), ':',
        (int) ($left > $right), ':', $left <=> $right, "\n";
}
"#,
        ),
        "0:1:0:-1\n0:0:1:1\n1:0:0:0\n1:0:0:0\n"
    );
}

#[test]
fn date_subclasses_preserve_custom_state_and_late_static_factories() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
class CustomDate extends DateTime { public bool $marker = true; }
class CustomImmutable extends DateTimeImmutable { public bool $marker = true; }
class CustomInterval extends DateInterval { public bool $marker = true; }
class CustomPeriod extends DatePeriod { public bool $marker = true; }

$objects = [
    new CustomDate('2024-01-02'),
    new CustomImmutable('2024-01-02'),
    new CustomInterval('P1D'),
    new CustomPeriod(new DateTimeImmutable('2024-01-02'), new DateInterval('P1D'), 1),
];
foreach ($objects as $object) {
    $copy = unserialize(serialize($object));
    echo get_class($copy), ':', (int) $copy->marker, "\n";
}
echo get_class(CustomDate::createFromImmutable(new DateTimeImmutable('@0'))), "\n";
echo get_class(CustomImmutable::createFromMutable(new DateTime('@0'))), "\n";

set_error_handler(function (int $severity, string $message): bool {
    echo $severity, ':', $message, "\n";
    return true;
});
var_dump(DATE_RFC7231 === DateTimeInterface::RFC7231);
"#,
        ),
        concat!(
            "CustomDate:1\n",
            "CustomImmutable:1\n",
            "CustomInterval:1\n",
            "CustomPeriod:1\n",
            "CustomDate\n",
            "CustomImmutable\n",
            "8192:Constant DATE_RFC7231 is deprecated since 8.5, as this format ignores the associated timezone and always uses GMT\n",
            "16384:Constant DateTimeInterface::RFC7231 is deprecated since 8.5, as this format ignores the associated timezone and always uses GMT\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn native_serialization_preserves_visibility_legacy_fields_and_expanded_years() {
    assert_eq!(
        run_php(
            r#"<?php
class ZoneState extends DateTimeZone {
    private int $hidden = 7;
    protected int $guard = 8;
    public function state(): string { return "$this->hidden/$this->guard/{$this->getName()}"; }
}
$zone = new ZoneState('Europe/Kyiv');
$serialized = serialize($zone);
echo str_replace(chr(0), '!', $serialized), "\n", unserialize($serialized)->state(), "\n";

$date = (new DateTime('UTC'))->setDate(20201, 1, 1)->setTime(0, 0);
$serialized = serialize($date);
echo $serialized, "\n", unserialize($serialized)->format('X-m-d H:i:s.u'), "\n";

$interval = unserialize('O:12:"DateInterval":8:{s:1:"y";s:1:"2";s:1:"m";s:1:"0";s:1:"d";s:3:"bla";s:1:"h";s:1:"6";s:1:"i";s:1:"8";s:1:"s";s:1:"0";s:6:"invert";i:0;s:4:"days";s:4:"aoeu";}');
echo $interval->format('%y/%m/%d %h:%i:%s %a'), "\n";
"#,
        ),
        concat!(
            "O:9:\"ZoneState\":4:{s:13:\"timezone_type\";i:3;s:8:\"timezone\";s:11:\"Europe/Kyiv\";s:17:\"!ZoneState!hidden\";i:7;s:8:\"!*!guard\";i:8;}\n",
            "7/8/Europe/Kyiv\n",
            "O:8:\"DateTime\":3:{s:4:\"date\";s:28:\"+20201-01-01 00:00:00.000000\";s:13:\"timezone_type\";i:3;s:8:\"timezone\";s:3:\"UTC\";}\n",
            "+20201-01-01 00:00:00.000000\n",
            "2/0/0 6:8:0 0\n",
        )
    );
}
