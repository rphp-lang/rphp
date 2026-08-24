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
