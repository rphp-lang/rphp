//! Process, configuration and terminal introspection of the CLI request:
//! `get_cfg_var()`, `php_ini_loaded_file()`, memory and load reports,
//! `getmypid()`, `gethostname()`, `uniqid()`, `stream_isatty()`,
//! `is_countable()` and `fnmatch()`.

use std::cell::RefCell;

use crate::compiler::make_internal_function;
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType, make_error_value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{
    FunctionCommon, InternalFunction, InternalFunctionDeprecation, InternalFunctionHandler,
    ParamTypeHint,
};

pub(crate) const FNM_PATHNAME: i64 = 1;
pub(crate) const FNM_NOESCAPE: i64 = 2;
pub(crate) const FNM_PERIOD: i64 = 4;
pub(crate) const FNM_CASEFOLD: i64 = 16;

thread_local! {
    /// Configuration values supplied on the command line with `-d`, in the
    /// spelling PHP's `get_cfg_var()` reports them.
    static STARTUP_CONFIG: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    static ENTROPY: RefCell<u64> = const { RefCell::new(0) };
}

static LCG_VALUE_DEPRECATION: InternalFunctionDeprecation = InternalFunctionDeprecation {
    since: "8.4",
    message: "use \\Random\\Randomizer::getFloat() instead",
};

/// Record the process-level `-d` configuration for `get_cfg_var()`.
pub fn set_startup_config(settings: &[(String, String)]) {
    STARTUP_CONFIG.with(|config| *config.borrow_mut() = settings.to_vec());
}

fn argument(ed: *mut ExecuteData, index: u32) -> Value {
    crate::stdlib::owned_argument(ed, index)
}

fn optional_argument(ed: *mut ExecuteData, index: u32) -> Option<Value> {
    let value = argument(ed, index);
    (value.value_type() != ValueType::Undef).then_some(value)
}

fn return_value(rv: *mut Value, value: Value) -> Result<(), VmError> {
    crate::stdlib::write_return_value(rv, value);
    Ok(())
}

fn fn_get_cfg_var(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(option) = super::typed_internal_string_argument(ed, eg, "get_cfg_var", 0, "option")?
    else {
        return Ok(());
    };
    let value = STARTUP_CONFIG.with(|config| {
        config
            .borrow()
            .iter()
            .rev()
            .find(|(name, _)| *name == option)
            .map(|(_, value)| value.clone())
    });
    return_value(rv, value.map_or_else(|| Value::bool(false), Value::string))
}

fn fn_php_ini_loaded_file(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // RPHP loads no php.ini; every setting comes from the command line.
    return_value(rv, Value::bool(false))
}

fn proc_self_field(file: &str, field: usize) -> Option<u64> {
    let text = std::fs::read_to_string(file).ok()?;
    text.split_whitespace().nth(field)?.parse().ok()
}

fn resident_bytes() -> i64 {
    proc_self_field("/proc/self/statm", 1).map_or(0, |pages| (pages * 4096) as i64)
}

fn peak_resident_bytes() -> i64 {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return resident_bytes();
    };
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kilobytes| kilobytes.parse::<i64>().ok())
        .map_or_else(resident_bytes, |kilobytes| kilobytes * 1024)
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn allocator_bytes(real_usage: bool) -> i64 {
    // SAFETY: mallinfo2 has no arguments and returns its snapshot by value.
    // The process allocator remains alive for the entire request.
    let info = unsafe { libc::mallinfo2() };
    let bytes = if real_usage {
        info.arena.saturating_add(info.hblkhd)
    } else {
        // Zend's request allocator reports committed usage in page-backed
        // chunks. glibc includes small-bin, arena and bookkeeping variations
        // in `uordblks`; project those implementation details back to Zend's
        // 64-KiB request chunk granularity.
        info.uordblks.saturating_add(info.hblkhd) / 65_536 * 65_536
    };
    i64::try_from(bytes).unwrap_or(i64::MAX)
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn allocator_bytes(_real_usage: bool) -> i64 {
    resident_bytes()
}

pub(crate) fn current_allocator_bytes() -> i64 {
    allocator_bytes(false)
}

fn memory_report(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    measure: fn() -> i64,
) -> Result<(), VmError> {
    if optional_argument(ed, 0).is_some()
        && super::typed_internal_bool_argument(ed, eg, function, 0, "real_usage")?.is_none()
    {
        return Ok(());
    }
    return_value(rv, Value::long(measure()))
}

fn fn_memory_get_usage(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let real_usage = if optional_argument(ed, 0).is_some() {
        let Some(value) =
            super::typed_internal_bool_argument(ed, eg, "memory_get_usage", 0, "real_usage")?
        else {
            return Ok(());
        };
        value
    } else {
        false
    };
    crate::value::prune_dead_cycle_root_storage();
    return_value(rv, Value::long(allocator_bytes(real_usage)))
}

fn fn_memory_get_peak_usage(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    memory_report(ed, rv, eg, "memory_get_peak_usage", peak_resident_bytes)
}

fn fn_memory_reset_peak_usage(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // The resident high-water mark belongs to the kernel; PHP's allocator
    // peak has no separate counterpart here.
    return_value(rv, Value::null())
}

fn fn_getmypid(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::long(i64::from(std::process::id())))
}

fn fn_sys_getloadavg(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let averages: Option<Vec<f64>> = std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|text| {
            text.split_whitespace()
                .take(3)
                .map(|field| field.parse::<f64>().ok())
                .collect::<Option<Vec<f64>>>()
        })
        .filter(|averages| averages.len() == 3);
    let Some(averages) = averages else {
        return return_value(rv, Value::bool(false));
    };
    let mut result = PhpArray::with_packed_capacity(3);
    for average in averages {
        result.push(Value::double(average));
    }
    return_value(rv, Value::array(result))
}

fn fn_gethostname(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let hostname = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|name| name.trim_end().to_string())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .filter(|name| !name.is_empty());
    return_value(
        rv,
        hostname.map_or_else(|| Value::bool(false), Value::string),
    )
}

/// A small request-independent generator for the entropy suffixes that PHP
/// draws from its combined linear congruential generator.
fn next_entropy() -> f64 {
    ENTROPY.with(|state| {
        let mut value = *state.borrow();
        if value == 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos() as u64);
            value = now ^ (u64::from(std::process::id()) << 32) | 1;
        }
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        *state.borrow_mut() = value;
        (value >> 11) as f64 / (1u64 << 53) as f64
    })
}

fn fn_lcg_value(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::double(next_entropy()))
}

fn fn_mt_getrandmax(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::long(i64::from(i32::MAX)))
}

fn fn_uniqid(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let prefix = match optional_argument(ed, 0) {
        Some(_) => {
            let Some(prefix) =
                super::typed_internal_string_argument(ed, eg, "uniqid", 0, "prefix")?
            else {
                return Ok(());
            };
            prefix
        }
        None => String::new(),
    };
    let more_entropy = match optional_argument(ed, 1) {
        Some(_) => {
            let Some(more) =
                super::typed_internal_bool_argument(ed, eg, "uniqid", 1, "more_entropy")?
            else {
                return Ok(());
            };
            more
        }
        None => false,
    };
    if !more_entropy {
        // Consecutive calls must differ; a microsecond of sleep separates them.
        std::thread::sleep(std::time::Duration::from_micros(1));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let mut id = format!("{prefix}{:08x}{:05x}", now.as_secs(), now.subsec_micros());
    if more_entropy {
        id.push_str(&format!("{:.8}", next_entropy() * 10.0));
    }
    return_value(rv, Value::string(id))
}

fn fn_is_countable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = argument(ed, 0);
    let value = value.dereferenced();
    let countable = match value.value_type() {
        ValueType::Array => true,
        ValueType::Object => value
            .as_object()
            .is_some_and(|object| eg.class_is_a(&object.class_name, "Countable")),
        _ => false,
    };
    return_value(rv, Value::bool(countable))
}

fn fn_stream_isatty(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let stream = argument(ed, 0);
    let stream = stream.dereferenced();
    let Some(id) = stream.as_resource_id() else {
        super::typed_internal_argument_error(eg, "stream_isatty", stream, 1, "stream", "resource");
        return Ok(());
    };
    let terminal = super::streams::with_stream(eg, id, |stream| stream.is_terminal());
    match terminal {
        Some(terminal) => return_value(rv, Value::bool(terminal)),
        None => {
            eg.exception = Some(make_error_value(
                "TypeError",
                "stream_isatty(): supplied resource is not a valid stream resource",
            ));
            Ok(())
        }
    }
}

// --- fnmatch ------------------------------------------------------------

struct Matcher {
    pathname: bool,
    no_escape: bool,
    period: bool,
    fold: bool,
}

impl Matcher {
    fn same(&self, left: u8, right: u8) -> bool {
        left == right || (self.fold && left.eq_ignore_ascii_case(&right))
    }

    /// Whether `name[index]` starts a path component (for `FNM_PERIOD`).
    fn component_start(&self, name: &[u8], index: usize) -> bool {
        index == 0 || (self.pathname && name[index - 1] == b'/')
    }

    /// Whether a wildcard may consume `name[index]`.
    fn wildcard_admits(&self, name: &[u8], index: usize) -> bool {
        let byte = name[index];
        !(self.pathname && byte == b'/')
            && !(self.period && byte == b'.' && self.component_start(name, index))
    }

    /// Parse the bracket expression starting after `[` at `pattern[start]`.
    /// Returns the matched state and the index after the closing bracket.
    fn bracket(&self, pattern: &[u8], start: usize, byte: u8) -> Option<(bool, usize)> {
        let mut index = start;
        let negate = matches!(pattern.get(index), Some(b'!') | Some(b'^'));
        if negate {
            index += 1;
        }
        let mut matched = false;
        let mut first = true;
        loop {
            let mut low = *pattern.get(index)?;
            if low == b']' && !first {
                index += 1;
                break;
            }
            first = false;
            if low == b'\\' && !self.no_escape {
                low = *pattern.get(index + 1)?;
                index += 1;
            }
            index += 1;
            if pattern.get(index) == Some(&b'-')
                && pattern.get(index + 1).is_some_and(|b| *b != b']')
            {
                let mut high = pattern[index + 1];
                index += 2;
                if high == b'\\' && !self.no_escape {
                    high = *pattern.get(index)?;
                    index += 1;
                }
                let in_range = |candidate: u8| candidate >= low && candidate <= high;
                if in_range(byte)
                    || (self.fold
                        && (in_range(byte.to_ascii_lowercase())
                            || in_range(byte.to_ascii_uppercase())))
                {
                    matched = true;
                }
            } else if self.same(low, byte) {
                matched = true;
            }
        }
        Some((matched != negate, index))
    }

    fn matches(&self, pattern: &[u8], name: &[u8], name_start: usize) -> bool {
        let mut p = 0;
        let mut n = name_start;
        while p < pattern.len() {
            match pattern[p] {
                b'*' => {
                    while pattern.get(p) == Some(&b'*') {
                        p += 1;
                    }
                    if n < name.len()
                        && self.period
                        && name[n] == b'.'
                        && self.component_start(name, n)
                    {
                        return false;
                    }
                    if p == pattern.len() {
                        return !self.pathname || !name[n..].contains(&b'/');
                    }
                    let mut end = n;
                    loop {
                        if self.matches(&pattern[p..], name, end) {
                            return true;
                        }
                        if end >= name.len() || (self.pathname && name[end] == b'/') {
                            return false;
                        }
                        end += 1;
                    }
                }
                b'?' => {
                    if n >= name.len() || !self.wildcard_admits(name, n) {
                        return false;
                    }
                    p += 1;
                    n += 1;
                }
                b'[' => {
                    if n >= name.len() {
                        return false;
                    }
                    match self.bracket(pattern, p + 1, name[n]) {
                        Some((matched, next)) => {
                            if !matched || !self.wildcard_admits(name, n) {
                                return false;
                            }
                            p = next;
                            n += 1;
                        }
                        None => {
                            // No closing bracket: the `[` is an ordinary byte.
                            if !self.same(b'[', name[n]) {
                                return false;
                            }
                            p += 1;
                            n += 1;
                        }
                    }
                }
                byte => {
                    let literal = if byte == b'\\' && !self.no_escape && p + 1 < pattern.len() {
                        p += 1;
                        pattern[p]
                    } else {
                        byte
                    };
                    if n >= name.len() || !self.same(literal, name[n]) {
                        return false;
                    }
                    p += 1;
                    n += 1;
                }
            }
        }
        n == name.len()
    }
}

/// Shell wildcard matching with PHP's `FNM_*` flags.
pub(crate) fn fnmatch(pattern: &[u8], name: &[u8], flags: i64) -> bool {
    Matcher {
        pathname: flags & FNM_PATHNAME != 0,
        no_escape: flags & FNM_NOESCAPE != 0,
        period: flags & FNM_PERIOD != 0,
        fold: flags & FNM_CASEFOLD != 0,
    }
    .matches(pattern, name, 0)
}

fn fn_fnmatch(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(pattern) = super::typed_internal_string_value_argument_expected(
        ed, eg, "fnmatch", 0, "pattern", "string",
    )?
    else {
        return Ok(());
    };
    let Some(filename) = super::typed_internal_string_value_argument_expected(
        ed, eg, "fnmatch", 1, "filename", "string",
    )?
    else {
        return Ok(());
    };
    let flags = match optional_argument(ed, 2) {
        Some(_) => {
            let Some(flags) = super::typed_internal_int_argument(ed, eg, "fnmatch", 2, "flags")?
            else {
                return Ok(());
            };
            flags
        }
        None => 0,
    };
    let pattern = pattern.php_string_bytes().unwrap_or_default();
    let filename = filename.php_string_bytes().unwrap_or_default();
    for (index, (bytes, parameter)) in [(&pattern, "pattern"), (&filename, "filename")]
        .into_iter()
        .enumerate()
    {
        if bytes.contains(&0) {
            eg.exception = Some(make_error_value(
                "ValueError",
                &format!(
                    "fnmatch(): Argument #{} (${parameter}) must not contain any null bytes",
                    index + 1
                ),
            ));
            return Ok(());
        }
    }
    if pattern.len() >= 4096 {
        super::report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            "fnmatch(): Pattern exceeds the maximum allowed length of 4095 bytes",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        return return_value(rv, Value::bool(false));
    }
    return_value(rv, Value::bool(fnmatch(&pattern, &filename, flags)))
}

struct Declaration {
    name: &'static str,
    handler: InternalFunctionHandler,
    parameters: &'static [&'static str],
    required: u32,
    hints: fn() -> Vec<ParamTypeHint>,
    result: fn() -> ParamTypeHint,
    defaults: fn() -> Vec<Option<Value>>,
    deprecation: Option<&'static InternalFunctionDeprecation>,
}

fn string_or_false() -> ParamTypeHint {
    ParamTypeHint::Union(vec![
        ParamTypeHint::String,
        ParamTypeHint::ClassName("false".to_string()),
    ])
}

pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let declarations = [
        Declaration {
            name: "get_cfg_var",
            handler: fn_get_cfg_var,
            parameters: &["option"],
            required: 1,
            hints: || vec![ParamTypeHint::String],
            result: || {
                ParamTypeHint::Union(vec![
                    ParamTypeHint::Array,
                    ParamTypeHint::String,
                    ParamTypeHint::ClassName("false".to_string()),
                ])
            },
            defaults: || vec![None],
            deprecation: None,
        },
        Declaration {
            name: "php_ini_loaded_file",
            handler: fn_php_ini_loaded_file,
            parameters: &[],
            required: 0,
            hints: Vec::new,
            result: string_or_false,
            defaults: Vec::new,
            deprecation: None,
        },
        Declaration {
            name: "php_ini_scanned_files",
            handler: fn_php_ini_loaded_file,
            parameters: &[],
            required: 0,
            hints: Vec::new,
            result: string_or_false,
            defaults: Vec::new,
            deprecation: None,
        },
        Declaration {
            name: "memory_get_usage",
            handler: fn_memory_get_usage,
            parameters: &["real_usage"],
            required: 0,
            hints: || vec![ParamTypeHint::Bool],
            result: || ParamTypeHint::Int,
            defaults: || vec![Some(Value::bool(false))],
            deprecation: None,
        },
        Declaration {
            name: "memory_get_peak_usage",
            handler: fn_memory_get_peak_usage,
            parameters: &["real_usage"],
            required: 0,
            hints: || vec![ParamTypeHint::Bool],
            result: || ParamTypeHint::Int,
            defaults: || vec![Some(Value::bool(false))],
            deprecation: None,
        },
        Declaration {
            name: "memory_reset_peak_usage",
            handler: fn_memory_reset_peak_usage,
            parameters: &[],
            required: 0,
            hints: Vec::new,
            result: || ParamTypeHint::Void,
            defaults: Vec::new,
            deprecation: None,
        },
        Declaration {
            name: "getmypid",
            handler: fn_getmypid,
            parameters: &[],
            required: 0,
            hints: Vec::new,
            result: || {
                ParamTypeHint::Union(vec![
                    ParamTypeHint::Int,
                    ParamTypeHint::ClassName("false".to_string()),
                ])
            },
            defaults: Vec::new,
            deprecation: None,
        },
        Declaration {
            name: "sys_getloadavg",
            handler: fn_sys_getloadavg,
            parameters: &[],
            required: 0,
            hints: Vec::new,
            result: || {
                ParamTypeHint::Union(vec![
                    ParamTypeHint::Array,
                    ParamTypeHint::ClassName("false".to_string()),
                ])
            },
            defaults: Vec::new,
            deprecation: None,
        },
        Declaration {
            name: "gethostname",
            handler: fn_gethostname,
            parameters: &[],
            required: 0,
            hints: Vec::new,
            result: string_or_false,
            defaults: Vec::new,
            deprecation: None,
        },
        Declaration {
            name: "lcg_value",
            handler: fn_lcg_value,
            parameters: &[],
            required: 0,
            hints: Vec::new,
            result: || ParamTypeHint::Float,
            defaults: Vec::new,
            deprecation: Some(&LCG_VALUE_DEPRECATION),
        },
        Declaration {
            name: "mt_getrandmax",
            handler: fn_mt_getrandmax,
            parameters: &[],
            required: 0,
            hints: Vec::new,
            result: || ParamTypeHint::Int,
            defaults: Vec::new,
            deprecation: None,
        },
        Declaration {
            name: "uniqid",
            handler: fn_uniqid,
            parameters: &["prefix", "more_entropy"],
            required: 0,
            hints: || vec![ParamTypeHint::String, ParamTypeHint::Bool],
            result: || ParamTypeHint::String,
            defaults: || vec![Some(Value::string("")), Some(Value::bool(false))],
            deprecation: None,
        },
        Declaration {
            name: "is_countable",
            handler: fn_is_countable,
            parameters: &["value"],
            required: 1,
            hints: || vec![ParamTypeHint::Mixed],
            result: || ParamTypeHint::Bool,
            defaults: || vec![None],
            deprecation: None,
        },
        Declaration {
            name: "stream_isatty",
            handler: fn_stream_isatty,
            parameters: &["stream"],
            required: 1,
            hints: || vec![ParamTypeHint::Mixed],
            result: || ParamTypeHint::Bool,
            defaults: || vec![None],
            deprecation: None,
        },
        Declaration {
            name: "fnmatch",
            handler: fn_fnmatch,
            parameters: &["pattern", "filename", "flags"],
            required: 2,
            hints: || {
                vec![
                    ParamTypeHint::String,
                    ParamTypeHint::String,
                    ParamTypeHint::Int,
                ]
            },
            result: || ParamTypeHint::Bool,
            defaults: || vec![None, None, Some(Value::long(0))],
            deprecation: None,
        },
    ];

    let mut functions = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        let mut function = Box::new(
            make_internal_function(
                declaration.handler,
                declaration.parameters.len() as u32,
                declaration.required,
                Vec::new(),
            )
            .with_static_parameter_names(declaration.parameters),
        );
        function.common.sig.param_type_hints = (declaration.hints)();
        function.common.sig.return_type_hint = (declaration.result)();
        function.handler_validates_types = true;
        if let Some(deprecation) = declaration.deprecation {
            function.set_deprecation(deprecation);
        }
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(declaration.name, pointer)
            .expect("runtime information functions register once per request");
        eg.register_internal_function_reflection_metadata(
            pointer,
            (declaration.defaults)(),
            "standard",
        );
        functions.push(function);
    }
    functions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnmatch_honors_each_flag() {
        assert!(fnmatch(b"*.php", b"a.php", 0));
        assert!(fnmatch(b"a?c", b"abc", 0));
        assert!(fnmatch(b"[a-c]*", b"bz", 0));
        assert!(!fnmatch(b"[!a-c]*", b"bz", 0));
        assert!(fnmatch(b"*", b".hidden", 0));
        assert!(!fnmatch(b"*", b".hidden", FNM_PERIOD));
        assert!(fnmatch(b".*", b".hidden", FNM_PERIOD));
        assert!(fnmatch(b"a/*", b"a/b/c", 0));
        assert!(!fnmatch(b"a/*", b"a/b/c", FNM_PATHNAME));
        assert!(fnmatch(b"a/*/c", b"a/b/c", FNM_PATHNAME));
        assert!(
            !fnmatch(
                b"a/*/.c",
                b"a/b/.c",
                FNM_PATHNAME | FNM_PERIOD | FNM_NOESCAPE
            ) == false
        );
        assert!(fnmatch(b"A*", b"abc", FNM_CASEFOLD));
        assert!(!fnmatch(b"A*", b"abc", 0));
        assert!(fnmatch(b"\\*", b"*", 0));
        assert!(!fnmatch(b"\\*", b"*", FNM_NOESCAPE));
        assert!(fnmatch(b"\\*", b"\\a", FNM_NOESCAPE));
        assert!(fnmatch(b"[]]", b"]", 0));
        assert!(fnmatch(b"[", b"[", 0));
        assert!(fnmatch(b"**a", b"xxa", 0));
        assert!(!fnmatch(b"a", b"ab", 0));
        assert!(fnmatch(b"", b"", 0));
        assert!(!fnmatch(b"", b"a", 0));
    }
}
