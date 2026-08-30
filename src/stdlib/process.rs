//! PHP 8.5 Unix command quoting and synchronous shell execution helpers.
//!
//! These functions deliberately use the process environment, working directory,
//! standard input and standard error inherited by the RPHP CLI request. PHP's
//! process-control and streaming APIs remain separate contracts.

use std::ffi::OsString;
use std::process::{Command, ExitStatus, Stdio};

use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::{
    bytes_to_php_string, php_byte_result, php_string_to_bytes, report_internal_diagnostic,
    typed_internal_string_argument,
};

const SHELL_ESCAPE_MAX_LENGTH: usize = 2 * 1024 * 1024;

fn value_error(eg: &mut ExecutorGlobals, message: &str) {
    eg.exception = Some(crate::value::make_error_value("ValueError", message));
}

fn validate_shell_escape_input(
    eg: &mut ExecutorGlobals,
    function: &str,
    parameter: &str,
    input: &[u8],
    noun: &str,
) -> bool {
    if input.contains(&0) {
        value_error(
            eg,
            &format!("{function}(): Argument #1 (${parameter}) must not contain any null bytes"),
        );
        return false;
    }
    // PHP reserves room for the quotes/escape terminator before constructing
    // either result. Keep this check independent from actual output expansion.
    if input.len() > SHELL_ESCAPE_MAX_LENGTH - 3 {
        value_error(
            eg,
            &format!("{noun} exceeds the allowed length of {SHELL_ESCAPE_MAX_LENGTH} bytes"),
        );
        return false;
    }
    true
}

/// The currently admitted locale is C/POSIX. In that locale php-src's
/// multibyte scanner keeps ASCII bytes and discards bytes that do not begin a
/// valid locale character. Locale expansion can replace this predicate later.
#[inline]
fn portable_shell_byte(byte: u8) -> bool {
    byte < 0x80
}

fn escape_shell_arg_bytes(input: &[u8]) -> Result<Vec<u8>, ()> {
    let quote_count = input.iter().filter(|&&byte| byte == b'\'').count();
    let mut output = Vec::with_capacity(
        input
            .len()
            .saturating_add(quote_count.saturating_mul(3))
            .saturating_add(2),
    );
    output.push(b'\'');
    for &byte in input {
        if !portable_shell_byte(byte) {
            continue;
        }
        if byte == b'\'' {
            output.extend_from_slice(b"'\\''");
        } else {
            output.push(byte);
        }
        if output.len() > SHELL_ESCAPE_MAX_LENGTH + 1 {
            return Err(());
        }
    }
    output.push(b'\'');
    (output.len() <= SHELL_ESCAPE_MAX_LENGTH + 1)
        .then_some(output)
        .ok_or(())
}

#[inline]
fn shell_command_metacharacter(byte: u8) -> bool {
    matches!(
        byte,
        b'\n'
            | b'#'
            | b'$'
            | b'&'
            | b'('
            | b')'
            | b'*'
            | b';'
            | b'<'
            | b'>'
            | b'?'
            | b'['
            | b'\\'
            | b']'
            | b'^'
            | b'`'
            | b'{'
            | b'|'
            | b'}'
            | b'~'
    )
}

fn escape_shell_command_bytes(input: &[u8]) -> Result<Vec<u8>, ()> {
    let mut output = Vec::with_capacity(input.len());
    let mut active_quote = None;
    for (index, &byte) in input.iter().enumerate() {
        if !portable_shell_byte(byte) {
            continue;
        }
        let escape = if matches!(byte, b'\'' | b'"') {
            match active_quote {
                Some(quote) if quote == byte => {
                    active_quote = None;
                    false
                }
                Some(_) => true,
                None if input[index + 1..].contains(&byte) => {
                    active_quote = Some(byte);
                    false
                }
                None => true,
            }
        } else {
            shell_command_metacharacter(byte)
        };
        if escape {
            output.push(b'\\');
        }
        output.push(byte);
        if output.len() > SHELL_ESCAPE_MAX_LENGTH + 1 {
            return Err(());
        }
    }
    Ok(output)
}

pub(super) fn fn_escapeshellarg(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(argument) = typed_internal_string_argument(ed, eg, "escapeshellarg", 0, "arg")? else {
        return Ok(());
    };
    let input = php_string_to_bytes(&argument);
    if !validate_shell_escape_input(eg, "escapeshellarg", "arg", &input, "Argument") {
        return Ok(());
    }
    let Ok(output) = escape_shell_arg_bytes(&input) else {
        value_error(
            eg,
            &format!(
                "Escaped argument exceeds the allowed length of {SHELL_ESCAPE_MAX_LENGTH} bytes"
            ),
        );
        return Ok(());
    };
    ret!(rv, php_byte_result(output, false));
}

pub(super) fn fn_escapeshellcmd(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(command) = typed_internal_string_argument(ed, eg, "escapeshellcmd", 0, "command")?
    else {
        return Ok(());
    };
    let input = php_string_to_bytes(&command);
    if !validate_shell_escape_input(eg, "escapeshellcmd", "command", &input, "Command") {
        return Ok(());
    }
    let Ok(output) = escape_shell_command_bytes(&input) else {
        value_error(
            eg,
            &format!(
                "Escaped command exceeds the allowed length of {SHELL_ESCAPE_MAX_LENGTH} bytes"
            ),
        );
        return Ok(());
    };
    ret!(rv, php_byte_result(output, false));
}

fn validate_command(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Result<Option<Vec<u8>>, VmError> {
    let Some(command) = typed_internal_string_argument(ed, eg, function, 0, "command")? else {
        return Ok(None);
    };
    let command = php_string_to_bytes(&command);
    if command.is_empty() {
        value_error(
            eg,
            &format!("{function}(): Argument #1 ($command) must not be empty"),
        );
        return Ok(None);
    }
    if command.contains(&0) {
        value_error(
            eg,
            &format!("{function}(): Argument #1 ($command) must not contain any null bytes"),
        );
        return Ok(None);
    }
    Ok(Some(command))
}

struct ShellOutput {
    stdout: Vec<u8>,
    status: ExitStatus,
}

#[cfg(unix)]
fn command_os_string(command: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt as _;
    OsString::from_vec(command.to_vec())
}

#[cfg(not(unix))]
fn command_os_string(command: &[u8]) -> OsString {
    OsString::from(bytes_to_php_string(command))
}

fn execute_shell(command: &[u8]) -> std::io::Result<ShellOutput> {
    #[cfg(unix)]
    let mut child = {
        use std::os::unix::process::CommandExt as _;

        let mut child = Command::new("/bin/sh");
        child.arg0("sh");
        child.arg("-c").arg(command_os_string(command));
        child
    };
    #[cfg(windows)]
    let mut child = {
        let mut child = Command::new("cmd.exe");
        child
            .arg("/d")
            .arg("/s")
            .arg("/c")
            .arg(command_os_string(command));
        child
    };
    #[cfg(not(any(unix, windows)))]
    let mut child = {
        let mut child = Command::new("sh");
        child.arg("-c").arg(command_os_string(command));
        child
    };
    let output = child
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?
        .wait_with_output()?;
    Ok(ShellOutput {
        stdout: output.stdout,
        status: output.status,
    })
}

fn exit_code(status: ExitStatus) -> i64 {
    if let Some(code) = status.code() {
        return i64::from(code);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        return i64::from(status.signal().unwrap_or(0)) + 128;
    }
    #[cfg(not(unix))]
    -1
}

#[inline]
fn exec_trailing_whitespace(byte: u8) -> bool {
    matches!(byte, b'\t'..=b'\r' | b' ')
}

fn split_exec_output(stdout: &[u8]) -> Vec<Vec<u8>> {
    if stdout.is_empty() {
        return Vec::new();
    }
    let mut pieces: Vec<&[u8]> = stdout.split(|&byte| byte == b'\n').collect();
    if stdout.ends_with(b"\n") {
        pieces.pop();
    }
    pieces
        .into_iter()
        .map(|line| {
            let end = line
                .iter()
                .rposition(|&byte| !exec_trailing_whitespace(byte))
                .map_or(0, |index| index + 1);
            line[..end].to_vec()
        })
        .collect()
}

fn process_failure(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    command: &[u8],
) -> Result<(), VmError> {
    report_internal_diagnostic(
        eg,
        ed,
        2,
        "Warning",
        &format!(
            "{function}(): Unable to execute {}",
            bytes_to_php_string(command)
        ),
    )?;
    Ok(())
}

pub(super) fn fn_exec(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(command) = validate_command(ed, eg, "exec")? else {
        return Ok(());
    };
    let output = match execute_shell(&command) {
        Ok(output) => output,
        Err(_) => {
            process_failure(ed, eg, "exec", &command)?;
            ret!(rv, Value::bool(false));
        }
    };
    let lines = split_exec_output(&output.stdout);

    if arg_opt!(ed, 1).is_some() {
        let output_pointer = arg_mut!(ed, 1);
        // SAFETY: Internal handlers run synchronously with a live execute-data
        // frame. `arg_mut!` follows an optional reference and returns the
        // unique writable destination for this by-reference parameter.
        let output_value = unsafe { &mut *output_pointer };
        if output_value.as_array().is_none() {
            *output_value = Value::array(PhpArray::new());
        }
        let output_array = output_value
            .as_array_mut()
            .expect("exec output was normalized to an array");
        for line in &lines {
            output_array.push(php_byte_result(line.clone(), false));
        }
    }
    if arg_opt!(ed, 2).is_some() {
        arg_mut!(ed, 2, Value::long(exit_code(output.status)));
    }

    let last_line = lines.last().map_or(&[][..], Vec::as_slice);
    ret!(rv, php_byte_result(last_line.to_vec(), false));
}

pub(super) fn fn_shell_exec(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(command) = validate_command(ed, eg, "shell_exec")? else {
        return Ok(());
    };
    let output = match execute_shell(&command) {
        Ok(output) => output,
        Err(_) => {
            process_failure(ed, eg, "shell_exec", &command)?;
            ret!(rv, Value::bool(false));
        }
    };
    if output.stdout.is_empty() {
        ret!(rv, Value::null());
    }
    ret!(rv, php_byte_result(output.stdout, false));
}

#[cfg(test)]
mod tests {
    use super::{escape_shell_arg_bytes, escape_shell_command_bytes, split_exec_output};

    #[test]
    fn unix_argument_escaping_quotes_and_filters_the_portable_c_locale() {
        assert_eq!(escape_shell_arg_bytes(b"").unwrap(), b"''");
        assert_eq!(
            escape_shell_arg_bytes(b"Mr O'Neil").unwrap(),
            b"'Mr O'\\''Neil'"
        );
        assert_eq!(escape_shell_arg_bytes(b"\x7f\x80\xff").unwrap(), b"'\x7f'");
    }

    #[test]
    fn unix_command_escaping_tracks_one_paired_quote_region() {
        assert_eq!(
            escape_shell_command_bytes(b"\"$`\\&;|*?~<>^()[]{}").unwrap(),
            b"\\\"\\$\\`\\\\\\&\\;\\|\\*\\?\\~\\<\\>\\^\\(\\)\\[\\]\\{\\}"
        );
        assert_eq!(escape_shell_command_bytes(b"'a&b'").unwrap(), b"'a\\&b'");
        assert_eq!(escape_shell_command_bytes(b"'''").unwrap(), b"''\\'");
        assert_eq!(
            escape_shell_command_bytes(b"\"''\"").unwrap(),
            b"\"\\'\\'\""
        );
    }

    #[test]
    fn exec_lines_drop_one_terminal_split_and_c_whitespace_only() {
        assert_eq!(
            split_exec_output(b" a  \n\nlast\t \n"),
            vec![b" a".to_vec(), Vec::new(), b"last".to_vec()]
        );
        assert_eq!(split_exec_output(b"a\0 \n"), vec![b"a\0".to_vec()]);
        assert!(split_exec_output(b"").is_empty());
    }
}
