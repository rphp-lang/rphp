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
fn direct_trait_uses_reject_every_non_trait_class_like_kind() {
    for (declaration, name) in [
        ("interface Contract {}", "Contract"),
        ("class Concrete {}", "Concrete"),
        ("final class FinalConcrete {}", "FinalConcrete"),
        ("abstract class AbstractConcrete {}", "AbstractConcrete"),
    ] {
        let source = format!("<?php\n{declaration}\nclass Consumer {{ use {name}; }}\n");
        let (status, stdout, stderr) = run_stdin(&source);
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(
            stderr,
            format!(
                "\nFatal error: Uncaught Error: Consumer cannot use {name} - it is not a trait in Standard input code:3\nStack trace:\n#0 {{main}}\n  thrown in Standard input code on line 3\n"
            )
        );
    }
}

#[test]
fn direct_non_trait_use_throws_a_catchable_error() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
class Concrete {}
try {
    class RecoverableConsumer { use Concrete; }
} catch (Error $error) {
    echo $error->getMessage(), "\n";
}
echo class_exists(RecoverableConsumer::class, false) ? 'bad' : 'after';
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        "RecoverableConsumer cannot use Concrete - it is not a trait\nafter"
    );
    assert_eq!(stderr, "");
}

#[test]
fn trait_adaptations_reject_non_trait_owners_as_link_fatals() {
    for adaptation in [
        "Concrete::select as copied;",
        "Concrete::select insteadof ValidSource;",
    ] {
        let source = format!(
            "<?php\nclass Concrete {{ public function select() {{}} }}\ntrait ValidSource {{ public function select() {{}} }}\nclass Consumer {{\n    use ValidSource {{\n        {adaptation}\n    }}\n}}\n"
        );
        let (status, stdout, stderr) = run_stdin(&source);
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(
            stderr,
            "\nFatal error: Class Concrete is not a trait, Only traits may be used in 'as' and 'insteadof' statements in Standard input code on line 4\n"
        );
    }
}

#[test]
fn valid_trait_use_alias_and_precedence_remain_executable() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
trait Primary { public function select() { return 'primary'; } }
trait Secondary { public function select() { return 'secondary'; } }
class Consumer {
    use Primary, Secondary {
        Primary::select insteadof Secondary;
        Secondary::select as secondary;
    }
}
$consumer = new Consumer();
echo $consumer->select(), '|', $consumer->secondary();
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "primary|secondary");
    assert_eq!(stderr, "");
}
