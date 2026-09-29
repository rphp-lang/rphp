mod common;

use common::{run_php, run_php_bytes_until_exit, run_php_with_source_context};

#[test]
fn release_plan_groups_aliases_across_distinct_local_containers() {
    assert_eq!(
        run_php(
            r#"<?php
class PlanLeaf {
    function __construct(public string $name) {}
    function __destruct() { echo $this->name, '|'; }
}
function retirePlan() {
    $first = new PlanLeaf('first');
    $second = new PlanLeaf('second');
    $left = [$first, $first];
    $right = [$first, $second];
    unset($first, $second);
    echo 'body|';
}
retirePlan();
echo 'after';
"#,
        ),
        "body|first|second|after"
    );
}

#[test]
fn throwing_release_preserves_deferred_and_unvisited_owners() {
    assert_eq!(
        run_php(
            r#"<?php
class PlanRelease {
    function __construct(public string $name, public bool $fail = false) {}
    function __destruct() {
        echo $this->name, '|';
        if ($this->fail) { throw new Exception($this->name); }
    }
}
$saved = new PlanRelease('saved');
function throwingPlan($saved) {
    $items = [$saved, new PlanRelease('throwing', true), new PlanRelease('last')];
    echo 'body|';
    unset($items);
}
try { throwingPlan($saved); }
catch (Exception $error) { echo 'caught:', $error->getMessage(), '|'; }
echo 'retained:', $saved->name, '|';
echo 'after|';
"#,
        ),
        "body|throwing|last|caught:throwing|retained:saved|after|saved|"
    );
}

#[test]
fn throwing_return_release_finishes_siblings_before_the_caller_catches() {
    for (return_type, completion, invocation) in [
        ("", "", "retire($saved)"),
        ("", "return 7;", "retire($saved)"),
        (
            ": int",
            "try { return 7; } finally { echo 'finally|'; }",
            "retire($saved)",
        ),
        ("", "return 7;", "array_map('retire', [$saved])"),
    ] {
        let source = format!(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class ReturnRelease {{
    function __construct(public string $name, public bool $fail = false) {{}}
    function __destruct() {{
        echo $this->name, '|';
        if ($this->fail) {{ throw new Exception($this->name); }}
    }}
}}
function retire($saved){return_type} {{
    $items = [$saved, new ReturnRelease('throwing', true), new ReturnRelease('last')];
    echo 'body|';
    {completion}
}}
$saved = new ReturnRelease('saved');
try {{ {invocation}; }}
catch (Exception $error) {{ echo 'caught:', $error->getMessage(), '|'; }}
echo 'retained:', $saved->name, '|';
unset($saved);
echo 'after';
"#,
        );
        let finally = if completion.contains("finally") {
            "finally|"
        } else {
            ""
        };
        assert_eq!(
            run_php(&source),
            format!("body|{finally}throwing|last|caught:throwing|retained:saved|saved|after"),
            "{invocation}: {completion}",
        );
    }
}

#[test]
fn later_return_destructor_exceptions_keep_the_replacement_chain() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class ReturnFailure {
    function __construct(public string $name) {}
    function __destruct() {
        echo $this->name, '|';
        throw new Exception($this->name);
    }
}
function retireFailures() {
    $first = new ReturnFailure('first');
    $items = [new ReturnFailure('second'), new ReturnFailure('third')];
    echo 'body|';
    return 7;
}
try { retireFailures(); }
catch (Exception $error) {
    do { echo 'caught:', $error->getMessage(), '|'; }
    while ($error = $error->getPrevious());
}
echo 'after';
"#,
        ),
        "body|first|second|third|caught:third|caught:second|caught:first|after",
    );
}

#[test]
fn exception_trace_arguments_retain_their_owners_until_the_trace_is_released() {
    for ignore in [0, 1] {
        let source = format!(
            r#"<?php
ini_set('zend.exception_ignore_args', '{ignore}');
class TraceRelease {{
    function __destruct() {{ echo 'drop|'; }}
}}
function retainInTrace($owner) {{ throw new Exception('trace'); }}
$owner = new TraceRelease;
try {{ retainInTrace($owner); }}
catch (Exception $error) {{ echo 'caught|'; }}
unset($owner);
echo 'owner-unset|';
unset($error);
echo 'trace-unset';
"#,
        );
        let expected = if ignore == 0 {
            "caught|owner-unset|drop|trace-unset"
        } else {
            "caught|drop|owner-unset|trace-unset"
        };
        assert_eq!(
            run_php_with_source_context(&source, "release-plan.php", "."),
            expected,
            "ignore_args={ignore}",
        );
    }
}

#[test]
fn detached_return_destructor_exit_reaches_the_embedding_boundary() {
    assert_eq!(
        run_php_bytes_until_exit(
            r#"<?php
class ExitRelease {
    function __destruct() { echo 'exit|'; exit(23); }
}
function exitOnReturn($value) {
    $local = new ExitRelease;
    echo 'body|';
    return $value;
}
array_map('exitOnReturn', [7, 11]);
echo 'unreachable';
"#,
        ),
        b"body|exit|",
    );
}
