mod common;

use common::run_php;

#[test]
fn throwable_constructors_commit_visible_fields_in_hook_order() {
    let source = r#"<?php
class HookedException extends Exception {
    private bool $seen = false;
    protected $code {
        set($value) {
            if ($this->seen) {
                throw new Exception('stop');
            }
            $this->seen = true;
            $this->code = $value;
        }
    }
}

$exception = new HookedException('old', 1, new Exception('previous'));
try {
    $exception->__construct('new', 2, null);
} catch (Exception $error) {
    echo $error->getMessage(), "\n";
}
echo $exception->getMessage(), '|', $exception->getCode(), '|',
    $exception->getPrevious()::class, "\n";

class HookedErrorException extends ErrorException {
    private bool $seen = false;
    protected $code {
        set($value) {
            if ($this->seen) {
                throw new Exception('stop error');
            }
            $this->seen = true;
            $this->code = $value;
        }
    }
}

$error = new HookedErrorException('old', 1, E_NOTICE, 'old.php', 7, new Exception('previous'));
try {
    $error->__construct('new', 2, E_WARNING, 'new.php', 9, null);
} catch (Exception $exception) {
    echo $exception->getMessage(), "\n";
}
echo $error->getMessage(), '|', $error->getCode(), '|', $error->getSeverity(), '|',
    $error->getFile(), '|', $error->getLine(), '|', $error->getPrevious()::class, "\n";
"#;

    assert_eq!(
        run_php(source),
        "stop\nnew|1|Exception\nstop error\nnew|1|8|old.php|7|Exception\n"
    );
}

#[test]
fn failed_generator_cleanup_chains_destructor_replacements() {
    assert_eq!(
        run_php(
            r#"<?php
class GeneratorPayload {
    public function __destruct() {
        throw new Exception('destructor');
    }
}
function values() {
    yield from [1, new GeneratorPayload];
}
$generator = values();
$generator->valid();
try {
    $generator->throw(new Exception('outer'));
} catch (Throwable $error) {
    echo $error->getMessage(), '|', $error->getPrevious()?->getMessage(), "\n";
}
"#,
        ),
        "destructor|outer\n"
    );
}
