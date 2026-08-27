use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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

struct TemporaryDirectory(std::path::PathBuf);

impl TemporaryDirectory {
    fn new() -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rphp-trait-prototype-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temporary directory should be created");
        Self(path)
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn abstract_trait_alias_remains_an_independent_requirement() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
trait ContractFragment { abstract public function original(); }
class ConcreteConsumer {
    use ContractFragment { original as renamedRequirement; }
    public function original() {}
}
"#,
    );

    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert!(stderr.contains("ConcreteConsumer::renamedRequirement"));
    assert!(!stderr.contains("ContractFragment::renamedRequirement"));
}

#[test]
fn trait_override_contract_reports_the_trait_source() {
    let directory = TemporaryDirectory::new();
    let trait_file = directory.0.join("returning_trait.php");
    let main_file = directory.0.join("consumer.php");
    std::fs::write(
        &trait_file,
        "<?php\ntrait ReturningTrait {\n    public function value(): string { return 'bad'; }\n}\n",
    )
    .expect("trait fixture should be written");
    std::fs::write(
        &main_file,
        "<?php\nrequire __DIR__ . '/returning_trait.php';\nclass NumericBase { public function value(): int { return 1; } }\nclass Consumer extends NumericBase { use ReturningTrait; }\n",
    )
    .expect("consumer fixture should be written");

    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .arg(&main_file)
        .output()
        .expect("rphp subprocess should finish");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");
    assert_eq!(output.status.code(), Some(255));
    assert!(stderr.contains(
        "Declaration of Consumer::value(): string must be compatible with NumericBase::value(): int"
    ));
    assert!(stderr.contains(&format!("{} on line 3", trait_file.display())));
    assert!(!stderr.contains(&format!("{} on line 3", main_file.display())));
}

#[test]
fn protected_overrides_share_their_oldest_non_private_prototype() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
class FamilyRoot { protected function signal() {} }
class CallerBranch extends FamilyRoot {
    public function invoke(FamilyRoot $peer) { $peer->signal(); }
}
class ClassBranch extends FamilyRoot { protected function signal() { echo 'class'; } }
trait SignalImplementation { protected function signal() { echo 'trait'; } }
class TraitBranch extends FamilyRoot { use SignalImplementation; }
$caller = new CallerBranch();
$caller->invoke(new ClassBranch());
$caller->invoke(new TraitBranch());
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "classtrait");
    assert_eq!(stderr, "");
}

#[test]
fn abstract_trait_requirement_preserves_an_inherited_method_prototype() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
class Root { protected function execute() {} }
class Implementation extends Root { protected function execute() { echo 'ok'; } }
trait ExecuteRequirement { abstract protected function execute(); }
class RequiredImplementation extends Implementation { use ExecuteRequirement; }
class SiblingCaller extends Root {
    public static function invoke($peer) { $peer->execute(); }
}
SiblingCaller::invoke(new RequiredImplementation());
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "ok");
    assert_eq!(stderr, "");
}

#[test]
fn a_private_ancestor_keeps_protected_sibling_methods_unrelated() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
class PrivateRoot { private function hidden() {} }
class Left extends PrivateRoot { public function invoke($peer) { $peer->hidden(); } }
class Right extends PrivateRoot { protected function hidden() {} }
(new Left())->invoke(new Right());
"#,
    );

    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert!(stderr.contains("Call to protected method Right::hidden() from scope Left"));
}
