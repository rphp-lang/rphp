mod common;

#[test]
fn property_closure_call_preserves_empty_and_captured_environments() {
    let output = common::run_php(
        r#"<?php
final class Transform {
    public $callback;
    public function __construct($callback) { $this->callback = $callback; }
    public function apply($value) {
        $callback = $this->callback;
        return $callback($value);
    }
}
function runTransform($transform, $iterations) {
    $result = 0;
    for ($index = 0; $index < $iterations; $index++) {
        $result = $transform->apply($result);
    }
    return $result;
}
$empty = new Transform(function ($value) { return $value + 1; });
$offset = 3;
$captured = new Transform(function ($value) use ($offset) { return $value + $offset; });
echo runTransform($empty, 1000), ':', runTransform($captured, 1000);
"#,
    );
    assert_eq!(output, "1000:3000");
}

#[test]
fn property_closure_call_falls_back_for_references_and_string_callables() {
    let output = common::run_php(
        r#"<?php
final class Transform {
    public $callback;
    public function __construct($callback) { $this->callback = $callback; }
    public function apply($value) {
        $callback = $this->callback;
        return $callback($value);
    }
}
function runTransform($transform, $iterations) {
    $result = 0;
    for ($index = 0; $index < $iterations; $index++) {
        $result = $transform->apply($result);
    }
    return $result;
}
function incrementValue($value) { return $value + 1; }
$callback = function ($value) { return $value + 2; };
$referenced = new Transform(null);
$referenced->callback =& $callback;
$named = new Transform('incrementValue');
echo runTransform($referenced, 1000), ':', runTransform($named, 1000);
"#,
    );
    assert_eq!(output, "2000:1000");
}
