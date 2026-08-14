mod common;

use common::{run_php, run_php_with_source_context};
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
fn null_and_omitted_registration_append_the_default_loader_once() {
    let output = run_php(
        r#"<?php
var_dump(spl_autoload_register());
var_dump(spl_autoload_register(null));
var_dump(spl_autoload_register(null, true, true));
var_dump(spl_autoload_functions());
var_dump(spl_autoload_unregister('spl_autoload'));
var_dump(spl_autoload_functions());
"#,
    );

    assert_eq!(
        output,
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "array(1) {\n  [0]=>\n  string(12) \"spl_autoload\"\n}\n",
            "bool(true)\n",
            "array(0) {\n}\n"
        )
    );
}

#[test]
fn default_spl_autoload_loads_lowercase_namespaced_paths_once() {
    let dir = TempPhpDir::new();
    std::fs::create_dir_all(dir.0.join("project")).unwrap();
    std::fs::write(
        dir.0.join("project/loadedclass.php"),
        "<?php namespace Project; class LoadedClass { public static function value() { return 'loaded'; } }",
    )
    .unwrap();
    let source_file = dir.0.join("autoload.php").to_string_lossy().into_owned();
    let source_dir = dir.0.to_string_lossy().into_owned();
    let output = run_php_with_source_context(
        r#"<?php
var_dump(spl_autoload_extensions());
var_dump(spl_autoload_register());
var_dump(class_exists('Project\\LoadedClass'));
echo Project\LoadedClass::value() . "|";
spl_autoload('Project\\LoadedClass');
var_dump(class_exists('Project\\LoadedClass', false));
"#,
        &source_file,
        &source_dir,
    );

    assert_eq!(
        output,
        concat!(
            "string(9) \".inc,.php\"\n",
            "bool(true)\n",
            "bool(true)\n",
            "loaded|bool(true)\n"
        )
    );
}

#[test]
fn included_class_autoloads_implemented_interface_before_constant_composition() {
    let dir = TempPhpDir::new();
    let contract = dir.write(
        "Contract.php",
        "<?php namespace Fixture; interface Contract { public const VALUE = 'inherited'; }",
    );
    let service = dir.write(
        "Service.php",
        "<?php namespace Fixture; class Service implements Contract {}",
    );
    let source = format!(
        "<?php function load_contract($class) {{ if ($class === 'Fixture\\\\Contract') require '{contract}'; }} spl_autoload_register('load_contract'); require '{service}'; echo Fixture\\Service::VALUE;"
    );
    assert_eq!(run_php(&source), "inherited");
}

#[test]
fn new_expression_invokes_registered_autoloaders_before_class_resolution() {
    let dir = TempPhpDir::new();
    std::fs::write(
        dir.0.join("loaded.php"),
        "<?php class ConstructedByLoader { public function value() { return 'loaded'; } }",
    )
    .unwrap();
    let source_file = dir.0.join("new.php").to_string_lossy().into_owned();
    let source_dir = dir.0.to_string_lossy().into_owned();
    let output = run_php_with_source_context(
        r#"<?php
function load_for_new($class) {
    echo "load:$class|";
    if ($class === 'ConstructedByLoader') { require __DIR__ . '/loaded.php'; }
}
spl_autoload_register('load_for_new');
$values = [];
for ($index = 0; $index < 3; $index++) {
    $object = new ConstructedByLoader();
    $values[] = $object->value();
}
echo implode(',', $values);
"#,
        &source_file,
        &source_dir,
    );

    assert_eq!(output, "load:ConstructedByLoader|loaded,loaded,loaded");
}

#[test]
fn dynamic_new_owns_its_class_name_across_autoload_reentry() {
    let dir = TempPhpDir::new();
    let loaded_class = dir.write(
        "DynamicLoaded.php",
        "<?php class DynamicLoaded { public function value() { return 'loaded'; } } class ReplacementClass {}",
    );
    let source = format!(
        r#"<?php
class DynamicNameState {{ public static $value = 'DynamicLoaded'; }}
$class = DynamicNameState::$value;
spl_autoload_register(function($requested) {{
    echo "load:$requested|";
    DynamicNameState::$value = 'ReplacementClass';
    require '{loaded_class}';
}});
$object = new $class();
echo get_class($object) . '|' . DynamicNameState::$value . '|' . $object->value();
"#,
    );

    assert_eq!(
        run_php(&source),
        "load:DynamicLoaded|DynamicLoaded|ReplacementClass|loaded"
    );
}

#[test]
fn static_method_call_invokes_registered_autoloaders_before_method_resolution() {
    let dir = TempPhpDir::new();
    std::fs::write(
        dir.0.join("static.php"),
        "<?php class StaticLoadedByComposerStyle { public static function value() { return 'loaded'; } }",
    )
    .unwrap();
    let source_file = dir.0.join("static-call.php").to_string_lossy().into_owned();
    let source_dir = dir.0.to_string_lossy().into_owned();
    let output = run_php_with_source_context(
        r#"<?php
function load_for_static_call($class) {
    echo "load:$class|";
    if ($class === 'StaticLoadedByComposerStyle') { require __DIR__ . '/static.php'; }
}
spl_autoload_register('load_for_static_call');
echo StaticLoadedByComposerStyle::value();
"#,
        &source_file,
        &source_dir,
    );

    assert_eq!(output, "load:StaticLoadedByComposerStyle|loaded");
}

#[test]
fn class_constant_fetch_invokes_registered_autoloaders_before_resolution() {
    let dir = TempPhpDir::new();
    let class_file = dir.write(
        "ConstantOwner.php",
        "<?php class ConstantOwner { public const VALUE = 'loaded'; }",
    );
    let source = format!(
        r#"<?php
function load_for_class_constant($class) {{
    echo "load:$class|";
    if ($class === 'ConstantOwner') {{ require '{class_file}'; }}
}}
spl_autoload_register('load_for_class_constant');
echo ConstantOwner::VALUE;
"#,
    );

    assert_eq!(run_php(&source), "load:ConstantOwner|loaded");
}

#[test]
fn missing_static_call_class_throws_class_not_found_after_autoload() {
    let output = run_php(
        r#"<?php
function observe_missing_static_class($class) { echo "load:$class|"; }
spl_autoload_register('observe_missing_static_class');
try {
    MissingStaticClass::value();
} catch (Error $error) {
    echo $error->getMessage();
}
"#,
    );

    assert_eq!(
        output,
        "load:MissingStaticClass|Class \"MissingStaticClass\" not found"
    );
}

#[test]
fn spl_autoload_honors_explicit_and_request_local_extensions() {
    let dir = TempPhpDir::new();
    std::fs::write(
        dir.0.join("explicitclass.custom"),
        "<?php class ExplicitClass {}",
    )
    .unwrap();
    std::fs::write(
        dir.0.join("configuredclass.inc.php"),
        "<?php class ConfiguredClass {}",
    )
    .unwrap();
    let source_file = dir.0.join("extensions.php").to_string_lossy().into_owned();
    let source_dir = dir.0.to_string_lossy().into_owned();
    let output = run_php_with_source_context(
        r#"<?php
var_dump(spl_autoload('ExplicitClass', '.custom'));
var_dump(class_exists('ExplicitClass', false));
var_dump(spl_autoload_extensions('.inc.php'));
spl_autoload_register(null);
var_dump(class_exists('ConfiguredClass'));
var_dump(spl_autoload_extensions(null));
"#,
        &source_file,
        &source_dir,
    );

    assert_eq!(
        output,
        concat!(
            "NULL\n",
            "bool(true)\n",
            "string(8) \".inc.php\"\n",
            "bool(true)\n",
            "string(8) \".inc.php\"\n"
        )
    );
}

#[test]
fn default_loader_keeps_include_locals_private_and_propagates_exceptions() {
    let dir = TempPhpDir::new();
    std::fs::write(
        dir.0.join("localclass.php"),
        "<?php $autoloadLocal = 'private'; class LocalClass {}",
    )
    .unwrap();
    std::fs::write(
        dir.0.join("throwclass.php"),
        "<?php throw new Exception('autoload boom'); class ThrowClass {}",
    )
    .unwrap();
    let source_file = dir.0.join("scope.php").to_string_lossy().into_owned();
    let source_dir = dir.0.to_string_lossy().into_owned();

    let output = run_php_with_source_context(
        r#"<?php
spl_autoload_register();
var_dump(class_exists('LocalClass'));
var_dump(isset($autoloadLocal));
try {
    class_exists('ThrowClass');
} catch (Exception $error) {
    echo $error->getMessage();
}
"#,
        &source_file,
        &source_dir,
    );

    assert_eq!(
        output,
        concat!("bool(true)\n", "bool(false)\n", "autoload boom")
    );
}

#[test]
fn false_throw_argument_is_ignored_with_php_notice() {
    assert_eq!(
        run_php(
            "<?php function loader($name) {} var_dump(spl_autoload_register('loader', false));"
        ),
        concat!(
            "Notice: spl_autoload_register(): Argument #2 ($do_throw) has been ignored, spl_autoload_register() will always throw\n",
            "bool(true)\n"
        )
    );
}

#[test]
fn spl_autoload_call_runs_the_stack_and_stops_after_the_symbol_loads() {
    let dir = TempPhpDir::new();
    std::fs::write(dir.0.join("loaded.php"), "<?php class LoadedByCall {}").unwrap();
    let source_file = dir
        .0
        .join("autoload-call.php")
        .to_string_lossy()
        .into_owned();
    let source_dir = dir.0.to_string_lossy().into_owned();
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
function firstLoader($name) { echo "first:$name|"; if ($name === 'LoadedByCall') { require __DIR__ . '/loaded.php'; } }
function secondLoader($name) { echo "second:$name|"; }
spl_autoload_register('firstLoader');
spl_autoload_register('secondLoader');
var_dump(spl_autoload_call('LoadedByCall'));
var_dump(class_exists('LoadedByCall', false));
var_dump(spl_autoload_call('StillMissing'));
"#,
            &source_file,
            &source_dir,
        ),
        concat!(
            "first:LoadedByCall|NULL\n",
            "bool(true)\n",
            "first:StillMissing|second:StillMissing|NULL\n"
        )
    );
}

#[test]
fn unregistering_spl_autoload_call_deprecates_and_clears_the_stack() {
    assert_eq!(
        run_php(
            r#"<?php
function loader($name) {}
spl_autoload_register('loader');
spl_autoload_register();
var_dump(spl_autoload_unregister('spl_autoload_call'));
var_dump(spl_autoload_functions());
"#,
        ),
        concat!(
            "Deprecated: spl_autoload_unregister(): Using spl_autoload_call() as a callback for spl_autoload_unregister() is deprecated, to remove all registered autoloaders, call spl_autoload_unregister() for all values returned from spl_autoload_functions()\n",
            "bool(true)\n",
            "array(0) {\n}\n"
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
fn static_method_array_autoloader_is_registered_and_invoked() {
    assert_eq!(
        run_php(
            "<?php class StaticArrayLoader { public static function loadClass($name) { echo $name; } } var_dump(spl_autoload_register(array(StaticArrayLoader::class, 'loadClass'))); class_exists('StaticArrayMissing');"
        ),
        "bool(true)\nStaticArrayMissing"
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
fn class_alias_reuses_class_identity_methods_and_type_relationships() {
    let output = run_php(
        r#"<?php
class OriginalClass {
    public static function value() { return 'method'; }
}
var_dump(class_alias('OriginalClass', 'AliasClass'));
$object = null;
for ($index = 0; $index < 3; $index++) { $object = new AliasClass(); }
echo get_class($object) . '|';
var_dump($object instanceof OriginalClass);
var_dump($object instanceof AliasClass);
echo AliasClass::value() . '|';
var_dump(method_exists('aliasclass', 'value'));
var_dump(class_exists('ALIASCLASS', false));
var_dump(class_alias('OriginalClass', 'aliasclass'));
"#,
    );

    assert_eq!(
        output,
        concat!(
            "bool(true)\n",
            "OriginalClass|bool(true)\n",
            "bool(true)\n",
            "method|bool(true)\n",
            "bool(true)\n",
            "Warning: class_alias(): Cannot declare class aliasclass, because the name is already in use\n",
            "bool(false)\n"
        )
    );
}

#[test]
fn an_included_alias_can_participate_in_a_later_override_contract() {
    let dir = TempPhpDir::new();
    let aliases = dir.write(
        "payload_alias.php",
        "<?php class CanonicalPayload {} class_alias('CanonicalPayload', 'ProjectedPayload');",
    );
    let source = format!(
        r#"<?php
require '{aliases}';
class CanonicalSink {{
    public function store(CanonicalPayload $payload): void {{}}
}}
class ProjectedSink extends CanonicalSink {{
    public function store(ProjectedPayload $payload): void {{ echo get_class($payload); }}
}}
(new ProjectedSink())->store(new CanonicalPayload());
"#
    );

    assert_eq!(run_php(&source), "CanonicalPayload");
}

#[test]
fn class_alias_exposes_trait_static_methods_with_late_static_returns() {
    assert_eq!(
        run_php(
            r#"<?php
trait AliasFactory {
    public static function make(): static { return new static(); }
}
class AliasedProduct {
    use AliasFactory;
}
class_alias('AliasedProduct', 'ProductAlias');
echo ProductAlias::make() instanceof AliasedProduct ? 'ok' : 'fail';
"#,
        ),
        "ok"
    );
}

#[test]
fn class_alias_autoloads_original_and_supports_alias_chains() {
    let dir = TempPhpDir::new();
    let class_file = dir.write(
        "AutoloadedOriginal.php",
        "<?php class AutoloadedOriginal { public function name() { return 'loaded'; } }",
    );
    let child_file = dir.write(
        "AliasChild.php",
        "<?php class AliasChild extends ChainedAlias {}",
    );
    let source = format!(
        r#"<?php
function alias_loader($name) {{
    echo "load:$name|";
    if ($name === 'AutoloadedOriginal') {{ require '{class_file}'; }}
}}
spl_autoload_register('alias_loader');
var_dump(class_alias('AutoloadedOriginal', 'FirstAlias'));
var_dump(class_alias('firstalias', 'ChainedAlias', false));
require '{child_file}';
$child = new AliasChild();
echo $child->name() . '|';
var_dump($child instanceof AutoloadedOriginal);
var_dump($child instanceof FirstAlias);
var_dump($child instanceof ChainedAlias);
var_dump(class_alias('MissingOriginal', 'NeverCreated', false));
"#
    );

    assert_eq!(
        run_php(&source),
        concat!(
            "load:AutoloadedOriginal|bool(true)\n",
            "bool(true)\n",
            "loaded|bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "Warning: class_alias(): Class \"MissingOriginal\" not found\n",
            "bool(false)\n"
        )
    );
}

#[test]
fn class_alias_preserves_interface_trait_and_enum_kinds() {
    let dir = TempPhpDir::new();
    let implementation_file = dir.write(
        "AliasImplementation.php",
        "<?php class AliasImplementation implements ContractAlias { use TraitAlias; }",
    );
    let source = format!(
        r#"<?php
interface OriginalContract {{ public function traitMethod(); }}
trait OriginalTrait {{ public function traitMethod() {{ return 'trait'; }} }}
enum OriginalEnum {{ case One; }}
class_alias('OriginalContract', 'ContractAlias');
class_alias('OriginalTrait', 'TraitAlias');
class_alias('OriginalEnum', 'EnumAlias');
var_dump(interface_exists('ContractAlias', false));
var_dump(trait_exists('TraitAlias', false));
var_dump(enum_exists('EnumAlias', false));
var_dump(class_exists('EnumAlias', false));
require '{implementation_file}';
$implementation = new AliasImplementation();
echo $implementation->traitMethod() . '|';
var_dump($implementation instanceof ContractAlias);
"#
    );

    assert_eq!(
        run_php(&source),
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "trait|bool(true)\n"
        )
    );
}

#[test]
fn function_exists_is_case_insensitive_and_accepts_a_leading_separator() {
    let output = run_php(
        r#"<?php
function ProjectFunction() {}
var_dump(function_exists('ProjectFunction'));
var_dump(function_exists('projectfunction'));
var_dump(function_exists('\\PROJECTFUNCTION'));
var_dump(function_exists('\\strlen'));
var_dump(function_exists('MissingFunction'));
"#,
    );
    assert_eq!(
        output,
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(false)\n"
        )
    );
}

#[test]
fn variadic_var_dump_preserves_argument_order() {
    assert_eq!(
        run_php("<?php var_dump(1, 'two', false, null);"),
        concat!("int(1)\n", "string(3) \"two\"\n", "bool(false)\n", "NULL\n")
    );
}

#[test]
fn class_relation_helpers_honor_alias_identity_and_string_autoload_policy() {
    let dir = TempPhpDir::new();
    let hierarchy_file = dir.write(
        "RelationChild.php",
        "<?php class RelationParent {} class RelationChild extends RelationParent {}",
    );
    let source = format!(
        r#"<?php
function relation_loader($name) {{
    echo "load:$name|";
    if ($name === 'RelationChild') {{ require '{hierarchy_file}'; }}
}}
spl_autoload_register('relation_loader');
var_dump(is_a('RelationChild', 'RelationParent'));
var_dump(is_a('RelationChild', 'RelationParent', true));
var_dump(is_subclass_of('RelationChild', 'RelationParent'));
class_alias('RelationParent', 'RelationParentAlias');
var_dump(is_a(new RelationChild(), 'RelationParentAlias'));
var_dump(is_a('RelationParent', 'RelationParentAlias', true));
var_dump(is_subclass_of('RelationParent', 'RelationParentAlias'));
var_dump(is_subclass_of('RelationChild', 'RelationParentAlias', false));
"#
    );
    assert_eq!(
        run_php(&source),
        concat!(
            "bool(false)\n",
            "load:RelationChild|bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(false)\n",
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

#[test]
fn autoloaded_child_loads_parent_before_inheriting_constructor() {
    let dir = TempPhpDir::new();
    let parent = dir.write(
        "ParentDependency.php",
        "<?php class ParentDependency { public function __construct(protected string $value) {} public function value(): string { return $this->value; } }",
    );
    let child = dir.write(
        "ChildDependency.php",
        "<?php class ChildDependency extends ParentDependency {}",
    );
    let source = format!(
        r#"<?php
$parentFile = '{parent}';
$childFile = '{child}';
$loader = function ($name) use ($parentFile, $childFile) {{
    require $name === 'ParentDependency' ? $parentFile : $childFile;
}};
spl_autoload_register($loader);
echo (new ChildDependency('inherited'))->value();
"#
    );
    assert_eq!(run_php(&source), "inherited");
}

#[test]
fn autoloaded_child_created_inside_generator_dispatches_its_override() {
    let dir = TempPhpDir::new();
    let parent = dir.write(
        "GeneratorParent.php",
        "<?php class GeneratorParent { public function build(): string { return 'parent'; } }",
    );
    let child = dir.write(
        "GeneratorChild.php",
        "<?php class GeneratorChild extends GeneratorParent { public function build(): string { return 'child'; } }",
    );
    let source = format!(
        r#"<?php
$parentFile = '{parent}';
$childFile = '{child}';
spl_autoload_register(function ($name) use ($parentFile, $childFile) {{
    require $name === 'GeneratorParent' ? $parentFile : $childFile;
}});
function generator_child(): iterable {{ yield new GeneratorChild(); }}
$child = generator_child()->current();
echo $child->build();
"#
    );

    assert_eq!(run_php(&source), "child");
}

#[test]
fn wrong_kind_exists_probe_does_not_autoload_an_already_loaded_symbol_again() {
    let dir = TempPhpDir::new();
    let interface = dir.write(
        "LoadedProbeInterface.php",
        "<?php interface LoadedProbeInterface {}",
    );
    let source = format!(
        r#"<?php
spl_autoload_register(function ($name) {{
    echo 'load|';
    require '{interface}';
}});
var_dump(class_exists('LoadedProbeInterface'));
var_dump(class_exists('LoadedProbeInterface'));
var_dump(interface_exists('LoadedProbeInterface', false));
"#
    );

    assert_eq!(
        run_php(&source),
        "load|bool(false)\nbool(false)\nbool(true)\n"
    );
}
