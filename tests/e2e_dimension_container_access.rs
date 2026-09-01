mod common;

use common::run_php;

#[test]
fn compiler_marks_terminal_dimension_write_contexts() {
    use rphp::compiler::compile::Compiler;
    use rphp::lexer::Lexer;
    use rphp::parser::Parser;
    use rphp::vm::instruction::{FETCH_DIM_INCDEC, FETCH_DIM_OBJECT};
    use rphp::vm::opcode::OpCode;

    let source = r#"<?php
$text = 'a';
++ $text[0];
$text[0]->property = 1;
$operation = function () use (&$text) { return ++$text[0]; };
$objectOperation = function () use (&$text) { return $text[0]->property = 1; };
"#;
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compiled = Compiler::new().compile(&statements).unwrap();
    let mut flags: Vec<u16> = compiled
        .main
        .instructions
        .iter()
        .filter(|instruction| instruction.opcode == OpCode::FetchDimR)
        .map(|instruction| instruction._pad)
        .collect();
    flags.extend(compiled.functions.iter().flat_map(|(_, function)| {
        function
            .op_array
            .instructions
            .iter()
            .filter(|instruction| instruction.opcode == OpCode::FetchDimR)
            .map(|instruction| instruction._pad)
    }));
    assert_eq!(
        flags,
        vec![
            FETCH_DIM_INCDEC | rphp::vm::instruction::FETCH_DIM_MUTABLE,
            FETCH_DIM_OBJECT,
            FETCH_DIM_INCDEC | rphp::vm::instruction::FETCH_DIM_MUTABLE,
            FETCH_DIM_OBJECT,
        ],
        "terminal dimension contexts must survive lowering"
    );
}

#[test]
fn invalid_scalar_containers_win_after_operand_evaluation_without_mutation() {
    assert_eq!(
        run_php(
            r#"<?php
function dim_key(string $label, mixed $value): mixed { echo "key:$label|"; return $value; }
function dim_rhs(): int { echo "rhs|"; return 7; }
function dim_attempt(string $label, Closure $operation, mixed &$state): void {
    echo "$label:";
    try { var_dump($operation()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
    echo 'state:'; var_dump($state);
}
$container = true;
dim_attempt('assign', function () use (&$container) {
    return $container[dim_key('array', [])] = dim_rhs();
}, $container);
$container = 1;
dim_attempt('compound', function () use (&$container) {
    return $container[dim_key('object', new stdClass())] += dim_rhs();
}, $container);
$container = 1.5;
dim_attempt('unset', function () use (&$container) {
    unset($container[dim_key('array', [])]);
    return null;
}, $container);
$container = true;
dim_attempt('nested', function () use (&$container) {
    return $container[dim_key('outer', null)][dim_key('inner', 2.5)] = dim_rhs();
}, $container);
"#,
        ),
        concat!(
            "assign:key:array|rhs|Error:Cannot use a scalar value as an array\n",
            "state:bool(true)\n",
            "compound:key:object|rhs|Error:Cannot use a scalar value as an array\n",
            "state:int(1)\n",
            "unset:key:array|Error:Cannot unset offset in a non-array variable\n",
            "state:float(1.5)\n",
            "nested:key:outer|key:inner|rhs|Error:Cannot use a scalar value as an array\n",
            "state:bool(true)\n",
        )
    );
}

#[test]
fn illegal_array_keys_distinguish_terminal_probes_from_coalesce_and_nested_access() {
    assert_eq!(
        run_php(
            r#"<?php
function dim_key(string $label, mixed $value): mixed { echo "key:$label|"; return $value; }
function dim_attempt(string $label, Closure $operation): void {
    echo "$label:";
    try { var_dump($operation()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
$array = [];
$key = [];
dim_attempt('array-read', fn () => $array[dim_key('outer', $key)]);
dim_attempt('array-isset', fn () => isset($array[dim_key('outer', $key)]));
dim_attempt('array-empty', fn () => empty($array[dim_key('outer', $key)]));
dim_attempt('array-coalesce', fn () => $array[dim_key('outer', $key)] ?? 'fallback');
dim_attempt('array-nested-isset', fn () => isset($array[dim_key('outer', $key)][dim_key('inner', $key)]));
dim_attempt('array-nested-coalesce', fn () => $array[dim_key('outer', $key)][dim_key('inner', $key)] ?? 'fallback');
$key = new stdClass();
dim_attempt('object-read', fn () => $array[dim_key('outer', $key)]);
dim_attempt('object-empty', fn () => empty($array[dim_key('outer', $key)]));
dim_attempt('object-coalesce', fn () => $array[dim_key('outer', $key)] ?? 'fallback');
"#,
        ),
        concat!(
            "array-read:key:outer|TypeError:Cannot access offset of type array on array\n",
            "array-isset:key:outer|TypeError:Cannot access offset of type array in isset or empty\n",
            "array-empty:key:outer|TypeError:Cannot access offset of type array in isset or empty\n",
            "array-coalesce:key:outer|TypeError:Cannot access offset of type array on array\n",
            "array-nested-isset:key:outer|key:inner|TypeError:Cannot access offset of type array on array\n",
            "array-nested-coalesce:key:outer|key:inner|TypeError:Cannot access offset of type array on array\n",
            "object-read:key:outer|TypeError:Cannot access offset of type stdClass on array\n",
            "object-empty:key:outer|TypeError:Cannot access offset of type stdClass in isset or empty\n",
            "object-coalesce:key:outer|TypeError:Cannot access offset of type stdClass on array\n",
        )
    );
}

#[test]
fn null_and_false_autovivification_preserve_diagnostic_order_and_state() {
    assert_eq!(
        run_php(
            r#"<?php
function dim_key(string $label, mixed $value): mixed { echo "key:$label|"; return $value; }
set_error_handler(function (int $level, string $message): bool {
    echo "handler:$level:$message|";
    return true;
});
$null = null;
echo 'null:';
var_dump($null[dim_key('null', null)] = 1);
var_dump($null);
$false = false;
echo 'false-nested:';
var_dump($false[dim_key('outer', 'o')][dim_key('inner', null)] = 2);
var_dump($false);
$throwing = false;
set_error_handler(function (int $level, string $message): never {
    echo "throwing:$level:$message|";
    throw new RuntimeException('stop');
});
echo 'throw-state:';
try {
    $throwing[dim_key('write', 'x')] = 3;
} catch (Throwable $error) {
    echo $error::class, ':', $error->getMessage(), "\n";
}
var_dump($throwing);
"#,
        ),
        concat!(
            "null:key:null|handler:8192:Using null as an array offset is deprecated, use an empty string instead|int(1)\n",
            "array(1) {\n  [\"\"]=>\n  int(1)\n}\n",
            "false-nested:key:outer|key:inner|handler:8192:Automatic conversion of false to array is deprecated|",
            "handler:8192:Using null as an array offset is deprecated, use an empty string instead|int(2)\n",
            "array(1) {\n  [\"o\"]=>\n  array(1) {\n    [\"\"]=>\n    int(2)\n  }\n}\n",
            "throw-state:key:write|throwing:8192:Automatic conversion of false to array is deprecated|",
            "RuntimeException:stop\n",
            "array(1) {\n  [\"x\"]=>\n  int(3)\n}\n",
        )
    );
}

#[test]
fn reentrant_key_diagnostics_abandon_stale_root_writeback_without_losing_order() {
    assert_eq!(
        run_php(
            r#"<?php
$root = null;
set_error_handler(function (int $level, string $message) use (&$root): bool {
    echo "compound:$level:$message|";
    $root = 7;
    return true;
});
$root['outer']['inner'] .= 'suffix';
restore_error_handler();
var_dump($root);

$items = [0 => 'kept'];
set_error_handler(function (int $level, string $message) use (&$items): bool {
    echo "unset:$level:$message|";
    $items = null;
    return true;
});
unset($items[1.0E+42]);
restore_error_handler();
var_dump($items);
"#,
        ),
        concat!(
            "compound:2:Undefined array key \"outer\"|",
            "compound:2:Undefined array key \"inner\"|int(7)\n",
            "unset:2:The float 1.0E+42 is not representable as an int, cast occurred|NULL\n",
        )
    );
}

#[test]
fn string_offset_reads_and_probes_apply_context_specific_key_rules() {
    assert_eq!(
        run_php(
            r#"<?php
function dim_key(string $label, mixed $value): mixed { echo "key:$label|"; return $value; }
function dim_attempt(string $label, Closure $operation): void {
    echo "$label:";
    try { var_dump($operation()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
set_error_handler(function (int $level, string $message): bool {
    echo "handler:$level:$message|";
    return true;
});
$text = 'abcd';
dim_attempt('read-float', fn () => $text[dim_key('float', 1.5)]);
dim_attempt('isset-float', fn () => isset($text[dim_key('float', 1.5)]));
dim_attempt('coalesce-float', fn () => $text[dim_key('float', 1.5)] ?? 'fallback');
dim_attempt('read-partial', fn () => $text[dim_key('partial', '3tail')]);
dim_attempt('isset-partial', fn () => isset($text[dim_key('partial', '3tail')]));
dim_attempt('coalesce-partial', fn () => $text[dim_key('partial', '3tail')] ?? 'fallback');
dim_attempt('read-text', fn () => $text[dim_key('text', 'text')]);
dim_attempt('isset-text', fn () => isset($text[dim_key('text', 'text')]));
dim_attempt('coalesce-text', fn () => $text[dim_key('text', 'text')] ?? 'fallback');
dim_attempt('coalesce-oob', fn () => $text[99] ?? 'fallback');
dim_attempt('nested-coalesce-oob', fn () => $text[0][99] ?? 'fallback');
$resource = fopen('php://memory', 'r');
dim_attempt('read-resource', fn () => $text[dim_key('resource', $resource)]);
dim_attempt('isset-resource', fn () => isset($text[dim_key('resource', $resource)]));
dim_attempt('empty-resource', fn () => empty($text[dim_key('resource', $resource)]));
dim_attempt('coalesce-resource', fn () => $text[dim_key('resource', $resource)] ?? 'fallback');
$array = [];
dim_attempt('read-array', fn () => $text[dim_key('array', $array)]);
dim_attempt('isset-array', fn () => isset($text[dim_key('array', $array)]));
dim_attempt('empty-array', fn () => empty($text[dim_key('array', $array)]));
dim_attempt('coalesce-array', fn () => $text[dim_key('array', $array)] ?? 'fallback');
$object = new stdClass();
dim_attempt('read-object', fn () => $text[dim_key('object', $object)]);
dim_attempt('isset-object', fn () => isset($text[dim_key('object', $object)]));
dim_attempt('empty-object', fn () => empty($text[dim_key('object', $object)]));
dim_attempt('coalesce-object', fn () => $text[dim_key('object', $object)] ?? 'fallback');
"#,
        ),
        concat!(
            "read-float:key:float|handler:2:String offset cast occurred|string(1) \"b\"\n",
            "isset-float:key:float|handler:8192:Implicit conversion from float 1.5 to int loses precision|bool(true)\n",
            "coalesce-float:key:float|string(1) \"b\"\n",
            "read-partial:key:partial|handler:2:Illegal string offset \"3tail\"|string(1) \"d\"\n",
            "isset-partial:key:partial|bool(false)\n",
            "coalesce-partial:key:partial|handler:2:Illegal string offset \"3tail\"|string(1) \"d\"\n",
            "read-text:key:text|TypeError:Cannot access offset of type string on string\n",
            "isset-text:key:text|bool(false)\n",
            "coalesce-text:key:text|string(8) \"fallback\"\n",
            "coalesce-oob:string(8) \"fallback\"\n",
            "nested-coalesce-oob:string(8) \"fallback\"\n",
            "read-resource:key:resource|TypeError:Cannot access offset of type resource on string\n",
            "isset-resource:key:resource|bool(false)\n",
            "empty-resource:key:resource|bool(true)\n",
            "coalesce-resource:key:resource|TypeError:Cannot access offset of type resource on string\n",
            "read-array:key:array|TypeError:Cannot access offset of type array on string\n",
            "isset-array:key:array|bool(false)\n",
            "empty-array:key:array|bool(true)\n",
            "coalesce-array:key:array|TypeError:Cannot access offset of type array on string\n",
            "read-object:key:object|TypeError:Cannot access offset of type stdClass on string\n",
            "isset-object:key:object|bool(false)\n",
            "empty-object:key:object|bool(true)\n",
            "coalesce-object:key:object|TypeError:Cannot access offset of type stdClass on string\n",
        )
    );
}

#[test]
fn string_offset_modification_preserves_key_diagnostics_context_and_storage() {
    assert_eq!(
        run_php(
            r#"<?php
function dim_key(string $label, mixed $value): mixed { echo "key:$label|"; return $value; }
function dim_rhs(mixed $value): mixed { echo "rhs|"; return $value; }
function dim_attempt(string $label, Closure $operation, string &$state): void {
    echo "$label:";
    try { var_dump($operation()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
    echo 'state:'; var_dump($state);
}
set_error_handler(function (int $level, string $message): bool {
    echo "handler:$level:$message|";
    return true;
});
$text = 'abcd';
dim_attempt('write-float', function () use (&$text) {
    return $text[dim_key('float', 1.5)] = dim_rhs('XYZ');
}, $text);
$text = 'abcd';
dim_attempt('write-partial', function () use (&$text) {
    return $text[dim_key('partial', '2tail')] = dim_rhs('XYZ');
}, $text);
$text = 'abcd';
dim_attempt('compound-float', function () use (&$text) {
    return $text[dim_key('float', 1.5)] += dim_rhs(2);
}, $text);
$text = 'abcd';
dim_attempt('pre-float', function () use (&$text) {
    return ++$text[dim_key('float', 1.5)];
}, $text);
$text = 'abcd';
dim_attempt('unset-float', function () use (&$text) {
    unset($text[dim_key('float', 1.5)]);
    return null;
}, $text);
$text = 'abcd';
dim_attempt('unset-text', function () use (&$text) {
    unset($text[dim_key('text', 'bad')]);
    return null;
}, $text);
$text = 'x';
dim_attempt('as-object', function () use (&$text) {
    return $text[0]->property = 1;
}, $text);
"#,
        ),
        concat!(
            "write-float:key:float|rhs|handler:2:String offset cast occurred|",
            "handler:2:Only the first byte will be assigned to the string offset|string(1) \"X\"\n",
            "state:string(4) \"aXcd\"\n",
            "write-partial:key:partial|rhs|handler:2:Illegal string offset \"2tail\"|",
            "handler:2:Only the first byte will be assigned to the string offset|string(1) \"X\"\n",
            "state:string(4) \"abXd\"\n",
            "compound-float:key:float|rhs|handler:2:String offset cast occurred|",
            "Error:Cannot use assign-op operators with string offsets\n",
            "state:string(4) \"abcd\"\n",
            "pre-float:key:float|handler:2:String offset cast occurred|",
            "Error:Cannot increment/decrement string offsets\n",
            "state:string(4) \"abcd\"\n",
            "unset-float:key:float|Error:Cannot unset string offsets\n",
            "state:string(4) \"abcd\"\n",
            "unset-text:key:text|Error:Cannot unset string offsets\n",
            "state:string(4) \"abcd\"\n",
            "as-object:Error:Cannot use string offset as an object\n",
            "state:string(1) \"x\"\n",
        )
    );
}

#[test]
fn arrayaccess_indirect_modification_reports_the_original_source_line_in_order() {
    assert_eq!(
        run_php(
            r#"<?php
function dim_key(string $label, mixed $value): mixed { echo "key:$label|"; return $value; }
function dim_attempt(string $label, Closure $operation): void {
    echo "$label:";
    try { var_dump($operation()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
final class OffsetValue implements ArrayAccess {
    public function offsetExists(mixed $offset): bool { echo 'exists|'; return true; }
    public function offsetGet(mixed $offset): mixed { echo 'get|'; return 5; }
    public function offsetSet(mixed $offset, mixed $value): void { echo 'set|'; }
    public function offsetUnset(mixed $offset): void { echo 'unset|'; }
}
$expectedLine = 0;
set_error_handler(function (int $level, string $message, string $file, int $line) use (&$expectedLine): bool {
    echo 'handler:', $line === $expectedLine ? 'line-ok:' : "line-$line-expected-$expectedLine:",
        $message, '|';
    return true;
});
$box = new OffsetValue();
$expectedLine = __LINE__ + 1;
dim_attempt('write', fn () => $box[dim_key('outer', 'x')][dim_key('inner', null)] = 1);
$expectedLine = __LINE__ + 1;
dim_attempt('compound', fn () => $box[dim_key('outer', 'x')][dim_key('inner', null)] += 1);
$expectedLine = __LINE__ + 1;
dim_attempt('pre', fn () => ++$box[dim_key('outer', 'x')][dim_key('inner', null)]);
$expectedLine = __LINE__ + 1;
dim_attempt('post', fn () => $box[dim_key('outer', 'x')][dim_key('inner', null)]++);
$expectedLine = __LINE__ + 1;
dim_attempt('unset', function () use ($box) { unset($box[dim_key('outer', 'x')][dim_key('inner', null)]); return null; });
"#,
        ),
        concat!(
            "write:key:outer|key:inner|get|handler:line-ok:",
            "Indirect modification of overloaded element of OffsetValue has no effect|",
            "Error:Cannot use a scalar value as an array\n",
            "compound:key:outer|key:inner|get|handler:line-ok:",
            "Indirect modification of overloaded element of OffsetValue has no effect|",
            "Error:Cannot use a scalar value as an array\n",
            "pre:key:outer|key:inner|get|handler:line-ok:",
            "Indirect modification of overloaded element of OffsetValue has no effect|",
            "Error:Cannot use a scalar value as an array\n",
            "post:key:outer|key:inner|get|handler:line-ok:",
            "Indirect modification of overloaded element of OffsetValue has no effect|",
            "Error:Cannot use a scalar value as an array\n",
            "unset:key:outer|key:inner|get|handler:line-ok:",
            "Indirect modification of overloaded element of OffsetValue has no effect|",
            "Error:Cannot unset offset in a non-array variable\n",
        )
    );
}

#[test]
fn failed_dimension_access_preserves_references_cow_and_undefined_state() {
    assert_eq!(
        run_php(
            r#"<?php
function dim_key(string $label, mixed $value): mixed { echo "key:$label|"; return $value; }
function dim_rhs(mixed $value): mixed { echo "rhs|"; return $value; }
function dim_attempt(string $label, Closure $operation): void {
    echo "$label:";
    try { var_dump($operation()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
set_error_handler(function (int $level, string $message): bool {
    echo "handler:$level:$message|";
    return true;
});
$shared = ['keep' => 1];
$copy = $shared;
dim_attempt('array-invalid', function () use (&$shared) {
    return $shared[dim_key('array', [])] = dim_rhs(2);
});
var_dump($shared, $copy);
$scalar = true;
$alias = &$scalar;
dim_attempt('scalar-ref', function () use (&$alias) {
    return $alias[dim_key('null', null)] = dim_rhs(3);
});
var_dump($scalar, $alias);
$original = 'abcd';
$stringCopy = $original;
dim_attempt('string-valid', function () use (&$stringCopy) {
    return $stringCopy[dim_key('one', 1)] = dim_rhs('XYZ');
});
var_dump($original, $stringCopy);
unset($tree);
dim_attempt('undefined-read', fn () => $tree[1][2][3]);
var_dump(isset($tree), $tree ?? null);
dim_attempt('undefined-write', function () use (&$tree) {
    return $tree[1][2][3] = 4;
});
var_dump($tree);
"#,
        ),
        concat!(
            "array-invalid:key:array|rhs|TypeError:Cannot access offset of type array on array\n",
            "array(1) {\n  [\"keep\"]=>\n  int(1)\n}\n",
            "array(1) {\n  [\"keep\"]=>\n  int(1)\n}\n",
            "scalar-ref:key:null|rhs|Error:Cannot use a scalar value as an array\n",
            "bool(true)\nbool(true)\n",
            "string-valid:key:one|rhs|handler:2:Only the first byte will be assigned to the string offset|",
            "string(1) \"X\"\n",
            "string(4) \"abcd\"\nstring(4) \"aXcd\"\n",
            "undefined-read:handler:2:Undefined variable $tree|",
            "handler:2:Trying to access array offset on null|",
            "handler:2:Trying to access array offset on null|",
            "handler:2:Trying to access array offset on null|NULL\n",
            "bool(false)\nNULL\n",
            "undefined-write:int(4)\n",
            "array(1) {\n  [1]=>\n  array(1) {\n    [2]=>\n    array(1) {\n",
            "      [3]=>\n      int(4)\n    }\n  }\n}\n",
        )
    );
}
