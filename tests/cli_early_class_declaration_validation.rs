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
fn invalid_class_declaration_shapes_fail_before_execution() {
    let cases = [
        (
            "<?php\nabstract class PendingOwner {\n    public function finishLater();\n}\necho 'unreachable';\n",
            "Fatal error: Non-abstract method PendingOwner::finishLater() must contain body in Standard input code on line 3\n",
        ),
        (
            "<?php\nclass CaseFoldedMethods {\n    public function Commit() {}\n    public function cOMMIT() {}\n}\necho 'unreachable';\n",
            "Fatal error: Cannot redeclare CaseFoldedMethods::cOMMIT() in Standard input code on line 4\n",
        ),
        (
            "<?php\nclass RepeatedStorage {\n    public string $slot;\n    protected string $slot;\n}\necho 'unreachable';\n",
            "Fatal error: Cannot redeclare RepeatedStorage::$slot in Standard input code on line 4\n",
        ),
        (
            "<?php\nclass LexicalOwner {\n    public function install(): void {\n        if (true) {\n            class ForbiddenNestedDeclaration {}\n        }\n    }\n}\necho 'unreachable';\n",
            "Fatal error: Class declarations may not be nested in Standard input code on line 5\n",
        ),
        (
            "<?php\nclass RecursiveAlias extends SELF {}\necho 'unreachable';\n",
            "Fatal error: Cannot use \"SELF\" as class name, as it is reserved in Standard input code on line 2\n",
        ),
        (
            "<?php\nclass ParentAlias extends PaReNt {}\necho 'unreachable';\n",
            "Fatal error: Cannot use \"PaReNt\" as class name, as it is reserved in Standard input code on line 2\n",
        ),
        (
            "<?php\nclass OwnContract implements sElF {}\necho 'unreachable';\n",
            "Fatal error: Cannot use \"sElF\" as interface name, as it is reserved in Standard input code on line 2\n",
        ),
        (
            "<?php\nclass ParentContract implements PARENT {}\necho 'unreachable';\n",
            "Fatal error: Cannot use \"PARENT\" as interface name, as it is reserved in Standard input code on line 2\n",
        ),
        (
            "<?php\nnamespace Oracle\\Declarations;\nclass RootlessConstant {\n    public const OWNER = parent::class;\n}\necho 'unreachable';\n",
            "Fatal error: Cannot use \"parent\" when current class scope has no parent in Standard input code on line 4\n",
        ),
        (
            "<?php\nnamespace Oracle\\Declarations;\nclass LateDefault {\n    public function resolve(string $owner = static::class): void {}\n}\necho 'unreachable';\n",
            "Fatal error: static::class cannot be used for compile-time class name resolution in Standard input code on line 4\n",
        ),
    ];

    for (source, expected) in cases {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "source unexpectedly succeeded: {source}");
        assert_eq!(stdout, "");
        assert_eq!(stderr, expected);
    }
}

#[test]
fn declaration_errors_keep_php_source_priority() {
    let cases = [
        (
            "<?php\nclass HeaderBeforeMembers extends parent {\n    public function repeated() {}\n    public function REPEATED() {}\n}\n",
            "Fatal error: Cannot use \"parent\" as class name, as it is reserved in Standard input code on line 2\n",
        ),
        (
            "<?php\nabstract class MethodOrder {\n    public function missingBody();\n    public function duplicate() {}\n    public function DUPLICATE() {}\n}\n",
            "Fatal error: Non-abstract method MethodOrder::missingBody() must contain body in Standard input code on line 3\n",
        ),
        (
            "<?php\nclass MemberOrder {\n    public int $value;\n    protected int $value;\n    public function late(string $owner = static::class): void {}\n}\n",
            "Fatal error: Cannot redeclare MemberOrder::$value in Standard input code on line 4\n",
        ),
        (
            "<?php\nclass ReverseMemberOrder {\n    public function early(string $owner = static::class): void {}\n    public int $value;\n    protected int $value;\n}\n",
            "Fatal error: static::class cannot be used for compile-time class name resolution in Standard input code on line 3\n",
        ),
        (
            "<?php\nclass MixedMemberOrder {\n    public function work() {}\n    public function WORK() {}\n    public int $value;\n    protected int $value;\n}\n",
            "Fatal error: Cannot redeclare MixedMemberOrder::WORK() in Standard input code on line 4\n",
        ),
    ];

    for (source, expected) in cases {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(stderr, expected);
    }
}

#[test]
fn adjacent_classlike_forms_share_the_declaration_contract() {
    let cases = [
        (
            "<?php\nabstract class AbstractBodyBoundary {\n    abstract public function invalid(): void {}\n}\n",
            "Fatal error: Abstract function AbstractBodyBoundary::invalid() cannot contain body in Standard input code on line 3\n",
        ),
        (
            "<?php\ninterface ReservedInterfaceParent extends SELF {}\n",
            "Fatal error: Cannot use \"SELF\" as interface name, as it is reserved in Standard input code on line 2\n",
        ),
        (
            "<?php\nenum ReservedEnumContract implements PARENT { case Ready; }\n",
            "Fatal error: Cannot use \"PARENT\" as interface name, as it is reserved in Standard input code on line 2\n",
        ),
        (
            "<?php\nclass AnonymousBase {}\nclass AnonymousFactory extends AnonymousBase {\n    public function make(): object { return new class extends parent {}; }\n}\n",
            "Fatal error: Cannot use \"parent\" as class name, as it is reserved in Standard input code on line 4\n",
        ),
        (
            "<?php\nclass ClosureOwner {\n    public function factory(): Closure {\n        return function (): void { class ForbiddenClosureClass {} };\n    }\n}\n",
            "Fatal error: Class declarations may not be nested in Standard input code on line 4\n",
        ),
    ];

    for (source, expected) in cases {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(stderr, expected);
    }
}

#[test]
fn valid_empty_abstract_relative_and_named_function_forms_remain_available() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
abstract class AbstractBoundary { abstract protected function contract(): int; }
interface InterfaceBoundary { public function contract(): int; }
class CaseSensitiveStorage { public int $slot = 1; public int $Slot = 2; public function nothing(): void {} }
class StaticInstanceStorage { public static string $slot = 'static'; public string $slot = 'instance'; }
class FunctionBoundary { public function register(): void { function declare_from_named_function(): void { class AllowedFunctionLocalClass {} } } }
class EvalBoundary { public function register(): void { eval('class AllowedEvalClass {}'); } }
class ParentConstantOwner {}
class ChildConstantOwner extends ParentConstantOwner { public const OWNER = parent::class; public function resolve(string $owner = self::class): string { return $owner; } }
trait ParentAwareTrait { public const OWNER = parent::class; }
class TraitChildOwner extends ParentConstantOwner { use ParentAwareTrait; }
(new FunctionBoundary())->register();
declare_from_named_function();
(new EvalBoundary())->register();
$value = new CaseSensitiveStorage();
$value->nothing();
$split = new StaticInstanceStorage();
echo $value->slot, ':', $value->Slot, ':', StaticInstanceStorage::$slot, ':', $split->slot, ':', AllowedFunctionLocalClass::class, ':', AllowedEvalClass::class, ':', ChildConstantOwner::OWNER, ':', (new ChildConstantOwner())->resolve(), ':', is_string(TraitChildOwner::OWNER) ? 'trait-ok' : 'bad', "\n";
"#,
    );
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        "1:2:static:instance:AllowedFunctionLocalClass:AllowedEvalClass:ParentConstantOwner:ChildConstantOwner:trait-ok\n"
    );
    assert_eq!(stderr, "");
}

#[test]
fn nested_static_class_defaults_are_rejected_recursively() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nclass NestedLateDefault {\n    public function resolve(array $owners = ['outer' => [static::class]]): void {}\n}\n",
    );
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "Fatal error: static::class cannot be used for compile-time class name resolution in Standard input code on line 3\n"
    );
}

#[test]
fn included_source_from_a_method_remains_a_separate_declaration_unit() {
    let include_path = std::env::temp_dir().join(format!(
        "rphp-early-class-declaration-{}.php",
        std::process::id()
    ));
    std::fs::write(
        &include_path,
        "<?php\nif (true) {\n    class IncludedFromMethodBoundary {\n        public static function value(): string { return 'included'; }\n    }\n}\n",
    )
    .expect("include fixture should be written");
    let escaped_path = include_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        "<?php\nclass IncludeCallerBoundary {{\n    public function load(string $path): void {{ include $path; }}\n}}\n(new IncludeCallerBoundary())->load('{escaped_path}');\necho IncludedFromMethodBoundary::value(), \"\\n\";\n"
    );
    let result = run_stdin(&source);
    let _ = std::fs::remove_file(&include_path);
    let (status, stdout, stderr) = result;
    assert_eq!(status, 0);
    assert_eq!(stdout, "included\n");
    assert_eq!(stderr, "");
}
