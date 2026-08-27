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

fn assert_compile_fatal(source: &str, message: &str, line: usize) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        format!("Fatal error: {message} in Standard input code on line {line}\n")
    );
}

fn assert_link_fatal(source: &str, message: &str, line: usize) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        format!("\nFatal error: {message} in Standard input code on line {line}\n")
    );
}

#[test]
fn static_and_abstract_are_not_trait_alias_modifiers() {
    assert_compile_fatal(
        "<?php\ntrait Source { function run() {} }\nclass Consumer { use Source { run as static; } }\n",
        "Cannot use \"static\" as method modifier in trait alias",
        3,
    );
    assert_compile_fatal(
        "<?php\ntrait Source { function run() {} }\nclass Consumer { use Source { run as abstract; } }\n",
        "Cannot use \"abstract\" as method modifier in trait alias",
        3,
    );
}

#[test]
fn one_trait_method_cannot_be_excluded_twice() {
    assert_link_fatal(
        "<?php\ntrait First { function run() {} }\ntrait Second { function run() {} }\nclass Consumer {\n    use First, Second { Second::run insteadof First; }\n    use First, Second { Second::run insteadof First; }\n}\n",
        "Failed to evaluate a trait precedence (run). Method of trait First was defined to be excluded multiple times",
        4,
    );
}

#[test]
fn inaccessible_trait_methods_throw_a_catchable_error_with_global_scope_wording() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\ntrait Source { protected function run() {} }\nclass Consumer { use Source; }\ntry { (new Consumer())->run(); } catch (Error $error) { echo $error->getMessage(); }\n",
    );
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        "Call to protected method Consumer::run() from global scope"
    );
    assert_eq!(stderr, "");
}

#[test]
fn nested_visibility_and_named_final_aliases_keep_effective_metadata() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
trait Origin { protected function run() { echo 'ok'; } }
trait Published { use Origin { run as public; run as final copy; } }
class Consumer { use Published; }
(new Consumer())->run();
$original = new ReflectionMethod(Consumer::class, 'run');
$copy = new ReflectionMethod(Consumer::class, 'copy');
echo '|', $original->isPublic() ? 'public' : 'hidden';
echo '|', $original->isFinal() ? 'final' : 'open';
echo '|', $copy->isFinal() ? 'final' : 'open';
"#,
    );
    assert_eq!(status, 0);
    assert_eq!(stdout, "ok|public|open|final");
    assert_eq!(stderr, "");
}

#[test]
fn final_methods_from_either_side_of_trait_composition_cannot_be_overridden() {
    assert_link_fatal(
        "<?php\ntrait Replacement { function run() {} }\nclass Base { final function run() {} }\nclass Consumer extends Base { use Replacement; }\n",
        "Cannot override final method Base::run()",
        4,
    );
    assert_link_fatal(
        "<?php\ntrait Source { function run() {} }\nclass Base { use Source { run as final; } }\nclass Consumer extends Base { function run() {} }\n",
        "Cannot override final method Base::run()",
        4,
    );
}

#[test]
fn making_a_private_trait_method_final_warns_and_updates_reflection() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\ntrait Source { private function run() {} }\nclass Consumer { use Source { run as final; } }\n$method = new ReflectionMethod(Consumer::class, 'run');\nvar_dump($method->isFinal(), $method->isPrivate());\n",
    );
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        "\nWarning: Private methods cannot be final as they are never overridden by other classes in Standard input code on line 3\nbool(true)\nbool(true)\n"
    );
    assert_eq!(stderr, "");
}
