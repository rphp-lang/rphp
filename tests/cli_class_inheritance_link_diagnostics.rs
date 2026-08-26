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
fn invalid_class_inheritance_links_are_located_fatals() {
    for (source, expected) in [
        (
            "<?php\nclass ExtendedGenerator extends Generator {}\n",
            "Fatal error: Class ExtendedGenerator cannot extend final class Generator in Standard input code on line 2\n",
        ),
        (
            "<?php\nclass Test extends WeakReference {}\n",
            "Fatal error: Class Test cannot extend final class WeakReference in Standard input code on line 2\n",
        ),
        (
            "<?php\nfinal readonly class ParentClass {}\nreadonly class ChildClass extends ParentClass {}\n",
            "Fatal error: Class ChildClass cannot extend final class ParentClass in Standard input code on line 3\n",
        ),
        (
            "<?php\nreadonly class ParentClass {}\nclass ChildClass extends ParentClass {}\n",
            "Fatal error: Non-readonly class ChildClass cannot extend readonly class ParentClass in Standard input code on line 3\n",
        ),
        (
            "<?php\nclass ParentClass {}\nreadonly class ChildClass extends ParentClass {}\n",
            "Fatal error: Readonly class ChildClass cannot extend non-readonly class ParentClass in Standard input code on line 3\n",
        ),
    ] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(stderr, expected);
    }
}

#[test]
fn valid_ordinary_readonly_final_and_runtime_inheritance_remain_executable() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
class OrdinaryParent { public function value(): string { return 'ordinary'; } }
class OrdinaryChild extends OrdinaryParent {}
readonly class ReadonlyParent { public function value(): string { return 'readonly'; } }
readonly class ReadonlyChild extends ReadonlyParent {}
final class StandaloneFinal {}
if (true) {
    class RuntimeParent { public function value(): string { return 'runtime'; } }
}
if (true) {
    class RuntimeChild extends RuntimeParent {}
}
echo (new OrdinaryChild())->value(), '|';
echo (new ReadonlyChild())->value(), '|';
echo StandaloneFinal::class, '|', (new RuntimeChild())->value();
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "ordinary|readonly|StandaloneFinal|runtime");
    assert_eq!(stderr, "");
}
