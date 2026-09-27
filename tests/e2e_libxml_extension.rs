mod common;

use common::run_php;

#[test]
fn libxml_extension_identity_functions_and_class_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
echo (int) extension_loaded('libxml'), (int) extension_loaded('LIBXML'), "\n";
echo implode(',', get_extension_funcs('libxml')), "\n";
foreach ([
    'libxml_set_streams_context', 'libxml_use_internal_errors',
    'libxml_get_last_error', 'libxml_get_errors', 'libxml_clear_errors',
    'libxml_disable_entity_loader', 'libxml_set_external_entity_loader',
    'libxml_get_external_entity_loader'
] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getExtensionName(), ':',
        $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), ':', $function->getReturnType();
    foreach ($function->getParameters() as $parameter) {
        echo ':', $parameter->getName(), '=', $parameter->hasType() ? $parameter->getType() : '-';
        if ($parameter->isDefaultValueAvailable()) {
            echo '=', var_export($parameter->getDefaultValue(), true);
        }
    }
    echo "\n";
}
$class = new ReflectionClass(LibXMLError::class);
echo $class->getExtensionName(), ':', (int) $class->isInstantiable(), ':',
    (int) $class->isFinal(), "\n";
foreach ($class->getProperties() as $property) {
    echo $property->getName(), ':', $property->getType(), ':',
        (int) $property->hasDefaultValue(), "\n";
}
"#,
        ),
        concat!(
            "11\n",
            "libxml_set_streams_context,libxml_use_internal_errors,",
            "libxml_get_last_error,libxml_get_errors,libxml_clear_errors,",
            "libxml_disable_entity_loader,libxml_set_external_entity_loader,",
            "libxml_get_external_entity_loader\n",
            "libxml_set_streams_context:libxml:1/1:void:context=-\n",
            "libxml_use_internal_errors:libxml:0/1:bool:use_errors=?bool=NULL\n",
            "libxml_get_last_error:libxml:0/0:LibXMLError|false\n",
            "libxml_get_errors:libxml:0/0:array\n",
            "libxml_clear_errors:libxml:0/0:void\n",
            "libxml_disable_entity_loader:libxml:0/1:bool:disable=bool=true\n",
            "libxml_set_external_entity_loader:libxml:1/1:true:resolver_function=?callable\n",
            "libxml_get_external_entity_loader:libxml:0/0:?callable\n",
            "libxml:1:0\n",
            "level:int:0\n",
            "code:int:0\n",
            "column:int:0\n",
            "message:string:0\n",
            "file:string:0\n",
            "line:int:0\n",
        )
    );
}

#[test]
fn libxml_constants_match_the_php_85_contract() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'LIBXML_VERSION', 'LIBXML_DOTTED_VERSION', 'LIBXML_LOADED_VERSION',
    'LIBXML_RECOVER', 'LIBXML_NOENT', 'LIBXML_DTDLOAD', 'LIBXML_DTDATTR',
    'LIBXML_DTDVALID', 'LIBXML_NOERROR', 'LIBXML_NOWARNING',
    'LIBXML_NOBLANKS', 'LIBXML_XINCLUDE', 'LIBXML_NSCLEAN',
    'LIBXML_NOCDATA', 'LIBXML_NONET', 'LIBXML_PEDANTIC', 'LIBXML_COMPACT',
    'LIBXML_NOXMLDECL', 'LIBXML_PARSEHUGE', 'LIBXML_BIGLINES',
    'LIBXML_NOEMPTYTAG', 'LIBXML_SCHEMA_CREATE', 'LIBXML_HTML_NOIMPLIED',
    'LIBXML_HTML_NODEFDTD', 'LIBXML_ERR_NONE', 'LIBXML_ERR_WARNING',
    'LIBXML_ERR_ERROR', 'LIBXML_ERR_FATAL'
] as $name) {
    echo $name, '=', constant($name), "\n";
}
"#,
        ),
        concat!(
            "LIBXML_VERSION=20914\n",
            "LIBXML_DOTTED_VERSION=2.9.14\n",
            "LIBXML_LOADED_VERSION=20914\n",
            "LIBXML_RECOVER=1\n",
            "LIBXML_NOENT=2\n",
            "LIBXML_DTDLOAD=4\n",
            "LIBXML_DTDATTR=8\n",
            "LIBXML_DTDVALID=16\n",
            "LIBXML_NOERROR=32\n",
            "LIBXML_NOWARNING=64\n",
            "LIBXML_NOBLANKS=256\n",
            "LIBXML_XINCLUDE=1024\n",
            "LIBXML_NSCLEAN=8192\n",
            "LIBXML_NOCDATA=16384\n",
            "LIBXML_NONET=2048\n",
            "LIBXML_PEDANTIC=128\n",
            "LIBXML_COMPACT=65536\n",
            "LIBXML_NOXMLDECL=2\n",
            "LIBXML_PARSEHUGE=524288\n",
            "LIBXML_BIGLINES=4194304\n",
            "LIBXML_NOEMPTYTAG=4\n",
            "LIBXML_SCHEMA_CREATE=1\n",
            "LIBXML_HTML_NOIMPLIED=8192\n",
            "LIBXML_HTML_NODEFDTD=4\n",
            "LIBXML_ERR_NONE=0\n",
            "LIBXML_ERR_WARNING=1\n",
            "LIBXML_ERR_ERROR=2\n",
            "LIBXML_ERR_FATAL=3\n",
        )
    );
}

#[test]
fn libxml_error_policy_and_empty_diagnostics_are_request_local() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(libxml_use_internal_errors(false));
var_dump(libxml_use_internal_errors(true));
var_dump(libxml_use_internal_errors());
var_dump(libxml_use_internal_errors(null));
var_dump(libxml_get_errors());
var_dump(libxml_get_last_error());
var_dump(libxml_clear_errors());
var_dump(libxml_use_internal_errors(false));
var_dump(libxml_use_internal_errors());
"#,
        ),
        concat!(
            "bool(false)\n",
            "bool(false)\n",
            "bool(true)\n",
            "bool(true)\n",
            "array(0) {\n}\n",
            "bool(false)\n",
            "NULL\n",
            "bool(true)\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn libxml_error_objects_expose_uninitialized_typed_slots() {
    assert_eq!(
        run_php(
            r#"<?php
$error = new LibXMLError();
echo count((array) $error), "\n";
foreach (['level', 'code', 'column', 'message', 'file', 'line'] as $name) {
    try { var_dump($error->$name); }
    catch (Error $exception) { echo $exception->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "0\n",
            "Typed property LibXMLError::$level must not be accessed before initialization\n",
            "Typed property LibXMLError::$code must not be accessed before initialization\n",
            "Typed property LibXMLError::$column must not be accessed before initialization\n",
            "Typed property LibXMLError::$message must not be accessed before initialization\n",
            "Typed property LibXMLError::$file must not be accessed before initialization\n",
            "Typed property LibXMLError::$line must not be accessed before initialization\n",
        )
    );
}

#[test]
fn libxml_external_entity_loader_retains_replaces_and_validates_callbacks() {
    assert_eq!(
        run_php(
            r#"<?php
class Loader {
    public function load($public, $system, $context) { return null; }
    public function __toString() { return 'loader'; }
    public function __destruct() { echo "drop\n"; }
}
var_dump(libxml_get_external_entity_loader());
$loader = new Loader();
var_dump(libxml_set_external_entity_loader([$loader, 'load']));
echo libxml_get_external_entity_loader()[0], "\n";
unset($loader);
gc_collect_cycles();
echo "held\n";
var_dump(libxml_set_external_entity_loader(null));
echo "cleared\n";
var_dump(libxml_get_external_entity_loader());
try { libxml_set_external_entity_loader('missing_loader'); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "NULL\n",
            "bool(true)\n",
            "loader\n",
            "held\n",
            "drop\n",
            "bool(true)\n",
            "cleared\n",
            "NULL\n",
            "libxml_set_external_entity_loader(): Argument #1 ($resolver_function) must be a valid callback or null, ",
            "function \"missing_loader\" not found or invalid function name\n",
        )
    );
}

#[test]
fn libxml_retained_loader_runs_its_destructor_at_request_shutdown() {
    assert_eq!(
        run_php(
            r#"<?php
class ShutdownLoader {
    public function load($public, $system, $context) { return null; }
    public function __destruct() { echo "drop\n"; }
}
$loader = new ShutdownLoader();
libxml_set_external_entity_loader([$loader, 'load']);
unset($loader);
echo "end\n";
"#,
        ),
        "end\ndrop\n"
    );
}

#[test]
#[cfg(feature = "stream-context")]
fn libxml_stream_context_accepts_only_stream_context_resources() {
    assert_eq!(
        run_php(
            r#"<?php
try { libxml_set_streams_context('not a resource'); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
$stream = fopen('php://memory', 'r');
try { libxml_set_streams_context($stream); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
$first = stream_context_create(['http' => ['method' => 'GET']]);
$second = stream_context_create(['http' => ['method' => 'POST']]);
var_dump(libxml_set_streams_context($first));
var_dump(libxml_set_streams_context($second));
echo "done\n";
"#,
        ),
        concat!(
            "libxml_set_streams_context(): Argument #1 ($context) must be of type resource, string given\n",
            "libxml_set_streams_context(): supplied resource is not a valid Stream-Context resource\n",
            "NULL\n",
            "NULL\n",
            "done\n",
        )
    );
}

#[test]
fn php_input_is_a_seekable_read_only_input_stream_not_a_context() {
    assert_eq!(
        run_php(
            r#"<?php
$stream = fopen('php://input', 'w');
$metadata = stream_get_meta_data($stream);
echo $metadata['wrapper_type'], ':', $metadata['stream_type'], ':',
    $metadata['mode'], ':', (int) $metadata['seekable'], "\n";
var_dump(fwrite($stream, 'x'));
var_dump(fseek($stream, 0));
var_dump(fread($stream, 1));
var_dump(feof($stream));
try { libxml_set_streams_context($stream); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "PHP:Input:rb:1\n",
            "bool(false)\n",
            "int(0)\n",
            "string(0) \"\"\n",
            "bool(true)\n",
            "libxml_set_streams_context(): supplied resource is not a valid Stream-Context resource\n",
        )
    );
}

#[test]
fn svg_image_headers_follow_php_85_root_namespace_and_unit_rules() {
    assert_eq!(
        run_php(
            r#"<?php
$inputs = [
    '<SVG width="1" height="1"/>',
    '<svg width="4cm" height="8cm"/>',
    '<!-- fake <svg width="9" height="9"/> -->' .
        '<x:svg width="2foo" height="3PX" xmlns:x="http://www.w3.org/2000/svg"/>',
    '<x:svg width="2" height="3"/>',
    '<svg width="1.5" height="3"/>',
];
foreach ($inputs as $input) {
    $info = getimagesizefromstring($input);
    if ($info === false) { echo "false\n"; continue; }
    echo $info[0], '|', $info[1], '|', $info[2], '|',
        ($info[3] ?? '-'), '|', $info['mime'], '|',
        $info['width_unit'], '|', $info['height_unit'], "\n";
}
"#,
        ),
        concat!(
            "1|1|21|width=\"1\" height=\"1\"|image/svg+xml|px|px\n",
            "4|8|21|-|image/svg+xml|cm|cm\n",
            "2|3|21|-|image/svg+xml|foo|PX\n",
            "false\n",
            "false\n",
        )
    );
}

#[test]
fn image_type_constants_mappings_and_signatures_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
echo IMAGETYPE_UNKNOWN, ',', IMAGETYPE_JPEG2000, ',', IMAGETYPE_SVG, ',',
    IMAGETYPE_COUNT, "\n";
foreach ([-1, 0, 1, 9, 13, 15, 21, 22] as $type) {
    echo $type, ':', var_export(image_type_to_extension($type), true), ':',
        var_export(image_type_to_extension($type, false), true), ':',
        image_type_to_mime_type($type), "\n";
}
$function = new ReflectionFunction('image_type_to_extension');
echo $function->getExtensionName(), ':', $function->getNumberOfRequiredParameters(), '/',
    $function->getNumberOfParameters(), ':', $function->getReturnType(), "\n";
foreach ($function->getParameters() as $parameter) {
    echo $parameter->getName(), ':', $parameter->getType(), ':';
    echo $parameter->isDefaultValueAvailable()
        ? var_export($parameter->getDefaultValue(), true) : '-';
    echo "\n";
}
"#,
        ),
        concat!(
            "0,9,21,22\n",
            "-1:false:false:application/octet-stream\n",
            "0:false:false:application/octet-stream\n",
            "1:'.gif':'gif':image/gif\n",
            "9:'.jpc':'jpc':application/octet-stream\n",
            "13:'.swf':'swf':application/x-shockwave-flash\n",
            "15:'.bmp':'bmp':image/vnd.wap.wbmp\n",
            "21:'.svg':'svg':image/svg+xml\n",
            "22:false:false:application/octet-stream\n",
            "standard:1/2:string|false\n",
            "image_type:int:-\n",
            "include_dot:bool:true\n",
        )
    );
}
