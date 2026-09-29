mod common;

use common::run_php;

#[test]
fn nested_retirement_keeps_outer_owners_and_exception_progress_independent() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class WorkspaceLeaf {
    function __construct(public string $name) {}
    function __destruct() { echo $this->name, '|'; }
}
function retireInner($fail) {
    $children = [new WorkspaceLeaf('a'), new WorkspaceLeaf('b')];
    echo 'inner|';
    if ($fail) { throw new Exception('inner'); }
}
class WorkspaceRoot {
    function __construct(public bool $fail) {}
    function __destruct() {
        echo 'root|';
        retireInner($this->fail);
        echo 'resumed|';
    }
}
function retireOuter($shared, $fail) {
    $alias = $shared;
    $container = [$shared, new WorkspaceRoot($fail), new WorkspaceLeaf('tail')];
    echo 'outer|';
}
$shared = new WorkspaceLeaf('shared');
foreach ([false, true] as $fail) {
    try { retireOuter($shared, $fail); }
    catch (Exception $error) { echo 'caught:', $error->getMessage(), '|'; }
    echo 'after|';
}
unset($shared);
"#,
        ),
        concat!(
            "outer|root|inner|a|b|resumed|tail|after|",
            "outer|root|inner|a|b|tail|caught:inner|after|shared|",
        ),
    );
}

#[test]
fn large_and_small_retirement_graphs_do_not_retain_previous_owners() {
    assert_eq!(
        run_php(
            r#"<?php
class WorkspaceCount {
    public static int $released = 0;
    function __destruct() { self::$released++; }
}

function retireBatch($size) {
    $items = [];
    for ($i = 0; $i < $size; $i++) { $items[] = new WorkspaceCount; }
}
foreach ([2050, 2, 0, 3] as $size) {
    retireBatch($size);
    echo WorkspaceCount::$released, '|';
}
"#,
        ),
        "2050|2052|2052|2055|",
    );
}

#[test]
fn direct_retirement_preserves_alias_order_in_compact_and_wide_frames() {
    for size in [4, 75] {
        let mut source = String::from(
            "<?php class DirectWorkspaceOwner {\n\
             function __construct(public int $id) {}\n\
             function __destruct() { echo $this->id, '|'; }\n\
             } function retireDirectWorkspace() {\n",
        );
        for index in 0..size {
            source.push_str(&format!(
                "$owner{index} = new DirectWorkspaceOwner({index});\n"
            ));
        }
        // PHP reaches the last aliases after the unaliased sibling slots.
        source.push_str("$lastAlias = $owner0; $firstAlias = $owner1; echo 'body|';\n");
        source.push_str("} retireDirectWorkspace(); echo 'after';");
        let mut expected = String::from("body|");
        for index in (2..size).chain([0, 1]) {
            expected.push_str(&format!("{index}|"));
        }
        expected.push_str("after");
        assert_eq!(run_php(&source), expected, "{size} direct owners");
    }
}

#[test]
fn returned_and_dynamic_owners_survive_ordered_retirement() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class RetiredOwner {
    public function __construct(public string $name, public $child = null, public bool $fail = false) {}
    public function __destruct() {
        echo $this->name, '|';
        if ($this->fail) { throw new Exception($this->name); }
    }
}
function returnOwners($mode) {
    $local = new RetiredOwner('returned');
    $alias = $local;
    $cell = &$alias;
    $closure = static function () use ($cell) { return $cell->name; };
    $kept = [$local, $closure];
    $last = new RetiredOwner('local');
    return $mode ? $kept : $closure;
}
foreach ([false, true] as $mode) {
    $saved = returnOwners($mode);
    echo 'saved|';
    unset($saved);
    echo 'freed|';
}
function retireDynamicOwners() {
    $owner = new RetiredOwner('dynamic-shared');
    $name = implode('', ['dyn', 'amic']);
    $$name = $owner;
    $second = $name . '-second';
    $$second = new RetiredOwner('dynamic-second');
    echo 'body|';
}
retireDynamicOwners();
function retireThrowingTree() {
    $tree = new RetiredOwner('parent', new RetiredOwner('child', null, true), true);
    $tail = new RetiredOwner('tail');
}
try { retireThrowingTree(); }
catch (Exception $e) {
    do { echo 'caught:', $e->getMessage(), '|'; } while ($e = $e->getPrevious());
}
echo 'done';
"#
        ),
        "local|saved|returned|freed|local|saved|returned|freed|body|dynamic-shared|dynamic-second|parent|child|tail|caught:child|caught:parent|done"
    );
}

#[test]
fn shutdown_callback_returns_keep_independent_exception_handlers() {
    assert_eq!(
        run_php(
            r#"<?php
class ShutdownLocal {
    public function __construct(public string $name) {}
    public function __destruct() { echo 'drop:', $this->name, '|'; throw new Exception($this->name); }
}
set_exception_handler(function ($error) {
    do { echo 'caught:', $error->getMessage(), '|'; } while ($error = $error->getPrevious());
});
register_shutdown_function(function () {
    $first = new ShutdownLocal('first');
    $second = new ShutdownLocal('second');
    echo 'body|';
});
"#
        ),
        "body|drop:first|caught:first|drop:second|caught:second|"
    );
}

#[test]
fn returned_reference_cells_survive_compact_and_wide_frame_cleanup() {
    for padding in [0, 75] {
        let mut source = String::from(
            "<?php class ReferenceReturnOwner { public int $marker = 7; function __destruct() { echo 'drop|'; } } function &returnReferenceCell() { $owner = new ReferenceReturnOwner; $alias =& $owner;\n",
        );
        for index in 0..padding {
            source.push_str(&format!("$padding{index} = {index};\n"));
        }
        source.push_str("return $alias; } $held =& returnReferenceCell(); echo 'live:', $held->marker, '|'; $copy = $held; unset($held); echo 'copy|'; unset($copy); echo 'after';");
        assert_eq!(
            run_php(&source),
            "live:7|copy|drop|after",
            "{padding} padding CVs"
        );
    }
}

#[test]
fn shutdown_destructor_direct_owners_match_php() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '0');
set_exception_handler(function($e) { do {echo 'caught:',$e->getMessage(),'|';}while($e=$e->getPrevious()); });
class Leaf {function __construct(public $name){} function __destruct(){echo 'drop:',$this->name,'|';throw new Exception($this->name);}}
class Root {function __destruct(){$first=new Leaf('first');$second=new Leaf('second');echo 'root|';}}
$root=new Root;echo "body|";"#
        ),
        "body|root|drop:first|caught:first|drop:second|caught:second|"
    );
}

#[test]
fn shutdown_destructor_array_owners_match_php() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '0');
set_exception_handler(function($e) { do {echo 'caught:',$e->getMessage(),'|';}while($e=$e->getPrevious()); });
class Leaf {function __construct(public $name){} function __destruct(){echo 'drop:',$this->name,'|';throw new Exception($this->name);}}
class Root {function __destruct(){$items=[new Leaf('first'),new Leaf('second')];echo 'root|';}}
$root=new Root;echo "body|";"#
        ),
        "body|root|drop:first|caught:first|drop:second|caught:second|"
    );
}

#[test]
fn shutdown_destructor_nested_owners_match_php() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '0');
set_exception_handler(function($e) { do {echo 'caught:',$e->getMessage(),'|';}while($e=$e->getPrevious()); });
class Leaf {function __construct(public $name){} function __destruct(){echo 'drop:',$this->name,'|';throw new Exception($this->name);}}
class Root {function __destruct(){(function(){$first=new Leaf('first');$second=new Leaf('second');echo 'helper|';})();echo 'root|';}}
$root=new Root;echo "body|";"#
        ),
        "body|helper|drop:first|drop:second|caught:second|caught:first|"
    );
}

#[test]
fn shutdown_destructor_aliases_owners_match_php() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '0');
set_exception_handler(function($e) { do {echo 'caught:',$e->getMessage(),'|';}while($e=$e->getPrevious()); });
class Leaf {function __construct(public $name){} function __destruct(){echo 'drop:',$this->name,'|';throw new Exception($this->name);}}
class Root {function __destruct(){$first=new Leaf('first');$second=new Leaf('second');$alias=$first;echo 'root|';}}
$root=new Root;echo "body|";"#
        ),
        "body|root|drop:second|caught:second|drop:first|caught:first|"
    );
}
