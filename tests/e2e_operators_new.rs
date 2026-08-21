mod common;
use common::{run_php, run_php_with_source_context};

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
fn integer_only_operators_share_php_85_checked_coercion() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) {
    echo $level, ":", $message, "\n";
});

var_dump(6.0 % 2);
var_dump("9.0" % 2);
var_dump(6.5 % 2);
var_dump("9.5" % 2);
var_dump("123tail" % 10);
var_dump(1.5 | 2);
var_dump("1.5" << 1);
var_dump(NAN % 3);
var_dump(9.223372036854776e18 % 3);
var_dump("1e309" % 3);

$slot = "45tail";
$slot %= 7;
var_dump($slot);

try {
    var_dump([] % []);
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}
try {
    var_dump(1 % 0);
} catch (DivisionByZeroError $error) {
    echo $error->getMessage(), "\n";
}
"#,
        ),
        "int(0)\nint(1)\n8192:Implicit conversion from float 6.5 to int loses precision\nint(0)\n8192:Implicit conversion from float-string \"9.5\" to int loses precision\nint(1)\n2:A non-numeric value encountered\nint(3)\n8192:Implicit conversion from float 1.5 to int loses precision\nint(3)\n8192:Implicit conversion from float-string \"1.5\" to int loses precision\nint(2)\n2:The float NAN is not representable as an int, cast occurred\n8192:Implicit conversion from float NAN to int loses precision\nint(0)\n2:The float 9.223372036854776E+18 is not representable as an int, cast occurred\nint(-2)\n8192:Implicit conversion from float-string \"1e309\" to int loses precision\nint(0)\n2:A non-numeric value encountered\nint(3)\nUnsupported operand types: array % array\nModulo by zero\n"
    );
}

#[test]
fn unsupported_subtraction_throws_typed_errors_without_committing_assignments() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
function reportSubtract(string $label, Closure $operation): void {
    try {
        $operation();
        echo $label, ":no-error\n";
    } catch (Throwable $error) {
        echo $label, ':', get_class($error), ':', $error->getMessage(), '|',
            $error->getFile() === __FILE__ ? 'source' : 'nested', ':',
            $error->getLine(), '|trace=', count($error->getTrace()), "\n";
    }
}

$array = [1];
reportSubtract('cv-const', fn() => $array - 1);
reportSubtract('const-cv', fn() => 1 - $array);
reportSubtract('tmp-tmp', fn() => [1] - [0]);

$slot = [1];
try {
    $slot -= 'x';
} catch (TypeError $error) {
    echo 'assign:', $error->getMessage(), '|slot=', count($slot), "\n";
}

reportSubtract('eval-const', fn() => eval('const BrokenSubtract = [1] - [0];'));
echo 'defined=', defined('BrokenSubtract') ? 'yes' : 'no', "\n";
"#,
            "/virtual/subtraction-type-error.php",
            "/virtual",
        ),
        concat!(
            "cv-const:TypeError:Unsupported operand types: array - int|source:14|trace=2\n",
            "const-cv:TypeError:Unsupported operand types: int - array|source:15|trace=2\n",
            "tmp-tmp:TypeError:Unsupported operand types: array - array|source:16|trace=2\n",
            "assign:Unsupported operand types: array - string|slot=1\n",
            "eval-const:TypeError:Unsupported operand types: array - array|nested:1|trace=3\n",
            "defined=no\n",
        )
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
