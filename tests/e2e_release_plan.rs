mod common;

use common::run_php;

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
