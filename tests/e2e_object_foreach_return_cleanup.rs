mod common;

use common::{run_php, run_php_with_source_context};
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
            "rphp_object_foreach_return_cleanup_{}_{}",
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

#[test]
fn throwing_temporary_source_cancels_pending_return_and_releases_its_value() {
    assert_eq!(
        run_php(
            r#"<?php
class ReturningObjectSource {
    public int $entry = 1;
    public function __construct(private string $label, private bool $throw) {
        echo "source-create:$label|";
    }
    public function __destruct() {
        echo "source-destroy:$this->label|";
        if ($this->throw) {
            throw new RuntimeException("source:$this->label");
        }
    }
}
class PendingObjectValue {
    public function __construct(private string $label) {
        echo "value-create:$label|";
    }
    public function __destruct() {
        echo "value-destroy:$this->label|";
    }
}
function returnFromObject(string $label, bool $throw): object {
    foreach (new ReturningObjectSource($label, $throw) as $entry) {
        echo "body:$label:$entry|";
        return new PendingObjectValue($label);
    }
    throw new LogicException('unreachable');
}
$value = returnFromObject('ok', false);
echo 'caller:ok|';
unset($value);
try {
    $value = returnFromObject('throw', true);
    echo 'caller:throw|';
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), ':previous=',
        $error->getPrevious() ? get_class($error->getPrevious()) : 'none', '|';
}
echo isset($value) ? 'value:set' : 'value:unset';
"#,
        ),
        concat!(
            "source-create:ok|body:ok:1|value-create:ok|source-destroy:ok|caller:ok|value-destroy:ok|",
            "source-create:throw|body:throw:1|value-create:throw|source-destroy:throw|value-destroy:throw|",
            "RuntimeException:source:throw:previous=none|value:unset",
        )
    );
}

#[test]
fn cleanup_exceptions_replace_return_expression_and_value_destructor_failures_in_order() {
    assert_eq!(
        run_php(
            r#"<?php
class PriorityObjectSource {
    public int $entry = 1;
    public function __construct(private string $label) {}
    public function __destruct() {
        echo "source:$this->label|";
        throw new RuntimeException("source:$this->label");
    }
}
class PriorityReturnValue {
    public function __construct(private string $label) {
        echo "value-create:$label|";
    }
    public function __destruct() {
        echo "value-destroy:$this->label|";
        throw new LogicException("value:$this->label");
    }
}
function priorityReturn(string $label): object {
    foreach (new PriorityObjectSource($label) as $entry) {
        if ($label === 'expression') {
            throw new DomainException('expression');
        }
        return new PriorityReturnValue($label);
    }
    throw new Exception('unreachable');
}
foreach (['expression', 'value'] as $label) {
    try {
        priorityReturn($label);
    } catch (Throwable $error) {
        echo 'caught:', get_class($error), ':', $error->getMessage(), ':previous=',
            $error->getPrevious()
                ? get_class($error->getPrevious()) . ':' . $error->getPrevious()->getMessage()
                : 'none', '|';
    }
}
"#,
        ),
        concat!(
            "source:expression|caught:RuntimeException:source:expression:previous=DomainException:expression|",
            "value-create:value|source:value|value-destroy:value|",
            "caught:LogicException:value:value:previous=RuntimeException:source:value|",
        )
    );
}

#[test]
fn finally_precedes_cleanup_and_nested_sources_release_inside_out() {
    assert_eq!(
        run_php(
            r#"<?php
class OrderedObjectSource {
    public int $entry = 1;
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
function returnThroughFinally(): string {
    foreach (new OrderedObjectSource('finally', true) as $entry) {
        try {
            echo 'try|';
            return 'pending';
        } finally {
            echo 'finally|';
        }
    }
    return 'after';
}
try {
    echo 'result:', returnThroughFinally(), '|';
} catch (Throwable $error) {
    echo 'caught:', get_class($error), ':', $error->getMessage(), '|';
}
function nestedReturn(): string {
    foreach (new OrderedObjectSource('outer') as $outer) {
        foreach (new OrderedObjectSource('inner') as $inner) {
            echo 'return-expression|';
            return 'done';
        }
    }
    return 'after';
}
echo nestedReturn(), '|caller';
"#,
        ),
        concat!(
            "result:create:finally|try|finally|destroy:finally|caught:RuntimeException:cleanup:finally|",
            "create:outer|create:inner|return-expression|destroy:inner|destroy:outer|done|caller",
        )
    );
}

#[test]
fn deferred_finally_cleanup_trace_points_back_to_the_return_site() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class FinallyTraceSource {
    public int $entry = 1;
    public function __destruct() {
        finally_trace_missing_constant;
    }
}
function finallyTraceReturn(): string {
    foreach (new FinallyTraceSource as $value) {
        try {
            return 'pending';
        } finally {
            echo 'finally|';
        }
    }
    return 'after';
}
try {
    finallyTraceReturn();
} catch (Throwable $error) {
    $trace = $error->getTrace();
    echo $error->getLine(), '|', $trace[0]['function'], ':', $trace[0]['line'];
}
"#,
            "/virtual/finally-trace.php",
            "/virtual",
        ),
        "finally|5|__destruct:11",
    );
}

#[test]
fn named_sources_and_non_object_foreach_controls_keep_their_existing_lifetime() {
    assert_eq!(
        run_php(
            r#"<?php
class RetainedObjectSource {
    public string $entry = 'object';
    public function __destruct() { echo 'object-destroy|'; }
}
function returnFromNamed(object $source): string {
    foreach ($source as $entry) {
        return "named:$entry";
    }
    return 'named:empty';
}
$source = new RetainedObjectSource;
$alias = $source;
echo returnFromNamed($source), '|after-return|';
unset($source);
echo 'after-source|';
unset($alias);

class DirectReturnIterator implements Iterator {
    private int $position = 0;
    public function rewind(): void { echo 'rewind|'; $this->position = 0; }
    public function valid(): bool { echo 'valid|'; return $this->position === 0; }
    public function current(): mixed { return 'iterator'; }
    public function key(): mixed { return 0; }
    public function next(): void { ++$this->position; }
    public function __destruct() { echo 'iterator-destroy|'; }
}
function returnFromIterator(): string {
    foreach (new DirectReturnIterator as $entry) {
        return $entry;
    }
    return 'iterator-empty';
}
function returnFromArray(): string {
    foreach (['array'] as $entry) {
        return $entry;
    }
    return 'array-empty';
}
echo returnFromIterator(), '|', returnFromArray();
"#,
        ),
        concat!(
            "named:object|after-return|after-source|object-destroy|",
            "rewind|valid|iterator-destroy|iterator|array",
        )
    );
}

#[test]
fn array_and_object_foreach_abrupt_completions_release_at_php_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
class ArrayLoopValue {
    public function __construct(private string $label) {}
    public function __destruct() { echo "destroy:$this->label|"; }
}
foreach ([new ArrayLoopValue('outer')] as $outer) {
    $outer = null;
    try {
        foreach ([new ArrayLoopValue('inner')] as $inner) {
            $inner = null;
            goto afterInner;
        }
    } finally {
        echo 'inner-finally|';
    }
afterInner:
}
echo 'goto-complete|';

class ThrowingObjectLoopSource {
    public int $entry = 1;
    public function __destruct() {
        echo 'source-destroy|';
        throw new RuntimeException('source-cleanup');
    }
}
foreach ([0] as $sentinel) {
    try {
        foreach (new ThrowingObjectLoopSource as $entry) {
            try {
                break 2;
            } finally {
                echo 'break-finally|';
            }
        }
    } catch (Throwable $error) {
        echo 'caught:', $error->getMessage(), '|';
    } finally {
        echo 'outer-finally|';
    }
}
echo 'break-complete|';

class RetainedReturnValue {
    public function __destruct() {
        echo 'return-destroy|';
        throw new Exception('return-cleanup');
    }
}
function returnWithRetainedIterationValue(): int {
    try {
        foreach ([new RetainedReturnValue] as $value) {
            try {
                echo 'return|';
                return 7;
            } finally {
                echo 'return-inner-finally|';
            }
        }
    } finally {
        echo 'return-outer-finally|';
    }
}
try {
    echo returnWithRetainedIterationValue();
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), '|';
}
"#,
        ),
        concat!(
            "destroy:inner|inner-finally|destroy:outer|goto-complete|",
            "break-finally|source-destroy|caught:source-cleanup|outer-finally|break-complete|",
            "return|return-inner-finally|return-outer-finally|return-destroy|",
            "Exception:return-cleanup|",
        )
    );
}

#[test]
fn caught_iteration_exception_keeps_the_remaining_array_snapshot_live() {
    assert_eq!(
        run_php(
            r#"<?php
class FailingIterationEntry {
    public function visit() {
        echo 'visit|';
        throw new RuntimeException('continue');
    }
}
$entries = [new FailingIterationEntry, new FailingIterationEntry];
foreach ($entries as $entry) {
    try {
        if ($entry->visit()) {
            return;
        }
    } catch (RuntimeException $error) {
        echo 'caught|';
    }
}
echo 'done:', count($entries), '|';
"#,
        ),
        "visit|caught|visit|caught|done:2|"
    );
}

#[test]
fn direct_include_and_eval_keep_destructor_origin_and_trace_frames() {
    let body = r#"static function (string $label): string {
    foreach (new OriginObjectSource($label) as $entry) {
        return 'pending';
    }
    return 'after';
}"#;
    let fixture = IncludeFixture::new(&format!("<?php\nreturn {body};\n"));
    let source = format!(
        r#"<?php
class OriginObjectSource {{
    public int $entry = 1;
    public function __construct(private string $label) {{}}
    public function __destruct() {{
        origin_cleanup_missing_constant;
    }}
}}
$direct = {body};
$included = include {include:?};
$evaluated = eval(<<<'PHP'
return {body};
PHP);
foreach ([
    'direct' => $direct,
    'include' => $included,
    'eval' => $evaluated,
] as $label => $callable) {{
    try {{
        $callable($label);
    }} catch (Throwable $error) {{
        echo $label, ':', basename($error->getFile()), ':', $error->getLine(), ':',
            get_class($error), ':', $error->getMessage(), ':trace=';
        foreach ($error->getTrace() as $frame) {{
            $function = $frame['function'] ?? '{{main}}';
            echo str_contains($function, 'closure') ? 'closure' : $function, ',';
        }}
        echo '|';
    }}
}}
"#,
        include = fixture.file.to_string_lossy(),
    );
    assert_eq!(
        run_php_with_source_context(&source, "/virtual/object-return-cleanup.php", "/virtual"),
        concat!(
            "direct:object-return-cleanup.php:6:Error:Undefined constant \"origin_cleanup_missing_constant\":trace=__destruct,closure,|",
            "include:object-return-cleanup.php:6:Error:Undefined constant \"origin_cleanup_missing_constant\":trace=__destruct,closure,|",
            "eval:object-return-cleanup.php:6:Error:Undefined constant \"origin_cleanup_missing_constant\":trace=__destruct,closure,|",
        )
    );
}
