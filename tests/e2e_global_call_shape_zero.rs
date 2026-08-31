mod common;

use common::run_php;

struct TemporaryPath(std::path::PathBuf);

impl TemporaryPath {
    fn unique(label: &str) -> Self {
        Self(std::env::temp_dir().join(format!(
            "rphp-call-shape-{label}-{}-{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )))
    }

    fn php_literal(&self) -> String {
        self.0
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
    }
}

impl Drop for TemporaryPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn seven_global_functions_expose_php_85_call_shapes_and_metadata() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'assert_options', 'debug_zval_dump', 'define', 'flock',
    'hash_init', 'hrtime', 'var_dump',
] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, '|', $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), '|', (string) $function->getReturnType(), '|',
        $function->getExtensionName(), '|',
        $function->isDeprecated() ? 'deprecated' : 'current', "\n";
    foreach ($function->getParameters() as $parameter) {
        echo $parameter->getName(), ':',
            $parameter->hasType() ? (string) $parameter->getType() : '-', ':',
            $parameter->isOptional() ? 'optional' : 'required', ':',
            $parameter->isDefaultValueAvailable()
                ? serialize($parameter->getDefaultValue())
                : '-', ':',
            $parameter->isPassedByReference() ? 'ref' : 'value', ':',
            $parameter->isVariadic() ? 'variadic' : 'fixed', "\n";
    }
}
echo 'HASH_HMAC=', HASH_HMAC, "\n";
"#,
        ),
        concat!(
            "assert_options|1/2|mixed|standard|deprecated\n",
            "option:int:required:-:value:fixed\n",
            "value:mixed:optional:-:value:fixed\n",
            "debug_zval_dump|1/2|void|standard|current\n",
            "value:mixed:required:-:value:fixed\n",
            "values:mixed:optional:-:value:variadic\n",
            "define|2/3|bool|Core|current\n",
            "constant_name:string:required:-:value:fixed\n",
            "value:mixed:required:-:value:fixed\n",
            "case_insensitive:bool:optional:b:0;:value:fixed\n",
            "flock|2/3|bool|standard|current\n",
            "stream:-:required:-:value:fixed\n",
            "operation:int:required:-:value:fixed\n",
            "would_block:-:optional:N;:ref:fixed\n",
            "hash_init|1/4|HashContext|hash|current\n",
            "algo:string:required:-:value:fixed\n",
            "flags:int:optional:i:0;:value:fixed\n",
            "key:string:optional:s:0:\"\";:value:fixed\n",
            "options:array:optional:a:0:{}:value:fixed\n",
            "hrtime|0/1|array|int|float|false|standard|current\n",
            "as_number:bool:optional:b:0;:value:fixed\n",
            "var_dump|1/2|void|standard|current\n",
            "value:mixed:required:-:value:fixed\n",
            "values:mixed:optional:-:value:variadic\n",
            "HASH_HMAC=1\n",
        )
    );
}

#[test]
fn assert_options_keeps_unknown_default_optional_and_updates_state() {
    assert_eq!(
        run_php(
            r#"<?php
$parameter = (new ReflectionFunction('assert_options'))->getParameters()[1];
var_dump($parameter->isOptional(), $parameter->isDefaultValueAvailable(), (string) $parameter);
set_error_handler(function (int $level, string $message): bool {
    echo "$level:$message\n";
    return true;
});
var_dump(assert_options(option: 1, value: 'off'));
var_dump(assert_options(1));
var_dump(assert_options(1, []));
var_dump(assert_options(1));
var_dump(assert_options(1, '1suffix'));
var_dump(assert_options(1));
try { assert_options(99); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "bool(true)\n",
            "bool(false)\n",
            "string(52) \"Parameter #1 [ <optional> mixed $value = <default> ]\"\n",
            "8192:Function assert_options() is deprecated since 8.3\n",
            "int(1)\n",
            "8192:Function assert_options() is deprecated since 8.3\n",
            "int(0)\n",
            "8192:Function assert_options() is deprecated since 8.3\n",
            "2:Array to string conversion\n",
            "int(0)\n",
            "8192:Function assert_options() is deprecated since 8.3\n",
            "int(0)\n",
            "8192:Function assert_options() is deprecated since 8.3\n",
            "int(0)\n",
            "8192:Function assert_options() is deprecated since 8.3\n",
            "int(1)\n",
            "8192:Function assert_options() is deprecated since 8.3\n",
            "ValueError:assert_options(): Argument #1 ($option) must be an ASSERT_* constant\n",
        )
    );
}

#[test]
fn assert_options_enforces_its_integer_contract_under_strict_types() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
set_error_handler(static fn(): bool => true);
try { assert_options('1'); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        "TypeError:assert_options(): Argument #1 ($option) must be of type int, string given\n"
    );
}

#[test]
fn dump_functions_consume_every_positional_variadic_value_and_return_void() {
    assert_eq!(
        run_php(
            r#"<?php
debug_zval_dump(7, 8);
var_dump('x', 9);
var_dump(var_dump(true));
"#,
        ),
        concat!(
            "int(7)\n",
            "int(8)\n",
            "string(1) \"x\"\n",
            "int(9)\n",
            "bool(true)\n",
            "NULL\n",
        )
    );
}

#[test]
fn define_accepts_the_legacy_third_argument_but_ignores_case_folding() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function (int $level, string $message): bool {
    echo "$level:$message\n";
    return true;
});
var_dump(define(
    constant_name: 'RPHP_CallShape_Camel',
    value: 17,
    case_insensitive: true,
));
var_dump(constant('RPHP_CallShape_Camel'), defined('rphp_callshape_camel'));
try { define('RPHP_CallShape::Invalid', 1, true); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "2:define(): Argument #3 ($case_insensitive) is ignored since declaration of case-insensitive constants is no longer supported\n",
            "bool(true)\n",
            "int(17)\n",
            "bool(false)\n",
            "ValueError:define(): Argument #1 ($constant_name) cannot be a class constant\n",
        )
    );
}

#[test]
fn define_validates_string_and_bool_arguments_under_strict_types() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
foreach ([
    static fn() => define(7, 'x'),
    static fn() => define('RPHP_STRICT_DEFINE', 1, 1),
] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "TypeError:define(): Argument #1 ($constant_name) must be of type string, int given\n",
            "TypeError:define(): Argument #3 ($case_insensitive) must be of type bool, int given\n",
        )
    );
}

#[test]
fn hash_contexts_stream_binary_data_for_every_admitted_algorithm() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['md5', 'xxh128', 'crc32'] as $algorithm) {
    $context = hash_init($algorithm);
    var_dump(hash_update($context, "a\0"), hash_update($context, 'b'));
    echo $algorithm, ':', hash_final($context), "\n";
}
"#,
        ),
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "md5:70350f6027bce3713f6b76473084309b\n",
            "bool(true)\n",
            "bool(true)\n",
            "xxh128:39797789ed4c7ea0d5a06cd078125351\n",
            "bool(true)\n",
            "bool(true)\n",
            "crc32:a1625aa1\n",
        )
    );
}

#[test]
fn hash_init_hmac_uses_the_key_and_preserves_embedded_nuls() {
    assert_eq!(
        run_php(
            r#"<?php
$context = hash_init('md5', HASH_HMAC, "secret");
hash_update($context, "a\0b");
echo hash_final($context), "\n";
$binary = hash_init('md5', HASH_HMAC, "secret");
hash_update($binary, "a\0b");
echo bin2hex(hash_final($binary, true)), "\n";
"#,
        ),
        concat!(
            "14c96434fa23325ec63500a00787f315\n",
            "14c96434fa23325ec63500a00787f315\n",
        )
    );
}

#[test]
fn hash_init_rejects_unknown_non_crypto_and_empty_hmac_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    static fn() => hash_init('dummy'),
    static fn() => hash_init('dummy', []),
    static fn() => hash_init('crc32', HASH_HMAC, 'key'),
    static fn() => hash_init('md5', HASH_HMAC),
] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "ValueError:hash_init(): Argument #1 ($algo) must be a valid hashing algorithm\n",
            "TypeError:hash_init(): Argument #2 ($flags) must be of type int, array given\n",
            "ValueError:hash_init(): Argument #1 ($algo) must be a cryptographic hashing algorithm if HMAC is requested\n",
            "ValueError:hash_init(): Argument #3 ($key) must not be empty when HMAC is requested\n",
        )
    );
}

#[test]
fn hash_init_named_options_select_the_xxh128_seed() {
    assert_eq!(
        run_php(
            r#"<?php
$context = hash_init(options: ['seed' => 1], algo: 'xxh128');
hash_update(data: "a\0b", context: $context);
echo hash_final(context: $context), "\n";
set_error_handler(function (int $level, string $message): bool {
    echo "$level:$message\n";
    return true;
});
$ignored = hash_init('xxh128', options: ['seed' => '1']);
echo hash_final($ignored), "\n";
$unseeded = hash_init('md5', options: ['seed' => '1']);
echo hash_final($unseeded), "\n";
"#,
        ),
        concat!(
            "9512284761e85bd28ebed4bebe43fbe0\n",
            "8192:hash_init(): Passing a seed of a type other than int is deprecated because it is ignored\n",
            "99aa06d3014798d86001c324468d497f\n",
            "d41d8cd98f00b204e9800998ecf8427e\n",
        )
    );
}

#[test]
fn hash_init_validates_every_declared_argument_under_strict_types() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
foreach ([
    static fn() => hash_init('md5', '0'),
    static fn() => hash_init('md5', 0, 7),
    static fn() => hash_init('md5', 0, '', null),
] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "TypeError:hash_init(): Argument #2 ($flags) must be of type int, string given\n",
            "TypeError:hash_init(): Argument #3 ($key) must be of type string, int given\n",
            "TypeError:hash_init(): Argument #4 ($options) must be of type array, null given\n",
        )
    );
}

#[test]
fn hash_final_binary_output_finalizes_the_context_once() {
    assert_eq!(
        run_php(
            r#"<?php
$context = hash_init('md5');
hash_update($context, "a\0b");
$digest = hash_final($context, true);
echo strlen($digest), ':', bin2hex($digest), "\n";
foreach ([
    static fn() => hash_update(new stdClass, 'x'),
    static fn() => hash_final(new stdClass),
    static fn() => hash_final($context),
    static fn() => hash_update($context, 'again'),
] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "16:70350f6027bce3713f6b76473084309b\n",
            "TypeError:hash_update(): Argument #1 ($context) must be of type HashContext, stdClass given\n",
            "TypeError:hash_final(): Argument #1 ($context) must be of type HashContext, stdClass given\n",
            "TypeError:hash_final(): Argument #1 ($context) must be a valid, non-finalized HashContext\n",
            "TypeError:hash_update(): Argument #1 ($context) must be a valid, non-finalized HashContext\n",
        )
    );
}

#[test]
fn flock_validates_operations_updates_would_block_and_rejects_closed_streams() {
    let path = TemporaryPath::unique("flock-contract");
    let source = format!(
        r#"<?php
$stream = fopen('{}', 'c+');
$wouldBlock = 99;
var_dump(flock(stream: $stream, operation: LOCK_EX, would_block: $wouldBlock), $wouldBlock);
var_dump(flock($stream, LOCK_UN));
var_dump(flock($stream, -1));
try {{ flock($stream, 0); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
$directory = opendir(dirname('{}'));
$wouldBlock = 99;
var_dump(flock($directory, LOCK_SH, $wouldBlock), $wouldBlock);
closedir($directory);
fclose($stream);
try {{ flock($stream, LOCK_SH); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
"#,
        path.php_literal(),
        path.php_literal(),
    );
    assert_eq!(
        run_php(&source),
        concat!(
            "bool(true)\n",
            "int(0)\n",
            "bool(true)\n",
            "bool(true)\n",
            "ValueError:flock(): Argument #2 ($operation) must be one of LOCK_SH, LOCK_EX, or LOCK_UN\n",
            "bool(false)\n",
            "int(0)\n",
            "TypeError:flock(): Argument #1 ($stream) must be an open stream resource\n",
        )
    );
}

#[test]
fn flock_nonblocking_contention_sets_the_reference_only_when_blocked() {
    let path = TemporaryPath::unique("flock-contention");
    let source = format!(
        r#"<?php
$owner = fopen('{}', 'c+');
$contender = fopen('{}', 'c+');
var_dump(flock($owner, LOCK_EX));
$wouldBlock = 99;
var_dump(flock($contender, LOCK_EX | LOCK_NB, $wouldBlock), $wouldBlock);
var_dump(flock($owner, LOCK_UN));
$wouldBlock = 99;
$dynamicFlock = 'flock';
var_dump($dynamicFlock($contender, LOCK_EX | LOCK_NB, $wouldBlock), $wouldBlock);
var_dump(flock($contender, LOCK_UN));
fclose($owner);
fclose($contender);
"#,
        path.php_literal(),
        path.php_literal(),
    );
    assert_eq!(
        run_php(&source),
        concat!(
            "bool(true)\n",
            "bool(false)\n",
            "int(1)\n",
            "bool(true)\n",
            "bool(true)\n",
            "int(0)\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn flock_enforces_the_operation_integer_under_strict_types() {
    let path = TemporaryPath::unique("flock-strict");
    let source = format!(
        r#"<?php declare(strict_types=1);
$stream = fopen('{}', 'c+');
try {{ flock($stream, '2'); }}
catch (Throwable $error) {{ echo get_class($error), ':', $error->getMessage(), "\n"; }}
fclose($stream);
"#,
        path.php_literal(),
    );
    assert_eq!(
        run_php(&source),
        "TypeError:flock(): Argument #2 ($operation) must be of type int, string given\n"
    );
}

#[test]
fn hrtime_switches_between_pair_and_scalar_using_the_as_number_argument() {
    assert_eq!(
        run_php(
            r#"<?php
$pair = hrtime();
echo is_array($pair) && count($pair) === 2 && is_int($pair[0]) && is_int($pair[1]) ? 'pair' : 'bad';
echo '|', is_int(hrtime(as_number: true)) ? 'number' : 'bad';
echo '|', is_array(hrtime(0)) ? 'weak-false' : 'bad';
echo '|', is_int(hrtime(1)) ? 'weak-true' : 'bad', "\n";
"#,
        ),
        "pair|number|weak-false|weak-true\n"
    );
}

#[test]
fn hrtime_enforces_boolean_under_strict_types() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
try { hrtime(1); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        "TypeError:hrtime(): Argument #1 ($as_number) must be of type bool, int given\n"
    );
}
