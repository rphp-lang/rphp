//! CLI request auto-globals: `$argv`, `$argc`, `$_SERVER`, `$_ENV` and the
//! empty HTTP input arrays of a command-line request.

use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value};

fn byte_string(bytes: &[u8]) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(text) => Value::string(text),
        Err(_) => Value::binary_string(bytes),
    }
}

/// The process environment as a PHP array, in process order.
fn environment() -> PhpArray {
    let mut environment = PhpArray::new();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        for (key, value) in std::env::vars_os() {
            let key = String::from_utf8_lossy(key.as_bytes()).into_owned();
            environment.set_str(&key, byte_string(value.as_bytes()));
        }
    }
    #[cfg(not(unix))]
    {
        for (key, value) in std::env::vars() {
            environment.set_str(&key, Value::string(value));
        }
    }
    environment
}

/// Publish PHP's command-line request globals. `script` is the script path
/// exactly as given on the command line; `None` describes inline (`-r`) or
/// standard-input code. Calling again replaces the previous request.
pub fn register_request_globals(
    eg: &mut ExecutorGlobals,
    script: Option<&str>,
    arguments: &[String],
) {
    let self_name = script.unwrap_or("Standard input code");
    let filename = script.unwrap_or("");
    let mut argv = PhpArray::with_packed_capacity(arguments.len() + 1);
    argv.push(Value::string(self_name));
    for argument in arguments {
        argv.push(Value::string(argument.as_str()));
    }
    let argc = argv.len() as i64;

    let order = super::ini_default(eg, "variables_order").unwrap_or_else(|| "EGPCS".to_string());
    let server_enabled = order.bytes().any(|byte| byte.eq_ignore_ascii_case(&b'S'));
    let environment_enabled = order.bytes().any(|byte| byte.eq_ignore_ascii_case(&b'E'));
    let environment = if server_enabled || environment_enabled {
        environment()
    } else {
        PhpArray::new()
    };
    let mut server = if server_enabled {
        environment.clone()
    } else {
        PhpArray::new()
    };
    if server_enabled {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |elapsed| elapsed.as_secs_f64());
        server.set_str("PHP_SELF", Value::string(self_name));
        server.set_str("SCRIPT_NAME", Value::string(self_name));
        server.set_str("SCRIPT_FILENAME", Value::string(filename));
        server.set_str("PATH_TRANSLATED", Value::string(filename));
        server.set_str("DOCUMENT_ROOT", Value::string(""));
        server.set_str("REQUEST_TIME_FLOAT", Value::double(now));
        server.set_str("REQUEST_TIME", Value::long(now as i64));
        server.set_str("argv", Value::array(argv.clone()));
        server.set_str("argc", Value::long(argc));
    }
    let environment = if environment_enabled {
        environment
    } else {
        PhpArray::new()
    };

    let globals = &mut eg.globals;
    globals.insert("argv".to_string(), Value::array(argv));
    globals.insert("argc".to_string(), Value::long(argc));
    for name in ["_GET", "_POST", "_COOKIE", "_FILES"] {
        globals.insert(name.to_string(), Value::array(PhpArray::new()));
    }
    // The CLI SAPI always activates `$_SERVER`; `auto_globals_jit` defers
    // `$_ENV` and `$_REQUEST` until code names them.
    globals.insert("_SERVER".to_string(), Value::array(server));
    for name in JIT_AUTO_GLOBALS {
        globals.remove(name);
    }
    let lazy = &mut eg.jit_auto_globals;
    lazy.insert("_ENV".to_string(), Value::array(environment));
    lazy.insert("_REQUEST".to_string(), Value::array(PhpArray::new()));
}

/// Auto-globals PHP creates lazily under the default `auto_globals_jit`.
pub const JIT_AUTO_GLOBALS: [&str; 2] = ["_ENV", "_REQUEST"];

/// PHP builds `$argv`/`$argc`, activates the eager request globals, then the
/// CLI's `$_SERVER`, and finally any JIT auto-global, so the global symbol
/// table lists them first in this order.
pub const REQUEST_AUTO_GLOBAL_ORDER: [&str; 9] = [
    "argv", "argc", "_GET", "_POST", "_COOKIE", "_FILES", "_SERVER", "_ENV", "_REQUEST",
];
