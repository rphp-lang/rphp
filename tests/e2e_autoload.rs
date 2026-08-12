mod common;

use common::run_php;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempPhpDir(std::path::PathBuf);

impl TempPhpDir {
    fn new() -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("rphp-autoload-{}-{sequence}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, source: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, source).unwrap();
        path.to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
    }
}

impl Drop for TempPhpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn autoload_registry_preserves_order_prepend_deduplication_and_unregister() {
    let output = run_php(
        r#"<?php
function first_loader($name) { echo "first:$name|"; }
function second_loader($name) { echo "second:$name|"; }
function prepended_loader($name) { echo "prepended:$name|"; }

var_dump(spl_autoload_register('first_loader'));
var_dump(spl_autoload_register('second_loader'));
var_dump(spl_autoload_register('first_loader'));
var_dump(spl_autoload_register('prepended_loader', true, true));

foreach (spl_autoload_functions() as $loader) { echo $loader . ','; }
echo "\n";
var_dump(class_exists('MissingClass'));
var_dump(spl_autoload_unregister('second_loader'));
var_dump(spl_autoload_unregister('second_loader'));
foreach (spl_autoload_functions() as $loader) { echo $loader . ','; }
"#,
    );

    assert_eq!(
        output,
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "prepended_loader,first_loader,second_loader,\n",
            "prepended:MissingClass|first:MissingClass|second:MissingClass|bool(false)\n",
            "bool(true)\n",
            "bool(false)\n",
            "prepended_loader,first_loader,"
        )
    );
}

#[test]
fn object_method_autoloader_can_require_a_class_and_is_listed_verbatim() {
    let dir = TempPhpDir::new();
    let class_file = dir.write(
        "LoadedClass.php",
        "<?php class LoadedClass { public static function value() { return 'loaded'; } }",
    );
    let source = format!(
        r#"<?php
class Loader {{
    public function loadClass($name) {{
        echo "load:$name|";
        if ($name === 'LoadedClass') {{ require '{class_file}'; }}
    }}
}}
$loader = new Loader();
var_dump(spl_autoload_register([$loader, 'loadClass']));
$registered = spl_autoload_functions();
var_dump($registered[0][0] === $loader);
echo $registered[0][1] . '|';
var_dump(class_exists('LoadedClass'));
echo LoadedClass::value();
"#
    );

    assert_eq!(
        run_php(&source),
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "loadClass|load:LoadedClass|bool(true)\n",
            "loaded"
        )
    );
}

#[test]
fn method_exists_autoloads_and_includes_abstract_and_non_public_methods() {
    let dir = TempPhpDir::new();
    let interface_file = dir.write(
        "LoadedContract.php",
        "<?php interface LoadedContract { public function requiredMethod(); }",
    );
    let class_file = dir.write(
        "MethodOwner.php",
        "<?php abstract class MethodOwner { abstract protected function hiddenMethod(); }",
    );
    let source = format!(
        r#"<?php
function method_loader($name) {{
    echo "load:$name|";
    if ($name === 'LoadedContract') {{ require '{interface_file}'; }}
    if ($name === 'MethodOwner') {{ require '{class_file}'; }}
}}
spl_autoload_register('method_loader');
var_dump(method_exists('LoadedContract', 'requiredMethod'));
var_dump(method_exists('MethodOwner', 'hiddenMethod'));
var_dump(method_exists('MethodOwner', 'missingMethod'));
"#
    );

    assert_eq!(
        run_php(&source),
        concat!(
            "load:LoadedContract|bool(true)\n",
            "load:MethodOwner|bool(true)\n",
            "bool(false)\n"
        )
    );
}

#[test]
fn existence_probes_honor_kind_case_leading_separator_and_autoload_flag() {
    let dir = TempPhpDir::new();
    let symbols_file = dir.write(
        "symbols.php",
        "<?php interface LoadedInterface {} trait LoadedTrait {} enum LoadedEnum {} class LoadedClass {}",
    );
    let source = format!(
        r#"<?php
function symbol_loader($name) {{
    echo "load:$name|";
    require_once '{symbols_file}';
}}
spl_autoload_register('symbol_loader');
var_dump(class_exists('LoadedClass', false));
var_dump(interface_exists('\\loadedinterface'));
var_dump(trait_exists('LOADEDTRAIT', false));
var_dump(enum_exists('LoadedEnum', false));
var_dump(class_exists('loadedclass', false));
var_dump(class_exists('LoadedInterface', false));
var_dump(class_exists('LoadedEnum', false));
"#
    );

    assert_eq!(
        run_php(&source),
        concat!(
            "bool(false)\n",
            "load:loadedinterface|bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(false)\n",
            "bool(true)\n"
        )
    );
}

#[test]
fn autoload_recursion_is_suppressed_and_callback_exceptions_propagate() {
    let output = run_php(
        r#"<?php
function recursive_loader($name) {
    echo "recursive:$name|";
    var_dump(class_exists($name));
}
spl_autoload_register('recursive_loader');
var_dump(class_exists('RecursiveMissing'));
spl_autoload_unregister('recursive_loader');

function throwing_loader($name) { throw new Exception("boom:$name"); }
spl_autoload_register('throwing_loader');
try {
    class_exists('ExplodingClass');
} catch (Exception $error) {
    echo get_class($error) . ':' . $error->getMessage();
}
"#,
    );

    assert_eq!(
        output,
        concat!(
            "recursive:RecursiveMissing|bool(false)\n",
            "bool(false)\n",
            "Exception:boom:ExplodingClass"
        )
    );
}

#[test]
fn closure_autoloader_can_be_unregistered_by_the_same_value() {
    let output = run_php(
        r#"<?php
$loader = function ($name) { echo "closure:$name|"; };
spl_autoload_register($loader);
var_dump(count(spl_autoload_functions()));
var_dump(spl_autoload_unregister($loader));
var_dump(count(spl_autoload_functions()));
var_dump(class_exists('NeverLoaded'));
"#,
    );

    assert_eq!(
        output,
        concat!("int(1)\n", "bool(true)\n", "int(0)\n", "bool(false)\n")
    );
}

#[test]
fn distinct_closures_from_one_source_remain_distinct_callbacks() {
    let output = run_php(
        r#"<?php
function make_loader($label) {
    return function ($name) use ($label) { echo "$label:$name|"; };
}
$first = make_loader('first');
$second = make_loader('second');
spl_autoload_register($first);
spl_autoload_register($second);
var_dump(count(spl_autoload_functions()));
spl_autoload_unregister($first);
var_dump(count(spl_autoload_functions()));
var_dump(class_exists('StillMissing'));
"#,
    );

    assert_eq!(
        output,
        concat!("int(2)\n", "int(1)\n", "second:StillMissing|bool(false)\n")
    );
}
