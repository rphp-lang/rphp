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
fn interface_method_modifiers_are_rejected_without_running_the_body() {
    for (modifier, expected) in [
        ("final", "must not be final"),
        ("abstract", "must not be abstract"),
    ] {
        let (status, stdout, stderr) = run_stdin(&format!(
            "<?php\necho 'must-not-run';\ninterface ModifierContract {{ {modifier} public function execute(); }}\n"
        ));
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert!(
            stderr.contains(&format!(
                "Interface method ModifierContract::execute() {expected}"
            )),
            "{stderr}"
        );
        assert!(
            stderr.ends_with("Standard input code on line 3\n"),
            "{stderr}"
        );
    }

    let (status, stdout, stderr) = run_stdin(
        r#"<?php
interface OrdinaryContract { public function execute(): string; }
class OrdinaryImplementation implements OrdinaryContract {
    public function execute(): string { return 'linked'; }
}
echo (new OrdinaryImplementation())->execute();
"#,
    );
    assert_eq!(
        (status, stdout.as_str(), stderr.as_str()),
        (0, "linked", "")
    );
}

#[test]
fn iterator_shape_and_all_abstract_obligations_fail_before_publication() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
echo "before-link\n";
abstract class AmbiguousCursor implements IteratorAggregate, Iterator {}
echo "after-link\n";
"#,
    );
    assert_eq!(status, 255);
    assert_eq!(stdout, "before-link\n");
    assert!(
        stderr.contains(
            "Class AmbiguousCursor cannot implement both Iterator and IteratorAggregate at the same time"
        ),
        "{stderr}"
    );
    assert!(!stdout.contains("after-link"));

    let (status, stdout, stderr) = run_stdin(
        r#"<?php
abstract class PartialContract { abstract protected function left(int $value): void; }
interface AdditionalContract { public function right(string $value): void; }
class MissingBoth extends PartialContract implements AdditionalContract {}
"#,
    );
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert!(
        stderr.contains("contains 2 abstract methods")
            && stderr.contains("PartialContract::left, AdditionalContract::right"),
        "{stderr}"
    );
    assert!(
        stderr.ends_with("Standard input code on line 4\n"),
        "{stderr}"
    );
}

#[test]
fn prototype_defaults_are_bound_to_the_declaring_class() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
abstract class DefaultOwner { public function execute($owner = self::class): void {} }
trait Replacement { public function execute(): void {} }
class InvalidReplacement extends DefaultOwner { use Replacement; }
"#,
    );
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert!(
        stderr.contains(
            "InvalidReplacement::execute(): void must be compatible with DefaultOwner::execute($owner = 'DefaultOwn...'): void"
        ),
        "{stderr}"
    );
    assert!(!stderr.contains("self::class"), "{stderr}");
}

#[test]
fn active_links_stay_hidden_and_variance_autoload_is_ordered() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
spl_autoload_register(function (string $name): void {
    echo "probe:$name\n";
    new ReflectionClass(ActiveDeclaration::class);
});
class ActiveDeclaration implements MissingContract {}
echo "must-not-run\n";
"#,
    );
    assert_eq!(status, 255);
    assert_eq!(stdout, "probe:MissingContract\n");
    assert!(
        stderr.contains("Uncaught ReflectionException: Class \"ActiveDeclaration\" does not exist"),
        "{stderr}"
    );
    assert!(
        stderr.contains("{closure:Standard input code:2}"),
        "{stderr}"
    );

    let (status, stdout, stderr) = run_stdin(
        r#"<?php
spl_autoload_register(function (string $name): void {
    echo "load:$name\n";
    if ($name === 'InputType') { class InputType {} return; }
    if ($name === 'ResultType') { class ResultType {} return; }
});
class VarianceBase { public function convert(InputType $value): object {} }
class VarianceChild extends VarianceBase { public function convert(object $value): ResultType {} }
echo "linked\n";
"#,
    );
    assert_eq!(status, 0);
    assert_eq!(stdout, "load:InputType\nload:ResultType\nlinked\n");
    assert_eq!(stderr, "");

    let (status, stdout, stderr) = run_stdin(
        r#"<?php
spl_autoload_register(function (string $name): void {
    echo "explode:$name\n";
    class ExplodingResult {}
    throw new Exception('link explosion');
});
class ExceptionBase { public function make(): object {} }
class ExceptionChild extends ExceptionBase { public function make(): ExplodingResult {} }
echo "must-not-run\n";
"#,
    );
    assert_eq!(status, 255);
    assert_eq!(stdout, "explode:ExplodingResult\n");
    assert!(
        stderr.contains(
            "During inheritance of ExceptionChild, while autoloading ExplodingResult: Uncaught Exception: link explosion"
        ),
        "{stderr}"
    );
    assert!(!stdout.contains("must-not-run"));

    let (status, stdout, stderr) = run_stdin(
        r#"<?php
class ProvisionalBase { public function make(): ProvisionalParent {} }
spl_autoload_register(function (string $name): void {
    echo "nested:$name\n";
    if ($name === 'ProvisionalParent') {
        class ProvisionalParent extends ProvisionalBase {
            public function make(): ProvisionalChild {}
        }
        return;
    }
    throw new Exception('provisional explosion');
});
new ProvisionalParent;
echo "must-not-run\n";
"#,
    );
    assert_eq!(status, 255);
    assert_eq!(
        stdout,
        "nested:ProvisionalParent\nnested:ProvisionalChild\n"
    );
    assert!(
        stderr.contains(
            "During inheritance of ProvisionalParent, while autoloading ProvisionalChild: Uncaught Exception: provisional explosion"
        ),
        "{stderr}"
    );
    assert!(!stdout.contains("must-not-run"));
}

#[test]
fn composed_properties_satisfy_only_the_hooks_they_actually_provide() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
trait Storage { public mixed $value; }
interface Readable { public mixed $value { get; } }
class TraitBacked implements Readable { use Storage; }
new TraitBacked();
echo "trait-linked\n";
"#,
    );
    assert_eq!(
        (status, stdout.as_str(), stderr.as_str()),
        (0, "trait-linked\n", "")
    );

    let (status, stdout, stderr) = run_stdin(
        r#"<?php
abstract class ReadBase { public abstract mixed $value { get; } }
class WriteOnly extends ReadBase { public mixed $value { set {} } }
"#,
    );
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert!(
        stderr.contains("remaining method (ReadBase::$value::get)"),
        "{stderr}"
    );

    let (status, stdout, stderr) = run_stdin(
        r#"<?php
abstract class MutableBase { protected abstract int $value { get; set; } }
class ReadonlyChild extends MutableBase {
    public function __construct(protected readonly int $value) {}
}
"#,
    );
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert!(
        stderr.contains("remaining method (MutableBase::$value::set)"),
        "{stderr}"
    );
}

#[test]
fn built_in_spl_observer_interfaces_link_without_user_autoload() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
namespace LinkBoundary;
class Observer implements \SplObserver {
    public function update(\SplSubject $subject): void { echo "observed\n"; }
}
class Subject implements \SplSubject {
    public function attach(\SplObserver $observer): void {}
    public function detach(\SplObserver $observer): void {}
    public function notify(): void {}
}
$observer = new Observer();
$subject = new Subject();
$observer->update($subject);
var_dump(\interface_exists('SplObserver'), \interface_exists('SplSubject'));
"#,
    );
    assert_eq!(
        (status, stdout.as_str(), stderr.as_str()),
        (0, "observed\nbool(true)\nbool(true)\n", "")
    );
}
