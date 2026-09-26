// Startup files are separate global source units inside one request. They
// reuse compilation, registration and scope writeback from include/eval,
// but are not PHP include calls and must not acquire a synthetic call frame.

pub enum StartupSource<'a> {
    File(&'a mut std::fs::File),
    Stdin(&'a [u8]),
}

/// The caller owns the root descriptor through pending fatal shutdown, just
/// as it does for execute(). Startup units do not own that logical caller.
#[cold]
pub fn execute_startup_request(
    eg: &mut ExecutorGlobals,
    root: &UserFunction,
    mut primary: StartupSource<'_>,
) -> Result<Value, VmError> {
    use std::io::Read;
    let main_path = root.op_array.source_file.as_ref();
    let stdin_request = matches!(&primary, StartupSource::Stdin(_));
    let prepend = crate::stdlib::ini_default(eg, "auto_prepend_file").unwrap_or_default();
    let append = crate::stdlib::ini_default(eg, "auto_append_file").unwrap_or_default();
    let mut body = |eg: &mut ExecutorGlobals, frame: *mut ExecuteData| {
        for (requested, main) in [
            (prepend.as_str(), false),
            (main_path, true),
            (append.as_str(), false),
        ] {
            if requested.is_empty() {
                continue;
            }
            let (source, canonical) = if main {
                let bytes = match &mut primary {
                    StartupSource::Stdin(source) => source.to_vec(),
                    StartupSource::File(file) => {
                        let mut bytes = Vec::new();
                        if let Err(error) = file.read_to_end(&mut bytes) {
                            return match startup_source_open_error(eg, frame, requested, error)? {
                                Some(error) => Err(error),
                                None => Ok(()),
                            };
                        }
                        bytes
                    }
                };
                (
                    crate::lexer::decode_php_source(&bytes),
                    main_path.to_string(),
                )
            } else {
                let resolved = startup_source_path(eg, requested, main);
                let bytes = match std::fs::read(&resolved) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        return match startup_source_open_error(eg, frame, requested, error)? {
                            Some(error) => Err(error),
                            None => Ok(()),
                        };
                    }
                };
                let canonical = std::fs::canonicalize(&resolved)
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or(resolved);
                (crate::lexer::decode_php_source(&bytes), canonical)
            };
            let outcome = execute_source_unit_inner(
                eg,
                source,
                &canonical,
                canonical.clone(),
                Value::long(1),
                !(main && stdin_request),
                Some((frame, &root.op_array)),
                None,
                true,
            )?;
            if let IncludeFileOutcome::Thrown(exception) = outcome {
                eg.exception = Some(exception);
            }
            if eg.exception.is_some() {
                break;
            }
        }
        Ok(())
    };
    execute_request(eg, root, Some(&mut body))
}

#[cold]
fn startup_source_path(eg: &ExecutorGlobals, requested: &str, main: bool) -> String {
    #[cfg(feature = "include-path")]
    if !main {
        return crate::stdlib::include_path::resolve_for_open(eg, requested, true);
    }
    #[cfg(not(feature = "include-path"))]
    let _ = (eg, main);
    requested.to_string()
}

#[cold]
fn startup_source_open_error(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    requested: &str,
    error: std::io::Error,
) -> Result<Option<VmError>, VmError> {
    let reason = if error.kind() == std::io::ErrorKind::NotFound {
        "No such file or directory".to_string()
    } else {
        error.to_string()
    };
    let warning = format!("Unknown: Failed to open stream: {reason}");
    let handled = crate::stdlib::dispatch_php_error(eg, frame, 2, &warning, "Unknown", 0)?;
    if eg.exception.is_some() {
        crate::stdlib::dispatch_pending_uncaught_exception_handlers(eg, frame)?;
        if eg.exception.is_some() {
            return Ok(None);
        }
    }
    if !handled {
        crate::stdlib::diagnostics::publish(
            eg,
            Some(frame),
            2,
            "Warning",
            &warning,
            "Unknown",
            0,
            eg.error_reporting & 2 != 0,
        )?;
    }
    let include_path = crate::stdlib::ini_default(eg, "include_path").unwrap_or_default();
    let message = format!("Failed opening required '{requested}' (include_path='{include_path}')");
    eg.record_last_error(1, &message, "Unknown", 0);
    let rendered = format!("{message} in Unknown on line 0");
    crate::stdlib::diagnostics::remember_terminal(
        eg,
        &rendered,
        "Unknown",
        0,
        false,
        rendered.len(),
    );
    Ok(Some(VmError::Fatal(rendered)))
}

#[cold]
fn startup_source_parse_error(eg: &mut ExecutorGlobals, error: String, source: &str) -> VmError {
    // Preserve the parser's diagnostic verbatim; only attach request error
    // state before CLI publication and pending shutdown callbacks.
    if let Some((message, line)) = error.rsplit_once(&format!(" in {source} on line "))
        && let Ok(line) = line.parse::<usize>()
    {
        eg.record_last_error(4, message, source, line);
        crate::stdlib::diagnostics::remember_terminal(eg, &error, source, line, false, error.len());
    }
    VmError::Parse(error)
}
