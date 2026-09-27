//! PHP 8.5 command quoting and process execution helpers.
//!
//! These functions deliberately use the process environment, working directory,
//! standard input and standard error inherited by the RPHP CLI request. The
//! asynchronous process boundary projects child pipes through the same native
//! stream resources used by the rest of the standard library.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};

use crate::runtime::ExecutorGlobals;
use crate::value::{ArrayKey, PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::stream::PhpStream;
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

enum ProcessCommand {
    Shell(Vec<u8>),
    Argv(Vec<Vec<u8>>),
}

#[derive(Clone, Copy)]
enum PipeDescriptor {
    Stdin,
    Stdout,
    Stderr,
}

struct ProcessResource {
    child: Child,
    pipe_ids: Vec<i64>,
}

#[cfg(feature = "resource-lifetime")]
fn insert_process(eg: &mut ExecutorGlobals, process: ProcessResource) -> Value {
    super::resource::insert_value_for_request(eg, "process", process)
}

#[cfg(not(feature = "resource-lifetime"))]
fn insert_process(eg: &mut ExecutorGlobals, process: ProcessResource) -> Value {
    Value::resource(super::resource::insert_for_request(eg, "process", process))
}

#[cfg(feature = "resource-lifetime")]
fn insert_process_pipe(eg: &mut ExecutorGlobals, stream: PhpStream) -> Value {
    super::resource::insert_value_for_request(eg, "stream", stream)
}

#[cfg(not(feature = "resource-lifetime"))]
fn insert_process_pipe(eg: &mut ExecutorGlobals, stream: PhpStream) -> Value {
    Value::resource(super::resource::insert_for_request(eg, "stream", stream))
}

fn process_argument_bytes(value: &Value) -> Vec<u8> {
    let value = value.dereferenced();
    value.php_string_bytes().map_or_else(
        || value.echo_to_string().into_bytes(),
        |bytes| bytes.into_owned(),
    )
}

fn parse_process_command(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Result<Option<ProcessCommand>, VmError> {
    let Some(command) =
        super::typed_internal_array_or_string_argument(ed, eg, "proc_open", 0, "command")?
    else {
        return Ok(None);
    };
    if let Some(arguments) = command.as_array() {
        if arguments.is_empty() {
            value_error(eg, "proc_open(): Argument #1 ($command) must not be empty");
            return Ok(None);
        }
        let mut argv = Vec::with_capacity(arguments.len());
        for (index, argument) in arguments.values().enumerate() {
            let bytes = process_argument_bytes(argument);
            if bytes.contains(&0) {
                value_error(
                    eg,
                    &format!("Command array element {} contains a null byte", index + 1),
                );
                return Ok(None);
            }
            if index == 0 && bytes.is_empty() {
                value_error(eg, "First element must contain a non-empty program name");
                return Ok(None);
            }
            argv.push(bytes);
        }
        return Ok(Some(ProcessCommand::Argv(argv)));
    }

    let bytes = command.php_string_bytes().unwrap_or_default().into_owned();
    if bytes.is_empty() {
        value_error(eg, "proc_open(): Argument #1 ($command) must not be empty");
        return Ok(None);
    }
    if bytes.contains(&0) {
        value_error(
            eg,
            "proc_open(): Argument #1 ($command) must not contain any null bytes",
        );
        return Ok(None);
    }
    Ok(Some(ProcessCommand::Shell(bytes)))
}

fn descriptor_text(value: &Value) -> String {
    value.dereferenced().php_string_bytes().map_or_else(
        || value.echo_to_string(),
        |bytes| bytes_to_php_string(&bytes),
    )
}

fn parse_pipe_descriptors(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    specification: &PhpArray,
) -> Result<Option<Vec<(i64, PipeDescriptor)>>, VmError> {
    let mut descriptors = Vec::with_capacity(specification.len());
    for (key, value) in specification.iter() {
        let ArrayKey::Int(number) = key else {
            value_error(
                eg,
                "proc_open(): descriptor spec must be an integer indexed array",
            );
            return Ok(None);
        };
        let Some(spec) = value.dereferenced().as_array() else {
            super::typed_internal_argument_error(
                eg,
                "proc_open",
                value.dereferenced(),
                2,
                "descriptor_spec",
                "array",
            );
            return Ok(None);
        };
        let Some(kind) = spec.get_int(0) else {
            report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                "proc_open(): descriptor item must contain an element 0",
            )?;
            return Ok(None);
        };
        let kind = descriptor_text(kind);
        let Some(mode) = spec.get_int(1) else {
            report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                "proc_open(): descriptor item must contain an element 1",
            )?;
            return Ok(None);
        };
        let mode = descriptor_text(mode);
        let descriptor = match (number, kind.as_str(), mode.as_str()) {
            (0, "pipe", "r") => PipeDescriptor::Stdin,
            (1, "pipe", "w") => PipeDescriptor::Stdout,
            (2, "pipe", "w") => PipeDescriptor::Stderr,
            _ => {
                report_internal_diagnostic(
                    eg,
                    ed,
                    2,
                    "Warning",
                    &format!("proc_open(): {kind} is not a valid descriptor spec/mode"),
                )?;
                return Ok(None);
            }
        };
        descriptors.push((number, descriptor));
    }
    Ok(Some(descriptors))
}

fn optional_process_string(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    index: u32,
    parameter: &str,
) -> Result<Option<Option<Vec<u8>>>, VmError> {
    let Some(value) = arg_opt!(ed, index) else {
        return Ok(Some(None));
    };
    if value.dereferenced().value_type() == ValueType::Null {
        return Ok(Some(None));
    }
    let Some(value) = super::typed_internal_string_value_argument_expected(
        ed,
        eg,
        "proc_open",
        index,
        parameter,
        "?string",
    )?
    else {
        return Ok(None);
    };
    Ok(Some(Some(
        value.php_string_bytes().unwrap_or_default().into_owned(),
    )))
}

fn optional_process_array(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    index: u32,
    parameter: &str,
) -> Option<Option<PhpArray>> {
    let Some(value) = arg_opt!(ed, index) else {
        return Some(None);
    };
    let value = value.dereferenced();
    if value.value_type() == ValueType::Null {
        return Some(None);
    }
    let Some(array) = value.as_array() else {
        super::typed_internal_argument_error(
            eg,
            "proc_open",
            value,
            index as usize + 1,
            parameter,
            "?array",
        );
        return None;
    };
    Some(Some(array.clone()))
}

fn configure_process_environment(command: &mut Command, environment: &PhpArray) {
    command.env_clear();
    for (key, value) in environment.iter() {
        let value = value.dereferenced();
        if value.value_type() == ValueType::Null {
            continue;
        }
        let value = process_argument_bytes(value);
        match key {
            ArrayKey::String(key) if !key.contains('=') && !key.contains('\0') => {
                command.env(command_os_string(key.as_bytes()), command_os_string(&value));
            }
            ArrayKey::Int(_) => {
                if let Some(separator) = value.iter().position(|byte| *byte == b'=') {
                    let key = &value[..separator];
                    if !key.is_empty() && !key.contains(&0) {
                        command.env(
                            command_os_string(key),
                            command_os_string(&value[separator + 1..]),
                        );
                    }
                }
            }
            ArrayKey::String(_) => {}
        }
    }
}

fn new_process_command(command: ProcessCommand) -> Command {
    match command {
        ProcessCommand::Argv(mut argv) => {
            let mut command = Command::new(command_os_string(&argv.remove(0)));
            command.args(argv.iter().map(|argument| command_os_string(argument)));
            command
        }
        ProcessCommand::Shell(command) => {
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt as _;

                let mut child = Command::new("/bin/sh");
                child.arg0("sh");
                child.arg("-c").arg(command_os_string(&command));
                child
            }
            #[cfg(windows)]
            {
                let mut child = Command::new("cmd.exe");
                child
                    .arg("/d")
                    .arg("/s")
                    .arg("/c")
                    .arg(command_os_string(&command));
                child
            }
            #[cfg(not(any(unix, windows)))]
            {
                let mut child = Command::new("sh");
                child.arg("-c").arg(command_os_string(&command));
                child
            }
        }
    }
}

fn process_error_message(error: &std::io::Error) -> String {
    let rendered = error.to_string();
    rendered
        .split_once(" (os error")
        .map_or_else(|| rendered.clone(), |(message, _)| message.to_string())
}

pub(super) fn fn_proc_open(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(command) = parse_process_command(ed, eg)? else {
        return Ok(());
    };
    let descriptor_value = super::owned_argument(ed, 1);
    let Some(descriptor_spec) = descriptor_value.dereferenced().as_array() else {
        super::typed_internal_argument_error(
            eg,
            "proc_open",
            descriptor_value.dereferenced(),
            2,
            "descriptor_spec",
            "array",
        );
        return Ok(());
    };
    let Some(descriptors) = parse_pipe_descriptors(ed, eg, descriptor_spec)? else {
        ret!(rv, Value::bool(false));
    };
    let Some(cwd) = optional_process_string(ed, eg, 3, "cwd")? else {
        return Ok(());
    };
    let Some(environment) = optional_process_array(ed, eg, 4, "env_vars") else {
        return Ok(());
    };
    let Some(options) = optional_process_array(ed, eg, 5, "options") else {
        return Ok(());
    };
    let suppress_errors = options
        .as_ref()
        .and_then(|options| options.get_str("suppress_errors"))
        .is_some_and(Value::is_truthy);

    let mut child = new_process_command(command);
    if let Some(cwd) = cwd {
        child.current_dir(PathBuf::from(command_os_string(&cwd)));
    }
    if let Some(environment) = environment.as_ref() {
        configure_process_environment(&mut child, environment);
    }
    for (_, descriptor) in &descriptors {
        match descriptor {
            PipeDescriptor::Stdin => {
                child.stdin(Stdio::piped());
            }
            PipeDescriptor::Stdout => {
                child.stdout(Stdio::piped());
            }
            PipeDescriptor::Stderr => {
                child.stderr(Stdio::piped());
            }
        }
    }

    let mut child = match child.spawn() {
        Ok(child) => child,
        Err(error) => {
            if !suppress_errors {
                report_internal_diagnostic(
                    eg,
                    ed,
                    2,
                    "Warning",
                    &format!(
                        "proc_open(): posix_spawn() failed: {}",
                        process_error_message(&error)
                    ),
                )?;
            }
            ret!(rv, Value::bool(false));
        }
    };

    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let process = insert_process(
        eg,
        ProcessResource {
            child,
            pipe_ids: Vec::with_capacity(descriptors.len()),
        },
    );
    let process_id = process
        .as_resource_id()
        .expect("inserted process has a resource id");
    let mut pipes = PhpArray::with_hash_capacity(descriptors.len());
    let mut pipe_ids = Vec::with_capacity(descriptors.len());
    for (number, descriptor) in descriptors {
        let stream = match descriptor {
            PipeDescriptor::Stdin => {
                PhpStream::process_stdin(stdin.take().expect("configured child stdin pipe"))
            }
            PipeDescriptor::Stdout => {
                PhpStream::process_stdout(stdout.take().expect("configured child stdout pipe"))
            }
            PipeDescriptor::Stderr => {
                PhpStream::process_stderr(stderr.take().expect("configured child stderr pipe"))
            }
        };
        let pipe = insert_process_pipe(eg, stream);
        pipe_ids.push(
            pipe.as_resource_id()
                .expect("inserted process pipe has a resource id"),
        );
        pipes.set_int(number, pipe);
    }
    let _ = super::resource::with_request_payload_mut::<ProcessResource, _>(
        eg,
        process_id,
        |process| process.pipe_ids = pipe_ids,
    );
    arg_mut!(ed, 2, Value::array(pipes));
    ret!(rv, process);
}

pub(super) fn fn_proc_close(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let process = arg!(ed, 0).dereferenced();
    let Some(resource) = process.as_resource_id() else {
        super::typed_internal_argument_error(eg, "proc_close", process, 1, "process", "resource");
        return Ok(());
    };
    if !super::resource::is_open_for_request(eg, resource)
        || super::resource::type_for_request(eg, resource) != "process"
    {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            "proc_close(): supplied resource is not a valid process resource",
        ));
        return Ok(());
    }
    let pipe_ids =
        super::resource::with_request_payload_mut::<ProcessResource, _>(eg, resource, |process| {
            process.pipe_ids.clone()
        })
        .unwrap_or_default();
    for pipe in pipe_ids {
        let _ = super::resource::close_for_request::<PhpStream>(eg, pipe);
    }
    let status =
        super::resource::with_request_payload_mut::<ProcessResource, _>(eg, resource, |process| {
            process.child.wait()
        });
    let _ = super::resource::close_for_request::<ProcessResource>(eg, resource);
    match status {
        Some(Ok(status)) => ret!(rv, Value::long(exit_code(status))),
        _ => ret!(rv, Value::long(-1)),
    }
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

pub(super) fn fn_system(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(command) = validate_command(ed, eg, "system")? else {
        return Ok(());
    };
    let output = match execute_shell(&command) {
        Ok(output) => output,
        Err(_) => {
            process_failure(ed, eg, "system", &command)?;
            ret!(rv, Value::bool(false));
        }
    };
    eg.write_output(&output.stdout);
    if arg_opt!(ed, 1).is_some() {
        arg_mut!(ed, 1, Value::long(exit_code(output.status)));
    }
    let lines = split_exec_output(&output.stdout);
    let last_line = lines.last().map_or(&[][..], Vec::as_slice);
    ret!(rv, php_byte_result(last_line.to_vec(), false));
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
