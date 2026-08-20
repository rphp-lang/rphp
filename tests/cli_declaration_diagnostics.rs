use std::io::Write;
use std::process::{Command, Stdio};

fn run_stdin(source: &str) -> (i32, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rphp subprocess should start");
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(source.as_bytes())
        .expect("source should be written");
    let output = child.wait_with_output().expect("rphp should finish");
    (
        output.status.code().expect("rphp should exit normally"),
        String::from_utf8(output.stderr).expect("diagnostic should be UTF-8"),
    )
}

#[test]
fn asymmetric_property_compile_errors_include_the_declaration_location() {
    let (status, stderr) = run_stdin("<?php\nclass Box {\n    public private(set) $value;\n}\n");
    assert_eq!(status, 255);
    assert_eq!(
        stderr,
        "Fatal error: Property with asymmetric visibility Box::$value must have type in Standard input code on line 3\n"
    );
}

#[test]
fn asymmetric_property_link_errors_include_the_child_declaration_location() {
    let (status, stderr) = run_stdin(
        "<?php\nclass ParentBox { public protected(set) string $value; }\nclass ChildBox extends ParentBox { public private(set) string $value; }\n",
    );
    assert_eq!(status, 255);
    assert_eq!(
        stderr,
        "Fatal error: Set access level of ChildBox::$value must be protected(set) (as in class ParentBox) or weaker in Standard input code on line 3\n"
    );
}

#[test]
fn duplicate_asymmetric_modifiers_are_a_located_compile_fatal() {
    let (status, stderr) =
        run_stdin("<?php\nclass Box {\n    public private(set) protected(set) string $value;\n}\n");
    assert_eq!(status, 255);
    assert_eq!(
        stderr,
        "Fatal error: Multiple access type modifiers are not allowed in Standard input code on line 3\n"
    );
}

#[test]
fn duplicate_member_modifiers_report_the_first_duplicate_at_the_declaration() {
    for (declaration, message) in [
        (
            "static public public static final final function test() {}",
            "Multiple access type modifiers are not allowed",
        ),
        (
            "static static function test() {}",
            "Multiple static modifiers are not allowed",
        ),
        (
            "final final function test() {}",
            "Multiple final modifiers are not allowed",
        ),
        (
            "abstract abstract function test() {}",
            "Multiple abstract modifiers are not allowed",
        ),
        (
            "readonly readonly int $value;",
            "Multiple readonly modifiers are not allowed",
        ),
    ] {
        let (status, stderr) = run_stdin(&format!(
            "<?php\nabstract class Box {{\n    {declaration}\n}}\n"
        ));
        assert_eq!(status, 255);
        assert_eq!(
            stderr,
            format!("Fatal error: {message} in Standard input code on line 3\n")
        );
    }
}

#[test]
fn final_abstract_method_is_a_compile_error_without_method_spelling() {
    let (status, stderr) =
        run_stdin("<?php\nabstract class Box {\n    final abstract function test();\n}\n");
    assert_eq!(status, 255);
    assert_eq!(
        stderr,
        "Fatal error: Cannot use the final modifier on an abstract method in Standard input code on line 3\n"
    );
}

#[test]
fn duplicate_member_modifiers_apply_to_every_class_like_declaration() {
    for source in [
        "trait T { public public function test() {} }",
        "interface I { static static function test(); }",
        "enum E { case A; final final function test() {} }",
        "$value = new class { protected protected int $value; };",
    ] {
        let (status, stderr) = run_stdin(&format!("<?php\n{source}\n"));
        assert_eq!(status, 255);
        assert!(
            stderr.starts_with("Fatal error: Multiple "),
            "unexpected diagnostic: {stderr}"
        );
        assert!(
            stderr.ends_with(" in Standard input code on line 2\n"),
            "unexpected location: {stderr}"
        );
    }
}

#[test]
fn readonly_property_contract_errors_include_the_declaration_location() {
    for (declaration, message) in [
        (
            "public static readonly int $value;",
            "Static property Box::$value cannot be readonly",
        ),
        (
            "public readonly $value;",
            "Readonly property Box::$value must have type",
        ),
        (
            "public readonly int $value = 1;",
            "Readonly property Box::$value cannot have default value",
        ),
    ] {
        let (status, stderr) = run_stdin(&format!("<?php\nclass Box {{\n    {declaration}\n}}\n"));
        assert_eq!(status, 255);
        assert_eq!(
            stderr,
            format!("Fatal error: {message} in Standard input code on line 3\n")
        );
    }
}

#[test]
fn inherited_property_errors_use_the_class_line_and_canonical_parent_type() {
    let (status, stderr) = run_stdin(
        "<?php\nclass X {}\ninterface Y {}\nclass ParentBox { public (X&Y)|string $value; }\nclass ChildBox extends ParentBox {\n    public int $value;\n}\n",
    );
    assert_eq!(status, 255);
    assert_eq!(
        stderr,
        "Fatal error: Type of ChildBox::$value must be (X&Y)|string (as in class ParentBox) in Standard input code on line 5\n"
    );

    let (status, stderr) = run_stdin(
        "<?php\nclass ParentBox { public $value; }\nclass ChildBox extends ParentBox { public mixed $value; }\n",
    );
    assert_eq!(status, 255);
    assert_eq!(
        stderr,
        "Fatal error: Type of ChildBox::$value must be omitted to match the parent definition in class ParentBox in Standard input code on line 3\n"
    );
}

#[test]
fn inherited_method_errors_use_the_child_method_declaration_line() {
    let (status, stderr) = run_stdin(
        "<?php\nclass Value {}\nclass ParentBox { public function accept(?Value $value) {} }\nclass ChildBox extends ParentBox { public function accept(Value $value) {} }\n",
    );
    assert_eq!(status, 255);
    assert_eq!(
        stderr,
        "Fatal error: Declaration of ChildBox::accept(Value $value) must be compatible with ParentBox::accept(?Value $value) in Standard input code on line 4\n"
    );
}

#[test]
fn composite_mixed_void_and_never_types_are_rejected_at_declaration_time() {
    for (declaration, message) in [
        (
            "function test(mixed|int $value) {}",
            "Type mixed can only be used as a standalone type",
        ),
        (
            "function test(?mixed $value) {}",
            "Type mixed cannot be marked as nullable since mixed already includes null",
        ),
        (
            "function test(): ?void {}",
            "Void can only be used as a standalone type",
        ),
        (
            "function test(): object|never {}",
            "never can only be used as a standalone type",
        ),
    ] {
        let (status, stderr) = run_stdin(&format!("<?php\n{declaration}\n"));
        assert_eq!(status, 255);
        assert_eq!(
            stderr,
            format!("Fatal error: {message} in Standard input code on line 2\n")
        );
    }
}

#[test]
fn properties_reject_function_only_types_with_property_diagnostics() {
    for (type_name, class_name) in [
        ("void", "Box"),
        ("never", "LowerBox"),
        ("callable", "CallbackBox"),
        ("?callable", "NullableCallbackBox"),
        ("callable|string", "UnionCallbackBox"),
    ] {
        let (status, stderr) = run_stdin(&format!(
            "<?php\nclass {class_name} {{ public {type_name} $value; }}\n"
        ));
        assert_eq!(status, 255);
        assert_eq!(
            stderr,
            format!(
                "Fatal error: Property {class_name}::$value cannot have type {type_name} in Standard input code on line 2\n"
            )
        );
    }

    let (status, stderr) = run_stdin(
        "<?php\nclass PromotedBox { public function __construct(public callable $value) {} }\n",
    );
    assert_eq!(status, 255);
    assert_eq!(
        stderr,
        "Fatal error: Property PromotedBox::$value cannot have type callable in Standard input code on line 2\n"
    );
}
