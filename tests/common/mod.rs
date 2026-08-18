use rphp::compiler::compile::Compiler;
use rphp::compiler::make_user_function;
/// Shared test helpers for end-to-end PHP tests.
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::runtime::ExecutorGlobals;
use rphp::stdlib;
use rphp::vm::execute;
use rphp::vm::function::FunctionCommon;

pub fn make_eg_with_capture() -> (ExecutorGlobals, std::sync::Arc<std::sync::Mutex<Vec<u8>>>) {
    let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let buf_clone = buf.clone();
    let writer = WriterCapture(buf_clone);
    let eg = ExecutorGlobals::with_output(Box::new(writer));
    (eg, buf)
}

struct WriterCapture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for WriterCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[allow(dead_code)]
fn captured_output(buf: &std::sync::Arc<std::sync::Mutex<Vec<u8>>>) -> String {
    String::from_utf8(buf.lock().unwrap().clone()).unwrap()
}

/// Helper: compile and execute PHP source, return captured output.
#[allow(dead_code)]
pub fn run_php(source: &str) -> String {
    run_php_with_functions(source, |_| {})
}

#[allow(dead_code)]
pub fn run_php_with_functions(source: &str, register: impl FnOnce(&mut ExecutorGlobals)) -> String {
    run_php_with_compiler(source, Compiler::new(), register)
}

#[allow(dead_code)]
pub fn run_php_with_source_context(source: &str, file: &str, directory: &str) -> String {
    run_php_with_compiler(
        source,
        Compiler::new().with_source_context(file, directory),
        |_| {},
    )
}

fn run_php_with_compiler(
    source: &str,
    compiler: Compiler,
    register: impl FnOnce(&mut ExecutorGlobals),
) -> String {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    let result = compiler.compile(&stmts).unwrap();
    let generic_metadata = result.generic_metadata;
    let constant_attributes = result.constant_attributes;
    let main_func = make_user_function(result.main);
    let (mut eg, buf) = make_eg_with_capture();
    eg.generic_metadata = generic_metadata;
    eg.constant_attributes = constant_attributes;
    eg.emit_compile_deprecations(&result.deprecations);
    // Register stdlib functions
    let _stdlib = stdlib::register_stdlib(&mut eg);
    // Register user-declared functions
    for (name, func) in &result.functions {
        eg.register_function(name, &func.common as *const FunctionCommon)
            .unwrap();
    }
    // Register class definitions
    for class_def in result.class_defs {
        eg.register_compiled_class(class_def).unwrap();
    }
    register(&mut eg);
    execute::execute(&mut eg, &main_func).unwrap();
    captured_output(&buf)
}

/// Run PHP source, discard output. For benchmarks.
#[allow(dead_code)]
pub fn run_php_silent(source: &str) {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&stmts).unwrap();
    let generic_metadata = result.generic_metadata;
    let constant_attributes = result.constant_attributes;
    let main_func = make_user_function(result.main);
    let (mut eg, _buf) = make_eg_with_capture();
    eg.generic_metadata = generic_metadata;
    eg.constant_attributes = constant_attributes;
    eg.emit_compile_deprecations(&result.deprecations);
    let _stdlib = stdlib::register_stdlib(&mut eg);
    for (name, func) in &result.functions {
        eg.register_function(name, &func.common as *const FunctionCommon)
            .unwrap();
    }
    for class_def in result.class_defs {
        eg.register_compiled_class(class_def).unwrap();
    }
    execute::execute(&mut eg, &main_func).unwrap();
}

/// Prepared PHP script — compiled once, executable many times.
/// Compile + register happens once. Each `execute_silent()` re-runs the
/// main script on the same EG — inline caches stay warm (steady-state).
/// Eliminates lex/parse/compile noise from benchmark measurements.
#[allow(dead_code)]
pub struct PreparedPhp {
    main_func: rphp::vm::function::UserFunction,
    // Keep functions alive (register_function stores raw pointers)
    _functions: Vec<(String, rphp::vm::function::UserFunction)>,
    // Keep stdlib alive (register_stdlib stores raw pointers)
    _stdlib: Vec<Box<rphp::vm::function::InternalFunction>>,
    eg: ExecutorGlobals,
    _buf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
}

#[allow(dead_code)]
impl PreparedPhp {
    /// Compile PHP source and set up runtime. Panics on error.
    pub fn new(source: &str) -> Self {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let stmts = Parser::new(tokens).parse().unwrap();
        let result = Compiler::new().compile(&stmts).unwrap();
        let generic_metadata = result.generic_metadata;
        let constant_attributes = result.constant_attributes;
        let main_func = make_user_function(result.main);
        let (mut eg, buf) = make_eg_with_capture();
        eg.generic_metadata = generic_metadata;
        eg.constant_attributes = constant_attributes;
        let stdlib = stdlib::register_stdlib(&mut eg);
        for (name, func) in &result.functions {
            eg.register_function(name, &func.common as *const FunctionCommon)
                .unwrap();
        }
        for class_def in result.class_defs {
            eg.register_compiled_class(class_def).unwrap();
        }
        Self {
            main_func,
            _functions: result.functions,
            _stdlib: stdlib,
            eg,
            _buf: buf,
        }
    }

    /// Execute the main script. Re-uses the same EG (inline caches warm).
    pub fn execute_silent(&mut self) {
        execute::execute(&mut self.eg, &self.main_func).unwrap();
    }
}

/// Like run_php but returns the VmError instead of panicking.
/// Catches errors from any stage: lexing, parsing, compilation, class registration, or execution.
#[allow(dead_code)]
pub fn run_php_expect_error(source: &str) -> execute::VmError {
    run_php_expect_error_with_compiler(source, Compiler::new())
}

#[allow(dead_code)]
pub fn run_php_expect_error_with_source_context(
    source: &str,
    file: &str,
    directory: &str,
) -> execute::VmError {
    run_php_expect_error_with_compiler(source, Compiler::new().with_source_context(file, directory))
}

fn run_php_expect_error_with_compiler(source: &str, compiler: Compiler) -> execute::VmError {
    let tokens = match Lexer::new(source).tokenize() {
        Ok(t) => t,
        Err(e) => return execute::VmError::Fatal(e),
    };
    let stmts = match Parser::new(tokens).parse() {
        Ok(s) => s,
        Err(e) => return execute::VmError::Fatal(e),
    };
    let result = match compiler.compile(&stmts) {
        Ok(r) => r,
        Err(e) => return execute::VmError::Fatal(e.message),
    };
    let generic_metadata = result.generic_metadata;
    let constant_attributes = result.constant_attributes;
    let main_func = make_user_function(result.main);
    let (mut eg, _buf) = make_eg_with_capture();
    eg.generic_metadata = generic_metadata;
    eg.constant_attributes = constant_attributes;
    let _stdlib = stdlib::register_stdlib(&mut eg);
    for (name, func) in &result.functions {
        if let Err(e) = eg.register_function(name, &func.common as *const FunctionCommon) {
            return execute::VmError::Fatal(format!("{}", e));
        }
    }
    for class_def in result.class_defs {
        if let Err(e) = eg.register_compiled_class(class_def) {
            return execute::VmError::Fatal(format!("{}", e));
        }
    }
    execute::execute(&mut eg, &main_func).unwrap_err()
}
