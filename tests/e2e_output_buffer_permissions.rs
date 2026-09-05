mod common;

use common::{run_php, run_php_expect_error_with_source_context, run_php_with_source_context};

#[test]
fn capabilities_are_independent_and_denial_preserves_data_and_callback_state() {
    for flags in [0, 16, 32, 48, 64, 80, 96, 112] {
        for (operation, required, phase, action, getter) in [
            ("ob_clean", 16, 3, "delete", false),
            ("ob_flush", 32, 5, "flush", false),
            ("ob_end_clean", 64, 11, "discard", false),
            ("ob_end_flush", 64, 9, "send", false),
            ("ob_get_clean", 64, 11, "discard", true),
            ("ob_get_flush", 64, 9, "send", true),
        ] {
            let source = format!(
                r#"<?php
$notes = []; $phases = []; $report = '';
$handler = function ($bytes, $phase) use (&$phases, &$report) {{
    $phases[] = $phase;
    return $report;
}};
set_error_handler(function ($level, $message) use (&$notes) {{
    $notes[] = [$level, $message];
    return true;
}});
ob_start($handler, 0, {flags});
echo "a\0b\xff";
$result = {operation}();
$after = ob_get_contents();
$report = json_encode([
    is_string($result) ? bin2hex($result) : $result,
    is_string($after) ? bin2hex($after) : $after,
    ob_get_level(), $phases, $notes
]);
echo $report;
"#
            );
            let allowed = flags & required != 0;
            let removed = allowed && required == 64;
            let display = "{closure:/spec/output-permission-flags.php:3}";
            let mut notes = Vec::new();
            if !allowed {
                notes.push(serde_json::json!([
                    8,
                    format!("{operation}(): Failed to {action} buffer of {display} (0)")
                ]));
                if getter {
                    notes.push(serde_json::json!([
                        8,
                        format!("{operation}(): Failed to delete buffer of {display} (0)")
                    ]));
                }
            }
            let result = if getter {
                serde_json::json!("610062ff")
            } else {
                serde_json::json!(allowed)
            };
            let after = if removed {
                serde_json::json!(false)
            } else {
                serde_json::json!(if allowed { "" } else { "610062ff" })
            };
            let expected = serde_json::json!([
                result,
                after,
                if removed { 0 } else { 1 },
                if allowed { vec![phase] } else { vec![] },
                notes
            ]);
            assert_eq!(
                run_php_with_source_context(&source, "/spec/output-permission-flags.php", "/spec"),
                expected.to_string().replace('/', "\\/"),
                "{operation}/{flags}"
            );
        }
    }
}

#[test]
fn missing_buffers_report_operation_specific_notices_without_changing_level() {
    assert_eq!(
        run_php(
            r#"<?php
$messages = [];
set_error_handler(function ($severity, $message) use (&$messages) {
    $messages[] = $severity . ':' . $message;
    return true;
});
$results = [ob_clean(), ob_flush(), ob_end_clean(), ob_end_flush(), ob_get_clean(), ob_get_flush()];
foreach ($results as $result) { echo $result ? 'T' : 'F'; }
echo '|', ob_get_level(), '|', implode('|', $messages);
"#,
        ),
        concat!(
            "FFFFFF|0|",
            "8:ob_clean(): Failed to delete buffer. No buffer to delete|",
            "8:ob_flush(): Failed to flush buffer. No buffer to flush|",
            "8:ob_end_clean(): Failed to delete buffer. No buffer to delete|",
            "8:ob_end_flush(): Failed to delete and flush buffer. No buffer to delete or flush|",
            "8:ob_get_flush(): Failed to delete and flush buffer. No buffer to delete or flush",
        )
    );
}

#[test]
fn denied_getters_keep_the_original_snapshot_and_the_live_buffer() {
    for operation in ["ob_get_clean", "ob_get_flush"] {
        let source = format!(
            r#"<?php
ob_start(null, 0, 0);
echo 'seed';
set_error_handler(function ($severity, $message) {{ echo '!'; return true; }});
$snapshot = {operation}();
$live = ob_get_contents();
echo '|', $snapshot, '|', $live, '|', ob_get_level();
"#
        );
        assert_eq!(run_php(&source), "seed!!|seed|seed!!|1", "{operation}");
    }
}

#[test]
fn throwing_notice_stops_the_getter_before_publishing_its_result() {
    for operation in ["ob_get_clean", "ob_get_flush"] {
        for throwing in [1, 2] {
            let source = format!(
                r#"<?php
ob_start(null, 0, 0);
echo 'kept';
$result = 'unchanged';
$hits = 0;
set_error_handler(function ($severity, $message) use (&$hits) {{
    $hits++;
    echo '!';
    if ($hits === {throwing}) {{ throw new Exception('denied'); }}
    return true;
}});
try {{ $result = {operation}(); }} catch (Exception $error) {{ echo ':', $error->getMessage(); }}
echo '|', $result, '|', $hits, '|', ob_get_level();
"#
            );
            assert_eq!(
                run_php(&source),
                format!("kept{}:denied|unchanged|{throwing}|1", "!".repeat(throwing)),
                "{operation}/{throwing}"
            );
        }
    }
}

#[test]
fn rejected_getter_snapshot_is_detached_from_live_bytes_and_reference_aliases() {
    assert_eq!(
        run_php(
            r#"<?php
$report = '';
ob_start(function ($bytes) use (&$report) { return $report; }, 0, 0);
echo "a\0b\xff";
set_error_handler(function () { echo '+'; return true; });
$result = 'old'; $alias =& $result;
$result = ob_get_clean(); $copy = $result; $alias .= '!';
$report = bin2hex($result) . '|' . bin2hex($copy) . '|' . bin2hex(ob_get_contents());
"#,
        ),
        "610062ff21|610062ff|610062ff2b2b"
    );
}

#[test]
fn missing_buffer_notice_does_not_remove_a_buffer_created_by_the_handler() {
    assert_eq!(
        run_php(
            r#"<?php
$hits = 0;
set_error_handler(function ($severity, $message) use (&$hits) {
    $hits++;
    ob_start();
    echo 'new-buffer';
    return true;
});
$result = ob_end_clean();
$level = ob_get_level();
$contents = ob_get_clean();
echo $result ? 'T' : 'F', '|', $hits, '|', $level, '|', $contents, '|', ob_get_level();
"#,
        ),
        "F|1|1|new-buffer|0"
    );
}

#[test]
fn second_getter_notice_uses_the_current_handler_but_not_its_contents() {
    for (operation, action) in [("ob_get_clean", "discard"), ("ob_get_flush", "send")] {
        let source = format!(
            r#"<?php
$notes = [];
function original_buffer($bytes) {{ return $bytes; }}
function replacement_buffer($bytes) {{ return $bytes; }}
ob_start('original_buffer', 0, 0); echo 'seed';
set_error_handler(function ($severity, $message) use (&$notes) {{
    $notes[] = $severity . ':' . $message;
    if (count($notes) === 1) {{ ob_start('replacement_buffer'); echo 'replacement'; }}
    return true;
}});
$saved = {operation}();
$level = ob_get_level(); $top = ob_get_contents();
restore_error_handler();
ob_end_clean();
$live = ob_get_contents();
echo '|', $saved, '|', $level, '|', $top, '|', $live, '|', implode('|', $notes);
"#
        );
        assert_eq!(
            run_php(&source),
            format!(
                "seed|seed|2|replacement|seed|8:{operation}(): Failed to {action} buffer of original_buffer (0)|8:{operation}(): Failed to delete buffer of replacement_buffer (1)"
            ),
            "{operation}"
        );
    }
}

#[test]
fn denial_notices_preserve_handler_spelling_and_zero_based_nested_level() {
    for (handler, display) in [
        ("'MiXeD_Output'", "MiXeD_Output"),
        ("'mixed_output'", "mixed_output"),
        ("['OutputOwner', 'Write']", "OutputOwner::Write"),
        ("[new OutputOwner, 'write']", "OutputOwner::write"),
        ("new InvokableOutput", "InvokableOutput::__invoke"),
    ] {
        let source = format!(
            r#"<?php
$notes = [];
function MiXeD_Output($bytes) {{ return $bytes; }}
class OutputOwner {{ public static function Write($bytes) {{ return $bytes; }} }}
class InvokableOutput {{ public function __invoke($bytes) {{ return $bytes; }} }}
ob_start(); ob_start({handler}, 0, 0);
set_error_handler(function ($level, $message) use (&$notes) {{ $notes[] = $level . ':' . $message; return true; }});
ob_clean(); ob_flush(); ob_end_clean(); ob_end_flush();
echo implode('|', $notes);
"#
        );
        let expected = [
            ("ob_clean", "delete"),
            ("ob_flush", "flush"),
            ("ob_end_clean", "discard"),
            ("ob_end_flush", "send"),
        ]
        .map(|(operation, action)| {
            format!("8:{operation}(): Failed to {action} buffer of {display} (1)")
        })
        .join("|");
        assert_eq!(run_php(&source), expected, "{handler}");
    }
}

#[test]
fn display_callbacks_cannot_mutate_the_output_buffer_stack() {
    for operation in [
        "ob_start",
        "ob_clean",
        "ob_flush",
        "ob_end_clean",
        "ob_end_flush",
        "ob_get_clean",
        "ob_get_flush",
    ] {
        let source = format!(
            "<?php\nob_start(function ($bytes) {{\n    try {{ {operation}(); }} catch (Throwable $error) {{ echo 'must-not-catch'; }}\n    return $bytes;\n}});\necho 'payload';\nob_end_flush();\necho 'must-not-run';"
        );
        let error = run_php_expect_error_with_source_context(
            &source,
            "/spec/output-permissions.php",
            "/spec",
        );
        assert!(
            matches!(error, rphp::vm::execute::VmError::Fatal(ref message)
                if message == &format!("{operation}(): Cannot use output buffering in output buffering display handlers in /spec/output-permissions.php on line 3")),
            "{operation}: {error}"
        );
    }
}

#[test]
fn permitted_clean_and_final_flush_preserve_the_callback_phases() {
    assert_eq!(
        run_php(
            r#"<?php
function label_buffer($bytes, $phase) { return '[' . $phase . ':' . $bytes . ']'; }
ob_start('label_buffer');
echo 'discard';
ob_clean();
echo 'keep';
$snapshot = ob_get_flush();
echo '|', $snapshot, '|', ob_get_level();
"#,
        ),
        "[8:keep]|keep|0"
    );
}
