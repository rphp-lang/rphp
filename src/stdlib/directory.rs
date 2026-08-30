//! Directory-stream functions shared by the baseline filesystem surface.
//!
//! PHP exposes directory handles as resources of type `stream`, but validates
//! them separately from ordinary file streams. The request resource registry
//! retains that concrete backend distinction without changing `Value` layout.

use std::io;
use std::path::{Path, PathBuf};

use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

const LAST_DIRECTORY_RESOURCE: &str = "\0rphp-last-directory-resource";
const INITIAL_WORKING_DIRECTORY: &str = "\0rphp-initial-working-directory";

struct DirectoryStream {
    path: PathBuf,
    entries: std::fs::ReadDir,
    synthetic_entry: u8,
}

impl DirectoryStream {
    fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            entries: std::fs::read_dir(path)?,
            synthetic_entry: 0,
        })
    }

    fn next_entry(&mut self) -> io::Result<Option<String>> {
        let synthetic = match self.synthetic_entry {
            0 => Some("."),
            1 => Some(".."),
            _ => None,
        };
        if let Some(entry) = synthetic {
            self.synthetic_entry += 1;
            return Ok(Some(entry.to_string()));
        }
        match self.entries.next() {
            Some(Ok(entry)) => Ok(Some(os_filename_to_php_string(&entry.file_name()))),
            Some(Err(error)) => Err(error),
            None => Ok(None),
        }
    }

    fn rewind(&mut self) -> io::Result<()> {
        self.entries = std::fs::read_dir(&self.path)?;
        self.synthetic_entry = 0;
        Ok(())
    }
}

#[cfg(unix)]
fn os_filename_to_php_string(name: &std::ffi::OsStr) -> String {
    use std::os::unix::ffi::OsStrExt;
    super::bytes_to_php_string(name.as_bytes())
}

#[cfg(not(unix))]
fn os_filename_to_php_string(name: &std::ffi::OsStr) -> String {
    name.to_string_lossy().into_owned()
}

fn io_message(error: &io::Error) -> String {
    let message = error.to_string();
    message
        .rsplit_once(" (os error ")
        .map_or(message.as_str(), |(message, _)| message)
        .to_string()
}

fn io_errno(error: &io::Error) -> i32 {
    error.raw_os_error().unwrap_or(0)
}

fn remember_initial_cwd(eg: &mut ExecutorGlobals) {
    if eg
        .constant_table
        .borrow()
        .contains_key(INITIAL_WORKING_DIRECTORY)
    {
        return;
    }
    let Ok(initial) = std::env::current_dir() else {
        return;
    };
    eg.constant_table.borrow_mut().insert(
        INITIAL_WORKING_DIRECTORY.into(),
        Value::string(initial.to_string_lossy().into_owned()),
    );
}

pub(super) fn restore_initial_cwd(eg: &ExecutorGlobals) {
    let initial = eg
        .constant_table
        .borrow()
        .get(INITIAL_WORKING_DIRECTORY)
        .and_then(Value::as_str)
        .map(str::to_string);
    if let Some(initial) = initial {
        let _ = std::env::set_current_dir(initial);
    }
}

fn validate_context(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    position: usize,
) -> bool {
    let Some(context) = arg_opt!(ed, index) else {
        return true;
    };
    let context = context.dereferenced();
    if context.value_type() == ValueType::Null {
        return true;
    }
    let Some(id) = context.as_resource_id() else {
        super::typed_internal_argument_error(
            eg,
            function,
            context,
            position,
            "context",
            "resource or null",
        );
        return false;
    };
    if super::resource::type_for_request(eg, id) != "stream-context" {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!("{function}(): supplied resource is not a valid Stream-Context resource"),
        ));
        return false;
    }
    true
}

#[cfg(feature = "resource-lifetime")]
fn insert_directory(eg: &mut ExecutorGlobals, stream: DirectoryStream) -> Value {
    super::resource::insert_value_for_request(eg, "stream", stream)
}

#[cfg(not(feature = "resource-lifetime"))]
fn insert_directory(eg: &mut ExecutorGlobals, stream: DirectoryStream) -> Value {
    Value::resource(super::resource::insert_for_request(eg, "stream", stream))
}

fn remember_last_directory(eg: &mut ExecutorGlobals, value: &Value) {
    #[cfg(feature = "resource-lifetime")]
    let retained = value.clone();
    #[cfg(not(feature = "resource-lifetime"))]
    let retained = Value::long(value.as_resource_id().unwrap());
    eg.constant_table
        .borrow_mut()
        .insert(LAST_DIRECTORY_RESOURCE.into(), retained);
}

fn last_directory_id(eg: &ExecutorGlobals) -> Option<i64> {
    let constants = eg.constant_table.borrow();
    let value = constants.get(LAST_DIRECTORY_RESOURCE)?;
    value.as_resource_id().or_else(|| value.as_long())
}

fn clear_last_directory_if(eg: &mut ExecutorGlobals, id: i64) {
    if last_directory_id(eg) == Some(id) {
        eg.constant_table
            .borrow_mut()
            .remove(LAST_DIRECTORY_RESOURCE);
    }
}

fn directory_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Result<Option<i64>, VmError> {
    let supplied = arg_opt!(ed, 0);
    let id = match supplied {
        None => {
            super::report_internal_deprecation(
                eg,
                ed,
                &format!(
                    "{function}(): Passing null is deprecated, instead the last opened directory stream should be provided"
                ),
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            last_directory_id(eg)
        }
        Some(value) if value.dereferenced().value_type() == ValueType::Null => {
            super::report_internal_deprecation(
                eg,
                ed,
                &format!(
                    "{function}(): Passing null is deprecated, instead the last opened directory stream should be provided"
                ),
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            last_directory_id(eg)
        }
        Some(value) => {
            let value = value.dereferenced();
            let Some(id) = value.as_resource_id() else {
                super::typed_internal_argument_error(
                    eg,
                    function,
                    value,
                    1,
                    "dir_handle",
                    "resource or null",
                );
                return Ok(None);
            };
            Some(id)
        }
    };

    let Some(id) = id else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            "No resource supplied",
        ));
        return Ok(None);
    };
    if !super::resource::is_open_for_request(eg, id) {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!("{function}(): Argument #1 ($dir_handle) must be an open stream resource"),
        ));
        return Ok(None);
    }
    let is_directory =
        super::resource::with_request_payload_mut::<DirectoryStream, _>(eg, id, |_| ()).is_some();
    #[cfg(feature = "stream-registry")]
    let is_directory = is_directory || super::streams::user_wrapper::is_user_directory(eg, id);
    if !is_directory {
        let message = if super::resource::type_for_request(eg, id) == "stream" {
            format!("{function}(): Argument #1 ($dir_handle) must be a valid Directory resource")
        } else {
            format!("{function}(): Argument #1 ($dir_handle) must be an open stream resource")
        };
        eg.exception = Some(crate::value::make_error_value("TypeError", &message));
        return Ok(None);
    }
    Ok(Some(id))
}

pub(super) fn fn_chdir(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(directory) = super::typed_internal_string_argument(ed, eg, "chdir", 0, "directory")?
    else {
        return Ok(());
    };
    remember_initial_cwd(eg);
    match std::env::set_current_dir(&directory) {
        Ok(()) => ret!(rv, Value::bool(true)),
        Err(error) => {
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!(
                    "chdir(): {} (errno {})",
                    io_message(&error),
                    io_errno(&error)
                ),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ret!(rv, Value::bool(false));
        }
    }
}

pub(super) fn fn_opendir(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(directory) = super::typed_internal_string_argument(ed, eg, "opendir", 0, "directory")?
    else {
        return Ok(());
    };
    if !validate_context(ed, eg, "opendir", 1, 2) {
        return Ok(());
    }
    if directory.is_empty() {
        ret!(rv, Value::bool(false));
    }
    #[cfg(feature = "stream-registry")]
    match super::streams::user_wrapper::open_directory(eg, &directory, 0)? {
        super::streams::user_wrapper::OpenResult::Opened(value) => {
            remember_last_directory(eg, &value);
            ret!(rv, value);
        }
        super::streams::user_wrapper::OpenResult::Declined { class } => {
            if eg.exception.is_some() {
                return Ok(());
            }
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!(
                    "opendir({directory}): Failed to open directory: \"{class}::dir_opendir\" call failed"
                ),
            )?;
            ret!(rv, Value::bool(false));
        }
        super::streams::user_wrapper::OpenResult::NotRegistered => {}
    }
    match DirectoryStream::open(Path::new(&directory)) {
        Ok(stream) => {
            let value = insert_directory(eg, stream);
            remember_last_directory(eg, &value);
            ret!(rv, value);
        }
        Err(error) => {
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!(
                    "opendir({directory}): Failed to open directory: {}",
                    io_message(&error)
                ),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ret!(rv, Value::bool(false));
        }
    }
}

pub(super) fn fn_readdir(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(id) = directory_argument(ed, eg, "readdir")? else {
        return Ok(());
    };
    #[cfg(feature = "stream-registry")]
    if let Some(entry) = super::streams::user_wrapper::directory_read(eg, id)? {
        match entry {
            Some(entry) => ret!(rv, Value::string(entry)),
            None => ret!(rv, Value::bool(false)),
        }
    }
    let result = super::resource::with_request_payload_mut::<DirectoryStream, _>(
        eg,
        id,
        DirectoryStream::next_entry,
    )
    .expect("validated directory resource remains open during readdir");
    match result {
        Ok(Some(entry)) => ret!(rv, Value::string(entry)),
        Ok(None) | Err(_) => ret!(rv, Value::bool(false)),
    }
}

pub(super) fn fn_rewinddir(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(id) = directory_argument(ed, eg, "rewinddir")? else {
        return Ok(());
    };
    #[cfg(feature = "stream-registry")]
    if super::streams::user_wrapper::directory_rewind(eg, id)?.is_some() {
        ret!(rv, Value::null());
    }
    let _ = super::resource::with_request_payload_mut::<DirectoryStream, _>(
        eg,
        id,
        DirectoryStream::rewind,
    );
    ret!(rv, Value::null());
}

pub(super) fn fn_closedir(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(id) = directory_argument(ed, eg, "closedir")? else {
        return Ok(());
    };
    #[cfg(feature = "stream-registry")]
    if super::streams::user_wrapper::is_user_directory(eg, id) {
        let _ = super::streams::user_wrapper::close(eg, id)?;
        clear_last_directory_if(eg, id);
        ret!(rv, Value::null());
    }
    let closed = super::resource::close_for_request::<DirectoryStream>(eg, id);
    debug_assert!(
        closed,
        "validated directory resource must close exactly once"
    );
    clear_last_directory_if(eg, id);
    ret!(rv, Value::null());
}

pub(super) fn fn_scandir(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(directory) = super::typed_internal_string_argument(ed, eg, "scandir", 0, "directory")?
    else {
        return Ok(());
    };
    let sorting_order = if arg_opt!(ed, 1).is_some() {
        let Some(order) =
            super::typed_internal_int_argument(ed, eg, "scandir", 1, "sorting_order")?
        else {
            return Ok(());
        };
        order
    } else {
        0
    };
    if !validate_context(ed, eg, "scandir", 2, 3) {
        return Ok(());
    }
    if directory.is_empty() {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "scandir(): Argument #1 ($directory) must not be empty",
        ));
        return Ok(());
    }

    let mut stream = match DirectoryStream::open(Path::new(&directory)) {
        Ok(stream) => stream,
        Err(error) => {
            let message = io_message(&error);
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("scandir({directory}): Failed to open directory: {message}"),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("scandir(): (errno {}): {message}", io_errno(&error)),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ret!(rv, Value::bool(false));
        }
    };

    let mut entries = Vec::new();
    while let Some(entry) = stream.next_entry().unwrap_or(None) {
        entries.push(entry);
    }
    match sorting_order {
        2 => {}
        0 => entries.sort(),
        _ => entries.sort_by(|left, right| right.cmp(left)),
    }
    let mut result = PhpArray::new();
    for entry in entries {
        result.push(Value::string(entry));
    }
    ret!(rv, Value::array(result));
}
