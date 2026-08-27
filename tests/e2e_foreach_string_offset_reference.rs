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
            "rphp_foreach_string_offset_reference_{}_{}",
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
fn direct_offsets_reject_reference_creation_after_key_diagnostics() {
    assert_eq!(
        run_php(
            r#"<?php
function inspectOffset(string $label, mixed $key): void {
    $events = [];
    set_error_handler(static function (int $severity, string $message) use (&$events): bool {
        $events[] = "warning:$severity:$message";
        return true;
    });
    $string = 'abcd';
    try {
        foreach ($string[$key] as &$value) {
            $events[] = 'loop';
        }
        $events[] = 'after-loop';
    } catch (Throwable $error) {
        $events[] = get_class($error) . ':' . $error->getMessage();
    } finally {
        unset($value);
        restore_error_handler();
    }
    echo $label, ':', implode('|', $events), ':', $string, ';';
}
inspectOffset('invalid', '2tail');
inspectOffset('integer', 2);
inspectOffset('out', 20);
inspectOffset('negative', -1);
"#,
        ),
        concat!(
            "invalid:warning:2:Illegal string offset \"2tail\"|Error:Cannot create references to/from string offsets:abcd;",
            "integer:Error:Cannot create references to/from string offsets:abcd;",
            "out:Error:Cannot create references to/from string offsets:abcd;",
            "negative:Error:Cannot create references to/from string offsets:abcd;",
        )
    );
}

#[test]
fn key_conversions_and_invalid_types_keep_their_earliest_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
$keys = [
    'float' => 1.75,
    'true' => true,
    'null' => null,
    'space' => ' 2',
    'array' => [],
    'object' => new stdClass(),
];
foreach ($keys as $label => $key) {
    $events = [];
    set_error_handler(static function (int $severity, string $message) use (&$events): bool {
        $events[] = "warning:$severity:$message";
        return true;
    });
    $string = 'abcd';
    try {
        foreach ($string[$key] as &$value) {
            $events[] = 'loop';
        }
    } catch (Throwable $error) {
        $events[] = get_class($error) . ':' . $error->getMessage();
    } finally {
        unset($value);
        restore_error_handler();
    }
    echo $label, ':', implode('|', $events), ';';
}
"#,
        ),
        concat!(
            "float:warning:2:String offset cast occurred|Error:Cannot create references to/from string offsets;",
            "true:warning:2:String offset cast occurred|Error:Cannot create references to/from string offsets;",
            "null:warning:2:String offset cast occurred|Error:Cannot create references to/from string offsets;",
            "space:Error:Cannot create references to/from string offsets;",
            "array:TypeError:Cannot access offset of type array on string;",
            "object:TypeError:Cannot access offset of type stdClass on string;",
        )
    );
}

#[test]
fn receiver_key_handler_and_state_order_precede_the_reference_error() {
    assert_eq!(
        run_php(
            r#"<?php
$events = [];
$receiver = static function () use (&$events): string {
    $events[] = 'receiver';
    return 'abcd';
};
$key = static function () use (&$events): string {
    $events[] = 'key';
    return '2tail';
};
set_error_handler(static function (int $severity, string $message) use (&$events): never {
    $events[] = "handler:$severity:$message";
    throw new RuntimeException('handler-stop');
});
try {
    foreach ($receiver()[$key()] as &$value) {
        $events[] = 'loop';
    }
} catch (Throwable $error) {
    $events[] = get_class($error) . ':' . $error->getMessage();
} finally {
    unset($value);
    restore_error_handler();
}
echo implode('|', $events);
"#,
        ),
        concat!(
            "receiver|key|handler:2:Illegal string offset \"2tail\"|",
            "RuntimeException:handler-stop",
        )
    );
}

#[test]
fn suppression_value_foreach_and_reference_return_controls_remain_distinct() {
    assert_eq!(
        run_php(
            r#"<?php
$events = [];
set_error_handler(static function (int $severity, string $message) use (&$events): bool {
    $events[] = "warning:$severity:$message";
    return true;
});
$string = 'abcd';
foreach (@$string['2tail'] as &$value) {
    $events[] = 'loop';
}
unset($value);
restore_error_handler();
echo 'suppressed:', implode('|', $events), ':', $string, ';';

$array = [1, 2];
foreach (@$array as &$value) {
    $value *= 10;
}
unset($value);
echo 'array:', json_encode($array), ';';

$referenced = [1, 2];
function &returnReference(): array {
    global $referenced;
    return $referenced;
}
foreach (@returnReference() as &$value) {
    $value *= 10;
}
unset($value);
echo 'call:', json_encode($referenced), ';';

$events = [];
set_error_handler(static function (int $severity, string $message) use (&$events): bool {
    $events[] = "warning:$severity:$message";
    return true;
});
foreach ($string['2tail'] as $key => $value) {
    $events[] = "loop:$key:$value";
}
restore_error_handler();
echo 'value:', implode('|', $events);
"#,
        ),
        concat!(
            "suppressed:warning:2:Illegal string offset \"2tail\"|warning:2:foreach() argument must be of type array|object, string given:abcd;",
            "array:[1,2];call:[10,20];",
            "value:warning:2:Illegal string offset \"2tail\"|warning:2:foreach() argument must be of type array|object, string given",
        )
    );
}

#[test]
fn direct_include_and_eval_sources_share_the_referenceability_boundary() {
    let body = r#"static function (string $label): string {
    $events = [];
    set_error_handler(static function (int $severity, string $message) use (&$events): bool {
        $events[] = "warning:$severity:$message";
        return true;
    });
    $string = 'abcd';
    try {
        foreach ($string['2tail'] as &$value) {
            $events[] = 'loop';
        }
    } catch (Throwable $error) {
        $events[] = get_class($error) . ':' . $error->getMessage();
    } finally {
        unset($value);
        restore_error_handler();
    }
    return $label . ':' . implode('|', $events) . ':' . $string;
}"#;
    let fixture = IncludeFixture::new(&format!("<?php\nreturn {body};\n"));
    let source = format!(
        r#"<?php
$direct = {body};
$included = include {include:?};
$evaluated = eval(<<<'PHP'
return {body};
PHP);
echo $direct('direct'), ';', $included('include'), ';', $evaluated('eval');
"#,
        include = fixture.file.to_string_lossy(),
    );
    let event = concat!(
        "warning:2:Illegal string offset \"2tail\"|",
        "Error:Cannot create references to/from string offsets:abcd",
    );
    assert_eq!(
        run_php_with_source_context(&source, "/virtual/string-offset-reference.php", "/virtual"),
        format!("direct:{event};include:{event};eval:{event}")
    );
}

#[test]
fn property_order_array_cow_and_reference_identity_remain_stable() {
    assert_eq!(
        run_php(
            r#"<?php
$events = [];
$holder = (object) ['text' => 'abcd'];
$receiver = static function () use (&$events, $holder): object {
    $events[] = 'receiver';
    return $holder;
};
$property = static function () use (&$events): string {
    $events[] = 'property';
    return 'text';
};
$key = static function () use (&$events): string {
    $events[] = 'key';
    return '2tail';
};
set_error_handler(static function (int $severity, string $message) use (&$events): bool {
    $events[] = "warning:$severity:$message";
    return true;
});
try {
    foreach ($receiver()->{$property()}[$key()] as &$value) {
        $events[] = 'loop';
    }
} catch (Throwable $error) {
    $events[] = get_class($error) . ':' . $error->getMessage();
} finally {
    unset($value);
    restore_error_handler();
}
echo 'property:', implode('|', $events), ':', $holder->text, ';';

$shared = ['slot' => [1, 2]];
$copy = $shared;
foreach ($copy['slot'] as &$value) {
    $value *= 10;
}
unset($value);
echo 'cow:', json_encode($shared), ':', json_encode($copy), ';';

$referenced = [3, 4];
$alias =& $referenced;
$outer = ['slot' => &$referenced];
foreach ($outer['slot'] as &$value) {
    $value *= 10;
}
unset($value);
echo 'reference:', json_encode($referenced), ':', json_encode($alias), ':', json_encode($outer);
"#,
        ),
        concat!(
            "property:receiver|property|key|warning:2:Illegal string offset \"2tail\"|",
            "Error:Cannot create references to/from string offsets:abcd;",
            "cow:{\"slot\":[1,2]}:{\"slot\":[10,20]};",
            "reference:[30,40]:[30,40]:{\"slot\":[30,40]}",
        )
    );
}
