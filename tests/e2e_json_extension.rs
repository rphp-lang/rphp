mod common;

use common::run_php;

#[test]
fn json_extension_identity_and_signatures_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
echo (int) extension_loaded('json'), (int) extension_loaded('JSON'),
    (int) in_array('json', get_loaded_extensions(), true), "\n";
foreach (['json_decode', 'json_encode', 'json_last_error', 'json_last_error_msg', 'json_validate'] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getExtensionName(), ':',
        $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), ':', $function->getReturnType(), "\n";
}
echo (new ReflectionClass(JsonException::class))->getExtensionName(), "\n";
"#,
        ),
        concat!(
            "111\n",
            "json_decode:json:1/4:mixed\n",
            "json_encode:json:1/3:string|false\n",
            "json_last_error:json:0/0:int\n",
            "json_last_error_msg:json:0/0:string\n",
            "json_validate:json:1/3:bool\n",
            "json\n",
        )
    );
}

#[test]
fn json_exception_uses_the_declared_throwable_layout_and_internal_trace() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    json_decode('{', false, 512, JSON_THROW_ON_ERROR);
} catch (JsonException $error) {
    $trace = $error->getTrace()[0];
    echo count((array) $error), ':', $error->getCode(), ':',
        $trace['function'], ':', count($trace['args']), "\n";
}
"#,
        ),
        "7:4:json_decode:4\n"
    );
}

#[test]
fn json_decode_number_fallback_preserves_object_handle_order() {
    assert_eq!(
        run_php(
            r#"<?php
$overflow = json_decode('[{}, {}, 1e400]');
echo spl_object_id($overflow[0]), ',', spl_object_id($overflow[1]), '|';
var_dump($overflow[2]);
$negativeZero = json_decode('[{}, {}, -0.0]');
echo spl_object_id($negativeZero[0]), ',', spl_object_id($negativeZero[1]), '|';
var_dump($negativeZero[2]);
$nestedFirst = json_decode('{"nested":{}}');
$scalarFirst = json_decode('{"scalar":1,"nested":{}}');
echo (int) (spl_object_id($nestedFirst) > spl_object_id($nestedFirst->nested)),
    (int) (spl_object_id($scalarFirst) < spl_object_id($scalarFirst->nested)), "\n";
"#,
        ),
        "1,2|float(INF)\n3,4|float(-0)\n11\n"
    );
}

#[test]
fn json_serializable_debug_projection_marks_reentrant_var_dump_as_recursive() {
    assert_eq!(
        run_php(
            r#"<?php
class DebugJson implements JsonSerializable {
    public $a = 1;
    public function __debugInfo() { return ['result' => json_encode($this)]; }
    public function jsonSerialize(): mixed { var_dump($this); return $this; }
}
var_dump(new DebugJson());
"#,
        ),
        concat!(
            "*RECURSION*\n",
            "object(DebugJson)#1 (1) {\n",
            "  [\"result\"]=>\n",
            "  string(7) \"{\"a\":1}\"\n",
            "}\n",
        )
    );
}
