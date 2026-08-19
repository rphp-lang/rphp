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

#[cold]
#[inline(never)]
fn op_declare_class<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let declaration_key = op_array.literals[opline.op1 as usize]
        .as_str()
        .expect("DeclareClass key must be a string literal")
        .to_string();
    let Some(class_def) = eg
        .take_runtime_class_declaration(&declaration_key)
        .map_err(VmError::Fatal)?
    else {
        return Ok(ColdResult::Done);
    };

    let dependencies = class_def
        .parent
        .iter()
        .map(|name| ("Class", name.clone()))
        .chain(
            class_def
                .uses
                .iter()
                .map(|name| ("Trait", name.clone())),
        )
        .chain(
            class_def
                .implements
                .iter()
                .map(|name| ("Interface", name.clone())),
        )
        .collect::<Vec<_>>();
    for (dependency_kind, dependency) in dependencies {
        if eg.find_class(&dependency).is_none()
            && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &dependency)?
        {
            if let Some(exception) = eg.exception.take() {
                eg.restore_runtime_class_declaration(declaration_key, class_def);
                return Ok(match throw_in_frame(eg, frame, exception) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
            let error = make_error_value(
                "Error",
                &format!("{dependency_kind} \"{dependency}\" not found"),
            );
            let instruction_index = op_array
                .instructions
                .iter()
                .position(|instruction| std::ptr::eq(instruction, opline))
                .expect("DeclareClass instruction belongs to its op array");
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            eg.restore_runtime_class_declaration(declaration_key, class_def);
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
        if let Some(exception) = eg.exception.take() {
            eg.restore_runtime_class_declaration(declaration_key, class_def);
            return Ok(match throw_in_frame(eg, frame, exception) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
    }
    for dependency in
        crate::runtime::property_hook_setter_variance_dependencies(eg, &class_def)
    {
        if eg.find_class(&dependency).is_some() {
            continue;
        }
        let _ = crate::stdlib::autoload::ensure_symbol_loaded(eg, &dependency)?;
        if let Some(exception) = eg.exception.take() {
            eg.restore_runtime_class_declaration(declaration_key, class_def);
            return Ok(match throw_in_frame(eg, frame, exception) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
    }

    let class_name = class_def.name.clone();
    eg.register_compiled_class(class_def)
        .map_err(VmError::Fatal)?;
    eg.mark_runtime_class_declared(declaration_key, class_name);
    Ok(ColdResult::Done)
}

fn include_parse_error(
    eg: &mut ExecutorGlobals,
    caller_present: bool,
    message: String,
) -> IncludeFileOutcome {
    let error = make_error_value("ParseError", &message);
    include_parse_error_value(eg, caller_present, error)
}

fn include_parse_error_at(
    eg: &mut ExecutorGlobals,
    caller_present: bool,
    message: &str,
    source_file: &str,
    line: usize,
) -> IncludeFileOutcome {
    let error = make_error_value("ParseError", message);
    if let Some(mut object) = error.as_object_mut() {
        object.set_property("file", Value::string(source_file));
        object.set_property("line", Value::long(line as i64));
    }
    include_parse_error_value(eg, caller_present, error)
}

fn include_parse_error_value(
    eg: &mut ExecutorGlobals,
    caller_present: bool,
    error: Value,
) -> IncludeFileOutcome {
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

/// Clone one variable binding across the synchronous include/eval scope
/// bridge. Ordinary `Value::clone()` intentionally reads through references,
/// while a shared PHP symbol table must retain the reference cell itself.
fn clone_scope_binding(value: &Value) -> Value {
    if value.is_owned_reference() {
        value.clone_owned_reference_alias()
    } else if value.is_reference() {
        // SAFETY: include/eval execution and writeback finish before the caller
        // frame can be released, so a borrowed frame-cell alias stays live.
        Value::reference(unsafe { value.as_ref_ptr() })
    } else {
        value.clone()
    }
}

/// Compile, register and execute one PHP source unit. Includes and eval use the
/// same scope bridge, declaration registration and exception propagation; the
/// caller supplies their distinct source identity and implicit return value.
#[cold]
fn execute_source_unit(
    eg: &mut ExecutorGlobals,
    source: String,
    resolved_path: &str,
    canonical: String,
    implicit_return: Value,
    record_included: bool,
    caller: Option<(*mut ExecuteData, &crate::compiler::OpArray)>,
    synthetic_trace_origin: Option<(String, usize)>,
) -> Result<IncludeFileOutcome, VmError> {
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
    let caller_class = caller.and_then(|(frame, _)| get_caller_class(frame, eg));
    let caller_parent = caller_class
        .as_deref()
        .and_then(|class| eg.find_class(class))
        .and_then(|class| class.parent.clone());
    let class_scope_active = caller_class.is_some();
    let stmts = match crate::parser::Parser::new(tokens)
        .with_source_name(canonical.clone())
        .with_class_scope_active(class_scope_active)
        .parse()
    {
        Ok(statements) => statements,
        Err(error) => {
            if error.starts_with("syntax error,")
                || error.starts_with("Invalid indentation")
                || error.starts_with("Invalid body indentation")
            {
                let location = format!(" in {canonical} on line ");
                if let Some((message, line)) = error.rsplit_once(&location)
                    && let Ok(line) = line.parse::<usize>()
                {
                    return Ok(include_parse_error_at(
                        eg,
                        caller.is_some(),
                        message,
                        &canonical,
                        line,
                    ));
                }
            }
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
            .with_zend_assertions(eg.assertion_state.startup_mode)
            .with_source_path(canonical.clone())
            .with_implicit_return_value(implicit_return.clone())
            .with_lexical_class_scope(caller_class.clone(), caller_parent.clone())
            .with_known_constants(compilation_constants(eg));
        match compiler.compile(&stmts) {
            Ok(result) => break result,
            Err(error) if compile_attempts < 16 => {
                let Some(owner) = unavailable_class_constant_owner(&error.message) else {
                    eg.emit_compile_deprecations(&error.deprecations);
                    return Ok(include_parse_error(
                        eg,
                        caller.is_some(),
                        format!("Compile error in {resolved_path}: {}", error.message),
                    ));
                };
                let class_name = imported_class_name(&stmts, owner)
                    .unwrap_or_else(|| owner.trim_start_matches('\\').to_string());
                let loaded = eg.find_class(&class_name).is_some()
                    || crate::stdlib::autoload::ensure_symbol_loaded(eg, &class_name)?;
                if !loaded {
                    eg.emit_compile_deprecations(&error.deprecations);
                    return Ok(include_parse_error(
                        eg,
                        caller.is_some(),
                        format!("Compile error in {resolved_path}: {}", error.message),
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
    if record_included {
        eg.record_included_file(canonical.clone());
    }
    eg.emit_compile_deprecations(&compile_result.deprecations);
    eg.constant_attributes
        .extend(std::mem::take(&mut compile_result.constant_attributes));
    eg.constant_expressions
        .extend(std::mem::take(&mut compile_result.constant_expressions));
    eg.refresh_constant_deprecation_metadata_presence();
    eg.bump_constant_deprecation_generation();

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
    for (declaration_key, class_def) in
        std::mem::take(&mut compile_result.runtime_class_defs)
    {
        eg.register_runtime_class_declaration(declaration_key, class_def)
            .map_err(VmError::Fatal)?;
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
        for dependency in
            crate::runtime::property_hook_setter_variance_dependencies(eg, &class_def)
        {
            if eg.find_class(&dependency).is_some() {
                continue;
            }
            // Signature dependencies are soft: PHP invokes autoload to learn
            // their relation, but a loader that leaves them undefined still
            // reaches the declaration-variance diagnostic.
            let _ = crate::stdlib::autoload::ensure_symbol_loaded(eg, &dependency)?;
            if let Some(exception) = eg.exception.take() {
                if caller.is_some() {
                    return Ok(IncludeFileOutcome::Thrown(exception));
                }
                eg.exception = Some(exception);
                return Ok(IncludeFileOutcome::Executed(Value::null()));
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

    let mut scope_vars: Vec<(u32, String)> = caller.map_or_else(Vec::new, |(_, op_array)| {
        if !op_array.all_cvs.is_empty() {
            op_array.all_cvs.clone()
        } else {
            op_array.main_scope_vars.clone()
        }
    });
    if let Some((frame, _)) = caller {
        let function = unsafe { (*frame).func };
        if !function.is_null()
            && unsafe { (*function).sig.this_offset == 1 }
            && !scope_vars.iter().any(|(_, name)| name == "this")
        {
            scope_vars.push((0, "this".to_string()));
        }
    }
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
                globals_backup.entry(var_name.clone()).or_insert_with(|| {
                    eg.globals.get(var_name).map(clone_scope_binding)
                });
            }
        }
    }
    if let Some((frame, _)) = caller {
        for (cv_idx, var_name) in &scope_vars {
            if included_global_vars
                .iter()
                .any(|(_, global_name)| global_name == var_name)
            {
                continue;
            }
            let val = unsafe { clone_scope_binding((*frame).cv(*cv_idx)) };
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
        let called_class_id = called_class_id_for_frame(eg, caller_frame, 0);
        if called_class_id != 0 {
            publish_late_static_call_class_id(eg, inc_frame, called_class_id);
        }
        if let Some((file, line)) = synthetic_trace_origin.as_ref() {
            eg.publish_detached_trace_caller(inc_frame as usize, caller_frame as usize);
            eg.publish_synthetic_trace_frame(
                inc_frame as usize,
                file.clone(),
                *line,
                "eval".to_string(),
            );
        }
    }
    if caller.is_some() {
        for (cv_idx, var_name) in &main_func.op_array.main_scope_vars {
            if let Some(val) = eg.globals.get(var_name) {
                let cv_ptr = unsafe { (*inc_frame).cv_mut(*cv_idx) as *mut Value };
                unsafe { frame_slot_set(inc_frame, cv_ptr, clone_scope_binding(val)) };
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
            let cv_ptr = unsafe { (*inc_frame).cv_mut(*cv_idx) as *mut Value };
            let val = unsafe { clone_scope_binding(&*cv_ptr) };
            globals_set(&mut eg.globals, var_name, val);
        }
    }

    eg.current_execute_data.set(prev_ed);
    if synthetic_trace_origin.is_some() {
        eg.discard_detached_trace_caller(inc_frame as usize);
    }
    unsafe { cleanup_frame_slots(inc_frame) };
    pop_vm_call_frame(eg, inc_frame);

    if let Some((frame, _)) = caller {
        for (cv_idx, var_name) in &scope_vars {
            if let Some(val) = eg.globals.get(var_name) {
                let cv_ptr = unsafe { (*frame).cv_mut(*cv_idx) as *mut Value };
                unsafe { frame_slot_set(frame, cv_ptr, clone_scope_binding(val)) };
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
    let canonical = std::fs::canonicalize(resolved_path)
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| resolved_path.to_string());
    if is_once && eg.included_files.contains(&canonical) {
        return Ok(IncludeFileOutcome::AlreadyIncluded);
    }
    let source = match std::fs::read(resolved_path) {
        Ok(bytes) => crate::lexer::decode_php_source(&bytes),
        Err(error) => return Ok(IncludeFileOutcome::Missing(error)),
    };
    execute_source_unit(
        eg,
        source,
        resolved_path,
        canonical,
        Value::long(1),
        true,
        caller,
        None,
    )
}

#[cold]
fn execute_eval_source(
    eg: &mut ExecutorGlobals,
    source: &str,
    source_name: String,
    caller: (*mut ExecuteData, &crate::compiler::OpArray),
    trace_origin: (String, usize),
) -> Result<IncludeFileOutcome, VmError> {
    execute_source_unit(
        eg,
        format!("<?php {source}"),
        &source_name,
        source_name.clone(),
        Value::null(),
        false,
        Some(caller),
        Some(trace_origin),
    )
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

#[inline(never)]
fn op_eval<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &crate::vm::instruction::Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let source = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) }
        .echo_to_string();
    let source_file = if op_array.source_file.is_empty() {
        op_array.name.as_str()
    } else {
        op_array.source_file.as_str()
    };
    let source_name = format!(
        "{}({}) : eval()'d code",
        source_file,
        opline.extended_value
    );
    let trace_origin = (source_file.to_string(), opline.extended_value as usize);
    match execute_eval_source(eg, &source, source_name, (frame, op_array), trace_origin)? {
        IncludeFileOutcome::Executed(value) => {
            write_include_result(frame, opline, value);
            Ok(ColdResult::Done)
        }
        IncludeFileOutcome::Thrown(exception) => Ok(match throw_in_frame(eg, frame, exception) {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        }),
        IncludeFileOutcome::AlreadyIncluded | IncludeFileOutcome::Missing(_) => {
            unreachable!("eval source is neither filesystem-backed nor include-once")
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

#[cold]
fn format_unhandled_match_float(number: f64) -> String {
    if number.is_nan() {
        return "NAN".to_string();
    }
    if number == f64::INFINITY {
        return "INF".to_string();
    }
    if number == f64::NEG_INFINITY {
        return "-INF".to_string();
    }
    let exponent = if number == 0.0 {
        0
    } else {
        number.abs().log10().floor() as i32
    };
    if !(-4..14).contains(&exponent) {
        let scientific = format!("{number:.13e}");
        let (mantissa, exponent) = scientific
            .split_once('e')
            .expect("Rust scientific float formatting has an exponent");
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        let mantissa = if mantissa.contains('.') {
            mantissa.to_string()
        } else {
            format!("{mantissa}.0")
        };
        let exponent = exponent.parse::<i32>().unwrap_or(0);
        return format!("{mantissa}E{exponent:+}");
    }
    let decimals = usize::try_from(13 - exponent).unwrap_or(0);
    let fixed = format!("{number:.decimals$}");
    let fixed = fixed.trim_end_matches('0').trim_end_matches('.');
    if fixed.contains('.') {
        fixed.to_string()
    } else {
        format!("{fixed}.0")
    }
}

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
    if opline._pad & THROW_FLAG_UNHANDLED_MATCH != 0 {
        let value = val.dereferenced();
        let ignore_arguments = crate::stdlib::ini_default(eg, "zend.exception_ignore_args")
            .as_deref()
            .is_some_and(crate::stdlib::ini_boolean);
        let string_max_len = crate::stdlib::exception_string_param_max_len(eg);
        let detail = if ignore_arguments {
            format!("of type {}", value.diagnostic_type_name())
        } else {
            match value.value_type() {
                ValueType::Null | ValueType::Undef => "NULL".to_string(),
                ValueType::False => "false".to_string(),
                ValueType::True => "true".to_string(),
                ValueType::Long => value.as_long().unwrap().to_string(),
                ValueType::Double => format_unhandled_match_float(value.as_double().unwrap()),
                ValueType::String if string_max_len == 0 => "of type string".to_string(),
                ValueType::String => crate::vm::trace::format_exception_string_argument(
                    value.as_str().unwrap(),
                    string_max_len,
                ),
                _ => format!("of type {}", value.diagnostic_type_name()),
            }
        };
        let error = make_error_value(
            "UnhandledMatchError",
            &format!("Unhandled match case {detail}"),
        );
        attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
        return Ok(match throw_in_frame(eg, frame, error) {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }
    // PHP validates the operand through a normal catchable Error at the throw
    // opcode. Scalar types and class names are intentionally absent from the
    // public PHP 8.2 messages.
    if val.as_object().is_none_or(|object| {
        !eg.class_is_a(&object.class_name, "Throwable")
    }) {
        let message = if val.value_type() == ValueType::Object {
            "Cannot throw objects that do not implement Throwable"
        } else {
            "Can only throw objects"
        };
        let error = make_error_value("Error", message);
        attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
        return Ok(match throw_in_frame(eg, frame, error) {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
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
