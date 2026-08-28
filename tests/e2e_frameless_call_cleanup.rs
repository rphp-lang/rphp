mod common;

use common::{run_php, run_php_with_source_context};
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::instruction::RELEASE_TEMPS_NESTED_OBJECTS;
use rphp::vm::opcode::OpCode;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

static INCLUDE_COUNTER: AtomicU64 = AtomicU64::new(0);

struct IncludeFixture {
    directory: std::path::PathBuf,
    file: std::path::PathBuf,
}

impl IncludeFixture {
    fn new(source: &str) -> Self {
        let identity = INCLUDE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let directory = std::env::temp_dir().join(format!(
            "rphp_frameless_call_cleanup_{}_{}",
            std::process::id(),
            identity
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("source.php");
        let mut output = std::fs::File::create(&file).unwrap();
        output.write_all(source.as_bytes()).unwrap();
        Self { directory, file }
    }
}

impl Drop for IncludeFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn compile_source(source: &str) -> rphp::compiler::compile::CompileResult {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    Compiler::new().compile(&statements).unwrap()
}

#[test]
fn compiler_marks_only_unambiguous_positional_frameless_calls() {
    let compiled = compile_source(
        r#"<?php
in_array(new stdClass, [new stdClass], true);
\in_array(new stdClass, [new stdClass]);
in_array(needle: new stdClass, haystack: [new stdClass], strict: true);
strlen('value');
"#,
    );
    let releases = compiled
        .main
        .instructions
        .iter()
        .filter(|instruction| instruction.opcode == OpCode::ReleaseTemps)
        .map(|instruction| instruction._pad & RELEASE_TEMPS_NESTED_OBJECTS != 0)
        .collect::<Vec<_>>();

    assert_eq!(releases, [true, true, false, false]);

    let scoped =
        compile_source("<?php namespace Scoped; in_array(new \\stdClass, [new \\stdClass], true);");
    assert!(scoped.main.instructions.iter().all(|instruction| {
        instruction.opcode != OpCode::ReleaseTemps
            || instruction._pad & RELEASE_TEMPS_NESTED_OBJECTS == 0
    }));
}

#[test]
fn successful_frameless_calls_release_nested_operands_before_the_next_statement() {
    assert_eq!(
        run_php(
            r#"<?php
class FramelessOperand {
    public function __construct(private string $label, private bool $throw = false) {
        echo "create:$label|";
    }
    public function __destruct() {
        echo "destroy:$this->label|";
        if ($this->throw) {
            throw new RuntimeException("cleanup:$this->label");
        }
    }
}
in_array(
    new FramelessOperand('two-needle'),
    [[new FramelessOperand('two-nested')]],
);
echo 'after-two|';
try {
    in_array(new FramelessOperand('needle'), [new FramelessOperand('haystack', true)], true);
    echo 'after-call|';
} catch (Throwable $error) {
    echo 'caught:', get_class($error), ':', $error->getMessage(), ':previous=',
        $error->getPrevious() ? get_class($error->getPrevious()) : 'none', '|';
}
echo 'done';
"#,
        ),
        concat!(
            "create:two-needle|create:two-nested|destroy:two-needle|destroy:two-nested|after-two|",
            "create:needle|create:haystack|destroy:needle|destroy:haystack|",
            "caught:RuntimeException:cleanup:haystack:previous=none|done",
        )
    );
}

#[test]
fn destructor_errors_replace_prior_cleanup_and_call_errors_in_zend_order() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class PriorityFramelessOperand {
    public function __construct(private string $label, private bool $throw) {
        echo "create:$label|";
    }
    public function __destruct() {
        echo "destroy:$this->label|";
        if ($this->throw) {
            throw new RuntimeException("cleanup:$this->label");
        }
    }
}
try {
    in_array(
        new PriorityFramelessOperand('needle', true),
        [new PriorityFramelessOperand('haystack', true)],
        true,
    );
} catch (Throwable $error) {
    echo 'both:', $error->getMessage(), ':previous=',
        $error->getPrevious() ? $error->getPrevious()->getMessage() : 'none', '|';
}
try {
    in_array(new PriorityFramelessOperand('type', true), new stdClass, true);
} catch (Throwable $error) {
    echo 'type:', get_class($error), ':', $error->getMessage(), ':previous=',
        $error->getPrevious() ? get_class($error->getPrevious()) : 'none', '|';
}
echo 'done';
"#,
        ),
        concat!(
            "create:needle|create:haystack|destroy:needle|destroy:haystack|",
            "both:cleanup:haystack:previous=cleanup:needle|",
            "create:type|destroy:type|",
            "type:RuntimeException:cleanup:type:previous=TypeError|done",
        )
    );
}

#[test]
fn argument_evaluation_errors_release_only_the_completed_left_operands() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class EvaluatedFramelessOperand {
    public function __construct(private string $label) {
        echo "create:$label|";
    }
    public function __destruct() {
        echo "destroy:$this->label|";
        throw new RuntimeException("cleanup:$this->label");
    }
}
function failHaystack(): array {
    echo 'evaluate:haystack|';
    throw new DomainException('argument');
}
try {
    in_array(new EvaluatedFramelessOperand('needle'), failHaystack(), true);
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), ':previous=',
        $error->getPrevious()
            ? get_class($error->getPrevious()) . ':' . $error->getPrevious()->getMessage()
            : 'none', '|';
}
echo 'done';
"#,
        ),
        concat!(
            "create:needle|evaluate:haystack|destroy:needle|",
            "RuntimeException:cleanup:needle:previous=DomainException:argument|done",
        )
    );
}

#[test]
fn named_cow_and_non_frameless_operands_remain_owned_by_their_variables() {
    assert_eq!(
        run_php(
            r#"<?php
class RetainedFramelessOperand {
    public function __construct(private string $label) {
        echo "create:$label|";
    }
    public function __destruct() { echo "destroy:$this->label|"; }
}
$needle = new RetainedFramelessOperand('needle');
$member = new RetainedFramelessOperand('member');
$haystack = [$member];
$copy = $haystack;
$reference =& $needle;
in_array($reference, $haystack, true);
echo 'after-reference|';
in_array(needle: $needle, haystack: $haystack, strict: true);
echo 'after-named|';
unset($haystack);
echo 'after-haystack|';
unset($copy);
echo 'after-copy|';
unset($member);
echo 'after-member|';
unset($needle);

function ordinaryCall(mixed $first, mixed $second, mixed $third): void {}
$ordinaryNeedle = new RetainedFramelessOperand('ordinary-needle');
$ordinaryMember = new RetainedFramelessOperand('ordinary-member');
$ordinaryHaystack = [$ordinaryMember];
ordinaryCall($ordinaryNeedle, $ordinaryHaystack, true);
echo 'after-ordinary|';
unset($ordinaryHaystack, $ordinaryMember, $ordinaryNeedle);
echo 'done';
"#,
        ),
        concat!(
            "create:needle|create:member|after-reference|after-named|after-haystack|after-copy|",
            "destroy:member|after-member|destroy:needle|",
            "create:ordinary-needle|create:ordinary-member|after-ordinary|",
            "destroy:ordinary-member|destroy:ordinary-needle|done",
        )
    );
}

#[test]
fn direct_include_and_eval_cleanup_keep_origin_and_consuming_call_site() {
    let body = r#"return static function (): void {
    in_array(0, [new ContextFramelessOperand], true);
};"#;
    let fixture = IncludeFixture::new(&format!("<?php\n{body}\n"));
    let source = format!(
        r#"<?php
class ContextFramelessOperand {{
    public function __destruct() {{
        context_frameless_missing_constant;
    }}
}}
$direct = static function (): void {{
    in_array(0, [new ContextFramelessOperand], true);
}};
$included = include {include:?};
$evaluated = eval(<<<'PHP'
return static function (): void {{
    in_array(0, [new ContextFramelessOperand], true);
}};
PHP);
foreach (['direct' => $direct, 'include' => $included, 'eval' => $evaluated] as $label => $callable) {{
    try {{
        $callable();
    }} catch (Throwable $error) {{
        $site = null;
        foreach ($error->getTrace() as $frame) {{
            if (isset($frame['file'], $frame['line'])) {{
                $site = $frame;
                break;
            }}
        }}
        $siteFile = str_contains($site['file'], "eval()'d code")
            ? 'eval'
            : basename($site['file']);
        echo $label, ':', basename($error->getFile()), ':', $error->getLine(), ':',
            get_class($error), ':site=', $siteFile, ':', $site['line'], '|';
    }}
}}
"#,
        include = fixture.file.to_string_lossy(),
    );

    assert_eq!(
        run_php_with_source_context(&source, "/virtual/frameless-context.php", "/virtual"),
        concat!(
            "direct:frameless-context.php:4:Error:site=frameless-context.php:8|",
            "include:frameless-context.php:4:Error:site=source.php:3|",
            "eval:frameless-context.php:4:Error:site=eval:2|",
        )
    );
}

#[test]
fn direct_cleanup_metadata_uses_the_destructor_origin_and_call_site() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class MetadataFramelessOperand {
    public function __destruct() {
        frameless_cleanup_missing_constant;
    }
}
function metadataFramelessCall(): void {
    in_array(0, [new MetadataFramelessOperand], true);
}
try {
    metadataFramelessCall();
} catch (Throwable $error) {
    $trace = $error->getTrace();
    echo basename($error->getFile()), ':', $error->getLine(), ':', get_class($error), ':',
        $error->getMessage(), ':trace=';
    foreach ($trace as $frame) {
        echo $frame['function'] ?? '{main}', ':', $frame['line'] ?? 0, ',';
    }
}
"#,
            "/virtual/frameless-cleanup.php",
            "/virtual",
        ),
        concat!(
            "frameless-cleanup.php:4:Error:Undefined constant \"frameless_cleanup_missing_constant\":",
            "trace=__destruct:8,metadataFramelessCall:11,",
        )
    );
}
