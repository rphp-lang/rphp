mod common;

use common::run_php;
use rphp::{compiler::compile::Compiler, lexer::Lexer, parser::Parser};

fn compile_error(source: &str, file: &str) -> String {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    Compiler::new()
        .with_source_path(file)
        .compile(&statements)
        .err()
        .expect("program should fail during compilation")
        .message
}

#[test]
fn attribute_callable_placeholder_reports_the_annotated_target_line() {
    assert_eq!(
        compile_error(
            r#"<?php

#[InvalidAttribute(...)]
final class AnnotatedTarget
{
}
"#,
            "/virtual/fcc-attribute.php",
        ),
        "Cannot create Closure as attribute argument in /virtual/fcc-attribute.php on line 4"
    );
}

#[test]
fn nested_nullsafe_callable_is_a_compile_error_before_receiver_evaluation() {
    assert_eq!(
        compile_error(
            r#"<?php

if (false) {
    $receiver
        ?->child
        ->run(...);
}
echo "unreachable\n";
"#,
            "/virtual/fcc-nullsafe.php",
        ),
        "Cannot combine nullsafe operator with Closure creation in /virtual/fcc-nullsafe.php on line 6"
    );
}

#[test]
fn discarded_bare_variables_skip_only_the_unobservable_rvalue_fetch() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function (int $severity, string $message): bool {
    echo "notice:", $message, "\n";
    return true;
});
$discarded;
($alsoDiscarded);
echo "after-discard\n";
var_dump($observed);
$defined = 41;
$defined;
echo $defined + 1, "\n";
"#,
        ),
        "after-discard\nnotice:Undefined variable $observed\nNULL\n42\n"
    );
}

#[test]
fn ordinary_nullsafe_calls_attributes_and_dynamic_callables_remain_observable() {
    assert_eq!(
        run_php(
            r#"<?php
final class CallableTarget {
    public ?CallableTarget $child = null;
    public function run(string $value): string { return "run:$value"; }
}
$receiver = new CallableTarget;
var_dump($receiver?->child?->run('unused'));
$method = 'run';
$callback = $receiver->$method(...);
echo $callback('ok'), "\n";
#[Attribute]
class PlainAttribute { public function __construct(public string $value) {} }
#[PlainAttribute('valid')]
class PlainTarget {}
echo (new ReflectionClass(PlainTarget::class))->getAttributes()[0]->getArguments()[0];
"#,
        ),
        "NULL\nrun:ok\nvalid"
    );
}
