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

fn assert_failure(source: &str, expected: &str) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(stderr, expected);
}

#[test]
fn traits_reject_inheritance_clauses_at_the_parser_boundary() {
    assert_failure(
        "<?php\nclass Base {}\ntrait Behavior extends Base {}\n",
        "Parse error: syntax error, unexpected token \"extends\", expecting \"{\" in Standard input code on line 3\n",
    );
    assert_failure(
        "<?php\ninterface Contract {}\ntrait Behavior implements Contract {}\n",
        "Parse error: syntax error, unexpected token \"implements\", expecting \"{\" in Standard input code on line 3\n",
    );
}

#[test]
fn interfaces_reject_trait_use_at_the_compile_boundary() {
    assert_failure(
        "<?php\ntrait SharedBehavior {}\ninterface Contract { use SharedBehavior; }\n",
        "Fatal error: Cannot use traits inside of interfaces. SharedBehavior is used in Contract in Standard input code on line 3\n",
    );
}

#[test]
fn classes_cannot_extend_traits() {
    assert_failure(
        "<?php\ntrait ParentBehavior {}\nclass ConcreteChild extends parentbehavior {}\n",
        "Fatal error: Class ConcreteChild cannot extend trait ParentBehavior in Standard input code on line 3\n",
    );
}

#[test]
fn direct_trait_instantiation_throws_a_catchable_error_with_the_canonical_name() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\ntrait SharedBehavior {}\ntry { new sharedbehavior(); } catch (Error $error) { echo get_class($error), '|', $error->getMessage(); }\n",
    );
    assert_eq!(status, 0);
    assert_eq!(stdout, "Error|Cannot instantiate trait SharedBehavior");
    assert_eq!(stderr, "");
}

#[test]
fn reflection_trait_instantiation_uses_the_same_catchable_error() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
trait SharedBehavior {}
$reflection = new ReflectionClass('sharedbehavior');
try { $reflection->newInstance(); } catch (Error $error) { echo $error->getMessage(), "\n"; }
try { $reflection->newInstanceWithoutConstructor(); } catch (Error $error) { echo $error->getMessage(); }
"#,
    );
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        "Cannot instantiate trait SharedBehavior\nCannot instantiate trait SharedBehavior"
    );
    assert_eq!(stderr, "");
}
