#[test]
fn named_arguments_initialize_reused_internal_call_slots() {
    assert_eq!(
        run_php(
            r#"<?php
echo substr("abcdef", 1, 3); echo ':';
echo substr(string: "uvwxyz", offset: 2, length: 3); echo ':';
echo str_replace("a", "bc", "a-a"); echo ':';
echo str_replace(search: "x", replace: "yz", subject: "x-x");
"#
        ),
        "bcd:wxy:bc-bc:yz-yz"
    );
}

#[test]
fn named_arguments_are_safe_inside_nested_calls_after_frame_reuse() {
    assert_eq!(
        run_php(
            r#"<?php
echo strtoupper(substr("abcdef", 1, 3)); echo ':';
echo strtoupper(substr(string: "uvwxyz", offset: 2, length: 3)); echo ':';
echo strlen(str_replace("a", "bc", "a-a")); echo ':';
echo strlen(str_replace(search: "x", replace: "yz", subject: "x-x"));
"#
        ),
        "BCD:WXY:5:5"
    );
}

#[test]
fn reused_user_method_static_and_constructor_frames_keep_named_holes_undef() {
    assert_eq!(
        run_php(
            r#"<?php
function combine($a, $b = "B", $c = "C") { return $a . $b . $c; }
combine("x", "y", "z");
echo combine(a: "A", c: "Z"); echo ':';

class NamedReuse {
    public $value;
    public function __construct($a, $b = "B") { $this->value = $a . $b; }
    public function join($a, $b = "B") { return $a . $b; }
    public static function pair($a, $b = "B") { return $a . $b; }
}
$dirty = new NamedReuse("x", "y");
$clean = new NamedReuse(a: "A");
$dirty->join("x", "y");
echo $clean->value; echo ':'; echo $dirty->join(a: "A"); echo ':';
NamedReuse::pair("x", "y");
echo NamedReuse::pair(a: "A");
"#
        ),
        "ABZ:AB:AB:AB"
    );
}

#[test]
fn invokable_objects_keep_named_arguments_aligned_with_hidden_this() {
    assert_eq!(
        run_php(
            r#"<?php
class NamedInvoker {
    public function __invoke($a, $b = "B") { return $a . $b; }
}
$invoke = new NamedInvoker();
echo $invoke(a: "A"); echo ':';
echo $invoke("X", b: "Y"); echo ':';
echo $invoke(a: strtolower(strtoupper("N"))); echo ':';
$inner = new NamedInvoker();
echo $invoke(a: $inner(a: "I"));
"#
        ),
        "AB:XY:nB:IBB"
    );
}
