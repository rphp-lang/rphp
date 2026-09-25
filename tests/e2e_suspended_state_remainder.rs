mod common;

use common::run_php;

#[test]
fn closing_a_generator_through_an_inner_finally_resumes_pending_exceptions() {
    assert_eq!(
        run_php(
            r#"<?php
function chainedPendingClose() {
    try { throw new LogicException('outer'); }
    finally {
        try { throw new RuntimeException('inner'); }
        finally { try { yield; } finally { echo 'close|'; } }
    }
}
try { chainedPendingClose()->rewind(); echo 'after|'; }
catch (Throwable $error) {
    do { echo get_class($error), ':', $error->getMessage(), '|'; }
    while ($error = $error->getPrevious());
}
"#
        ),
        "close|RuntimeException:inner|LogicException:outer|"
    );
}

#[test]
fn closing_finally_rethrows_one_pending_exception_without_resuming_the_body() {
    assert_eq!(
        run_php(
            r#"<?php
function crossedPendingClose() {
    try { throw new LogicException('pending'); }
    finally {
        try { yield; } finally { echo 'inner|'; }
        echo 'abandoned|';
    }
}
try { crossedPendingClose()->rewind(); echo 'after|'; }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), '|'; }
"#
        ),
        "inner|LogicException:pending|"
    );
}

#[test]
fn a_pending_generator_exception_is_discarded_without_a_further_finally() {
    assert_eq!(
        run_php(
            r#"<?php
function abandonedPendingClose() {
    try { throw new LogicException('discarded'); }
    finally { yield; echo 'abandoned|'; }
}
try { abandonedPendingClose()->rewind(); echo 'temporary|'; }
catch (Throwable $error) { echo 'unexpected|'; }
$generator = abandonedPendingClose();
$generator->rewind(); echo 'suspended|';
try { unset($generator); echo 'released|'; }
catch (Throwable $error) { echo 'unexpected|'; }
"#
        ),
        "temporary|suspended|released|"
    );
}

#[test]
fn an_explicit_return_in_closing_finally_overrides_the_pending_exception() {
    assert_eq!(
        run_php(
            r#"<?php
function returnedPendingClose() {
    try { throw new LogicException('discarded'); }
    finally { try { yield; } finally { echo 'return|'; return 7; } }
}
try { returnedPendingClose()->rewind(); echo 'after|'; }
catch (Throwable $error) { echo 'unexpected|'; }
"#
        ),
        "return|after|"
    );
}

#[test]
fn locally_caught_closing_errors_do_not_discard_the_outer_pending_throw() {
    assert_eq!(
        run_php(
            r#"<?php
function caughtPendingClose() {
    try { throw new LogicException('outer'); }
    finally {
        try { yield; }
        finally {
            try { throw new RuntimeException('local'); }
            catch (Throwable $error) { echo $error->getMessage(), '|'; }
            echo 'close|';
        }
    }
}
try { caughtPendingClose()->rewind(); echo 'after|'; }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), '|'; }
"#
        ),
        "local|close|LogicException:outer|"
    );
}

#[test]
fn a_replacing_closing_exception_keeps_the_original_as_previous() {
    assert_eq!(
        run_php(
            r#"<?php
function replacedPendingClose() {
    try { throw new LogicException('old'); }
    finally {
        try { yield; }
        finally { echo 'close|'; throw new RuntimeException('replacement'); }
    }
}
try { replacedPendingClose()->rewind(); echo 'after|'; }
catch (Throwable $error) {
    do { echo get_class($error), ':', $error->getMessage(), '|'; }
    while ($error = $error->getPrevious());
}
"#
        ),
        "close|RuntimeException:replacement|LogicException:old|"
    );
}

#[test]
fn normal_generator_resume_and_exception_free_force_close_remain_distinct() {
    assert_eq!(
        run_php(
            r#"<?php
function normallyResumedPending() {
    try { throw new LogicException('saved'); }
    finally { yield; echo 'resumed|'; }
}
$generator = normallyResumedPending(); $generator->rewind(); echo 'suspended|';
try { $generator->next(); echo 'after|'; }
catch (Throwable $error) { echo $error->getMessage(), '|'; }
unset($generator);
function ordinaryClosedGenerator() {
    try { yield; echo 'abandoned|'; } finally { echo 'normal-close|'; }
}
ordinaryClosedGenerator()->rewind(); echo 'end|';
"#
        ),
        "suspended|resumed|saved|normal-close|end|"
    );
}

#[test]
fn reference_foreach_replacement_restarts_at_the_new_array_pointer() {
    assert_eq!(
        run_php(
            r#"<?php
$values = [1, 2, 3];
foreach ($values as &$value) {
    echo $value, '|';
    if ($value === 1) $values = [4, 5];
}
unset($value);
$values = [1, 2, 3]; $replacement = [4, 5, 6]; next($replacement);
foreach ($values as &$value) {
    echo $value, '|';
    if ($value === 1) $values = $replacement;
}
echo current($replacement), '|';
"#
        ),
        "1|4|5|1|5|6|5|"
    );
}

#[test]
fn aliases_and_nested_reference_loops_observe_replacement_independently() {
    assert_eq!(
        run_php(
            r#"<?php
$values = [1, 2, 3]; $alias =& $values;
foreach ($values as &$value) {
    echo $value, '|';
    if ($value === 1) $alias = [4, 5];
}
unset($value, $alias);
$values = [1, 2, 3]; $steps = 0;
foreach ($values as &$value) {
    echo 'o', $value, '|';
    if ($steps++ === 0) {
        foreach ($values as &$inner) {
            echo 'i', $inner, '|'; $values = [4, 5]; break;
        }
    }
    if ($steps === 6) break;
}
"#
        ),
        "1|4|5|o1|i1|o4|o5|"
    );
}

#[test]
fn self_assignment_and_element_cow_do_not_restart_reference_foreach() {
    assert_eq!(
        run_php(
            r#"<?php
$values = [1, 2, 3]; $steps = 0;
foreach ($values as &$value) {
    echo $value, '|'; $values = $values;
    if (++$steps === 6) break;
}
unset($value);
$values = [1, 2, 3]; $steps = 0;
foreach ($values as &$value) {
    echo $value, '|'; $copy = $values; $values[2] = 9;
    if (++$steps === 6) break;
}
echo implode(',', $copy), '|';
unset($value);
$values = [1, 2, 3]; $steps = 0;
foreach ($values as &$value) {
    echo $value, '|';
    if ($steps++ === 0) $values = $values + [7 => 9];
    if ($steps === 8) break;
}
"#
        ),
        "1|2|3|1|2|9|1,2,9|1|2|3|9|"
    );
}

#[test]
fn selecting_a_cow_branch_retires_the_previous_array_cursor_copies() {
    assert_eq!(
        run_php(
            r#"<?php
$values = [1, 2, 3]; $steps = 0;
foreach ($values as &$value) {
    echo $value, '|';
    if ($steps++ === 0) { $copy = $values; $values[2] = 9; }
    elseif ($steps === 2) { $values = $copy; }
    if ($steps === 6) break;
}
unset($value);
$values = [1, 2, 3]; $steps = 0;
foreach ($values as &$value) {
    echo $value, '|';
    if ($steps++ === 0) { $copy = $values; $values = [4, 5, 6]; }
    elseif ($steps === 2) { $values = $copy; }
    if ($steps === 6) break;
}
"#
        ),
        "1|2|1|2|3|1|4|1|2|3|"
    );
}

#[test]
fn reference_foreach_append_unset_and_splice_keep_their_live_positions() {
    assert_eq!(
        run_php(
            r#"<?php
$values = [1, 2];
foreach ($values as &$value) { echo $value, '|'; if ($value === 1) $values[] = 3; }
unset($value);
$values = [1, 2, 3];
foreach ($values as &$value) { echo $value, '|'; if ($value === 1) unset($values[0]); }
unset($value);
$values = [1, 2, 3, 4];
foreach ($values as &$value) { echo $value, '|'; if ($value === 2) array_splice($values, 0, 2); }
unset($value);
$values = [1, 2, 3];
foreach ($values as $value) { echo $value, '|'; if ($value === 1) $values = [4, 5]; }
"#
        ),
        "1|2|3|1|2|3|1|2|3|4|1|2|3|"
    );
}

#[test]
fn reference_foreach_cursor_survives_generator_suspension_and_early_exit() {
    assert_eq!(
        run_php(
            r#"<?php
function resumedReferenceLoop() {
    $values = [1, 2, 3];
    foreach ($values as &$value) {
        yield $value;
        if ($value === 1) $values = [4, 5];
    }
}
foreach (resumedReferenceLoop() as $value) echo $value, '|';
function escapedReferenceLoop() {
    $values = [7, 8];
    foreach ($values as &$value) { throw new Exception('stop'); }
}
for ($round = 0; $round < 3; ++$round) {
    try { escapedReferenceLoop(); } catch (Exception $error) { echo $error->getMessage(), '|'; }
    $values = [9, 10];
    foreach ($values as &$value) echo $value, '|';
    unset($value);
}
"#
        ),
        "1|4|5|stop|9|10|stop|9|10|stop|9|10|"
    );
}
