// Kept in the execute module through include! so this structural split does not change visibility or code generation.

// ── Cold opcode helpers ──────────────────────────────────────────────
// Extracted from execute_ex to reduce icache pressure on the hot dispatch loop.
// Each helper is #[inline(never)] so LLVM keeps their code out of the jump table.

pub(crate) enum IncludeFileOutcome {
    Executed(Value),
    AlreadyIncluded,
    Missing(std::io::Error),
    Thrown(Value),
}

fn include_parse_error(
    eg: &mut ExecutorGlobals,
    caller_present: bool,
    message: String,
) -> IncludeFileOutcome {
    let error = make_error_value("ParseError", &message);
    if caller_present {
        IncludeFileOutcome::Thrown(error)
    } else {
        eg.exception = Some(error);
        IncludeFileOutcome::Executed(Value::null())
    }
}

fn imported_class_name(statements: &[crate::parser::Stmt], alias: &str) -> Option<String> {
    for statement in statements {
        match statement {
            crate::parser::Stmt::UseDecl { imports } => {
                if let Some((_, name, _)) = imports
                    .iter()
                    .find(|(kind, _, imported_alias)| {
                        *kind == crate::parser::UseKind::Class
                            && imported_alias.eq_ignore_ascii_case(alias)
                    })
                {
                    return Some(name.trim_start_matches('\\').to_string());
                }
            }
            crate::parser::Stmt::Namespace { body, .. } => {
                if let Some(name) = imported_class_name(body, alias) {
                    return Some(name);
                }
            }
            _ => {}
        }
    }
    None
}

fn unavailable_class_constant_owner(error: &str) -> Option<&str> {
    let marker = "class constant ";
    let remainder = error.rsplit_once(marker)?.1;
    remainder.split_once("::").map(|(owner, _)| owner)
}

fn compilation_constants(eg: &ExecutorGlobals) -> HashMap<String, Value> {
    let mut known = eg.constant_table.borrow().clone();
    for (registered_name, class) in &eg.class_table {
        for constant in &class.constants {
            if constant.evaluation_error.is_some() {
                continue;
            }
            known.insert(
                format!("{}::{}", class.name, constant.name),
                constant.value.clone(),
            );
            known.insert(
                format!("{registered_name}::{}", constant.name),
                constant.value.clone(),
            );
        }
    }
    known
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

    let source = match std::fs::read(&resolved_path) {
        Ok(bytes) => crate::lexer::decode_php_source(&bytes),
        Err(error) => return Ok(IncludeFileOutcome::Missing(error)),
    };

    let tokens = match crate::lexer::Lexer::new(&source).tokenize() {
        Ok(tokens) => tokens,
        Err(error) => {
            return Ok(include_parse_error(
                eg,
                caller.is_some(),
                format!("Syntax error in {resolved_path}: {error}"),
            ));
        }
    };
    let stmts = match crate::parser::Parser::new(tokens)
        .with_source_name(canonical.clone())
        .parse()
    {
        Ok(statements) => statements,
        Err(error) => {
            let error = if error.starts_with("memory exhausted in ") {
                format!("Parse error: {error}")
            } else {
                format!("Parse error in {resolved_path}: {error}")
            };
            return Ok(include_parse_error(
                eg,
                caller.is_some(),
                error,
            ));
        }
    };
    let mut compile_attempts = 0usize;
    let mut compile_result = loop {
        let compiler = crate::compiler::compile::Compiler::new()
            .with_source_path(canonical.clone())
            .with_implicit_return_value(Value::long(1))
            .with_known_constants(compilation_constants(eg));
        match compiler.compile(&stmts) {
            Ok(result) => break result,
            Err(error) if compile_attempts < 16 => {
                let Some(owner) = unavailable_class_constant_owner(&error) else {
                    return Ok(include_parse_error(
                        eg,
                        caller.is_some(),
                        format!("Compile error in {resolved_path}: {error}"),
                    ));
                };
                let class_name = imported_class_name(&stmts, owner)
                    .unwrap_or_else(|| owner.trim_start_matches('\\').to_string());
                let loaded = eg.find_class(&class_name).is_some()
                    || crate::stdlib::autoload::ensure_symbol_loaded(eg, &class_name)?;
                if !loaded {
                    return Ok(include_parse_error(
                        eg,
                        caller.is_some(),
                        format!("Compile error in {resolved_path}: {error}"),
                    ));
                }
                compile_attempts += 1;
            }
            Err(error) => {
                return Ok(include_parse_error(
                    eg,
                    caller.is_some(),
                    format!("Compile error in {resolved_path}: {error}"),
                ));
            }
        }
    };
    eg.record_included_file(canonical.clone());

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
        if class_def.is_anonymous() {
            eg.register_compiled_class(class_def)
                .map_err(VmError::Fatal)?;
            continue;
        }
        let dependencies = class_def
            .parent
            .iter()
            .chain(class_def.uses.iter())
            .chain(class_def.implements.iter())
            .cloned()
            .collect::<Vec<_>>();
        for dependency in dependencies {
            if eg.find_class(&dependency).is_none()
                && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &dependency)?
            {
                if let Some(exception) = eg.exception.take() {
                    if caller.is_some() {
                        return Ok(IncludeFileOutcome::Thrown(exception));
                    }
                    eg.exception = Some(exception);
                    return Ok(IncludeFileOutcome::Executed(Value::null()));
                }
                return Err(VmError::Fatal(format!(
                    "Class dependency \"{dependency}\" not found"
                )));
            }
        }
        eg.register_class(class_def).map_err(|e| VmError::Fatal(e))?;
    }

    let mut inc_op_array_main = compile_result.main;
    inc_op_array_main.name = resolved_path.to_string();
    let main_func_boxed = Box::new(crate::compiler::make_user_function(inc_op_array_main));
    eg.included_functions.push(main_func_boxed);
    let main_func: &UserFunction = unsafe {
        &*(&**eg.included_functions.last().unwrap() as *const UserFunction)
    };
    let include_caller_class = caller.and_then(|(frame, _)| get_caller_class(frame, eg));
    if let Some(caller_class) = include_caller_class {
        eg.method_declaring_class.insert(
            &main_func.common as *const FunctionCommon,
            caller_class,
        );
    }

    let scope_vars: Vec<(u32, String)> = caller.map_or_else(Vec::new, |(_, op_array)| {
        if !op_array.all_cvs.is_empty() {
            op_array.all_cvs.clone()
        } else {
            op_array.main_scope_vars.clone()
        }
    });
    let caller_is_local_scope = caller.is_some_and(|(_, op_array)| op_array.main_scope_vars.is_empty());
    let included_global_vars = &main_func.op_array.global_vars;
    let caller_global_vars = caller.map(|(_, op_array)| &op_array.global_vars[..]).unwrap_or(&[]);
    let mut globals_backup: HashMap<String, Option<Value>> = HashMap::new();
    if caller_is_local_scope || caller.is_none() {
        for (_, var_name) in scope_vars
            .iter()
            .chain(main_func.op_array.main_scope_vars.iter())
        {
            let explicitly_global = caller_global_vars
                .iter()
                .chain(included_global_vars.iter())
                .any(|(_, global_name)| global_name == var_name);
            if !explicitly_global {
                globals_backup
                    .entry(var_name.clone())
                    .or_insert_with(|| eg.globals.get(var_name).cloned());
            }
        }
    }
    if let Some((frame, op_array)) = caller {
        for (cv_idx, var_name) in &scope_vars {
            if included_global_vars
                .iter()
                .any(|(_, global_name)| global_name == var_name)
            {
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
    if let Some((caller_frame, _)) = caller {
        eg.alias_dynamic_scope(inc_frame as usize, caller_frame as usize);
    }
    if caller.is_some() {
        for (cv_idx, var_name) in &main_func.op_array.main_scope_vars {
            if let Some(val) = eg.globals.get(var_name) {
                let cv_ptr = unsafe { (*inc_frame).get_op_mut(*cv_idx, OpType::Cv) };
                unsafe { frame_slot_set(inc_frame, cv_ptr, val.clone()) };
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
    pop_vm_call_frame(eg, inc_frame);

    if let Some((frame, _)) = caller {
        for (cv_idx, var_name) in &scope_vars {
            if let Some(val) = eg.globals.get(var_name) {
                let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                unsafe { frame_slot_set(frame, cv_ptr, val.clone()) };
            }
        }
    }

    for (var_name, previous) in globals_backup {
        if let Some(value) = previous {
            globals_set(&mut eg.globals, &var_name, value);
        } else {
            eg.globals.remove(&var_name);
        }
    }

    if caller.is_none() && eg.exception.is_some() {
        inc_result?;
        return Ok(IncludeFileOutcome::Executed(inc_return_value));
    }
    if let Some(exc) = eg.exception.take() {
        return Ok(IncludeFileOutcome::Thrown(exc));
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
fn op_include<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &crate::vm::instruction::Instruction,
) -> Result<ColdResult<'a>, VmError> {
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
            Ok(ColdResult::Done)
        }
        IncludeFileOutcome::AlreadyIncluded => {
            write_include_result(frame, opline, Value::bool(true));
            Ok(ColdResult::Done)
        }
        IncludeFileOutcome::Thrown(exception) => Ok(match throw_in_frame(eg, frame, exception) {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        }),
        IncludeFileOutcome::Missing(error) if is_require => {
            let message = format!(
                "require({path_str}): Failed opening required '{resolved_path}' ({error})"
            );
            eg.write_output(format!("Warning: {message}\n").as_bytes());
            let exception = make_error_value("Error", &message);
            Ok(match throw_in_frame(eg, frame, exception) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            })
        }
        IncludeFileOutcome::Missing(error) => {
            eg.write_output(
                format!(
                    "Warning: include({path_str}): Failed opening '{resolved_path}' for inclusion ({error})\n"
                )
                .as_bytes(),
            );
            write_include_result(frame, opline, Value::bool(false));
            unsafe { (*frame).opline = (*frame).opline.add(1) };
            Ok(ColdResult::Continue)
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
    // SAFETY: `opline`, its compiler-allocated operand and the predecessor bit
    // all belong to the live active frame for the complete opcode dispatch.
    let (val, instruction_index) = unsafe {
        (
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
            (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize,
        )
    };
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
    attach_throwable_origin(&thrown, eg, frame, op_array, instruction_index);

    match throw_in_frame(eg, frame, thrown) {
        ThrowResult::Handled(new_frame, new_op_array) => {
            Ok(ColdResult::NewFrame(new_frame, new_op_array))
        }
        ThrowResult::Unhandled(exc) => {
            Ok(ColdResult::Unhandled(exc))
        }
    }
}
