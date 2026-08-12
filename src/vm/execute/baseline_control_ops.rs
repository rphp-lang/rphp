// Kept in the execute module through include! so this structural split does not change visibility or code generation.

// ── Cold opcode helpers ──────────────────────────────────────────────
// Extracted from execute_ex to reduce icache pressure on the hot dispatch loop.
// Each helper is #[inline(never)] so LLVM keeps their code out of the jump table.

pub(crate) enum IncludeFileOutcome {
    Executed(Value),
    AlreadyIncluded,
    Missing(std::io::Error),
}

/// Compile, register and execute one already-resolved PHP file. Ordinary
/// include opcodes provide their caller frame for the existing scope bridge;
/// internal loaders deliberately execute without borrowing an internal frame
/// as a user `OpArray`.
#[cold]
pub(crate) fn execute_included_file(
    eg: &mut ExecutorGlobals,
    resolved_path: &str,
    is_once: bool,
    caller: Option<(*mut ExecuteData, &crate::compiler::OpArray)>,
) -> Result<IncludeFileOutcome, VmError> {
    let canonical = std::fs::canonicalize(&resolved_path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| resolved_path.to_string());

    if is_once && eg.included_files.contains(&canonical) {
        return Ok(IncludeFileOutcome::AlreadyIncluded);
    }

    let source = match std::fs::read_to_string(&resolved_path) {
        Ok(s) => s,
        Err(error) => return Ok(IncludeFileOutcome::Missing(error)),
    };

    if is_once {
        eg.included_files.insert(canonical.clone());
    }

    let tokens = crate::lexer::Lexer::new(&source).tokenize()
        .map_err(|e| VmError::Fatal(format!("Syntax error in {}: {}", resolved_path, e)))?;
    let stmts = crate::parser::Parser::new(tokens).parse()
        .map_err(|e| VmError::Fatal(format!("Parse error in {}: {}", resolved_path, e)))?;
    let mut compile_result = crate::compiler::compile::Compiler::new()
        .with_source_path(canonical.clone())
        .with_implicit_return_value(Value::long(1))
        .compile(&stmts)
        .map_err(|e| VmError::Fatal(format!("Compile error in {}: {}", resolved_path, e)))?;

    // Includes are separate compilation units, but both generic runtimes and
    // Reflection consume one executor-wide interned metadata graph. Merge the
    // cold graph and relocate only this unit's explicit use-site operands.
    let generic_use_site_base = eg
        .generic_metadata
        .merge(std::mem::take(&mut compile_result.generic_metadata));
    #[cfg(feature = "php-generics-reified")]
    eg.clear_reified_nested_arguments_cache();
    compile_result
        .relocate_generic_use_sites(generic_use_site_base)
        .map_err(VmError::Fatal)?;

    for (name, func) in compile_result.functions {
        let boxed = Box::new(func);
        let ptr = &boxed.common as *const FunctionCommon;
        eg.included_functions.push(boxed);
        let _ = eg.register_function(&name, ptr);
    }
    for class_def in compile_result.class_defs {
        eg.register_class(class_def).map_err(|e| VmError::Fatal(e))?;
    }

    let mut inc_op_array_main = compile_result.main;
    inc_op_array_main.name = resolved_path.to_string();
    let main_func_boxed = Box::new(crate::compiler::make_user_function(inc_op_array_main));
    eg.included_functions.push(main_func_boxed);
    let main_func: &UserFunction = unsafe {
        &*(&**eg.included_functions.last().unwrap() as *const UserFunction)
    };

    let scope_vars: Vec<(u32, String)> = caller.map_or_else(Vec::new, |(_, op_array)| {
        if !op_array.all_cvs.is_empty() {
            op_array.all_cvs.clone()
        } else {
            op_array.main_scope_vars.clone()
        }
    });
    if let Some((frame, op_array)) = caller {
        for (cv_idx, var_name) in &scope_vars {
            if var_name == "this" {
                continue;
            }
            let val = unsafe {
                let cv_ptr = (*frame).get_op_ptr(*cv_idx, OpType::Cv, op_array);
                (*cv_ptr).clone()
            };
            globals_set(&mut eg.globals, var_name, val);
        }
    }

    let inc_func_ptr = &main_func.common as *const FunctionCommon;
    let mut inc_return_value = Value::null();
    let inc_frame = eg.vm_stack.push_call_frame(
        inc_func_ptr,
        0,
        0,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    unsafe {
        (*inc_frame).return_value = &mut inc_return_value;
        (*inc_frame).opline = main_func.op_array.instructions.as_ptr();
    }
    if caller.is_some() {
        for (cv_idx, var_name) in &main_func.op_array.main_scope_vars {
            if let Some(val) = eg.globals.get(var_name) {
                let cv_ptr = unsafe { (*inc_frame).get_op_mut(*cv_idx, OpType::Cv) };
                unsafe { slot_set(cv_ptr, val.clone()) };
            }
        }
    }

    let prev_ed = eg.current_execute_data.get();
    eg.current_execute_data.set(inc_frame);
    let inc_result = execute_ex(eg, inc_frame);

    if caller.is_some() {
        let inc_op_array = unsafe { (*inc_frame).op_array() };
        let inc_scope = if !inc_op_array.all_cvs.is_empty() {
            &inc_op_array.all_cvs
        } else {
            &inc_op_array.main_scope_vars
        };
        for (cv_idx, var_name) in inc_scope {
            let cv_ptr = unsafe { (*inc_frame).get_op_mut(*cv_idx, OpType::Cv) };
            let val = unsafe { (*cv_ptr).clone() };
            globals_set(&mut eg.globals, var_name, val);
        }
    }

    eg.current_execute_data.set(prev_ed);
    unsafe { cleanup_frame_slots(inc_frame) };
    unsafe { pop_vm_call_frame(eg, inc_frame) };

    if let Some((frame, _)) = caller {
        for (cv_idx, var_name) in &scope_vars {
            if var_name == "this" {
                continue;
            }
            if let Some(val) = eg.globals.get(var_name) {
                let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                unsafe { slot_set(cv_ptr, val.clone()) };
            }
        }
    }

    if caller.is_none() && eg.exception.is_some() {
        inc_result?;
        return Ok(IncludeFileOutcome::Executed(inc_return_value));
    }
    if let Some(exc) = eg.exception.take() {
        let (class_name, message) = if let Some(obj) = exc.as_object() {
            let cls = obj.class_name.clone();
            let msg = obj.get_property("message")
                .map(|v| v.echo_to_string())
                .unwrap_or_default();
            (cls, msg)
        } else {
            (std::rc::Rc::from("Exception"), exc.echo_to_string())
        };
        return Err(VmError::Fatal(format!("Uncaught {}: {}", class_name, message)));
    }

    if let Some((frame, _)) = caller {
        let new_op_array = unsafe { (*frame).op_array() };
        for (cv_idx, var_name) in &new_op_array.main_scope_vars {
            if let Some(val) = eg.globals.get(var_name) {
                let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                unsafe { slot_set(cv_ptr, val.clone()) };
            }
        }
    }

    inc_result?;
    Ok(IncludeFileOutcome::Executed(inc_return_value))
}

fn write_include_result(
    frame: *mut ExecuteData,
    opline: &crate::vm::instruction::Instruction,
    value: Value,
) {
    if opline.result_type == OpType::Unused {
        return;
    }
    // SAFETY: the compiler assigns Include results to a live caller-frame
    // operand; temporary writes use the frame ownership bitmap contract.
    unsafe {
        let result = (*frame).get_op_mut(opline.result as u32, opline.result_type);
        if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
            frame_tmp_set(frame, result, value);
        } else {
            slot_set(result, value);
        }
    }
}

/// Returns true if the caller should `continue` (skip opline advance).
#[inline(never)]
fn op_include(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &crate::vm::instruction::Instruction,
) -> Result<bool, VmError> {
    let path_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let path_str = path_val.echo_to_string();
    let is_require = (opline.extended_value & 1) != 0;
    let is_once = (opline.extended_value & 2) != 0;

    #[cfg(not(feature = "include-path"))]
    let resolved_path = if std::path::Path::new(&path_str).is_absolute() {
        path_str.clone()
    } else {
        let base_dir = {
            let op_name = &op_array.name;
            let path = std::path::Path::new(op_name);
            path.is_file().then(|| path.parent()).flatten().map(std::path::Path::to_path_buf)
        }
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        base_dir.join(&path_str).to_string_lossy().into_owned()
    };
    #[cfg(feature = "include-path")]
    let resolved_path = if let Some(path) = crate::stdlib::include_path::resolve_existing(eg, &path_str) {
        path
    } else if std::path::Path::new(&path_str).is_absolute() {
        path_str.clone()
    } else {
        let base_dir = {
            let op_name = &op_array.name;
            let path = std::path::Path::new(op_name);
            path.is_file().then(|| path.parent()).flatten().map(std::path::Path::to_path_buf)
        }
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        base_dir.join(&path_str).to_string_lossy().into_owned()
    };

    match execute_included_file(eg, &resolved_path, is_once, Some((frame, op_array)))? {
        IncludeFileOutcome::Executed(value) => {
            write_include_result(frame, opline, value);
            Ok(false)
        }
        IncludeFileOutcome::AlreadyIncluded => {
            write_include_result(frame, opline, Value::bool(true));
            Ok(false)
        }
        IncludeFileOutcome::Missing(error) if is_require => Err(VmError::Fatal(format!(
            "require({path_str}): Failed opening required '{resolved_path}' ({error})"
        ))),
        IncludeFileOutcome::Missing(error) => {
            eg.write_output(
                format!(
                    "Warning: include({path_str}): Failed opening '{resolved_path}' for inclusion ({error})\n"
                )
                .as_bytes(),
            );
            write_include_result(frame, opline, Value::bool(false));
            unsafe { (*frame).opline = (*frame).opline.add(1) };
            Ok(true)
        }
    }
}

/// Result type for cold opcode helpers that may change the VM frame (e.g. via throw_in_frame).
enum ColdResult<'a> {
    /// Normal completion — advance opline as usual.
    Done,
    /// Skip opline advance (already advanced or jumped).
    Continue,
    /// Frame changed (exception was caught by a handler in a different frame).
    NewFrame(*mut ExecuteData, &'a crate::compiler::OpArray),
    /// Unhandled exception — propagate via eg.exception and return from execute_ex.
    Unhandled(Value),
    /// Generator suspend / return — execute_ex should return Ok(()).
    Return,
}

// ── Additional cold opcode helpers ─────────────────────────────────────

#[inline(never)]
fn op_throw<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    // PHP 8: only Throwable objects can be thrown
    if val.as_object().is_none() || {
        let obj = val.as_object().unwrap();
        !eg.class_is_a(&obj.class_name, "Throwable")
    } {
        let type_name = match val.value_type() {
            ValueType::Long => "int",
            ValueType::Double => "float",
            ValueType::String => "string",
            ValueType::True | ValueType::False => "bool",
            ValueType::Null | ValueType::Undef => "null",
            ValueType::Array => "array",
            ValueType::Object => {
                // Object but not Throwable
                let obj = val.as_object().unwrap();
                return Err(VmError::Fatal(format!(
                    "Cannot throw objects that do not implement Throwable (class {})", obj.class_name
                )));
            }
            _ => "unknown",
        };
        return Err(VmError::Fatal(format!(
            "Can only throw objects implementing Throwable, {} given", type_name
        )));
    }
    let thrown = val.clone();

    match throw_in_frame(eg, frame, thrown) {
        ThrowResult::Handled(new_frame, new_op_array) => {
            Ok(ColdResult::NewFrame(new_frame, new_op_array))
        }
        ThrowResult::Unhandled(exc) => {
            Ok(ColdResult::Unhandled(exc))
        }
    }
}
