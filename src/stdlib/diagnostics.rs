//! Cold request diagnostic publication shared by internal and VM callers.

use super::{ExecuteData, ExecutorGlobals, PhpArray, Value, VmError};
use std::{borrow::Cow, io::Write};

#[derive(Clone, Copy, Default)]
pub(crate) struct MessageContext<'a> {
    pub(crate) binary: bool,
    pub(crate) function: Option<&'a str>,
}

pub(super) fn message_value(message: &str, binary: bool) -> Value {
    if binary {
        Value::binary_string(&crate::value::php_byte_string_bytes(message))
    } else {
        Value::string(message)
    }
}

pub(super) fn ini_default(name: &str) -> Option<&'static str> {
    match name {
        "display_errors" | "display_startup_errors" | "fatal_error_backtraces" => Some("1"),
        "log_errors" | "ignore_repeated_errors" | "ignore_repeated_source" | "html_errors" => {
            Some("0")
        }
        "docref_root" | "docref_ext" | "error_log" => Some(""),
        _ => None,
    }
}

fn setting<'a>(eg: &'a ExecutorGlobals, name: &str) -> &'a str {
    eg.ini_overrides
        .as_deref()
        .and_then(|settings| settings.get(name))
        .map_or_else(|| ini_default(name).unwrap_or(""), String::as_str)
}

fn enabled(eg: &ExecutorGlobals, name: &str) -> bool {
    super::gc_ini_boolean(setting(eg, name))
}

/// Snapshot the still-live caller chain before an included/evaluated source
/// fails compilation. There is no callee activation to unwind or inspect yet.
#[cold]
pub(crate) fn compile_fatal(
    eg: &mut ExecutorGlobals,
    error: crate::compiler::compile::CompileFailure,
    caller: Option<*mut ExecuteData>,
) -> VmError {
    if let Some(diagnostic) = error.diagnostic {
        eg.record_last_error(64, &diagnostic.message, &diagnostic.file, diagnostic.line);
        return recorded_compile_fatal(eg, error.message, caller);
    }
    VmError::CompileFatal(error.message)
}

#[cold]
pub(crate) fn recorded_compile_fatal(
    eg: &mut ExecutorGlobals,
    mut rendered: String,
    caller: Option<*mut ExecuteData>,
) -> VmError {
    let location_end = rendered.len();
    if eg
        .last_error
        .as_ref()
        .is_some_and(|error| error.level == 64)
    {
        if enabled(eg, "fatal_error_backtraces") && caller.is_some() {
            let mut trace = PhpArray::new();
            let options = if super::ini_default(eg, "zend.exception_ignore_args")
                .as_deref()
                .is_some_and(super::ini_boolean)
            {
                2
            } else {
                0
            };
            let caller_trace = super::collect_live_debug_backtrace(
                caller.expect("live compilation caller"),
                options,
                0,
                eg,
                true,
            );
            for (_, entry) in caller_trace.iter() {
                trace.push(entry.clone());
            }
            rendered.push_str("\nStack trace:\n");
            rendered.push_str(&crate::vm::trace::format_throwable_trace(
                &trace,
                super::exception_string_param_max_len(eg),
                eg,
            ));
            eg.last_error
                .as_mut()
                .expect("recorded compile fatal")
                .trace = Some(Box::new(Value::array(trace)));
        }
    }
    if let Some(error) = eg.last_error.as_ref() {
        let (file, line) = (error.file.clone(), error.line);
        remember_terminal(eg, &rendered, &file, line, false, location_end);
    }
    VmError::CompileFatal(rendered)
}

pub(crate) fn remember_terminal(
    eg: &mut ExecutorGlobals,
    rendered: &str,
    file: &str,
    line: usize,
    binary: bool,
    location_end: usize,
) {
    eg.terminal_diagnostic = Some(Box::new(crate::runtime::PhpTerminalDiagnostic {
        rendered: rendered.to_string(),
        location_end,
        file: file.to_string(),
        line,
        binary,
        report: eg.error_reporting & eg.last_error.as_ref().map_or(1, |error| error.level) != 0,
    }));
}

fn message_bytes(message: &str, binary: bool) -> Cow<'_, [u8]> {
    if binary {
        Cow::Owned(crate::value::php_byte_string_bytes(message))
    } else {
        Cow::Borrowed(message.as_bytes())
    }
}

fn envelope(prefix: &str, body: &[u8], suffix: &str) -> Vec<u8> {
    let mut result = Vec::with_capacity(prefix.len() + body.len() + suffix.len());
    result.extend_from_slice(prefix.as_bytes());
    result.extend_from_slice(body);
    result.extend_from_slice(suffix.as_bytes());
    result
}

fn stderr(bytes: &[u8]) {
    let mut sink = std::io::stderr().lock();
    let _ = sink.write_all(bytes);
    let _ = sink.flush();
}

#[cold]
fn log(eg: &ExecutorGlobals, bytes: &[u8]) {
    let path = setting(eg, "error_log");
    if !path.is_empty()
        && path != "syslog"
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
    {
        let timestamp = super::date::current_timestamp();
        let (zone, _, offset) = super::date::timezone_spec(eg, timestamp);
        let (year, month, day, hour, minute, second, _, _) =
            super::unix_to_parts(timestamp.saturating_add(offset));
        let months = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let prefix = format!(
            "[{day:02}-{}-{year:04} {hour:02}:{minute:02}:{second:02} {zone}] ",
            months[(month - 1) as usize]
        );
        if file.write_all(&envelope(&prefix, bytes, "")).is_ok() {
            return;
        }
    }
    stderr(bytes);
}

/// Final CLI publication does not dispatch a recoverable error handler or
/// re-enter user buffers. Runtime INI changes apply just like startup values.
#[cold]
pub fn publish_cli_fatal(eg: &ExecutorGlobals, label: &str, message: &str) {
    let terminal = eg
        .terminal_diagnostic
        .as_deref()
        .filter(|error| error.rendered == message);
    let binary = terminal.is_some_and(|error| error.binary);
    let bytes = message_bytes(message, binary);
    eg.flush_output();
    if !terminal.map_or_else(
        || eg.error_reporting & if label == "Parse error" { 4 } else { 1 } != 0,
        |error| error.report,
    ) {
        return;
    }
    if enabled(eg, "log_errors") {
        log(eg, &envelope(&format!("PHP {label}:  "), &bytes, "\n"));
    }
    let to_stderr = setting(eg, "display_errors").eq_ignore_ascii_case("stderr");
    if !to_stderr && !enabled(eg, "display_errors") {
        return;
    }
    let explicit_display = eg
        .ini_overrides
        .as_deref()
        .is_some_and(|settings| settings.contains_key("display_errors"));
    if to_stderr {
        stderr(&envelope(&format!("{label}: "), &bytes, "\n"));
    } else if enabled(eg, "html_errors") {
        let (headline, trace) =
            terminal.map_or((message, ""), |error| message.split_at(error.location_end));
        let body = if let Some(error) = terminal {
            let file = if binary {
                crate::value::php_byte_string_from_bytes(error.file.bytes())
            } else {
                error.file.clone()
            };
            let suffix = format!(" in {file} on line {}", error.line);
            if let Some(prefix) = headline.strip_suffix(&suffix) {
                let mut body =
                    super::strings::escape_diagnostic_html(eg, &message_bytes(prefix, binary));
                body.extend_from_slice(b" in <b>");
                body.extend_from_slice(&super::strings::escape_diagnostic_html(
                    eg,
                    error.file.as_bytes(),
                ));
                body.extend_from_slice(format!("</b> on line <b>{}</b>", error.line).as_bytes());
                body
            } else {
                super::strings::escape_diagnostic_html(eg, &message_bytes(headline, binary))
            }
        } else {
            super::strings::escape_diagnostic_html(eg, &bytes)
        };
        eg.write_fatal_output(&envelope(
            &format!("<br />\n<b>{label}</b>:  "),
            &body,
            "<br />\n",
        ));
        if !trace.is_empty() {
            eg.write_fatal_output(&envelope(
                "",
                &message_bytes(trace.trim_start_matches('\n'), binary),
                "\n",
            ));
        }
    } else if explicit_display {
        eg.write_fatal_output(&envelope(&format!("\n{label}: "), &bytes, "\n"));
    } else {
        // Keep the historical unconfigured CLI channel. A configured display
        // policy, including one set at runtime, always uses PHP's channel.
        stderr(&envelope(&format!("\n{label}: "), &bytes, "\n"));
    }
}

/// Suppression follows the last unhandled diagnostic, including masked ones;
/// a user handler is always dispatched before this policy is applied. PHP
/// compares the message and optionally the source, but not the error level.
#[cold]
pub(crate) fn publish(
    eg: &mut ExecutorGlobals,
    caller: Option<*mut ExecuteData>,
    level: i64,
    label: &str,
    message: &str,
    file: &str,
    line: usize,
    report_builtin: bool,
) -> Result<(), VmError> {
    publish_with_context(
        eg,
        caller,
        level,
        label,
        message,
        file,
        line,
        report_builtin,
        MessageContext::default(),
    )
}

#[cold]
pub(crate) fn publish_with_context(
    eg: &mut ExecutorGlobals,
    caller: Option<*mut ExecuteData>,
    level: i64,
    label: &str,
    message: &str,
    file: &str,
    line: usize,
    report_builtin: bool,
    context: MessageContext<'_>,
) -> Result<(), VmError> {
    if enabled(eg, "ignore_repeated_errors")
        && eg.last_error.as_ref().is_some_and(|previous| {
            (if previous.message_is_binary == context.binary {
                previous.message == message
            } else {
                message_bytes(&previous.message, previous.message_is_binary)
                    == message_bytes(message, context.binary)
            }) && (enabled(eg, "ignore_repeated_source")
                || (previous.file == file && previous.line == line))
        })
    {
        return Ok(());
    }
    eg.record_last_error(level, message, file, line);
    eg.last_error
        .as_mut()
        .expect("recorded diagnostic")
        .message_is_binary = context.binary;
    if !report_builtin {
        return Ok(());
    }
    let bytes = message_bytes(message, context.binary);
    if enabled(eg, "log_errors") {
        eg.flush_output();
        log(
            eg,
            &envelope(
                &format!("PHP {label}:  "),
                &bytes,
                &format!(" in {file} on line {line}\n"),
            ),
        );
    }
    if setting(eg, "display_errors").eq_ignore_ascii_case("stderr") {
        eg.flush_output();
        stderr(&envelope(
            &format!("{label}: "),
            &bytes,
            &format!(" in {file} on line {line}\n"),
        ));
        return Ok(());
    }
    if !enabled(eg, "display_errors") {
        return Ok(());
    }
    let diagnostic = if enabled(eg, "html_errors") {
        let message = html_message(eg, level, message, context);
        envelope(
            &format!("<br />\n<b>{label}</b>:  "),
            &message,
            &format!(" in <b>{file}</b> on line <b>{line}</b><br />\n"),
        )
    } else {
        envelope(
            &format!("\n{label}: "),
            &bytes,
            &format!(" in {file} on line {line}\n"),
        )
    };
    if caller.is_none() {
        // Startup compilation has no active PHP call or output callback.
        eg.write_output(&diagnostic);
        Ok(())
    } else {
        super::write_php_output(eg, &diagnostic, caller)
    }
}

fn html_message(
    eg: &ExecutorGlobals,
    level: i64,
    message: &str,
    context: MessageContext<'_>,
) -> Vec<u8> {
    // User diagnostics deliberately contain raw, trusted HTML, including raw
    // invalid bytes. Only engine-generated messages repair and escape text.
    if matches!(level, 256 | 512 | 1024 | 16384) {
        return message_bytes(message, context.binary).into_owned();
    }
    let Some(function) = context.function else {
        return super::strings::escape_diagnostic_html(eg, &message_bytes(message, context.binary));
    };
    let root = setting(eg, "docref_root");
    let Some(split) = message.rfind("): ").filter(|_| !root.is_empty()) else {
        return super::strings::escape_diagnostic_html(eg, &message_bytes(message, context.binary));
    };
    let (prefix, suffix) = message.split_at(split + 1);
    let doc = format!(
        "function.{}{}",
        function.to_ascii_lowercase().replace('_', "-"),
        setting(eg, "docref_ext")
    );
    let mut output =
        super::strings::escape_diagnostic_html(eg, &message_bytes(prefix, context.binary));
    output.extend_from_slice(b" [<a href='");
    output.extend_from_slice(&super::strings::escape_diagnostic_html(eg, root.as_bytes()));
    output.extend_from_slice(format!("{doc}'>{doc}</a>]").as_bytes());
    output.extend_from_slice(&super::strings::escape_diagnostic_html(
        eg,
        &message_bytes(suffix, context.binary),
    ));
    output
}
