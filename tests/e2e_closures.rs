/// Tests for closures / anonymous functions
mod common;
use common::run_php;

#[test]
fn test_closure_basic() {
    assert_eq!(
        run_php(
            r#"<?php
$greet = function($name) {
    echo "Hello " . $name;
};
$greet("World");
"#
        ),
        "Hello World"
    );
}

#[test]
fn test_closure_with_use() {
    assert_eq!(
        run_php(
            r#"<?php
$prefix = "Hi";
$greet = function($name) use ($prefix) {
    echo $prefix . " " . $name;
};
$greet("PHP");
"#
        ),
        "Hi PHP"
    );
}

#[test]
fn class_scoped_closure_extra_arguments_do_not_overwrite_captures() {
    assert_eq!(
        run_php(
            r#"<?php
trait ScopedClosureFactory {
    public function callback(object $loader): Closure {
        return function (CapturedContainer $container) use ($loader) {
            return $container->marker . ':' . $loader->marker . ':' . $this->state;
        };
    }
}
class ScopedClosureLoader {
    use ScopedClosureFactory;

    private string $state = 'bound';
}

class CapturedContainer {
    public string $marker = 'container';
}
class CapturedLoader {
    public string $marker = 'loader';
}
class ClosureInvoker {
    private CapturedContainer $container;
    private ?string $environment = 'ignored-extra-argument';

    public function __construct() {
        $this->container = new CapturedContainer();
    }

    public function invoke(mixed $resource) {
        return $resource($this->container, $this->environment);
    }
}
$loader = new CapturedLoader();
$callback = (new ScopedClosureLoader())->callback($loader);
echo (new ClosureInvoker())->invoke($callback);
"#,
        ),
        "container:loader:bound"
    );
}

#[test]
fn test_closure_multiple_params() {
    assert_eq!(
        run_php(
            r#"<?php
$add = function($a, $b) {
    return $a + $b;
};
echo $add(3, 4);
"#
        ),
        "7"
    );
}

#[test]
fn test_closure_return_value() {
    assert_eq!(
        run_php(
            r#"<?php
$double = function($x) {
    return $x * 2;
};
$result = $double(21);
echo $result;
"#
        ),
        "42"
    );
}

#[test]
fn test_closure_use_captures_value() {
    // use captures by value — changing outer var doesn't affect closure
    assert_eq!(
        run_php(
            r#"<?php
$x = 10;
$fn = function() use ($x) {
    echo $x;
};
$x = 20;
$fn();
"#
        ),
        "10"
    );
}

#[test]
fn reference_capture_shares_state_across_recursive_activations() {
    assert_eq!(
        run_php(
            r#"<?php
$remaining = 3;
$walk = function($self, $depth) use (&$remaining) {
    echo $depth . ':' . $remaining . '|';
    $remaining--;
    if ($depth < 2) {
        $self($self, $depth + 1);
    }
};
$walk($walk, 0);
echo 'outer:' . $remaining;
"#
        ),
        "0:3|1:2|2:1|outer:0"
    );
}

#[test]
fn closures_from_one_reference_capture_observe_the_same_cell() {
    assert_eq!(
        run_php(
            r#"<?php
$total = 5;
$factory = function() use (&$total) {
    return function($delta) use (&$total) {
        $total += $delta;
        return $total;
    };
};
$left = $factory();
$right = $factory();
echo $left(2) . '|' . $right(4) . '|' . $total;
"#
        ),
        "7|11|11"
    );
}

#[test]
fn reference_capture_outlives_its_creating_frame() {
    assert_eq!(
        run_php(
            r#"<?php
function make_accumulator() {
    $total = 7;
    return function($delta) use (&$total) {
        $total += $delta;
        return $total;
    };
}
$accumulate = make_accumulator();
echo $accumulate(3) . '|' . $accumulate(5);
"#
        ),
        "10|15"
    );
}

#[test]
fn reference_capture_outlives_reference_forwarding_frames() {
    assert_eq!(
        run_php(
            r#"<?php
function bind_accumulator(&$value) {
    return function($delta) use (&$value) {
        $value += $delta;
        return $value;
    };
}
function make_forwarded_accumulator() {
    $local = 4;
    return bind_accumulator($local);
}
$accumulate = make_forwarded_accumulator();
echo $accumulate(3) . '|' . $accumulate(5);
"#
        ),
        "7|12"
    );
}

#[test]
fn reference_capture_and_static_local_keep_distinct_persistent_cells() {
    assert_eq!(
        run_php(
            r#"<?php
$trail = 'seed';
$advance = function() use (&$trail) {
    static $step = 0;
    $step++;
    $trail = $step . '/' . $trail;
    return $trail;
};
echo $advance() . '|' . $advance() . '|' . $advance();
"#
        ),
        "1/seed|2/1/seed|3/2/1/seed"
    );
}

#[test]
fn test_closure_multiple_use_vars() {
    assert_eq!(
        run_php(
            r#"<?php
$a = "foo";
$b = "bar";
$fn = function() use ($a, $b) {
    echo $a . " " . $b;
};
$fn();
"#
        ),
        "foo bar"
    );
}

#[test]
fn test_closure_no_params() {
    assert_eq!(
        run_php(
            r#"<?php
$fn = function() {
    echo "no params";
};
$fn();
"#
        ),
        "no params"
    );
}

#[test]
fn test_closure_as_callback() {
    // Pass closure to a function
    assert_eq!(
        run_php(
            r#"<?php
function apply($fn, $val) {
    return $fn($val);
}
$double = function($x) { return $x * 2; };
echo apply($double, 5);
"#
        ),
        "10"
    );
}

#[test]
fn test_multiple_closures() {
    assert_eq!(
        run_php(
            r#"<?php
$add = function($a, $b) { return $a + $b; };
$mul = function($a, $b) { return $a * $b; };
echo $add(2, 3) . " " . $mul(4, 5);
"#
        ),
        "5 20"
    );
}

#[test]
fn test_closure_use_with_params() {
    assert_eq!(
        run_php(
            r#"<?php
$base = 100;
$add_to_base = function($x) use ($base) {
    return $base + $x;
};
echo $add_to_base(42);
"#
        ),
        "142"
    );
}

#[test]
fn test_variable_function_call_string() {
    assert_eq!(
        run_php(
            r#"<?php
function greet($name) {
    echo "Hello " . $name;
}
$fn = "greet";
$fn("World");
"#
        ),
        "Hello World"
    );
}

#[test]
fn test_closure_body_compile_error_no_panic() {
    // break inside closure body should be a compile error, not a panic
    let tokens = rphp::lexer::Lexer::new("<?php $f = function() { break; };")
        .tokenize()
        .unwrap();
    let stmts = rphp::parser::Parser::new(tokens).parse().unwrap();
    let result = rphp::compiler::compile::Compiler::new().compile(&stmts);
    assert!(
        result.is_err(),
        "Expected compile error for break inside closure"
    );
}

// ============================================================================
// Arrow functions: fn($x) => expr
// ============================================================================

#[test]
fn test_arrow_basic() {
    assert_eq!(
        run_php(
            r#"<?php
$double = fn($x) => $x * 2;
echo $double(5);
"#
        ),
        "10"
    );
}

#[test]
fn test_arrow_multiple_params() {
    assert_eq!(
        run_php(
            r#"<?php
$add = fn($a, $b) => $a + $b;
echo $add(3, 7);
"#
        ),
        "10"
    );
}

#[test]
fn test_arrow_captures_outer_variable() {
    assert_eq!(
        run_php(
            r#"<?php
$factor = 3;
$mul = fn($x) => $x * $factor;
echo $mul(5);
"#
        ),
        "15"
    );
}

#[test]
fn test_arrow_captures_multiple_vars() {
    assert_eq!(
        run_php(
            r#"<?php
$prefix = "Hello";
$suffix = "!";
$greet = fn($name) => $prefix . " " . $name . $suffix;
echo $greet("PHP");
"#
        ),
        "Hello PHP!"
    );
}

#[test]
fn test_arrow_no_params() {
    assert_eq!(
        run_php(
            r#"<?php
$val = 42;
$get = fn() => $val;
echo $get();
"#
        ),
        "42"
    );
}

#[test]
fn test_arrow_with_default_param() {
    assert_eq!(
        run_php(
            r#"<?php
$f = fn($x, $y = 10) => $x + $y;
echo $f(5);
echo " ";
echo $f(5, 20);
"#
        ),
        "15 25"
    );
}

#[test]
fn test_arrow_nested() {
    assert_eq!(
        run_php(
            r#"<?php
$add = fn($a) => fn($b) => $a + $b;
$add5 = $add(5);
echo $add5(3);
"#
        ),
        "8"
    );
}

#[test]
fn test_arrow_in_array_map_style() {
    // Using arrow function as callback passed to another function
    assert_eq!(
        run_php(
            r#"<?php
function apply($fn, $val) {
    return $fn($val);
}
$square = fn($x) => $x * $x;
echo apply($square, 4);
"#
        ),
        "16"
    );
}

#[test]
fn test_arrow_with_string_concat() {
    assert_eq!(
        run_php(
            r#"<?php
$wrap = fn($s) => "[" . $s . "]";
echo $wrap("test");
"#
        ),
        "[test]"
    );
}

#[test]
fn test_arrow_with_ternary() {
    assert_eq!(
        run_php(
            r#"<?php
$abs = fn($x) => $x >= 0 ? $x : -$x;
echo $abs(5);
echo " ";
echo $abs(-3);
"#
        ),
        "5 3"
    );
}

#[test]
fn instance_closures_bind_this_and_closure_bind_can_replace_it() {
    assert_eq!(
        run_php(
            r#"<?php
class BoundValue {
    public $value;
    public function __construct($value) { $this->value = $value; }
    public function reader() { return function() { return $this->value; }; }
}
$first = new BoundValue('first');
$second = new BoundValue('second');
$reader = $first->reader();
$rebound = Closure::bind($reader, $second, $second);
echo $reader() . ':' . $rebound();
"#,
        ),
        "first:second"
    );
}

#[test]
fn closure_bind_accepts_composer_static_null_binding() {
    assert_eq!(
        run_php(
            r#"<?php
$captured = 'ready';
$closure = static function($suffix) use ($captured) { return $captured . $suffix; };
$bound = Closure::bind($closure, null, null);
echo $bound('!');
"#,
        ),
        "ready!"
    );

    assert_eq!(
        run_php(
            r#"<?php
$closure = static function() { return 'unused'; };
var_dump(Closure::bind($closure, new stdClass()));
"#,
        ),
        concat!(
            "Warning: Closure::bind(): Cannot bind an instance to a static closure\n",
            "NULL\n"
        )
    );
}

#[test]
fn closure_bind_to_rebinds_an_arrow_function_receiver() {
    assert_eq!(
        run_php(
            r#"<?php
class ArrowBinding {
    public function __construct(public string $value) {}
    public function reader(): Closure { return fn () => $this->value; }
}
$first = new ArrowBinding('first');
$second = new ArrowBinding('second');
$reader = $first->reader();
$rebound = $reader->bindTo($second);
echo $reader(), ':', $rebound();
"#,
        ),
        "first:second"
    );
}

#[test]
fn closure_bind_scope_grants_lexical_private_property_access() {
    assert_eq!(
        run_php(
            r#"<?php
class ScopedStore {
    private $values = [];
    public function value() { return $this->values['ready']; }
}
$store = new ScopedStore();
$writer = function ($target) { $target->values['ready'] = 42; };
$bound = Closure::bind($writer, null, ScopedStore::class);
call_user_func($bound, $store);
echo $store->value();
"#,
        ),
        "42"
    );
}

#[test]
fn bound_closure_context_survives_callback_standard_functions() {
    assert_eq!(
        run_php(
            r#"<?php
class CallbackContext {
    private $factor = 3;
    private $direction = -1;
    private $seen = '';

    public function reduce() {
        return Closure::bind(
            function($carry, $value) { return $carry + $value * $this->factor; },
            $this,
            self::class
        );
    }
    public function compare() {
        return Closure::bind(
            function($left, $right) { return ($left - $right) * $this->direction; },
            $this,
            self::class
        );
    }
    public function visit() {
        return Closure::bind(
            function($value, $key) { $this->seen = $this->seen . $key . $value; },
            $this,
            self::class
        );
    }
    public function replace() {
        return Closure::bind(
            function($matches) { return $this->factor . $matches[0]; },
            $this,
            self::class
        );
    }
    public function seen() { return $this->seen; }
}
$context = new CallbackContext();
echo array_reduce([1, 2], $context->reduce(), 0) . '|';
$values = [1, 3, 2];
usort($values, $context->compare());
echo $values[0] . $values[1] . $values[2] . '|';
array_walk($values, $context->visit());
echo $context->seen() . '|';
echo preg_replace_callback('/x/', $context->replace(), 'x-x');
"#,
        ),
        "9|321|031221|3x-3x"
    );
}
