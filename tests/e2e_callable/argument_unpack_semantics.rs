#[test]
fn argument_unpack_crosses_dynamic_method_static_and_constructor_boundaries() {
    let output = run_php(
        r#"<?php
class ParcelSpread {
    public function __construct(public string $name, public int $size) {}
    public function describe(string $prefix, ...$parts): string {
        return $prefix . ':' . implode(',', $parts);
    }
    public static function combine(string $left, string $right): string {
        return $left . '-' . $right;
    }
}
function parcelParts() { yield 4; yield 5; }

$parcel = new ParcelSpread(...['size' => 3, 'name' => 'box']);
echo $parcel->describe('M', ...parcelParts()), '|';
echo ParcelSpread::combine(...['L', 'R']), '|';
$dynamic = [$parcel, 'describe'];
echo $dynamic(...['D', 8, 9]);
"#,
    );

    assert_eq!(output, "M:4,5|L-R|D:8,9");
}

#[test]
fn array_unpack_references_detach_copies_and_update_each_original_segment() {
    let output = run_php(
        r#"<?php
function raiseSlots(&...$slots): void {
    foreach ($slots as &$slot) { $slot += 10; }
}
$left = [1, 2];
$snapshot = $left;
$right = [3];
raiseSlots(...$left, ...$right);
echo $left[0], ',', $left[1], '|', $snapshot[0], ',', $snapshot[1], '|', $right[0];
"#,
    );

    assert_eq!(output, "11,12|1,2|13");
}

#[test]
fn unpack_validates_iterator_keys_and_preserves_iterator_exceptions() {
    let output = run_php(
        r#"<?php
function receiveSpread(...$values): void {}
function invalidSpreadKeys() { yield [] => 'bad'; }
function interruptedSpread() { yield 1; throw new Exception('stream-stopped'); }

try { receiveSpread(...invalidSpreadKeys()); }
catch (Error $error) { echo $error->getMessage(), '|'; }
try { receiveSpread(...interruptedSpread()); }
catch (Exception $error) { echo $error->getMessage(), '|'; }
try { receiveSpread(...false); }
catch (Error $error) { echo $error->getMessage(); }
"#,
    );

    assert_eq!(
        output,
        "Keys must be of type int|string during argument unpacking|stream-stopped|Only arrays and Traversables can be unpacked, bool given"
    );
}

#[test]
fn unpack_preserves_named_variadics_and_internal_null_mapping() {
    let output = run_php(
        r#"<?php
function ledgerSpread(string $head, ...$entries): string {
    $rendered = [];
    foreach ($entries as $key => $value) { $rendered[] = $key . '=' . $value; }
    return $head . ':' . implode(',', $rendered);
}
echo ledgerSpread(...['head' => 'H'], alpha: 1, beta: 2), '|';
echo json_encode(array_map(null, ...[[2, 4], [3, 5]]));
"#,
    );

    assert_eq!(output, "H:alpha=1,beta=2|[[2,3],[4,5]]");
}

#[test]
fn unpack_snapshots_all_by_value_arguments_before_reentrant_coercion() {
    let output = run_php(
        r#"<?php
class SpreadTextSnapshot {
    public function __toString(): string { $GLOBALS['later'] = 'changed'; return 'converted'; }
}
function spreadFixedSnapshot(string $first, string $second): string { return $first . '|' . $second; }
function spreadVariadicSnapshot(string ...$values): string { return implode('|', $values); }
$later = 'old'; $arguments = [new SpreadTextSnapshot(), &$later];
echo spreadFixedSnapshot(...$arguments), ':', $later, '|';
$later = 'old'; echo spreadVariadicSnapshot(...$arguments), ':', $later, '|';
$later = 'old'; echo call_user_func_array('spreadVariadicSnapshot', $arguments), ':', $later;
"#,
    );

    assert_eq!(output, "converted|old:changed|converted|old:changed|converted|old:changed");
}

#[test]
fn unpack_keeps_reentrant_by_reference_aliases_live() {
    let output = run_php(
        r#"<?php
class SpreadReferenceText {
    public function __toString(): string { $GLOBALS['later'] = 'changed'; return 'converted'; }
}
function spreadReferenceText(string &...$values): string { return implode('|', $values); }
$later = 'old'; $first = new SpreadReferenceText(); $arguments = [&$first, &$later];
echo spreadReferenceText(...$arguments), ':', $later, ':', gettype($first), '|';
$later = 'old'; $first = new SpreadReferenceText(); $arguments = [&$first, &$later];
echo call_user_func_array('spreadReferenceText', $arguments), ':', $later, ':', gettype($first);
"#,
    );

    assert_eq!(output, "converted|changed:changed:string|converted|changed:changed:string");
}

#[test]
fn unpack_uses_source_strictness_and_keeps_internal_callbacks_weak() {
    let output = run_php(
        r#"<?php declare(strict_types=1);
function spreadIntegerStrict(int $value): int { return $value; }
function spreadFloatStrict(float $value): string { return gettype($value) . ':' . $value; }
function spreadReferenceStrict(int &$value): void { $value++; }
try { spreadIntegerStrict(...['42']); } catch (TypeError $error) { echo 'strict|'; }
echo spreadFloatStrict(...[3]), '|', array_map('spreadIntegerStrict', ['42'])[0], '|';
$value = '7'; $arguments = [&$value];
try { spreadReferenceStrict(...$arguments); } catch (TypeError $error) { echo 'reference:', gettype($value), ':', $value, '|'; }
$closure = fn(int $value): int => $value;
try { $closure(...['8']); } catch (TypeError $error) { echo 'closure'; }
"#,
    );

    assert_eq!(output, "strict|double:3|42|reference:string:7|closure");
}

#[test]
fn unpack_coerces_references_and_checks_callable_visibility_in_callee_scope() {
    let output = run_php(
        r#"<?php
function spreadScalarsWeak(int &$value, float ...$values): string { $value++; return implode(',', $values); }
$value = '7'; $arguments = [&$value, '2.5', 3];
echo spreadScalarsWeak(...$arguments), '|', gettype($value), ':', $value, '|';
class SpreadCallableScope {
    private static function hidden(): void {}
    public static function accept(callable $value): void { echo 'local|'; }
    public static function check(): void {
        self::accept(...[[self::class, 'hidden']]);
        spreadCallableOutside(...[[self::class, 'hidden']]);
    }
}
function spreadCallableOutside(callable $value): void { echo 'outside-body'; }
try { SpreadCallableScope::check(); } catch (TypeError $error) { echo 'callee-scope|'; }
try { spreadCallableOutside(...[[SpreadCallableScope::class, 'hidden']]); } catch (TypeError $error) { echo 'outside'; }
"#,
    );

    assert_eq!(output, "2.5,3|integer:8|local|callee-scope|outside");
}

#[test]
fn unpack_arity_and_type_errors_keep_source_origin_and_declaring_function() {
    let output = run_php_with_source_context(
        "<?php\n\
function sourcePair(int $first, int $last): void {}\n\
try { sourcePair(...['first' => 1]); } catch (ArgumentCountError $error) { echo $error->getMessage(), '|', $error->getLine(), \"\\n\"; }\n\
class SourceParent { public static function accept(int $number): void {} }\n\
class SourceChild extends SourceParent {}\n\
try { SourceChild::accept(...['bad']); } catch (TypeError $error) { echo $error->getMessage(), '|', $error->getLine(); }",
        "source-unpack.php",
        ".",
    );

    assert_eq!(output, "Too few arguments to function sourcePair(), 1 passed in source-unpack.php on line 3 and exactly 2 expected|2\nSourceParent::accept(): Argument #1 ($number) must be of type int, string given, called in source-unpack.php on line 6|4");
}

#[test]
fn unpack_traversable_reference_warning_runs_handler_and_preserves_its_exception() {
    let output = run_php_with_source_context(
        "<?php\n\
function spreadWarnRef(int &$value): void { echo 'body|'; }\n\
function spreadWarnStream() { yield 4; }\n\
set_error_handler(function($level, $message, $file, $line) { echo $level, ':', basename($file), ':', $line, '|'; return true; });\n\
spreadWarnRef(...spreadWarnStream());\n\
set_error_handler(function($level, $message, $file, $line) { echo $level, ':', basename($file), ':', $line, '|'; throw new Exception('handler-stop'); });\n\
try { spreadWarnRef(...spreadWarnStream()); } catch (Throwable $error) { echo $error->getMessage(), '|'; }\n\
restore_error_handler(); $values = [5]; spreadWarnRef(...$values); echo 'done';",
        "source-unpack.php",
        ".",
    );

    assert_eq!(output, "2:source-unpack.php:5|body|2:source-unpack.php:7|handler-stop|body|done");
}

#[test]
fn unpack_validates_supplied_values_before_trailing_arity_and_keeps_named_holes_first() {
    let output = run_php(
        r#"<?php
function spreadMissingOrder(int $first, int $last): void { echo 'body'; }
try { spreadMissingOrder(...['bad']); } catch (Throwable $error) { echo get_class($error), '|'; }
class SpreadMissingText { public function __toString(): string { echo 'convert|'; return 'ok'; } }
function spreadMissingText(string $first, int $last): void { echo 'body'; }
try { spreadMissingText(...[new SpreadMissingText()]); } catch (Throwable $error) { echo get_class($error), '|'; }
function spreadNamedHole(int $first, int $last, int $optional = 0, ...$extra): void { echo 'body'; }
try { spreadNamedHole(...['first' => 'bad', 'optional' => 1]); } catch (Throwable $error) { echo $error->getMessage(), '|'; }
$captured = 'not-a-parameter'; $closure = function(int $first, int $last) use ($captured) {};
try { $closure(...[1]); } catch (Throwable $error) { echo get_class($error), '|'; }
$optional = function(int $first, int $last = 42) use ($captured) { echo $first, ':', $last, ':', $captured, '|'; };
$optional(...[1]);
function spreadMissingVariadic(int $first, int $last, int ...$extra): void { echo 'body'; }
try { spreadMissingVariadic(...[1], named: 'bad', another: 'bad'); } catch (Throwable $error) { echo get_class($error); }
"#,
    );

    assert_eq!(
        output,
        "TypeError|convert|ArgumentCountError|spreadNamedHole(): Argument #2 ($last) not passed|ArgumentCountError|1:42:not-a-parameter|ArgumentCountError"
    );
}
