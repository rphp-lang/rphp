mod common;

use std::sync::atomic::{AtomicU64, Ordering};

use common::{run_php, run_php_bytes_until_exit};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempPhpDir(std::path::PathBuf);

impl TempPhpDir {
    fn new() -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rphp-high-reach-builtins-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, source: &str) -> String {
        self.write_bytes(name, source.as_bytes())
    }

    fn write_bytes(&self, name: &str, contents: &[u8]) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, contents).unwrap();
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
fn high_reach_builtins_expose_php_85_signatures_and_named_arguments() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'class_exists',
    'interface_exists',
    'trait_exists',
    'enum_exists',
    'preg_quote',
    'base64_encode',
] as $name) {
    $reflection = new ReflectionFunction($name);
    echo $name, '|', $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), '|', $reflection->getReturnType(), "\n";
    foreach ($reflection->getParameters() as $parameter) {
        echo $parameter->getName(), ':', $parameter->getType(), ':';
        echo $parameter->isDefaultValueAvailable()
            ? var_export($parameter->getDefaultValue(), true)
            : '-';
        echo "\n";
    }
}

class PresentClass {}
echo 'named|', class_exists(class: 'PresentClass', autoload: false) ? 'true' : 'false', '|',
    preg_quote(str: 'a.b/c', delimiter: '/'), '|', base64_encode(string: 'foo'), "\n";

foreach ([
    'class' => static fn() => class_exists(class_name: 'PresentClass'),
    'preg' => static fn() => preg_quote(string: 'a.b'),
    'base64' => static fn() => base64_encode(data: 'foo'),
] as $label => $call) {
    try { $call(); }
    catch (Throwable $error) {
        echo $label, '|', get_class($error), '|', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "class_exists|1/2|bool\n",
            "class:string:-\n",
            "autoload:bool:true\n",
            "interface_exists|1/2|bool\n",
            "interface:string:-\n",
            "autoload:bool:true\n",
            "trait_exists|1/2|bool\n",
            "trait:string:-\n",
            "autoload:bool:true\n",
            "enum_exists|1/2|bool\n",
            "enum:string:-\n",
            "autoload:bool:true\n",
            "preg_quote|1/2|string\n",
            "str:string:-\n",
            "delimiter:?string:NULL\n",
            "base64_encode|1/1|string\n",
            "string:string:-\n",
            "named|true|a\\.b\\/c|Zm9v\n",
            "class|Error|Unknown named parameter $class_name\n",
            "preg|Error|Unknown named parameter $string\n",
            "base64|Error|Unknown named parameter $data\n",
        )
    );
}

#[test]
fn high_reach_builtins_follow_weak_type_and_binary_string_rules() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function (int $level, string $message): bool {
    echo 'diagnostic|', $level, '|', $message, "\n";
    return true;
});

final class ClassNameText {
    public function __toString(): string { return 'stdClass'; }
}
final class QuotedText {
    public function __toString(): string { return 'a.b'; }
}
final class Base64Text {
    public function __toString(): string { return 'é€'; }
}

var_dump(class_exists(new ClassNameText(), false));
echo bin2hex(preg_quote(new QuotedText())), "\n";
echo base64_encode(new Base64Text()), "\n";
var_dump(class_exists(null, false));
echo bin2hex(preg_quote(null)), "\n";
echo bin2hex(base64_encode(null)), "\n";

foreach ([
    static fn() => class_exists([], false),
    static fn() => preg_quote([]),
    static fn() => base64_encode([]),
    static fn() => class_exists('stdClass', []),
    static fn() => preg_quote('x', []),
] as $call) {
    try { $call(); }
    catch (Throwable $error) {
        echo get_class($error), '|', $error->getMessage(), "\n";
    }
}

echo bin2hex(preg_quote("\0\x7f\x80\xff")), "\n";
echo bin2hex(preg_quote('äé', 'é')), "\n";
echo strlen(preg_quote("\xff")), ':', bin2hex(preg_quote("\xff")), "\n";
echo base64_encode('é€'), "\n";
echo base64_encode("\0\x7f\x80\xff"), "\n";
restore_error_handler();
"#,
        ),
        concat!(
            "bool(true)\n",
            "615c2e62\n",
            "w6nigqw=\n",
            "diagnostic|8192|class_exists(): Passing null to parameter #1 ($class) of type string is deprecated\n",
            "bool(false)\n",
            "diagnostic|8192|preg_quote(): Passing null to parameter #1 ($str) of type string is deprecated\n",
            "\n",
            "diagnostic|8192|base64_encode(): Passing null to parameter #1 ($string) of type string is deprecated\n",
            "\n",
            "TypeError|class_exists(): Argument #1 ($class) must be of type string, array given\n",
            "TypeError|preg_quote(): Argument #1 ($str) must be of type string, array given\n",
            "TypeError|base64_encode(): Argument #1 ($string) must be of type string, array given\n",
            "TypeError|class_exists(): Argument #2 ($autoload) must be of type bool, array given\n",
            "TypeError|preg_quote(): Argument #2 ($delimiter) must be of type ?string, array given\n",
            "5c3030307f80ff\n",
            "5cc3a45cc3a9\n",
            "1:ff\n",
            "w6nigqw=\n",
            "AH+A/w==\n",
        )
    );
}

#[test]
fn high_reach_builtins_preserve_bytes_composed_with_file_get_contents() {
    assert_eq!(
        run_php(
            r#"<?php
$bytes = file_get_contents('data://text/plain;base64,AH+A/w==');
echo bin2hex($bytes), "\n";
echo base64_encode($bytes), "\n";
echo bin2hex(preg_quote($bytes)), "\n";
"#,
        ),
        concat!("007f80ff\n", "AH+A/w==\n", "5c3030307f80ff\n")
    );
}

#[test]
fn high_reach_builtins_preserve_binary_file_lines() {
    let directory = TempPhpDir::new();
    let fixture = directory.write_bytes("bytes.txt", &[0xff, b'\n', 0x80]);
    assert_eq!(
        run_php(&format!(
            r#"<?php
$lines = file('{fixture}');
foreach ($lines as $line) {{
    echo bin2hex($line), '|', base64_encode($line), '|', bin2hex(preg_quote($line)), "\n";
}}
"#
        )),
        concat!("ff0a|/wo=|ff0a\n", "80|gA==|80\n")
    );
}

#[test]
fn high_reach_builtins_preserve_bytes_from_decoders_and_output_handlers() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['urldecode', 'rawurldecode'] as $decode) {
    $bytes = $decode('%00%7f%80%ff');
    echo $decode, '|', bin2hex($bytes), '|', base64_encode($bytes), '|',
        bin2hex(preg_quote($bytes)), "\n";
}

$seen = '';
ob_start();
ob_start(static function (string $buffer) use (&$seen): string {
    $seen = strlen($buffer) . ':' . bin2hex($buffer);
    return "\xff";
});
echo "\0\x80";
ob_end_flush();
$transformed = ob_get_clean();
echo 'output|', bin2hex($transformed), '|', $seen, "\n";
"#,
        ),
        concat!(
            "urldecode|007f80ff|AH+A/w==|5c3030307f80ff\n",
            "rawurldecode|007f80ff|AH+A/w==|5c3030307f80ff\n",
            "output|ff|2:0080\n",
        )
    );
}

#[test]
fn query_keys_retain_php_bytes_across_lookup_array_apis_and_yield_from() {
    assert_eq!(
        run_php(
            r#"<?php
function emit_key(string $label, string $key): void {
    echo $label, '=', strlen($key), ':', bin2hex($key), ':',
        base64_encode($key), ':', bin2hex(preg_quote($key)), "\n";
}

parse_str('%C3%A9=x', $query);
emit_key('first', array_key_first($query));
emit_key('last', array_key_last($query));
emit_key('cursor', key($query));
emit_key('keys', array_keys($query)[0]);
emit_key('filtered-keys', array_keys($query, 'x', true)[0]);
emit_key('search', array_search('x', $query, true));
emit_key('rand', array_rand($query));
emit_key('flip-value', array_flip($query)['x']);

$seen = [];
$filtered = array_filter($query, static function ($value, $key) use (&$seen): bool {
    $seen[] = $key;
    return true;
}, ARRAY_FILTER_USE_BOTH);
emit_key('filter-callback', $seen[0]);
emit_key('filter-result', array_key_first($filtered));

$keyOnly = [];
array_filter($query, static function ($key) use (&$keyOnly): bool {
    $keyOnly[] = $key;
    return true;
}, ARRAY_FILTER_USE_KEY);
emit_key('filter-key', $keyOnly[0]);

$walked = [];
$copy = $query;
array_walk($copy, static function (&$value, $key) use (&$walked): void {
    $walked[] = $key;
});
emit_key('walk', $walked[0]);

parse_str('root[%C3%A9]=x', $nested);
$recursive = [];
array_walk_recursive($nested, static function (&$value, $key) use (&$recursive): void {
    $recursive[] = $key;
});
emit_key('walk-recursive', $recursive[0]);

foreach ($query as $key => $value) {
    emit_key('foreach', $key);
}
$generator = static function (array $input): Generator {
    yield from $input;
};
foreach ($generator($query) as $key => $value) {
    emit_key('yield-from', $key);
}

var_dump(isset($query['é']), $query['é']);
$query['ö'] = 'y';
emit_key('inserted', array_key_last($query));
unset($query['é']);
var_dump(isset($query['é']), isset($query['ö']));

parse_str('%FF=a&%80=b', $ordered);
ksort($ordered, SORT_STRING);
foreach (array_keys($ordered) as $index => $key) {
    emit_key('ksort-' . $index, $key);
}
krsort($ordered, SORT_STRING);
foreach (array_keys($ordered) as $index => $key) {
    emit_key('krsort-' . $index, $key);
}
"#,
        ),
        concat!(
            "first=2:c3a9:w6k=:c3a9\n",
            "last=2:c3a9:w6k=:c3a9\n",
            "cursor=2:c3a9:w6k=:c3a9\n",
            "keys=2:c3a9:w6k=:c3a9\n",
            "filtered-keys=2:c3a9:w6k=:c3a9\n",
            "search=2:c3a9:w6k=:c3a9\n",
            "rand=2:c3a9:w6k=:c3a9\n",
            "flip-value=2:c3a9:w6k=:c3a9\n",
            "filter-callback=2:c3a9:w6k=:c3a9\n",
            "filter-result=2:c3a9:w6k=:c3a9\n",
            "filter-key=2:c3a9:w6k=:c3a9\n",
            "walk=2:c3a9:w6k=:c3a9\n",
            "walk-recursive=2:c3a9:w6k=:c3a9\n",
            "foreach=2:c3a9:w6k=:c3a9\n",
            "yield-from=2:c3a9:w6k=:c3a9\n",
            "bool(true)\n",
            "string(1) \"x\"\n",
            "inserted=2:c3b6:w7Y=:c3b6\n",
            "bool(false)\n",
            "bool(true)\n",
            "ksort-0=1:80:gA==:80\n",
            "ksort-1=1:ff:/w==:ff\n",
            "krsort-0=1:ff:/w==:ff\n",
            "krsort-1=1:80:gA==:80\n",
        )
    );
}

#[test]
fn high_reach_consumers_compose_with_byte_transform_results() {
    assert_eq!(
        run_php(
            r#"<?php
$values = [
    'range' => range("\xff", "\xff")[0],
    'stristr' => stristr("x\xff", "\xff"),
    'str_pad' => str_pad("\xff", 2, 'x'),
    'strtok' => strtok("\xff,", ','),
    'str_shuffle' => str_shuffle("\xff"),
    'unicode_pad' => str_pad('é', 4, 'x'),
    'unicode_stristr' => stristr('xé', 'é'),
];
foreach ($values as $name => $value) {
    echo $name, '|', strlen($value), '|', bin2hex($value), '|',
        base64_encode($value), '|', bin2hex(preg_quote($value)), "\n";
}
"#,
        ),
        concat!(
            "range|1|ff|/w==|ff\n",
            "stristr|1|ff|/w==|ff\n",
            "str_pad|2|ff78|/3g=|ff78\n",
            "strtok|1|ff|/w==|ff\n",
            "str_shuffle|1|ff|/w==|ff\n",
            "unicode_pad|4|c3a97878|w6l4eA==|c3a97878\n",
            "unicode_stristr|2|c3a9|w6k=|c3a9\n",
        )
    );
}

#[test]
fn high_reach_builtins_reject_scalar_coercion_in_strict_calls() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
foreach ([
    static fn() => class_exists(123, false),
    static fn() => class_exists('stdClass', 1),
    static fn() => preg_quote(123),
    static fn() => preg_quote('a1', 1),
    static fn() => base64_encode(123),
] as $call) {
    try { $call(); }
    catch (Throwable $error) {
        echo get_class($error), '|', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "TypeError|class_exists(): Argument #1 ($class) must be of type string, int given\n",
            "TypeError|class_exists(): Argument #2 ($autoload) must be of type bool, int given\n",
            "TypeError|preg_quote(): Argument #1 ($str) must be of type string, int given\n",
            "TypeError|preg_quote(): Argument #2 ($delimiter) must be of type ?string, int given\n",
            "TypeError|base64_encode(): Argument #1 ($string) must be of type string, int given\n",
        )
    );
}

#[test]
fn class_exists_preserves_loaded_exotic_names_and_filters_only_autoload_misses() {
    assert_eq!(
        run_php(
            r#"<?php
class AliasSource {}
var_dump(class_alias(AliasSource::class, 'Alias-Hyphen'));
var_dump(class_exists('Alias-Hyphen', false));
var_dump(class_alias(AliasSource::class, 'Alias Space'));
var_dump(class_exists('Alias Space', false));
$anonymous = new class {};
var_dump(class_exists(get_class($anonymous), false));

spl_autoload_register(static function (string $name): void {
    echo 'autoload|', strlen($name), '|', bin2hex($name), "\n";
});
foreach ([
    '' => '',
    'space' => 'Bad Name',
    'dash' => 'Bad-Name',
    'slash' => 'Bad/Name',
    'del' => "\x7f",
    'digit' => '123',
    'one-leading' => '\\',
    'two-leading' => '\\\\',
    'binary' => "\xff",
] as $label => $name) {
    echo $label, '|';
    var_dump(class_exists($name));
}
"#,
        ),
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "|bool(false)\n",
            "space|bool(false)\n",
            "dash|bool(false)\n",
            "slash|bool(false)\n",
            "del|bool(false)\n",
            "digit|autoload|3|313233\n",
            "bool(false)\n",
            "one-leading|autoload|0|\n",
            "bool(false)\n",
            "two-leading|autoload|1|5c\n",
            "bool(false)\n",
            "binary|autoload|1|ff\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn class_exists_stops_after_an_autoloader_defines_another_class_like_kind() {
    let directory = TempPhpDir::new();
    let definition = directory.write("WrongKind.php", "<?php interface WrongKind {}");
    assert_eq!(
        run_php(&format!(
            r#"<?php
$definition = '{definition}';
spl_autoload_register(static function (string $name) use ($definition): void {{
    echo 'first:', $name, "\n";
    require $definition;
}});
spl_autoload_register(static function (string $name): void {{
    echo 'second:', $name, "\n";
}});
var_dump(class_exists('WrongKind'));
var_dump(interface_exists('WrongKind', false));
"#
        )),
        concat!("first:WrongKind\n", "bool(false)\n", "bool(true)\n",)
    );
}

#[test]
fn query_keys_survive_array_rebuilders_and_key_callbacks() {
    assert_eq!(
        run_php(
            r#"<?php
function emit_rebuilt_key(string $label, string $key): void {
    echo $label, '=', strlen($key), ':', bin2hex($key), ':', base64_encode($key), "\n";
}
function emit_wide_key(string $label, array $array): void {
    foreach (array_keys($array) as $key) {
        if (is_string($key) && strlen($key) > 1) {
            emit_rebuilt_key($label, $key);
            return;
        }
    }
    echo $label, "=missing\n";
}

parse_str('%C3%A9=x&z=2', $query);
$copy = $query;
array_unshift($copy, 'head');
emit_wide_key('unshift', $copy);
emit_wide_key('case', array_change_key_case($query, CASE_UPPER));
emit_wide_key('reverse', array_reverse($query, true));
emit_wide_key('slice', array_slice($query, 0, null, true));
emit_wide_key('unique', array_unique($query));
emit_wide_key('pad', array_pad($query, 4, 'pad'));
emit_wide_key('chunk', array_chunk($query, 1, true)[0]);

parse_str('%C3%A9=x&%C3%B6=y', $spliced);
$removed = array_splice($spliced, 0, 1);
emit_wide_key('splice-target', $spliced);
emit_wide_key('splice-removed', $removed);

emit_wide_key('map', array_map(static fn($value) => strtoupper($value), $query));
$copy = $query;
natsort($copy);
emit_wide_key('natsort', $copy);
$copy = $query;
natcasesort($copy);
emit_wide_key('natcasesort', $copy);
$copy = $query;
array_multisort($copy, SORT_ASC, SORT_STRING);
emit_wide_key('multisort', $copy);
emit_wide_key('iterator', iterator_to_array(new ArrayIterator($query), true));
emit_wide_key('str-replace', str_replace('x', 'X', $query));
emit_wide_key('str-ireplace', str_ireplace('X', 'Y', $query));
emit_wide_key('substr-replace', substr_replace($query, 'Q', 0, 1));
emit_wide_key('preg-replace', preg_replace('/x/', 'X', $query));
emit_rebuilt_key(
    'find-key',
    array_find_key($query, static fn($value, $key) => $value === 'x')
);
var_dump(array_any($query, static fn($value, $key) => $key === 'é'));
var_dump(array_all($query, static fn($value, $key) => is_string($key)));
"#,
        ),
        concat!(
            "unshift=2:c3a9:w6k=\n",
            "case=2:c3a9:w6k=\n",
            "reverse=2:c3a9:w6k=\n",
            "slice=2:c3a9:w6k=\n",
            "unique=2:c3a9:w6k=\n",
            "pad=2:c3a9:w6k=\n",
            "chunk=2:c3a9:w6k=\n",
            "splice-target=2:c3b6:w7Y=\n",
            "splice-removed=2:c3a9:w6k=\n",
            "map=2:c3a9:w6k=\n",
            "natsort=2:c3a9:w6k=\n",
            "natcasesort=2:c3a9:w6k=\n",
            "multisort=2:c3a9:w6k=\n",
            "iterator=2:c3a9:w6k=\n",
            "str-replace=2:c3a9:w6k=\n",
            "str-ireplace=2:c3a9:w6k=\n",
            "substr-replace=2:c3a9:w6k=\n",
            "preg-replace=2:c3a9:w6k=\n",
            "find-key=2:c3a9:w6k=\n",
            "bool(true)\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn query_keys_collide_by_php_bytes_across_composition_and_mutation_paths() {
    assert_eq!(
        run_php(
            r#"<?php
function emit_composed_array(string $label, array $array): void {
    $key = array_key_first($array);
    echo $label, '=', count($array), ':', strlen($key), ':', bin2hex($key), ':',
        json_encode($array[$key]), "\n";
}

$ordinary = ['é' => 'ordinary'];
parse_str('%C3%A9=raw', $external);
emit_composed_array('merge-o-e', array_merge($ordinary, $external));
emit_composed_array('merge-e-o', array_merge($external, $ordinary));
emit_composed_array('replace-o-e', array_replace($ordinary, $external));
emit_composed_array('replace-e-o', array_replace($external, $ordinary));
emit_composed_array('union-o-e', $ordinary + $external);
emit_composed_array('union-e-o', $external + $ordinary);
emit_composed_array('spread-o-e', [...$ordinary, ...$external]);
emit_composed_array('spread-e-o', [...$external, ...$ordinary]);

$merged = array_merge_recursive($ordinary, $external);
$key = array_key_first($merged);
echo 'merge-rec=', count($merged), ':', strlen($key), ':', bin2hex($key), ':',
    implode(',', $merged[$key]), "\n";
emit_composed_array('replace-rec', array_replace_recursive($ordinary, $external));

$ordinaryNested = ['n' => ['é' => 'ordinary']];
parse_str('n[%C3%A9]=raw', $externalNested);
$merged = array_merge_recursive($ordinaryNested, $externalNested);
$key = array_key_first($merged['n']);
echo 'nested-merge=', count($merged['n']), ':', strlen($key), ':', bin2hex($key), ':',
    implode(',', $merged['n'][$key]), "\n";
$replaced = array_replace_recursive($ordinaryNested, $externalNested);
$key = array_key_first($replaced['n']);
echo 'nested-replace=', count($replaced['n']), ':', strlen($key), ':', bin2hex($key), ':',
    $replaced['n'][$key], "\n";

parse_str('%C3%A9=column', $row);
var_dump(array_column([$row], 'é'));
parse_str('value=x&index=%C3%A9', $indexed);
$columns = array_column([$indexed], 'value', 'index');
$key = array_key_first($columns);
echo 'column-index=', count($columns), ':', strlen($key), ':', bin2hex($key), ':',
    $columns[$key], "\n";

parse_str('%C3%A9=x', $query);
function mutate_query_value(&$value): void { $value = 'y'; }
mutate_query_value($query['é']);
$reference =& $query['é'];
$reference = 'z';
emit_composed_array('reference', $query);

parse_str('%C3%A9=x', $query);
$object = new ArrayObject($query);
var_dump($object['é'], isset($object['é']));
$object['é'] = 'y';
echo 'object-set=', count($object), ':', $object['é'], "\n";
unset($object['é']);
echo 'object-unset=', count($object), ':', isset($object['é']) ? '1' : '0', "\n";

$rawKey = array_key_first($external);
$same = ['é' => 'raw'];
var_dump(
    $rawKey === 'é',
    $rawKey == 'é',
    $rawKey <=> 'é',
    $external === $same,
    $external == $same
);
"#,
        ),
        concat!(
            "merge-o-e=1:2:c3a9:\"raw\"\n",
            "merge-e-o=1:2:c3a9:\"ordinary\"\n",
            "replace-o-e=1:2:c3a9:\"raw\"\n",
            "replace-e-o=1:2:c3a9:\"ordinary\"\n",
            "union-o-e=1:2:c3a9:\"ordinary\"\n",
            "union-e-o=1:2:c3a9:\"raw\"\n",
            "spread-o-e=1:2:c3a9:\"raw\"\n",
            "spread-e-o=1:2:c3a9:\"ordinary\"\n",
            "merge-rec=1:2:c3a9:ordinary,raw\n",
            "replace-rec=1:2:c3a9:\"raw\"\n",
            "nested-merge=1:2:c3a9:ordinary,raw\n",
            "nested-replace=1:2:c3a9:raw\n",
            "array(1) {\n",
            "  [0]=>\n",
            "  string(6) \"column\"\n",
            "}\n",
            "column-index=1:2:c3a9:x\n",
            "reference=1:2:c3a9:\"z\"\n",
            "string(1) \"x\"\n",
            "bool(true)\n",
            "object-set=1:y\n",
            "object-unset=0:0\n",
            "bool(true)\n",
            "bool(true)\n",
            "int(0)\n",
            "bool(true)\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn php_byte_values_compare_and_transform_without_bridge_expansion() {
    assert_eq!(
        run_php(
            r#"<?php
const FOLDED_UTF8_BYTES = 'é' | "\0\0";
const FOLDED_RAW_BYTES = "\xff" ^ "\0";
echo 'folded=', bin2hex(FOLDED_UTF8_BYTES), ':', bin2hex(FOLDED_RAW_BYTES), "\n";
const RAW_KEYS = ["\xff" => 'raw'];
const TEXT_KEYS = ['ÿ' => 'text'];
const EXPLICIT_BYTE_KEYS = ["\xff" => 'raw', 'ÿ' => 'text', "\xc3\xbf" => 'same-bytes'];
const UNPACK_RAW_TEXT = [...RAW_KEYS, ...TEXT_KEYS];
const UNPACK_TEXT_RAW = [...TEXT_KEYS, ...RAW_KEYS];
const FOLDED_KEY = "\xf0" | "\x0f";
const FOLDED_KEYS = [FOLDED_KEY => 'raw', 'ÿ' => 'text', "\xff" => 'raw-overwrite'];
const CONCAT_BYTES = "\xff" . 'ÿ';
const CONCAT_UTF8 = "\xc3\xbf" . '';
const CONCAT_KEY = "\xff" . '';
const CONCAT_KEYS = [CONCAT_KEY => 'raw', 'ÿ' => 'text'];
foreach ([
    'explicit' => EXPLICIT_BYTE_KEYS,
    'unpack-raw-text' => UNPACK_RAW_TEXT,
    'unpack-text-raw' => UNPACK_TEXT_RAW,
    'folded-keys' => FOLDED_KEYS,
    'concat-keys' => CONCAT_KEYS,
] as $label => $array) {
    echo $label, '=', count($array), ':';
    foreach ($array as $key => $value) {
        echo bin2hex($key), '=', $value, ',';
    }
    echo "\n";
}
echo 'constant-access=',
    EXPLICIT_BYTE_KEYS["\xff"], ',',
    EXPLICIT_BYTE_KEYS['ÿ'], ',',
    EXPLICIT_BYTE_KEYS["\xc3\xbf"], "\n";
echo 'folded-concat=', strlen(CONCAT_BYTES), ':', bin2hex(CONCAT_BYTES), '|',
    strlen(CONCAT_UTF8), ':', bin2hex(CONCAT_UTF8), "\n";

class ConcatBytes {
    private $value;
    public function __construct($value) { $this->value = $value; }
    public function __toString(): string { return $this->value; }
}
$raw = hex2bin('ff');
$utf8Bytes = hex2bin('c3a9');
$object = new ConcatBytes($raw);
$array = ['x'];
set_error_handler(static fn(int $level, string $message): bool => true);
foreach ([
    'string-long' => $raw . 7,
    'long-string' => 7 . $raw,
    'string-bool' => $raw . true,
    'bool-string' => false . $raw,
    'string-array' => $raw . $array,
    'array-string' => $array . $raw,
    'object-long' => $object . 7,
    'long-object' => 7 . $object,
    'utf8-long' => $utf8Bytes . 7,
] as $label => $value) {
    echo $label, '=', strlen($value), ':', bin2hex($value), "\n";
}
$assigned = $raw;
$assigned .= 7;
echo 'concat-assign=', strlen($assigned), ':', bin2hex($assigned), "\n";
$assignedObject = $raw;
$assignedObject .= $object;
echo 'concat-assign-object=', strlen($assignedObject), ':', bin2hex($assignedObject), "\n";
$assignedArray = $raw;
$assignedArray .= $array;
echo 'concat-assign-array=', strlen($assignedArray), ':', bin2hex($assignedArray), "\n";
class ConcatBox { public string $value; }
$box = new ConcatBox;
$box->value = $raw;
$constrained =& $box->value;
$constrained .= 7;
echo 'concat-constrained=', strlen($box->value), ':', bin2hex($box->value), "\n";

parse_str('x=%C3%A9Ab', $query);
echo 'lower=', bin2hex(strtolower($query['x'])), "\n";
echo 'upper=', bin2hex(strtoupper($query['x'])), "\n";
echo 'ucfirst=', bin2hex(ucfirst($query['x'])), "\n";
echo 'lcfirst=', bin2hex(lcfirst($query['x'])), "\n";
var_dump(levenshtein($query['x'], 'éAb'));

parse_str('x=%C3%A9', $query);
var_dump(count(array_unique([$query['x'], 'é'])));
var_dump(array_diff([$query['x']], ['é']));
$intersection = array_intersect([$query['x']], ['é']);
var_dump(count($intersection));
echo bin2hex($intersection[0]), "\n";
"#,
        ),
        concat!(
            "folded=c3a9:ff\n",
            "explicit=2:ff=raw,c3bf=same-bytes,\n",
            "unpack-raw-text=2:ff=raw,c3bf=text,\n",
            "unpack-text-raw=2:c3bf=text,ff=raw,\n",
            "folded-keys=2:ff=raw-overwrite,c3bf=text,\n",
            "concat-keys=2:ff=raw,c3bf=text,\n",
            "constant-access=raw,same-bytes,same-bytes\n",
            "folded-concat=3:ffc3bf|2:c3bf\n",
            "string-long=2:ff37\n",
            "long-string=2:37ff\n",
            "string-bool=2:ff31\n",
            "bool-string=1:ff\n",
            "string-array=6:ff4172726179\n",
            "array-string=6:4172726179ff\n",
            "object-long=2:ff37\n",
            "long-object=2:37ff\n",
            "utf8-long=3:c3a937\n",
            "concat-assign=2:ff37\n",
            "concat-assign-object=2:ffff\n",
            "concat-assign-array=6:ff4172726179\n",
            "concat-constrained=2:ff37\n",
            "lower=c3a96162\n",
            "upper=c3a94142\n",
            "ucfirst=c3a94162\n",
            "lcfirst=c3a94162\n",
            "int(0)\n",
            "int(1)\n",
            "array(0) {\n",
            "}\n",
            "int(1)\n",
            "c3a9\n",
        )
    );
}

#[test]
fn php_byte_key_promotion_preserves_aliases_append_state_and_iterator_provenance() {
    assert_eq!(
        run_php(
            r#"<?php
$value = 1;
$left = [0 => 'a', 'k' => &$value, 1 => 'b'];
unset($left[1]);
parse_str('x=y', $query);
$replaced = array_replace($left, $query);
$replaced['k'] = 2;
$replaced[] = 'c';
var_dump($value, array_keys($replaced));

$value = 1;
$direct = ['k' => &$value];
reset($direct);
$direct[hex2bin('ff')] = 'b';
$direct['k'] = 2;
var_dump($value, bin2hex(array_key_last($direct)), key($direct));

parse_str('%C3%A9=x', $query);
$key = array_key_first($query);
$make = static function () use ($key): Generator { yield $key => 1; };
$spread = [...$make()];
echo 'spread=', strlen(array_key_first($spread)), ':', bin2hex(array_key_first($spread)), "\n";
$iterated = iterator_to_array($make(), true);
echo 'iterator=', strlen(array_key_first($iterated)), ':', bin2hex(array_key_first($iterated)), "\n";

$object = new stdClass;
$object->property = $query;
$object->property['é'] = 'y';
var_dump(count($object->property), $object->property['é']);
$object->property = [];
$object->property[$key] = 1;
echo 'object=', strlen(array_key_first($object->property)), ':',
    bin2hex(array_key_first($object->property)), "\n";
"#,
        ),
        concat!(
            "int(2)\n",
            "array(4) {\n",
            "  [0]=>\n",
            "  int(0)\n",
            "  [1]=>\n",
            "  string(1) \"k\"\n",
            "  [2]=>\n",
            "  string(1) \"x\"\n",
            "  [3]=>\n",
            "  int(2)\n",
            "}\n",
            "int(2)\n",
            "string(2) \"ff\"\n",
            "string(1) \"k\"\n",
            "spread=2:c3a9\n",
            "iterator=2:c3a9\n",
            "int(1)\n",
            "string(1) \"y\"\n",
            "object=2:c3a9\n",
        )
    );
}

#[test]
fn php_byte_boundaries_preserve_weak_coercion_named_unpack_and_symbol_lookup() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(
    strtolower(123),
    strtoupper(123),
    ucfirst(123),
    lcfirst(123),
    levenshtein(123, '123')
);

$key = hex2bin(bin2hex('é'));
$array = ['é' => 1];
var_dump($array[$key], array_key_exists($key, $array), isset($array[$key]));

function named_unicode($é) { return $é; }
$make = static function () use ($key): Generator { yield $key => 'ok'; };
var_dump(named_unicode(...$make()));
try { named_unicode(...['é' => 'first'], ...$make()); }
catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), "\n";
}

class Éx {}
$class = hex2bin(bin2hex('Éx'));
var_dump(class_exists($class, false));
spl_autoload_register(static function (string $name): void {
    echo 'autoload=', bin2hex($name), "\n";
    if ($name === 'ÉAuto') { eval('class ÉAuto {}'); }
});
$class = hex2bin(bin2hex('ÉAuto'));
var_dump(class_exists($class));
"#,
        ),
        concat!(
            "string(3) \"123\"\n",
            "string(3) \"123\"\n",
            "string(3) \"123\"\n",
            "string(3) \"123\"\n",
            "int(0)\n",
            "int(1)\n",
            "bool(true)\n",
            "bool(true)\n",
            "string(2) \"ok\"\n",
            "Error|Named parameter $é overwrites previous argument\n",
            "bool(true)\n",
            "autoload=c3894175746f\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn php_byte_values_cross_public_output_and_mixed_named_argument_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
$value = preg_quote('é');
var_dump([$value]);
echo 'export=', bin2hex(var_export($value, true)), "\n";
echo 'serialize=', bin2hex(serialize($value)), "\n";
echo 'json=', json_encode($value, JSON_UNESCAPED_UNICODE), "\n";
parse_str('%C3%A9=x', $query);
echo 'json-key=', json_encode($query, JSON_UNESCAPED_UNICODE), "\n";

$bad = hex2bin('ff');
var_dump(
    json_encode($bad),
    json_last_error(),
    json_encode([$bad], JSON_PARTIAL_OUTPUT_ON_ERROR),
    json_last_error()
);
echo 'bad-serialize=', bin2hex(serialize($bad)), "\n";

function mixed_names($é, ...$rest): void {
    echo 'bound=', $é, ':';
    foreach (array_keys($rest) as $key) { echo bin2hex($key), ','; }
    echo "\n";
}
$arguments = [];
$arguments[hex2bin('ff')] = 1;
$arguments[hex2bin(bin2hex('é'))] = 2;
mixed_names(...$arguments);

class MagicNames {
    public function __call($name, $arguments): void {
        echo 'magic=';
        foreach (array_keys($arguments) as $key) { echo bin2hex($key), ','; }
        echo "\n";
    }
}
(new MagicNames)->missing(...$arguments);
"#,
        ),
        concat!(
            "array(1) {\n",
            "  [0]=>\n",
            "  string(2) \"é\"\n",
            "}\n",
            "export=27c3a927\n",
            "serialize=733a323a22c3a9223b\n",
            "json=\"é\"\n",
            "json-key={\"é\":\"x\"}\n",
            "bool(false)\n",
            "int(5)\n",
            "string(6) \"[null]\"\n",
            "int(5)\n",
            "bad-serialize=733a313a22ff223b\n",
            "bound=2:ff,\n",
            "magic=ff,c3a9,\n",
        )
    );
}

#[test]
fn exit_writes_php_bytes_without_utf8_expansion() {
    assert_eq!(
        run_php_bytes_until_exit(
            r#"<?php
exit(hex2bin('ff'));
"#,
        ),
        vec![0xff]
    );
}

#[test]
fn serialized_and_json_byte_streams_round_trip_without_utf8_storage_expansion() {
    let directory = TempPhpDir::new();
    let json = directory.write_bytes("unicode.json", b"{\"x\":\"\xc3\xa9\"}");
    assert_eq!(
        run_php(&format!(
            r#"<?php
$raw = hex2bin('ff');
echo 'string=', bin2hex(unserialize(serialize($raw))), "\n";
$roundtrip = unserialize(serialize([$raw => 1, 'é' => 2]));
foreach ($roundtrip as $key => $value) {{
    echo bin2hex($key), '=', $value, ',';
}}
echo "\n", 'lookup=', $roundtrip[$raw], ':', $roundtrip['é'], "\n";

$decoded = json_decode(file_get_contents('{json}'), true);
echo 'decoded=', bin2hex($decoded['x']), "\n";
var_dump(json_decode($raw), json_last_error());

$invalid = hex2bin('ff61');
var_dump(
    json_encode($invalid, JSON_INVALID_UTF8_IGNORE),
    json_last_error(),
    json_encode($invalid, JSON_INVALID_UTF8_SUBSTITUTE | JSON_UNESCAPED_UNICODE),
    json_last_error()
);
$invalidKey = [$invalid => 1];
var_dump(
    json_encode($invalidKey, JSON_INVALID_UTF8_IGNORE),
    json_encode(
        $invalidKey,
        JSON_INVALID_UTF8_SUBSTITUTE | JSON_UNESCAPED_UNICODE
    )
);
"#
        )),
        concat!(
            "string=ff\n",
            "ff=1,c3a9=2,\n",
            "lookup=1:2\n",
            "decoded=c3a9\n",
            "NULL\n",
            "int(5)\n",
            "string(3) \"\"a\"\"\n",
            "int(0)\n",
            "string(6) \"\"�a\"\"\n",
            "int(0)\n",
            "string(7) \"{\"a\":1}\"\n",
            "string(10) \"{\"�a\":1}\"\n",
        )
    );
}

#[test]
fn arbitrary_byte_names_keys_and_sorting_do_not_alias_unicode_text() {
    assert_eq!(
        run_php(
            r#"<?php
$raw = hex2bin('ff');
function collision($ÿ = 'default', ...$rest): void {
    echo 'named=', $ÿ, ':';
    foreach (array_keys($rest) as $key) { echo bin2hex($key), ','; }
    echo "\n";
}
collision(...[$raw => 1]);
class ÿ {}
var_dump(class_exists($raw, false));

class ByteKey {
    private $value;
    public function __construct($value) { $this->value = $value; }
    public function __toString(): string { return $this->value; }
}
$filled = array_fill_keys([new ByteKey($raw), new ByteKey('é')], 1);
echo 'fill=';
foreach ($filled as $key => $value) { echo bin2hex($key), ','; }
echo "\n";
$combined = array_combine(
    [new ByteKey($raw), new ByteKey('é')],
    [1, 2]
);
echo 'combine=';
foreach ($combined as $key => $value) { echo bin2hex($key), '=', $value, ','; }
echo "\n";

$object = new stdClass;
$object->{'é'} = 3;
var_dump(array_column([$object], hex2bin('c3a9')));
var_dump(array_column([$object], $raw));

$values = [$raw, 'ÿ', 'é'];
sort($values, SORT_STRING);
echo 'sort=';
foreach ($values as $value) { echo bin2hex($value), ','; }
echo "\n";
"#,
        ),
        concat!(
            "named=default:ff,\n",
            "bool(false)\n",
            "fill=ff,c3a9,\n",
            "combine=ff=1,c3a9=2,\n",
            "array(1) {\n",
            "  [0]=>\n",
            "  int(3)\n",
            "}\n",
            "array(0) {\n",
            "}\n",
            "sort=c3a9,c3bf,ff,\n",
        )
    );
}
