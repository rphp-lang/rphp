// ── Recovery after return type error ──

#[test]
fn test_return_type_error_recovery() {
    assert_eq!(
        run_php(
            r#"<?php
function good(): int { return 42; }
function bad(): int { return "x"; }
try { bad(); } catch (TypeError $e) { echo "caught "; }
echo good();
"#
        ),
        "caught 42"
    );
}

#[test]
fn test_exact_int_fast_scalar_rejects_bad_argument_after_warmup() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function addOne(int $value): int { return $value + 1; }
for ($i = 0; $i < 100; $i++) { addOne($i); }
try { addOne("bad"); } catch (TypeError $e) { echo "caught"; }
"#
        ),
        "caught"
    );
}

#[test]
fn test_exact_int_fast_scalar_rejects_bad_return_after_warmup() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function maybeBad(int $value): int {
    if ($value < 1000) { return $value + 1; }
    return "bad";
}
for ($i = 0; $i < 100; $i++) { maybeBad($i); }
try { maybeBad(1000); } catch (TypeError $e) { echo "caught"; }
"#
        ),
        "caught"
    );
}

#[test]
fn test_hot_untyped_caller_rechecks_typed_scalar_callee() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function typedTarget(int $value): int { return $value + 1; }
function forward($value) { return typedTarget($value); }
for ($i = 0; $i < 100; $i++) { forward($i); }
try { forward(1.5); } catch (TypeError $e) { echo "caught"; }
"#
        ),
        "caught"
    );
}

#[test]
fn test_typed_scalar_method_rejects_bad_argument_after_warmup() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
class TypedCounter {
    function add(int $value): int { return $value + 1; }
}
$counter = new TypedCounter();
for ($i = 0; $i < 100; $i++) { $counter->add($i); }
try { $counter->add(1.5); } catch (TypeError $e) { echo "caught"; }
"#
        ),
        "caught"
    );
}

#[test]
fn test_fast_return_only_hint_rejects_bad_value_after_warmup() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function returnOnly($value): int { return $value; }
for ($i = 0; $i < 100; $i++) { returnOnly($i); }
try { returnOnly("bad"); } catch (TypeError $e) { echo "caught"; }
"#
        ),
        "caught"
    );
}

// ── declare(strict_types=1) ──

#[test]
fn test_strict_types_float_rejects_int() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function f(float $x): void { echo $x; }
try { f(10); } catch (TypeError $e) { echo "caught"; }
"#
        ),
        "caught"
    );
}

#[test]
fn test_strict_types_float_accepts_float() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function f(float $x): void { echo $x; }
f(10.5);
"#
        ),
        "10.5"
    );
}

#[test]
fn test_no_strict_types_float_accepts_int() {
    assert_eq!(
        run_php(
            r#"<?php
function f(float $x): void { echo $x; }
f(10);
"#
        ),
        "10"
    );
}

#[test]
fn test_strict_types_int_still_works() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function f(int $x): void { echo $x; }
f(42);
"#
        ),
        "42"
    );
}

#[test]
fn test_strict_types_string_still_works() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function f(string $x): void { echo $x; }
f("hello");
"#
        ),
        "hello"
    );
}

#[test]
fn test_strict_types_return_type() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function f(): float { return 10; }
try { f(); } catch (TypeError $e) { echo "caught"; }
"#
        ),
        "caught"
    );
}

#[test]
fn test_strict_types_0_allows_coercion() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=0);
function f(float $x): void { echo $x; }
f(10);
"#
        ),
        "10"
    );
}

#[test]
fn weak_string_arguments_invoke_object_string_conversion() {
    assert_eq!(
        run_php(
            r#"<?php
class StringArgument {
    public function __toString(): string { return "target"; }
}
function acceptString(string $value): void { echo gettype($value), ':', $value; }
acceptString(new StringArgument());
"#
        ),
        "string:target"
    );
}
