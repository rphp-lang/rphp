use std::io::Write;
use std::process::{Command, Stdio};

fn run_stdin(source: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
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
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    )
}

#[test]
fn invalid_constructor_promotions_are_located_compile_fatals() {
    for (source, expected) in [
        (
            "<?php\nabstract class Example {\n    abstract public function __construct(public int $value);\n}\n",
            "Fatal error: Cannot declare promoted property in an abstract constructor in Standard input code on line 3\n",
        ),
        (
            "<?php\ninterface Example {\n    public function __construct(public int $value);\n}\n",
            "Fatal error: Cannot declare promoted property in an abstract constructor in Standard input code on line 3\n",
        ),
        (
            "<?php\nfunction __construct(public $value) {}\n",
            "Fatal error: Cannot declare promoted property outside a constructor in Standard input code on line 2\n",
        ),
        (
            "<?php\nclass Example {\n    public function method(public int $value) {}\n}\n",
            "Fatal error: Cannot declare promoted property outside a constructor in Standard input code on line 3\n",
        ),
        (
            "<?php\nclass Example {\n    public function __construct(public string ...$values) {}\n}\n",
            "Fatal error: Cannot declare variadic promoted property in Standard input code on line 3\n",
        ),
        (
            "<?php\nclass Example {\n    public $value;\n    public function __construct(\n        public $value,\n    ) {}\n}\n",
            "Fatal error: Cannot redeclare Example::$value in Standard input code on line 5\n",
        ),
    ] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(stderr, expected);
    }
}

#[test]
fn valid_promotions_properties_and_variadics_remain_executable() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
class Example {
    public string $explicit = 'property';
    public function __construct(
        public int $typed,
        public string $defaulted = 'default',
    ) {}
    public function join(string ...$values): string {
        return implode('-', $values);
    }
}
class ByReference {
    public function __construct(public array &$value) {}
}
trait TraitPromotion {
    public function __construct(public int $value) {}
}
class UsesTraitPromotion {
    use TraitPromotion;
}
$example = new Example(42);
$referenced = [];
new ByReference($referenced);
new UsesTraitPromotion(7);
echo $example->typed, '|', $example->defaulted, '|';
echo $example->explicit, '|', $example->join('a', 'b'), '|accepted';
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "42|default|property|a-b|accepted");
    assert_eq!(stderr, "");
}
