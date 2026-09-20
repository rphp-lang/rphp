mod common;

use common::{run_php, run_php_with_source_context};

#[test]
fn array_unset_runs_final_destructor_after_the_bucket_is_detached() {
    assert_eq!(
        run_php(
            r#"<?php
class ArrayBackedLifetime {
    public array $owner;
    public string $label;

    public function __construct(string $label) {
        $this->label = $label;
    }

    public function __destruct() {
        echo "destroy:$this->label:", count($this->owner[0]), '|';
    }
}

$owner = [[new ArrayBackedLifetime('direct')]];
$owner[0][0]->owner =& $owner;
unset($owner[0][0]);
echo 'after-direct|';

$owner = [[new ArrayBackedLifetime('aliased')]];
$owner[0][0]->owner =& $owner;
$kept = $owner[0][0];
unset($owner[0][0]);
echo 'after-aliased|';
unset($kept);
echo 'done';
"#,
        ),
        "destroy:direct:0|after-direct|after-aliased|destroy:aliased:0|done"
    );
}

#[test]
fn array_unset_keeps_referenced_bucket_payload_alive_until_the_last_alias_is_rebound() {
    assert_eq!(
        run_php(
            r#"<?php
class ReferencedBucketLifetime {
    public function __destruct() {
        echo 'destroy|';
    }
}

$value = new ReferencedBucketLifetime;
$bucket =& $value;
$array = [&$bucket];
unset($array[0]);
echo 'after-unset|';
$bucket = null;
echo 'after-rebind|';
unset($value);
echo 'done';
"#,
        ),
        "after-unset|destroy|after-rebind|done"
    );
}

#[test]
fn throwing_destructor_from_array_unset_is_catchable_after_the_mutation() {
    assert_eq!(
        run_php(
            r#"<?php
class ThrowingUnsetLifetime {
    public array $owner;

    public function __destruct() {
        echo 'destroy:', count($this->owner), '|';
        throw new RuntimeException('released');
    }
}

$owner = [new ThrowingUnsetLifetime];
$owner[0]->owner =& $owner;
try {
    unset($owner[0]);
    echo 'after-unset|';
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), ':', count($owner), '|';
}
echo 'done';
"#,
        ),
        "destroy:0|RuntimeException:released:0|done"
    );
}

#[test]
fn internal_reference_outputs_commit_before_replaced_values_are_destroyed() {
    assert_eq!(
        run_php(
            r#"<?php
class HeaderOutputHolder {
    public stdClass|string $key;
}

$map = new WeakMap;
$holder = new HeaderOutputHolder;
$holder->key = new stdClass;
$map[$holder->key] = new class {
    public function __destruct() {
        global $holder;
        echo 'destroy:', get_debug_type($holder->key), ':', strlen($holder->key), '|';
        throw new RuntimeException('output');
    }
};

try {
    headers_sent($holder->key);
    echo 'returned|';
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), ':';
    echo get_debug_type($error->getTrace()[1]['args'][0]), '|';
}
echo 'after:', get_debug_type($holder->key), '|';
"#,
        ),
        "destroy:string:0|RuntimeException:output:string|after:string|"
    );
}

#[test]
fn multiple_internal_reference_outputs_release_old_values_in_reverse_order() {
    assert_eq!(
        run_php(
            r#"<?php
class HeaderOutputLifetime {
    public function __construct(private string $label) {}
    public function __destruct() {
        echo "destroy:$this->label|";
        if ($this->label === 'line') {
            throw new RuntimeException('line');
        }
    }
}

$file = new HeaderOutputLifetime('file');
$line = new HeaderOutputLifetime('line');
try {
    headers_sent($file, $line);
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), '|';
}
echo get_debug_type($file), ':', get_debug_type($line), '|';
"#,
        ),
        "destroy:line|destroy:file|RuntimeException:line|string:int|"
    );
}

#[test]
fn internal_reference_output_destructor_trace_keeps_the_builtin_frame() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
$map = new WeakMap;
$key = new stdClass;
$map[$key] = new class {
    public function __destruct() {
        throw new RuntimeException('trace');
    }
};

try {
    headers_sent($key);
} catch (Throwable $error) {
    foreach ($error->getTrace() as $index => $frame) {
        echo $index, ':', $frame['function'], '|';
    }
    $builtin = $error->getTrace()[1];
    echo $builtin['file'], '|', get_debug_type($builtin['args'][0]), '|';
}
"#,
            "/virtual/finalization.php",
            "/virtual",
        ),
        "0:__destruct|1:headers_sent|/virtual/finalization.php|null|"
    );
}
