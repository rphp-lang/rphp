mod common;
use common::run_php;

#[test]
fn test_print_returns_1() {
    assert_eq!(
        run_php(
            r#"<?php
$x = print "hello";
echo $x;
"#
        ),
        "hello1"
    );
}

#[test]
fn test_standalone_print_statement() {
    assert_eq!(run_php("<?php print 'hello';"), "hello");
}

#[test]
fn test_spaceship() {
    assert_eq!(
        run_php(
            r#"<?php
echo (1 <=> 2) . " " . (2 <=> 2) . " " . (3 <=> 2);
"#
        ),
        "-1 0 1"
    );
}

#[test]
fn test_spaceship_strings() {
    assert_eq!(
        run_php(
            r#"<?php
echo ("a" <=> "b") . " " . ("b" <=> "b") . " " . ("c" <=> "b");
"#
        ),
        "-1 0 1"
    );
}

#[test]
fn compound_and_cross_type_comparisons_follow_php_85_ordering() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    [1, 2] == [1, 2],
    [1] > 0,
    0 < [1],
    [] == false,
    null < -1,
    "123" == "123.0",
    1 < "2x",
    2 > "10x",
    [1] <=> 0,
    0 <=> [1],
    NAN < 0,
    NAN > "",
    NAN <=> NAN,
    [NAN] > [0],
    [NAN] <=> [0],
] as $result) {
    var_dump($result);
}
"#,
        ),
        "bool(true)\nbool(true)\nbool(true)\nbool(true)\nbool(true)\nbool(true)\nbool(true)\nbool(true)\nint(1)\nint(-1)\nbool(false)\nbool(false)\nint(1)\nbool(false)\nint(1)\n"
    );
}

#[test]
fn object_comparisons_convert_scalars_and_survive_reentrant_string_casts() {
    assert_eq!(
        run_php(
            r#"<?php
class PlainComparison {}
set_error_handler(function ($level, $message) { echo $message, "\n"; });
$plain = new PlainComparison;
var_dump($plain == 1, 2.0 > $plain);
restore_error_handler();

class RenderedComparison {
    public function __toString() { return "2"; }
}
var_dump(new RenderedComparison < "10");

class InvalidRenderedComparison {
    public function __toString() {}
}
try {
    var_dump(new InvalidRenderedComparison > "");
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}

$left = ["value" => "test"];
$right = ["value" => new class {
    public function __toString() {
        global $left, $right;
        $left = $right = null;
        return "";
    }
}];
var_dump($left > $right);

$arrayLeft = [new RenderedComparison];
$arrayRight = ["2"];
var_dump($arrayLeft == $arrayRight, $arrayLeft <=> $arrayRight);

class ComparisonContainer {
    public function __construct(public $value) {}
}
$objectLeft = new ComparisonContainer(new RenderedComparison);
$objectRight = new ComparisonContainer("2");
var_dump($objectLeft == $objectRight, $objectLeft <=> $objectRight);

$closure = function () {};
set_error_handler(function ($level, $message) { echo $message, "\n"; });
var_dump($closure == 1);
restore_error_handler();
var_dump("x" <=> $closure);
"#,
        ),
        "Object of class PlainComparison could not be converted to int\nObject of class PlainComparison could not be converted to float\nbool(true)\nbool(true)\nbool(true)\nInvalidRenderedComparison::__toString(): Return value must be of type string, none returned\nbool(true)\nbool(true)\nint(0)\nbool(true)\nint(0)\nObject of class Closure could not be converted to int\nbool(true)\nint(-1)\n"
    );
}

#[test]
fn object_comparison_initializes_lazy_properties_but_string_cast_does_not() {
    assert_eq!(
        run_php(
            r#"<?php
class ComparableBox {
    public int $value;

    public function __construct(int $value) {
        $this->value = $value;
    }

    public function __toString(): string {
        return 'C';
    }
}

class OtherBox {
    public int $value = 1;
}

$reflection = new ReflectionClass(ComparableBox::class);
$ghost = $reflection->newLazyGhost(function ($object) {
    echo "ghost\n";
    $object->__construct(1);
});
$proxy = $reflection->newLazyProxy(function () {
    echo "proxy\n";
    return new ComparableBox(1);
});

var_dump($ghost > $proxy);
var_dump($ghost == $proxy);
var_dump($ghost <=> $proxy);

$equalLeft = $reflection->newLazyGhost(function ($object) {
    echo "equal-left\n";
    $object->__construct(1);
});
$equalRight = $reflection->newLazyProxy(function () {
    echo "equal-right\n";
    return new ComparableBox(1);
});
var_dump($equalLeft == $equalRight);

$greaterLeft = $reflection->newLazyGhost(function ($object) {
    echo "greater-left\n";
    $object->__construct(2);
});
$greaterRight = $reflection->newLazyProxy(function () {
    echo "greater-right\n";
    return new ComparableBox(1);
});
var_dump($greaterLeft >= $greaterRight);

$identity = $reflection->newLazyGhost(function ($object) {
    echo "unexpected identity initialization\n";
    $object->__construct(1);
});
var_dump($identity == $identity);
var_dump($identity >= $identity);

$otherReflection = new ReflectionClass(OtherBox::class);
$differentLeft = $reflection->newLazyGhost(function ($object) {
    echo "unexpected left class initialization\n";
    $object->__construct(1);
});
$differentRight = $otherReflection->newLazyGhost(function ($object) {
    echo "unexpected right class initialization\n";
    $object->value = 1;
});
var_dump($differentLeft < $differentRight);

$throwing = $reflection->newLazyProxy(function () {
    throw new Exception('compare initialization');
});
$notReached = $reflection->newLazyGhost(function ($object) {
    echo "right-before-throw\n";
    $object->__construct(1);
});
try {
    $throwing < $notReached;
} catch (Exception $exception) {
    echo "caught: ", $exception->getMessage(), "\n";
}

$low = new ComparableBox(1);
$high = new ComparableBox(2);
var_dump($low < $high);
var_dump($low <=> $high);
var_dump($low < new OtherBox());
var_dump($low <=> new OtherBox());

$stringGhost = $reflection->newLazyGhost(function ($object) {
    echo "unexpected initialization\n";
    $object->__construct(9);
});
var_dump('A' < $stringGhost);
"#
        ),
        "ghost\nproxy\nbool(false)\nbool(true)\nint(0)\nequal-right\nequal-left\nbool(true)\ngreater-left\ngreater-right\nbool(true)\nbool(true)\nbool(true)\nbool(false)\nright-before-throw\ncaught: compare initialization\nbool(true)\nint(-1)\nbool(false)\nint(1)\nbool(true)\n"
    );
}

#[test]
fn test_power() {
    assert_eq!(
        run_php(
            r#"<?php
echo 2 ** 10;
"#
        ),
        "1024"
    );
}

#[test]
fn test_power_float() {
    assert_eq!(
        run_php(
            r#"<?php
echo 4 ** 0.5;
"#
        ),
        "2"
    );
}

#[test]
fn test_power_assign() {
    assert_eq!(
        run_php(
            r#"<?php
$x = 2;
$x **= 3;
echo $x;
"#
        ),
        "8"
    );
}

#[test]
fn test_bitwise_and() {
    assert_eq!(
        run_php(
            r#"<?php
echo 0b1100 & 0b1010;
"#
        ),
        "8"
    );
}

#[test]
fn test_bitwise_or() {
    assert_eq!(
        run_php(
            r#"<?php
echo 0b1100 | 0b1010;
"#
        ),
        "14"
    );
}

#[test]
fn test_bitwise_xor() {
    assert_eq!(
        run_php(
            r#"<?php
echo 0b1100 ^ 0b1010;
"#
        ),
        "6"
    );
}

#[test]
fn test_bitwise_not() {
    assert_eq!(
        run_php(
            r#"<?php
echo ~0;
"#
        ),
        "-1"
    );
}

#[test]
fn test_shift_left() {
    assert_eq!(
        run_php(
            r#"<?php
echo 1 << 4;
"#
        ),
        "16"
    );
}

#[test]
fn test_shift_right() {
    assert_eq!(
        run_php(
            r#"<?php
echo 16 >> 2;
"#
        ),
        "4"
    );
}

#[test]
fn test_bitwise_compound_assign() {
    assert_eq!(
        run_php(
            r#"<?php
$x = 0xFF;
$x &= 0x0F;
echo $x . " ";
$y = 0x0F;
$y |= 0xF0;
echo $y . " ";
$z = 0xFF;
$z ^= 0x0F;
echo $z;
"#
        ),
        "15 255 240"
    );
}

#[test]
fn test_shift_compound_assign() {
    assert_eq!(
        run_php(
            r#"<?php
$x = 1;
$x <<= 4;
echo $x . " ";
$x >>= 2;
echo $x;
"#
        ),
        "16 4"
    );
}
