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
