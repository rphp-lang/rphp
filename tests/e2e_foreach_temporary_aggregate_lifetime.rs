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
            "rphp_foreach_temporary_aggregate_{}_{}",
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
fn empty_temporary_aggregate_destructor_is_caught_before_loop_exit() {
    assert_eq!(
        run_php(
            r#"<?php
class EmptyAggregate implements IteratorAggregate {
    public function getIterator(): Traversable {
        echo 'iterator|';
        return new ArrayIterator([]);
    }
    public function __destruct() {
        echo 'destructor|';
        throw new RuntimeException('temporary');
    }
}
$value = 'preserved';
try {
    foreach (new EmptyAggregate as $value) {
        echo 'body|';
    }
    echo 'after-loop|';
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), '|';
} finally {
    echo 'finally|';
}
echo $value;
"#,
        ),
        "iterator|destructor|RuntimeException:temporary|finally|preserved"
    );
}

#[test]
fn user_iterator_releases_aggregate_between_first_valid_and_current() {
    assert_eq!(
        run_php(
            r#"<?php
class LoggedIterator implements Iterator {
    private int $position = 0;
    public function __construct(private string $label) {}
    public function rewind(): void { echo "rewind:$this->label|"; $this->position = 0; }
    public function valid(): bool { echo "valid:$this->label|"; return $this->position < 2; }
    public function current(): mixed { echo "current:$this->label|"; return $this->position + 10; }
    public function key(): mixed { echo "key:$this->label|"; return $this->position; }
    public function next(): void { echo "next:$this->label|"; ++$this->position; }
}
class LoggedAggregate implements IteratorAggregate {
    public function __construct(private string $label) { echo "construct:$label|"; }
    public function getIterator(): Traversable { echo "iterator:$this->label|"; return new LoggedIterator($this->label); }
    public function __destruct() { echo "destructor:$this->label|"; }
}
foreach (new LoggedAggregate('break') as $key => $value) {
    echo "body:break:$key:$value|";
    break;
}
foreach (new LoggedAggregate('continue') as $key => $value) {
    echo "body:continue:$key:$value|";
    continue;
}
try {
    foreach (new LoggedAggregate('throw') as $key => $value) {
        echo "body:throw:$key:$value|";
        throw new LogicException('body');
    }
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), '|';
}
"#,
        ),
        concat!(
            "construct:break|iterator:break|rewind:break|valid:break|destructor:break|current:break|key:break|body:break:0:10|",
            "construct:continue|iterator:continue|rewind:continue|valid:continue|destructor:continue|current:continue|key:continue|body:continue:0:10|next:continue|valid:continue|current:continue|key:continue|body:continue:1:11|next:continue|valid:continue|",
            "construct:throw|iterator:throw|rewind:throw|valid:throw|destructor:throw|current:throw|key:throw|body:throw:0:10|LogicException:body|",
        )
    );
}

#[test]
fn throwing_destructor_preempts_value_materialization_without_previous_exception() {
    assert_eq!(
        run_php(
            r#"<?php
class PriorityIterator implements Iterator {
    public function rewind(): void { echo 'rewind|'; }
    public function valid(): bool { echo 'valid|'; return true; }
    public function current(): mixed { echo 'current|'; return 99; }
    public function key(): mixed { echo 'key|'; return 7; }
    public function next(): void { echo 'next|'; }
}
class PriorityAggregate implements IteratorAggregate {
    public function getIterator(): Traversable { echo 'iterator|'; return new PriorityIterator; }
    public function __destruct() { echo 'destructor|'; throw new RuntimeException('destructor'); }
}
$key = 'old-key';
$value = 'old-value';
try {
    foreach (new PriorityAggregate as $key => $value) {
        echo 'body|';
        throw new LogicException('body');
    }
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), ':previous=',
        $error->getPrevious() ? get_class($error->getPrevious()) : 'none', '|';
}
echo "$key:$value";
"#,
        ),
        "iterator|rewind|valid|destructor|RuntimeException:destructor:previous=none|old-key:old-value"
    );
}

#[test]
fn named_and_aliased_aggregate_receivers_keep_their_last_reference_lifetime() {
    assert_eq!(
        run_php(
            r#"<?php
class RetainedAggregate implements IteratorAggregate {
    public function __construct(private string $label) { echo "construct:$label|"; }
    public function getIterator(): Traversable { echo "iterator:$this->label|"; return new ArrayIterator([]); }
    public function __destruct() { echo "destructor:$this->label|"; }
}
$named = new RetainedAggregate('named');
foreach ($named as $value) {}
echo 'after:named|';
unset($named);

$first = new RetainedAggregate('aliased');
$second = $first;
foreach ($first as $value) {}
unset($first);
echo 'after:first-unset|';
unset($second);
echo 'after:second-unset|';
"#,
        ),
        concat!(
            "construct:named|iterator:named|after:named|destructor:named|",
            "construct:aliased|iterator:aliased|after:first-unset|destructor:aliased|after:second-unset|",
        )
    );
}

#[test]
fn direct_include_and_eval_share_the_temporary_aggregate_release_boundary() {
    let body = r#"static function (string $label): string {
    echo $label, ':';
    try {
        foreach (new SharedTemporaryAggregate($label) as $value) {
            echo 'body|';
        }
        return 'after-loop';
    } catch (Throwable $error) {
        return get_class($error) . ':' . $error->getMessage();
    }
}"#;
    let fixture = IncludeFixture::new(&format!("<?php\nreturn {body};\n"));
    let source = format!(
        r#"<?php
class SharedTemporaryAggregate implements IteratorAggregate {{
    public function __construct(private string $label) {{}}
    public function getIterator(): Traversable {{
        echo 'iterator:', $this->label, '|';
        return new ArrayIterator([]);
    }}
    public function __destruct() {{
        echo 'destructor:', $this->label, '|';
        throw new RuntimeException('temporary:' . $this->label);
    }}
}}
$direct = {body};
$included = include {include:?};
$evaluated = eval(<<<'PHP'
return {body};
PHP);
echo $direct('direct'), ';', $included('include'), ';', $evaluated('eval');
"#,
        include = fixture.file.to_string_lossy(),
    );
    assert_eq!(
        run_php_with_source_context(
            &source,
            "/virtual/temporary-aggregate-lifetime.php",
            "/virtual",
        ),
        concat!(
            "direct:iterator:direct|destructor:direct|RuntimeException:temporary:direct;",
            "include:iterator:include|destructor:include|RuntimeException:temporary:include;",
            "eval:iterator:eval|destructor:eval|RuntimeException:temporary:eval",
        )
    );
}
