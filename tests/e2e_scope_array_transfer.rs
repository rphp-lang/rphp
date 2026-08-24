mod common;
use common::run_php;

#[test]
fn compact_flattens_names_in_order_and_copies_reference_values() {
    let output = run_php(
        r#"<?php
function collectScope() {
    $first = 1;
    $second = 2;
    $alias =& $first;
    set_error_handler(function ($level, $message) {
        echo "warning:$message\n";
        return true;
    });
    $result = compact('second', [['alias', 'missing']], false);
    restore_error_handler();
    $result['alias'] = 9;
    echo implode(',', array_keys($result)), ':', $first, ':', $result['alias'];
}
collectScope();
"#,
    );
    assert_eq!(
        output,
        concat!(
            "warning:compact(): Undefined variable $missing\n",
            "warning:compact(): Argument #3 must be string or array of strings, false given\n",
            "second,alias:1:9",
        )
    );
}

#[test]
fn compact_observes_this_recursion_and_dynamic_call_boundaries() {
    let output = run_php(
        r#"<?php
class ScopeBox {
    public function collect() {
        return (function () { return compact([['this']]); })();
    }
    public function defined() {
        return (function () { return get_defined_vars(); })();
    }
}
$result = (new ScopeBox())->collect();
$box = new ScopeBox();
echo get_class($result['this']), ':', count(compact('this')), ':', count($box->defined()), "\n";

$known = 1;
$recursive = ['known'];
$recursive[] =& $recursive;
try { compact($recursive); }
catch (Error $error) { echo $error->getMessage(), "\n"; }

$callable = 'compact';
try { $callable('known'); }
catch (Error $error) { echo $error->getMessage(), "\n"; }

ob_start('compact');
try { ob_end_clean(); }
catch (Error $error) { echo $error->getMessage(); }
"#,
    );
    assert_eq!(
        output,
        concat!(
            "ScopeBox:0:0\n",
            "Recursion detected\n",
            "Cannot call compact() dynamically\n",
            "Cannot call compact() dynamically",
        )
    );
}

#[test]
fn extract_applies_prefix_modes_and_validates_errors_before_writes() {
    let output = run_php(
        r#"<?php
function extractModes() {
    $same = 'old';
    $source = ['same' => 'new', 7 => 'seven', 'fresh' => 3];
    echo extract($source, EXTR_SKIP), ":$same:$fresh\n";
    echo extract($source, EXTR_PREFIX_ALL, ''), ":$_same:$_7:$_fresh\n";

    foreach ([[-1, null], [EXTR_PREFIX_IF_EXISTS, null], [EXTR_OVERWRITE, '85bad']] as [$flags, $prefix]) {
        try {
            if ($prefix === null) {
                extract($source, $flags);
            } else {
                extract($source, $flags, $prefix);
            }
        } catch (ValueError $error) {
            echo $error->getMessage(), "\n";
        }
    }
    try { extract($source, []); }
    catch (TypeError $error) { echo $error->getMessage(); }
}
extractModes();
"#,
    );
    assert_eq!(
        output,
        concat!(
            "1:old:3\n",
            "3:new:seven:3\n",
            "extract(): Argument #2 ($flags) must be a valid extract type\n",
            "extract(): Argument #3 ($prefix) is required when using this extract type\n",
            "extract(): Argument #3 ($prefix) must be a valid identifier\n",
            "extract(): Argument #2 ($flags) must be of type int, array given",
        )
    );
}

#[test]
fn extract_separates_values_rebinds_refs_and_checks_typed_reference_targets() {
    let output = run_php(
        r#"<?php
$value = 1;
$source = ['item' => &$value];
extract($source);
$item = 2;
echo "default:$value:", $source['item'], "\n";

$previous = 3;
$bound =& $previous;
$references = ['bound' => 4];
extract($references, EXTR_REFS);
$bound = 5;
echo "refs:$previous:", $references['bound'], "\n";

class TypedScope {
    public int $number = 0;
    public string $text = '';
}
$typed = new TypedScope();
$number =& $typed->number;
$text =& $typed->text;
try { extract(['number' => 'bad', 'text' => 42]); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
echo "typed:", $typed->number, ':', $typed->text, "\n";
extract(['number' => 2.0]);
echo "coerced:", $typed->number;
"#,
    );
    assert_eq!(
        output,
        concat!(
            "default:1:1\n",
            "refs:3:5\n",
            "Cannot assign string to reference held by property TypedScope::$number of type int\n",
            "typed:0:\n",
            "coerced:2",
        )
    );
}

#[test]
fn extract_accepts_prefer_reference_globals_and_preserves_source_cow_refs() {
    let output = run_php(
        r#"<?php
$sentinel = 'live';
$count = extract($GLOBALS, EXTR_REFS);
echo ($count > 0 ? 'globals' : 'empty'), ":$sentinel\n";

$source = ['entry' => 'first'];
extract($source, EXTR_REFS);
$copy = $source;
$copy['entry'] = 'second';
extract($source, EXTR_REFS);
echo $entry, ':', $source['entry'];
"#,
    );
    assert_eq!(output, "globals:live\nsecond:second");
}

#[test]
fn extract_materializes_self_overwrite_before_scope_writes_and_keeps_refs() {
    let output = run_php(
        r#"<?php
function overwriteSource(int $flags): void {
    $source = ['source' => 10, 'tail' => 20];
    $count = extract($source, $flags, 'p');
    echo "$flags:$count:";
    var_dump($source, $tail ?? null, $p_source ?? null, $p_tail ?? null);
    if (($flags & EXTR_REFS) && ($flags & 0xff) === EXTR_OVERWRITE) {
        $tail = 21;
        var_dump($tail);
    }
    if (($flags & EXTR_REFS) && ($flags & 0xff) === EXTR_PREFIX_ALL) {
        $p_tail = 22;
        var_dump($source['tail']);
    }
}

overwriteSource(EXTR_OVERWRITE);
overwriteSource(EXTR_OVERWRITE | EXTR_REFS);
overwriteSource(EXTR_PREFIX_ALL | EXTR_REFS);

function overwriteAliasedPrefix(): void {
    $source = ['source' => 10, 'tail' => 20];
    $p_source =& $source;
    $count = extract($source, EXTR_PREFIX_ALL | EXTR_REFS, 'p');
    echo "alias:$count:";
    var_dump($source, $p_source, $p_tail);
    $p_tail = 23;
    var_dump($source['tail']);
}
overwriteAliasedPrefix();
"#,
    );
    assert_eq!(
        output,
        concat!(
            "0:2:int(10)\nint(20)\nNULL\nNULL\n",
            "256:2:int(10)\nint(20)\nNULL\nNULL\nint(21)\n",
            "259:2:array(2) {\n",
            "  [\"source\"]=>\n",
            "  &int(10)\n",
            "  [\"tail\"]=>\n",
            "  &int(20)\n",
            "}\n",
            "NULL\n",
            "int(10)\n",
            "int(20)\n",
            "int(22)\n",
            "alias:2:array(2) {\n",
            "  [\"source\"]=>\n",
            "  &int(10)\n",
            "  [\"tail\"]=>\n",
            "  &int(20)\n",
            "}\n",
            "int(10)\n",
            "int(20)\n",
            "int(23)\n",
        )
    );
}

#[test]
fn extract_prefix_modes_snapshot_before_replacing_a_source_named_target() {
    assert_eq!(
        run_php(
            r#"<?php
function probePrefixMode(int $mode): void {
    $bucket = ['bucket' => 11, 'tail' => 22];
    $count = extract($bucket, $mode, 'p');
    echo $mode, ':', $count, ':',
        is_array($bucket) ? 'array' : $bucket, ':',
        $p_bucket ?? '-', ':', $p_tail ?? '-', ':', $tail ?? '-', "\n";
}
foreach ([EXTR_PREFIX_SAME, EXTR_PREFIX_ALL, EXTR_PREFIX_INVALID, EXTR_PREFIX_IF_EXISTS] as $mode) {
    probePrefixMode($mode);
}
"#,
        ),
        concat!(
            "2:2:array:11:-:22\n",
            "3:2:array:11:22:-\n",
            "4:2:11:-:-:22\n",
            "5:1:array:11:-:-\n",
        ),
    );
}

#[test]
fn extract_treats_this_as_restricted_across_every_mode_and_refs_variant() {
    let output = run_php(
        r#"<?php
class ExtractThisBoundary {
    private function probe(int $flags): void {
        $source = ['this' => 'value'];
        try {
            $count = extract($source, $flags, 'safe');
            echo "$flags:$count:", isset($safe_this) ? $safe_this : 'missing', ':', get_class($this), "\n";
        } catch (Throwable $error) {
            echo "$flags:", $error->getMessage(), ':', get_class($this), "\n";
        }
    }

    public function run(): void {
        foreach ([
            EXTR_OVERWRITE,
            EXTR_SKIP,
            EXTR_PREFIX_SAME,
            EXTR_PREFIX_ALL,
            EXTR_PREFIX_INVALID,
            EXTR_IF_EXISTS,
            EXTR_PREFIX_IF_EXISTS,
        ] as $mode) {
            $this->probe($mode);
            $this->probe($mode | EXTR_REFS);
        }
    }
}

(new ExtractThisBoundary())->run();
"#,
    );
    assert_eq!(
        output,
        concat!(
            "0:Cannot re-assign $this:ExtractThisBoundary\n",
            "256:Cannot re-assign $this:ExtractThisBoundary\n",
            "1:0:missing:ExtractThisBoundary\n",
            "257:0:missing:ExtractThisBoundary\n",
            "2:1:value:ExtractThisBoundary\n",
            "258:1:value:ExtractThisBoundary\n",
            "3:1:value:ExtractThisBoundary\n",
            "259:1:value:ExtractThisBoundary\n",
            "4:1:value:ExtractThisBoundary\n",
            "260:1:value:ExtractThisBoundary\n",
            "6:0:missing:ExtractThisBoundary\n",
            "262:0:missing:ExtractThisBoundary\n",
            "5:0:missing:ExtractThisBoundary\n",
            "261:0:missing:ExtractThisBoundary\n",
        )
    );
}

#[test]
fn conditional_global_bindings_do_not_mirror_inactive_locals_during_extract() {
    let output = run_php(
        r#"<?php
$alpha = 5;
$beta = 6;
function transferGlobals(bool $bind): void {
    $GLOBALS['alpha'] = 10;
    $GLOBALS['beta'] = 11;
    if ($bind) {
        global $alpha, $beta;
    } else {
        $alpha = null;
        $beta = null;
    }
    echo $bind ? 'bound:' : 'local:';
    var_dump($alpha, $beta, $GLOBALS['alpha'], $GLOBALS['beta']);
    extract($GLOBALS, EXTR_REFS);
    $alpha = 12;
    $GLOBALS['beta'] = 13;
    var_dump($alpha, $beta, $GLOBALS['alpha'], $GLOBALS['beta']);
}
transferGlobals(false);
echo "main:$alpha:$beta\n";
transferGlobals(true);
echo "main:$alpha:$beta\n";
"#,
    );
    assert_eq!(
        output,
        concat!(
            "local:NULL\nNULL\nint(10)\nint(11)\n",
            "int(12)\nint(11)\nint(10)\nint(13)\n",
            "main:10:13\n",
            "bound:int(10)\nint(11)\nint(10)\nint(11)\n",
            "int(12)\nint(13)\nint(12)\nint(13)\n",
            "main:12:13\n",
        )
    );
}
