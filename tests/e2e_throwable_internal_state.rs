mod common;
use common::run_php;

#[test]
fn throwable_constructors_validate_inherited_internal_signatures() {
    assert_eq!(
        run_php(
            r#"<?php
class ChildException extends Exception {}
class ChildError extends Error {}
class ChildErrorException extends ErrorException {}

foreach ([
    'Exception' => fn() => new Exception(new stdClass),
    'ChildException' => fn() => new ChildException(new stdClass),
    'Error' => fn() => new Error(new stdClass),
    'ChildError' => fn() => new ChildError(new stdClass),
    'ErrorException' => fn() => new ErrorException(new stdClass),
    'ChildErrorException' => fn() => new ChildErrorException(new stdClass),
    'previous' => fn() => new Exception('message', 0, new stdClass),
] as $label => $construct) {
    try {
        $construct();
    } catch (Throwable $throwable) {
        echo $label, '|', $throwable::class, '|', $throwable->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "Exception|TypeError|Exception::__construct(): Argument #1 ($message) must be of type string, stdClass given\n",
            "ChildException|TypeError|Exception::__construct(): Argument #1 ($message) must be of type string, stdClass given\n",
            "Error|TypeError|Error::__construct(): Argument #1 ($message) must be of type string, stdClass given\n",
            "ChildError|TypeError|Error::__construct(): Argument #1 ($message) must be of type string, stdClass given\n",
            "ErrorException|TypeError|ErrorException::__construct(): Argument #1 ($message) must be of type string, stdClass given\n",
            "ChildErrorException|TypeError|ErrorException::__construct(): Argument #1 ($message) must be of type string, stdClass given\n",
            "previous|TypeError|Exception::__construct(): Argument #3 ($previous) must be of type ?Throwable, stdClass given\n",
        )
    );
}

#[test]
fn lowercase_parent_spelling_inherits_canonical_throwable_layout() {
    assert_eq!(
        run_php(
            r#"<?php
class LowercaseThrowable extends exception {
    public function rewrite(): void {
        $this->message = 'changed';
        $this->code = 12;
        $this->file = 'virtual.php';
        $this->line = 34;
    }
}

$throwable = new LowercaseThrowable('original', 7);
$throwable->rewrite();
$keys = array_map(static fn(string $key): string => str_replace("\0", '@', $key), array_keys((array) $throwable));
echo $throwable->getMessage(), '|', $throwable->getCode(), '|',
    $throwable->getFile(), '|', $throwable->getLine(), '|', implode(',', $keys), "\n";
"#,
        ),
        "changed|12|virtual.php|34|@*@message,@Exception@string,@*@code,@*@file,@*@line,@Exception@trace,@Exception@previous\n"
    );
}

#[test]
fn throwable_string_cache_and_trace_use_private_declared_storage() {
    assert_eq!(
        run_php(
            r#"<?php
$throwable = new Exception('cached');
$string = new ReflectionProperty(Exception::class, 'string');
$trace = new ReflectionProperty(Exception::class, 'trace');

echo '[', $string->getValue($throwable), ']|', count($trace->getValue($throwable)), '|';
$first = (string) $throwable;
$second = (string) $throwable;
echo (int) ($string->getValue($throwable) === $first), '|',
    (int) ($first === $second), '|', count((array) $throwable), "\n";
"#,
        ),
        "[]|0|1|1|7\n"
    );
}

#[test]
fn escaping_finally_exception_does_not_duplicate_a_shared_previous_ancestor() {
    assert_eq!(
        run_php(
            r#"<?php
$root = new LogicException('root');
try {
    try {
        throw new RuntimeException('pending', 0, $root);
    } finally {
        throw new DomainException('replacement', 0, $root);
    }
} catch (Throwable $throwable) {
    echo $throwable->getMessage(), '|', $throwable->getPrevious()->getMessage(), '|',
        (int) ($throwable->getPrevious() === $root), '|',
        (int) ($root->getPrevious() === null), "\n";
}
"#,
        ),
        "replacement|root|1|1\n"
    );
}

#[test]
fn explicit_empty_origin_survives_the_later_throw_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
class EmptyOriginException extends exception {
    public function __construct() {
        $this->file = '';
        $this->line = 0;
    }
}

try {
    throw new EmptyOriginException;
} catch (Throwable $throwable) {
    echo '[', $throwable->getFile(), ']|', $throwable->getLine(), '|',
        count($throwable->getTrace()), "\n";
}
"#,
        ),
        "[]|0|0\n"
    );
}

#[test]
fn malformed_private_trace_warns_and_renders_each_valid_frame_best_effort() {
    assert_eq!(
        run_php(
            r#"<?php
$throwable = new Exception('trace');
$trace = new ReflectionProperty(Exception::class, 'trace');
set_error_handler(static function (int $level, string $message): bool {
    echo 'warning|', $level, '|', $message, "\n";
    return true;
});

$trace->setValue($throwable, [null]);
echo 'null|', $throwable->getTraceAsString(), "\n";
$trace->setValue($throwable, [[]]);
echo 'empty|', $throwable->getTraceAsString(), "\n";
$trace->setValue($throwable, [[
    'file' => null,
    'line' => null,
    'class' => null,
    'type' => null,
    'function' => null,
    'args' => null,
]]);
echo 'invalid|', $throwable->getTraceAsString(), "\n";
"#,
        ),
        concat!(
            "null|warning|2|Expected array for frame 0\n",
            "#0 {main}\n",
            "empty|#0 [internal function]: ()\n#1 {main}\n",
            "invalid|warning|2|File name is not a string\n",
            "warning|2|Value for class is not a string\n",
            "warning|2|Value for type is not a string\n",
            "warning|2|Value for function is not a string\n",
            "warning|2|args element is not an array\n",
            "#0 [unknown file]: [unknown][unknown][unknown]()\n#1 {main}\n",
        )
    );
}
