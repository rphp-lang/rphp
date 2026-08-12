// Kept in the execute module through include! so this structural split does not change visibility or code generation.

// ── Cold opcode helpers ──────────────────────────────────────────────
// Extracted from execute_ex to reduce icache pressure on the hot dispatch loop.
// Each helper is #[inline(never)] so LLVM keeps their code out of the jump table.

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
            let p = std::path::Path::new(op_name);
            if p.is_file() {
                p.parent().map(|d| d.to_path_buf())
            } else {
                None
            }
        }.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        base_dir.join(&path_str).to_string_lossy().to_string()
    };
    #[cfg(feature = "include-path")]
    let resolved_path = if let Some(path) = crate::stdlib::include_path::resolve_existing(eg, &path_str) {
        path
    } else if std::path::Path::new(&path_str).is_absolute() {
        path_str.clone()
    } else {
        let base_dir = {
            let op_name = &op_array.name;
            let p = std::path::Path::new(op_name);
            if p.is_file() {
                p.parent().map(|d| d.to_path_buf())
            } else {
                None
            }
        }.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        base_dir.join(&path_str).to_string_lossy().to_string()
    };

    let canonical = std::fs::canonicalize(&resolved_path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| resolved_path.clone());

    if is_once && eg.included_files.contains(&canonical) {
        return Ok(false);
    }

    let source = match std::fs::read_to_string(&resolved_path) {
        Ok(s) => s,
        Err(e) => {
            if is_require {
                return Err(VmError::Fatal(format!(
                    "require({}): Failed opening required '{}' ({})",
                    path_str, resolved_path, e
                )));
            } else {
                let warning = format!(
                    "Warning: include({}): Failed opening '{}' for inclusion ({})\n",
                    path_str, resolved_path, e
                );
                eg.write_output(warning.as_bytes());
                unsafe { (*frame).opline = (*frame).opline.add(1); }
                return Ok(true); // continue
            }
        }
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
    inc_op_array_main.name = resolved_path.clone();
    let main_func_boxed = Box::new(crate::compiler::make_user_function(inc_op_array_main));
    eg.included_functions.push(main_func_boxed);
    let main_func: &UserFunction = unsafe {
        &*(&**eg.included_functions.last().unwrap() as *const UserFunction)
    };

    let scope_vars: Vec<(u32, String)> = if !op_array.all_cvs.is_empty() {
        op_array.all_cvs.clone()
    } else {
        op_array.main_scope_vars.clone()
    };
    for (cv_idx, var_name) in &scope_vars {
        if var_name == "this" { continue; }
        let cv_ptr = unsafe { (*frame).get_op_ptr(*cv_idx, OpType::Cv, op_array) };
        let val = unsafe { (*cv_ptr).clone() };
        globals_set(&mut eg.globals, var_name, val);
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
    for (cv_idx, var_name) in &main_func.op_array.main_scope_vars {
        if let Some(val) = eg.globals.get(var_name) {
            let cv_ptr = unsafe { (*inc_frame).get_op_mut(*cv_idx, OpType::Cv) };
            unsafe { slot_set(cv_ptr, val.clone()) };
        }
    }

    let prev_ed = eg.current_execute_data.get();
    eg.current_execute_data.set(inc_frame);
    let inc_result = execute_ex(eg, inc_frame);

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

    eg.current_execute_data.set(prev_ed);
    unsafe { cleanup_frame_slots(inc_frame) };
    unsafe { pop_vm_call_frame(eg, inc_frame) };

    for (cv_idx, var_name) in &scope_vars {
        if var_name == "this" { continue; }
        if let Some(val) = eg.globals.get(var_name) {
            let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
            unsafe { slot_set(cv_ptr, val.clone()) };
        }
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

    let new_op_array = unsafe { (*frame).op_array() };
    for (cv_idx, var_name) in &new_op_array.main_scope_vars {
        if let Some(val) = eg.globals.get(var_name) {
            let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
            unsafe { slot_set(cv_ptr, val.clone()) };
        }
    }

    inc_result?;
    Ok(false)
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
