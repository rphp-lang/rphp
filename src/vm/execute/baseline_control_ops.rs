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

fn collect_unconditional_function_names(
    statements: &[crate::parser::Stmt],
    namespace: Option<&str>,
    names: &mut std::collections::HashSet<String>,
) {
    for statement in statements {
        match statement {
            crate::parser::Stmt::Function { name, .. } => {
                let name = namespace.map_or_else(
                    || name.trim_start_matches('\\').to_string(),
                    |namespace| format!("{namespace}\\{name}"),
                );
                names.insert(name.to_lowercase());
            }
            crate::parser::Stmt::Namespace { name, body } => {
                collect_unconditional_function_names(
                    body,
                    (!name.is_empty()).then_some(name.as_str()),
                    names,
                );
            }
            crate::parser::Stmt::Block(body) => {
                collect_unconditional_function_names(body, namespace, names);
            }
            _ => {}
        }
    }
}

fn runtime_class_dependency_exception<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    declaration_key: String,
    class_def: crate::compiler::compile::ClassDef,
    exception: Value,
) -> Result<ColdResult<'a>, VmError> {
    if eg.active_runtime_class_has_variance_dependents(&class_def.name) {
        let mut exception = format_uncaught_throwable(eg, &exception);
        if let Some(thrown_suffix) = exception.rfind("\n  thrown in ") {
            exception.truncate(thrown_suffix);
        }
        let inheritance_location = class_def.source_file.as_ref().map_or_else(
            String::new,
            |file| format!(" in {file} on line {}", class_def.declaration_line),
        );
        return Err(VmError::Fatal(format!(
            "During inheritance of {} with variance dependencies: {exception}{inheritance_location}",
            class_def.name
        )));
    }
    eg.restore_runtime_class_declaration(declaration_key, class_def);
    Ok(match throw_in_frame(eg, frame, exception)? {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
    })
}

fn runtime_variance_dependency_exception(
    eg: &ExecutorGlobals,
    class_name: &str,
    source_file: Option<&str>,
    declaration_line: usize,
    dependency: &str,
    exception: &Value,
) -> VmError {
    let mut exception = format_uncaught_throwable(eg, exception);
    if let Some(thrown_suffix) = exception.rfind("\n  thrown in ") {
        exception.truncate(thrown_suffix);
    }
    let inheritance_location = source_file.map_or_else(String::new, |file| {
        format!(" in {file} on line {declaration_line}")
    });
    VmError::Fatal(format!(
        "During inheritance of {class_name}, while autoloading {dependency}: {exception}{inheritance_location}"
    ))
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
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
                return runtime_class_dependency_exception(
                    eg,
                    frame,
                    declaration_key,
                    class_def,
                    exception,
                );
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
            return Ok(match throw_in_frame(eg, frame, error)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
        if let Some(exception) = eg.exception.take() {
            return runtime_class_dependency_exception(
                eg,
                frame,
                declaration_key,
                class_def,
                exception,
            );
        }
    }
    if let Some(message) = eg.direct_non_trait_use_error(&class_def) {
        let error = make_error_value("Error", &message);
        let instruction_index = op_array
            .instructions
            .iter()
            .position(|instruction| std::ptr::eq(instruction, opline))
            .expect("DeclareClass instruction belongs to its op array");
        attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
        eg.restore_runtime_class_declaration(declaration_key, class_def);
        return Ok(match throw_in_frame(eg, frame, error)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }
    if let Some(error) = eg.enum_trait_case_constant_compile_fatal(&class_def) {
        eg.abort_runtime_class_link(&class_def.name);
        return Err(VmError::CompileFatal(error));
    }
    let class_name = class_def.name.clone();
    let class_source_file = class_def.source_file.clone();
    let class_declaration_line = class_def.declaration_line;
    let class_is_enum = class_def.is_enum;
    let class_parent_is_enum = class_def
        .parent
        .as_deref()
        .and_then(|parent| eg.find_class(parent))
        .is_some_and(|parent| parent.is_enum);
    let link_deprecations = eg.class_link_deprecations(&class_def);
    eg.emit_runtime_compile_deprecations(frame, &link_deprecations)?;
    if let Some((active_parent, outstanding_dependencies)) =
        eg.active_parent_link_dependencies(&class_def)
    {
        for dependency in outstanding_dependencies {
            if eg.find_class(&dependency).is_some()
                || eg.runtime_class_link_is_active(&dependency)
            {
                continue;
            }
            let loaded = crate::stdlib::autoload::ensure_symbol_loaded(eg, &dependency)?;
            if let Some(exception) = eg.exception.take() {
                eg.abort_runtime_class_link(&class_name);
                return Err(runtime_variance_dependency_exception(
                    eg,
                    &class_name,
                    class_source_file.as_deref(),
                    class_declaration_line,
                    &dependency,
                    &exception,
                ));
            }
            if !loaded
                && let Some(error) = eg
                    .active_class_unavailable_method_variance_dependency_error(
                        &active_parent,
                        &dependency,
                    )
            {
                eg.abort_runtime_class_link(&class_name);
                return Err(VmError::Fatal(error));
            }
            if let Err(error) = eg.finalize_provisional_runtime_class(&active_parent) {
                eg.abort_runtime_class_link(&class_name);
                return Err(VmError::Fatal(error));
            }
        }
    }
    let (method_variance_dependencies, requires_provisional_publication) =
        eg.method_variance_dependency_plan(&class_def);
    let property_variance_dependencies =
        crate::runtime::property_hook_setter_variance_dependencies(eg, &class_def);

    if requires_provisional_publication && !method_variance_dependencies.is_empty() {
        let outstanding_variance_dependencies = method_variance_dependencies
            .iter()
            .chain(&property_variance_dependencies)
            .cloned()
            .collect::<Vec<_>>();
        if let Err(error) = eg.register_provisional_runtime_class(
            class_def,
            outstanding_variance_dependencies,
        ) {
            eg.abort_runtime_class_link(&class_name);
            if class_is_enum || class_parent_is_enum {
                return Err(VmError::CompileFatal(error));
            }
            return Err(VmError::Fatal(error));
        }
        for dependency in method_variance_dependencies
            .into_iter()
            .chain(property_variance_dependencies)
        {
            if eg.find_class(&dependency).is_some() {
                continue;
            }
            let loaded = crate::stdlib::autoload::ensure_symbol_loaded(eg, &dependency)?;
            if let Some(exception) = eg.exception.take() {
                eg.abort_runtime_class_link(&class_name);
                return Err(runtime_variance_dependency_exception(
                    eg,
                    &class_name,
                    class_source_file.as_deref(),
                    class_declaration_line,
                    &dependency,
                    &exception,
                ));
            }
            if !loaded
                && let Some(error) = eg
                    .active_class_unavailable_method_variance_dependency_error(
                        &class_name,
                        &dependency,
                    )
            {
                eg.abort_runtime_class_link(&class_name);
                return Err(VmError::Fatal(error));
            }
            if let Err(error) = eg.finalize_provisional_runtime_class(&class_name) {
                eg.abort_runtime_class_link(&class_name);
                return Err(VmError::Fatal(error));
            }
        }
        if let Err(error) = eg.finalize_provisional_runtime_class(&class_name) {
            eg.abort_runtime_class_link(&class_name);
            return Err(VmError::Fatal(error));
        }
        eg.mark_runtime_class_declared(declaration_key, class_name);
        if let Some(exception) = eg.exception.take() {
            return Ok(match throw_in_frame(eg, frame, exception)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
        return Ok(ColdResult::Done);
    }

    if !requires_provisional_publication {
        let mut unavailable_dependencies = Vec::new();
        for dependency in method_variance_dependencies {
            if eg.find_class(&dependency).is_some() {
                continue;
            }
            let loaded = crate::stdlib::autoload::ensure_symbol_loaded(eg, &dependency)?;
            if let Some(exception) = eg.exception.take() {
                eg.abort_runtime_class_link(&class_name);
                return Err(runtime_variance_dependency_exception(
                    eg,
                    &class_name,
                    class_source_file.as_deref(),
                    class_declaration_line,
                    &dependency,
                    &exception,
                ));
            }
            if !loaded {
                unavailable_dependencies.push(dependency);
            }
        }
        // PHP attempts every type needed by one variance contract before
        // naming the first still-unavailable dependency. A later loader may
        // also publish an earlier name, so recheck after the complete pass.
        for dependency in unavailable_dependencies {
            if eg.find_class(&dependency).is_none()
                && let Some(error) = eg
                    .unavailable_method_variance_dependency_error(&class_def, &dependency)
            {
                eg.abort_runtime_class_link(&class_name);
                return Err(VmError::Fatal(error));
            }
        }
    }

    for dependency in property_variance_dependencies {
        if eg.find_class(&dependency).is_some() {
            continue;
        }
        let _ = crate::stdlib::autoload::ensure_symbol_loaded(eg, &dependency)?;
        if let Some(exception) = eg.exception.take() {
            eg.abort_runtime_class_link(&class_name);
            return Err(runtime_variance_dependency_exception(
                eg,
                &class_name,
                class_source_file.as_deref(),
                class_declaration_line,
                &dependency,
                &exception,
            ));
        }
    }

    if let Err(error) = eg.register_runtime_compiled_class(class_def) {
        eg.abort_runtime_class_link(&class_name);
        // Reached enum declaration/link failures are uncatchable compile
        // fatals. Dependency-kind errors have already taken their catchable
        // Error path above; everything returned by registration here must
        // preserve prior output without the runtime-fatal newline envelope.
        if class_is_enum || class_parent_is_enum {
            return Err(VmError::CompileFatal(error));
        }
        return Err(VmError::Fatal(error));
    }
    eg.mark_runtime_class_declared(declaration_key, class_name);
    if let Some(exception) = eg.exception.take() {
        return Ok(match throw_in_frame(eg, frame, exception)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }
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

fn eval_compile_error(
    eg: &mut ExecutorGlobals,
    caller_present: bool,
    message: &str,
    source_file: &str,
) -> IncludeFileOutcome {
    let location = format!(" in {source_file} on line ");
    let (message, line) = message
        .rsplit_once(&location)
        .and_then(|(message, line)| line.parse::<usize>().ok().map(|line| (message, line)))
        .unwrap_or((message, 1));
    let error = make_error_value("CompileError", message);
    if let Some(mut object) = error.as_object_mut() {
        object.set_property("file", Value::string(source_file));
        object.set_property("line", Value::long(line as i64));
    }
    include_parse_error_value(eg, caller_present, error)
}

fn eval_parser_compile_error_is_catchable(statement: &crate::parser::Stmt) -> bool {
    let crate::parser::Stmt::ExprStmt(crate::parser::Expr::CompileError { message, .. }) = statement
    else {
        return false;
    };
    matches!(
        message.as_str(),
        "Multiple final modifiers are not allowed"
            | "Multiple access type modifiers are not allowed"
            | "Cannot use the final modifier on an abstract class"
            | "__HALT_COMPILER() can only be used from the outermost scope"
            | "Encoding must be a literal"
    )
}

fn imported_class_name(statements: &[crate::parser::Stmt], alias: &str) -> Option<String> {
    for statement in statements {
        match statement {
            crate::parser::Stmt::UseDecl { imports, .. } => {
                if let Some((_, name, _, _)) = imports
                    .iter()
                    .find(|(kind, _, imported_alias, _)| {
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
    let mut known: HashMap<String, Value> = eg
        .constant_table
        .borrow()
        .iter()
        .map(|(name, value)| (name.to_string(), value.clone()))
        .collect();
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

fn emit_source_unit_compile_deprecations(
    eg: &mut ExecutorGlobals,
    caller: Option<(*mut ExecuteData, &crate::compiler::OpArray)>,
    diagnostics: &[crate::compiler::compile::CompileDeprecation],
) -> Result<Option<IncludeFileOutcome>, VmError> {
    if let Some((frame, _)) = caller {
        eg.emit_runtime_compile_deprecations(frame, diagnostics)?;
        Ok(eg.exception.take().map(IncludeFileOutcome::Thrown))
    } else {
        eg.emit_compile_deprecations(diagnostics);
        Ok(None)
    }
}

/// Clone one variable binding across the synchronous include/eval scope
/// bridge. Ordinary `Value::clone()` intentionally reads through references,
/// while a shared PHP symbol table must retain the reference cell itself.
#[inline]
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
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
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
    // An eval/include reached while a destructor or generator finally is
    // unwinding must execute independently of the already-pending Throwable.
    // Restore it after successful source-unit execution, or append it behind
    // a newly escaping exception using the ordinary replacement contract.
    let displaced = caller.and_then(|(frame, _)| {
        take_source_unit_displaced_exception(eg, frame)
    });
    let result = execute_source_unit_inner(
        eg,
        source,
        resolved_path,
        canonical,
        implicit_return,
        record_included,
        caller,
        synthetic_trace_origin,
    );
    let Some((owner, displaced)) = displaced else {
        return result;
    };
    match result {
        Ok(IncludeFileOutcome::Thrown(replacement)) => {
            append_replaced_exception(&replacement, &displaced, eg);
            Ok(IncludeFileOutcome::Thrown(replacement))
        }
        Ok(outcome) => {
            if let Some(replacement) = eg.exception.take() {
                append_replaced_exception(&replacement, &displaced, eg);
                eg.exception = Some(replacement);
            } else {
                restore_source_unit_displaced_exception(eg, owner, displaced);
            }
            Ok(outcome)
        }
        Err(error) => {
            if let Some(replacement) = eg.exception.take() {
                append_replaced_exception(&replacement, &displaced, eg);
                eg.exception = Some(replacement);
            }
            Err(error)
        }
    }
}

fn take_source_unit_displaced_exception(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
) -> Option<(Option<usize>, Value)> {
    if let Some(exception) = eg.exception.take() {
        return Some((None, exception));
    }
    let mut current = frame;
    for _ in 0..16 {
        let owner = current as usize;
        let exception = eg
            .finally_exceptions
            .get_mut(&owner)
            .and_then(Vec::pop);
        let empty = eg
            .finally_exceptions
            .get(&owner)
            .is_some_and(Vec::is_empty);
        if empty {
            eg.finally_exceptions.remove(&owner);
        }
        if let Some(exception) = exception {
            return Some((Some(owner), exception));
        }
        // SAFETY: a synchronous eval/include retains its complete live caller
        // chain. Detached destructor/generator frames publish the same logical
        // caller relation used by backtrace collection.
        let physical = unsafe { (*current).prev_execute_data };
        let caller = eg.trace_caller(owner, physical);
        if caller.is_null() || caller == current {
            break;
        }
        current = caller;
    }
    None
}

fn restore_source_unit_displaced_exception(
    eg: &mut ExecutorGlobals,
    owner: Option<usize>,
    exception: Value,
) {
    if let Some(owner) = owner {
        eg.finally_exceptions
            .entry(owner)
            .or_default()
            .push(exception);
    } else {
        eg.exception = Some(exception);
    }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn execute_source_unit_inner(
    eg: &mut ExecutorGlobals,
    source: String,
    resolved_path: &str,
    canonical: String,
    implicit_return: Value,
    record_included: bool,
    caller: Option<(*mut ExecuteData, &crate::compiler::OpArray)>,
    synthetic_trace_origin: Option<(String, usize)>,
) -> Result<IncludeFileOutcome, VmError> {
    let source_offset_base = if synthetic_trace_origin.is_some() { 6 } else { 0 };
    let mut lexer = crate::lexer::Lexer::new(&source).with_source_offset_base(source_offset_base);
    let tokenized = if synthetic_trace_origin.is_some() {
        lexer.tokenize()
    } else {
        lexer.tokenize_included_source()
    };
    let tokens = match tokenized {
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
            let error = if synthetic_trace_origin.is_some()
                && error == "Expected expression, got Eof"
            {
                format!("syntax error, unexpected end of file in {canonical} on line 1")
            } else {
                error
            };
            if error.starts_with("syntax error,")
                || error.starts_with("Invalid indentation")
                || error.starts_with("Invalid body indentation")
                || error.starts_with("Unterminated comment starting line ")
                || error.starts_with("Unclosed '{'")
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
    // Parser-origin compile diagnostics raised from eval are catchable
    // CompileError objects. Semantic compiler fatals (for example a reserved
    // class name) remain uncatchable, exactly like the same primary source.
    let catchable_eval_compile_error = synthetic_trace_origin.is_some()
        && stmts.iter().any(eval_parser_compile_error_is_catchable);
    let mut compile_attempts = 0usize;
    let mut compile_result = loop {
        let compiler = crate::compiler::compile::Compiler::new()
            .with_zend_assertions(eg.assertion_state.startup_mode)
            .with_precision(eg.precision)
            .with_source_path(canonical.clone())
            .with_implicit_return_value(implicit_return.clone())
            .with_lexical_class_scope(caller_class.clone(), caller_parent.clone())
            .with_known_constants(compilation_constants(eg));
        match compiler.compile(&stmts) {
            Ok(result) => break result,
            Err(error) if compile_attempts < 16 => {
                let Some(owner) = unavailable_class_constant_owner(&error.message) else {
                    if let Some(outcome) =
                        emit_source_unit_compile_deprecations(eg, caller, &error.deprecations)?
                    {
                        return Ok(outcome);
                    }
                    if catchable_eval_compile_error {
                        return Ok(eval_compile_error(
                            eg,
                            caller.is_some(),
                            &error.message,
                            &canonical,
                        ));
                    }
                    return Err(VmError::CompileFatal(error.message));
                };
                let class_name = imported_class_name(&stmts, owner)
                    .unwrap_or_else(|| owner.trim_start_matches('\\').to_string());
                let loaded = eg.find_class(&class_name).is_some()
                    || crate::stdlib::autoload::ensure_symbol_loaded(eg, &class_name)?;
                if !loaded {
                    if let Some(outcome) =
                        emit_source_unit_compile_deprecations(eg, caller, &error.deprecations)?
                    {
                        return Ok(outcome);
                    }
                    if catchable_eval_compile_error {
                        return Ok(eval_compile_error(
                            eg,
                            caller.is_some(),
                            &error.message,
                            &canonical,
                        ));
                    }
                    return Err(VmError::CompileFatal(error.message));
                }
                compile_attempts += 1;
            }
            Err(error) => {
                if let Some(outcome) =
                    emit_source_unit_compile_deprecations(eg, caller, &error.deprecations)?
                {
                    return Ok(outcome);
                }
                if catchable_eval_compile_error {
                    return Ok(eval_compile_error(
                        eg,
                        caller.is_some(),
                        &error.message,
                        &canonical,
                    ));
                }
                return Err(VmError::CompileFatal(error.message));
            }
        }
    };
    if record_included {
        eg.record_included_file(canonical.clone());
    }
    if let Some(outcome) =
        emit_source_unit_compile_deprecations(eg, caller, &compile_result.deprecations)?
    {
        return Ok(outcome);
    }
    for (name, attributes) in std::mem::take(&mut compile_result.constant_attributes) {
        eg.constant_attributes.entry(name).or_insert(attributes);
    }
    for (name, expression) in std::mem::take(&mut compile_result.constant_expressions) {
        eg.constant_expressions.entry(name).or_insert(expression);
    }
    eg.refresh_constant_deprecation_metadata_presence();
    eg.bump_constant_deprecation_generation();
    if let Some(offset) = compile_result.compiler_halt_offset {
        eg.register_compiler_halt_offset(canonical.clone(), offset);
    }

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

    let mut unconditional_function_names = std::collections::HashSet::new();
    collect_unconditional_function_names(&stmts, None, &mut unconditional_function_names);
    for (name, func) in compile_result.functions {
        let boxed = Box::new(func);
        let ptr = &boxed.common as *const FunctionCommon;
        eg.included_functions.push(boxed);
        if let Err(error) = eg.register_function(&name, ptr)
            && unconditional_function_names.contains(&name.to_lowercase())
        {
            return Err(VmError::Fatal(error));
        }
    }
    for (declaration_key, class_def) in
        std::mem::take(&mut compile_result.runtime_class_defs)
    {
        eg.register_runtime_class_declaration(declaration_key, class_def)
            .map_err(VmError::Fatal)?;
    }
    for class_def in compile_result.class_defs {
        if class_def.is_anonymous() {
            let class_parent_is_enum = class_def
                .parent
                .as_deref()
                .and_then(|parent| eg.find_class(parent))
                .is_some_and(|parent| parent.is_enum);
            if let Err(error) = eg.register_compiled_class(class_def) {
                if class_parent_is_enum {
                    return Err(VmError::CompileFatal(error));
                }
                return Err(VmError::Fatal(error));
            }
            continue;
        }
        let class_is_enum = class_def.is_enum;
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
        let link_deprecations = eg.class_link_deprecations(&class_def);
        if let Some((frame, _)) = caller {
            eg.emit_runtime_compile_deprecations(frame, &link_deprecations)?;
        } else {
            eg.emit_compile_deprecations(&link_deprecations);
        }
        let mut unavailable_variance_dependencies = Vec::new();
        for dependency in eg.method_variance_dependencies(&class_def) {
            if eg.find_class(&dependency).is_some() {
                continue;
            }
            // Method-signature dependencies are also soft. Only class
            // relationships that could make the complete contract valid are
            // returned by the dependency collector.
            let loaded = crate::stdlib::autoload::ensure_symbol_loaded(eg, &dependency)?;
            if let Some(exception) = eg.exception.take() {
                if caller.is_some() {
                    return Ok(IncludeFileOutcome::Thrown(exception));
                }
                eg.exception = Some(exception);
                return Ok(IncludeFileOutcome::Executed(Value::null()));
            }
            if !loaded {
                unavailable_variance_dependencies.push(dependency);
            }
        }
        for dependency in unavailable_variance_dependencies {
            if eg.find_class(&dependency).is_none()
                && let Some(error) = eg
                    .unavailable_method_variance_dependency_error(&class_def, &dependency)
            {
                return Err(VmError::Fatal(error));
            }
        }
        for dependency in crate::runtime::property_hook_setter_variance_dependencies(eg, &class_def)
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
        let class_parent_is_enum = class_def
            .parent
            .as_deref()
            .and_then(|parent| eg.find_class(parent))
            .is_some_and(|parent| parent.is_enum);
        if let Err(error) = eg.register_class(class_def) {
            if class_is_enum || class_parent_is_enum {
                return Err(VmError::CompileFatal(error));
            }
            return Err(VmError::Fatal(error));
        }
        if let Some(exception) = eg.exception.take() {
            if caller.is_some() {
                return Ok(IncludeFileOutcome::Thrown(exception));
            }
            eg.exception = Some(exception);
            return Ok(IncludeFileOutcome::Executed(Value::null()));
        }
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
    let include_trace_origin = unsafe {
        (*inc_frame).return_value = &mut inc_return_value;
        (*inc_frame).opline = main_func.op_array.instructions.as_ptr();
        // SAFETY: source-unit execution is synchronous and the caller remains
        // parked on its live Include instruction until this helper returns.
        caller.and_then(|(caller_frame, caller_op_array)| {
            if synthetic_trace_origin.is_some() {
                return None;
            }
            let caller_opline = &*(*caller_frame).opline;
            (caller_opline.opcode == OpCode::Include).then(|| {
                let (file, line) = include_source_origin(caller_op_array, caller_opline);
                let is_require = caller_opline.extended_value & 1 != 0;
                let is_once = caller_opline.extended_value & 2 != 0;
                (file, line, is_require, is_once)
            })
        })
    };
    if let Some((caller_frame, _)) = caller {
        eg.alias_dynamic_scope(inc_frame as usize, caller_frame as usize);
        let called_class_id = called_class_id_for_frame(eg, caller_frame, 0);
        if called_class_id != 0 {
            publish_late_static_call_class_id(eg, inc_frame, called_class_id);
        }
        eg.publish_detached_trace_caller(inc_frame as usize, caller_frame as usize);
        if let Some((file, line)) = synthetic_trace_origin.as_ref() {
            eg.publish_synthetic_trace_frame(
                inc_frame as usize,
                file.clone(),
                *line,
                "eval".to_string(),
            );
        } else if let Some((file, line, is_require, is_once)) = include_trace_origin {
            eg.publish_synthetic_trace_frame(
                inc_frame as usize,
                file,
                line,
                include_call_name(is_require, is_once).to_string(),
            );
        }
    }
    if !main_func.op_array.static_vars.is_empty() {
        eg.publish_closure_static_vars(
            inc_frame as usize,
            std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
        );
    }
    if caller.is_some() {
        for (cv_idx, var_name) in &main_func.op_array.main_scope_vars {
            if let Some(val) = eg.globals.get(var_name) {
                unsafe {
                    let cv_ptr = (*inc_frame).cv_mut(*cv_idx) as *mut Value;
                    frame_slot_set(inc_frame, cv_ptr, clone_scope_binding(val));
                }
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
    if caller.is_some() {
        eg.discard_detached_trace_caller(inc_frame as usize);
    }
    unsafe { cleanup_frame_slots(inc_frame) };
    pop_vm_call_frame(eg, inc_frame);

    if let Some((frame, _)) = caller {
        for (cv_idx, var_name) in &scope_vars {
            if let Some(val) = eg.globals.get(var_name) {
                unsafe {
                    let cv_ptr = (*frame).cv_mut(*cv_idx) as *mut Value;
                    frame_slot_set(frame, cv_ptr, clone_scope_binding(val));
                }
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

#[cfg(feature = "stream-registry")]
#[cold]
fn execute_included_user_source(
    eg: &mut ExecutorGlobals,
    source: Vec<u8>,
    requested_path: &str,
    canonical: String,
    is_once: bool,
    caller: Option<(*mut ExecuteData, &crate::compiler::OpArray)>,
) -> Result<IncludeFileOutcome, VmError> {
    if is_once && eg.included_files.contains(&canonical) {
        return Ok(IncludeFileOutcome::AlreadyIncluded);
    }
    execute_source_unit(
        eg,
        crate::lexer::decode_php_source(&source),
        requested_path,
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
    let suppressed = opline._pad & crate::vm::instruction::EVAL_FLAG_ERROR_SUPPRESS != 0;
    if suppressed {
        eg.begin_error_suppression(frame as usize);
    }
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
    let outcome = execute_eval_source(eg, &source, source_name, (frame, op_array), trace_origin);
    let outcome = match outcome {
        Ok(outcome) => {
            if suppressed {
                eg.end_error_suppression(frame as usize);
            }
            outcome
        }
        Err(error) => {
            // Fatal source-unit bailout does not unwind the `@` frame before
            // PHP runs shutdown callbacks. Keep its reporting mask active.
            return Err(error);
        }
    };
    match outcome {
        IncludeFileOutcome::Executed(value) => {
            write_include_result(frame, opline, value);
            Ok(ColdResult::Done)
        }
        IncludeFileOutcome::Thrown(exception) => Ok(match throw_in_frame(eg, frame, exception)? {
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

/// Return the public PHP spelling for the active include opcode.
fn include_call_name(is_require: bool, is_once: bool) -> &'static str {
    match (is_require, is_once) {
        (false, false) => "include",
        (false, true) => "include_once",
        (true, false) => "require",
        (true, true) => "require_once",
    }
}

fn include_origin_index(
    op_array: &crate::compiler::OpArray,
    opline: &crate::vm::instruction::Instruction,
) -> usize {
    let ip = op_array
        .instructions
        .iter()
        .position(|instruction| std::ptr::eq(instruction, opline))
        .expect("active include instruction belongs to its op array");
    if op_array.source_line(ip).is_some() {
        ip
    } else {
        (0..ip)
            .rev()
            .find(|index| op_array.source_line(*index).is_some())
            .unwrap_or(ip)
    }
}

fn include_source_origin(
    op_array: &crate::compiler::OpArray,
    opline: &crate::vm::instruction::Instruction,
) -> (String, usize) {
    let file = if op_array.source_file.is_empty() {
        op_array.name.clone()
    } else {
        op_array.source_file.to_string()
    };
    let ip = include_origin_index(op_array, opline);
    (file, op_array.source_line(ip).unwrap_or(0))
}

fn source_unit_filesystem_directory(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
) -> std::path::PathBuf {
    fn op_array_directory(op_array: &crate::compiler::OpArray) -> Option<std::path::PathBuf> {
        let source = if !op_array.source_file.is_empty() {
            op_array.source_file.as_str()
        } else {
            op_array.name.as_str()
        };
        if source.is_empty()
            || source == "<main>"
            || source.contains(" : eval()'d code")
            || source.contains("://")
        {
            return None;
        }
        let path = std::path::Path::new(source);
        if op_array.source_file.is_empty()
            && !path.is_absolute()
            && path.components().count() == 1
            && !path.is_file()
        {
            return None;
        }
        path.parent().map(std::path::Path::to_path_buf)
    }

    if let Some(directory) = op_array_directory(op_array) {
        return directory;
    }
    let mut current = frame;
    for _ in 0..16 {
        // SAFETY: include resolution walks only the live physical/detached
        // caller chain. Each returned caller owns a live user op array until
        // synchronous source-unit execution returns to this frame.
        let (caller, caller_op_array) = unsafe {
            let physical = (*current).prev_execute_data;
            let caller = eg.trace_caller(current as usize, physical);
            if caller.is_null() || caller == current {
                break;
            }
            (caller, (*caller).op_array())
        };
        if let Some(directory) = op_array_directory(caller_op_array) {
            return directory;
        }
        current = caller;
    }
    std::env::current_dir().unwrap_or_default()
}

fn report_include_warning(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &crate::vm::instruction::Instruction,
    function: &str,
    message: &str,
) -> Result<(), VmError> {
    let (file, line) = include_source_origin(op_array, opline);
    let publish = eg.detached_trace_origin(frame as usize).is_none();
    if publish {
        eg.publish_synthetic_trace_frame(
            frame as usize,
            file.clone(),
            line,
            function.to_string(),
        );
    }
    let handled = crate::stdlib::dispatch_php_error(eg, frame, 2, message, &file, line)?;
    if publish {
        eg.discard_detached_trace_origin(frame as usize);
    }
    if !handled {
        eg.record_last_error(2, message, &file, line);
    }
    if !handled && eg.error_reporting & 2 != 0 {
        eg.write_output(format!("\nWarning: {message} in {file} on line {line}\n").as_bytes());
    }
    Ok(())
}

fn include_failure<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &crate::vm::instruction::Instruction,
    path: &str,
    class: Option<&str>,
    is_require: bool,
    is_once: bool,
) -> Result<ColdResult<'a>, VmError> {
    let function = include_call_name(is_require, is_once);
    let reason = class.map_or_else(
        || "No such file or directory".to_string(),
        |class| format!("\"{class}::stream_open\" call failed"),
    );
    report_include_warning(
        eg,
        frame,
        op_array,
        opline,
        function,
        &format!("{function}({path}): Failed to open stream: {reason}"),
    )?;
    if let Some(exception) = eg.exception.take() {
        return Ok(match throw_in_frame(eg, frame, exception)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }

    #[cfg(feature = "include-path")]
    let include_path = crate::stdlib::include_path::current(eg);
    #[cfg(not(feature = "include-path"))]
    let include_path = ".";
    let second = if is_require {
        format!(
            "Failed opening required '{path}' (include_path='{include_path}')"
        )
    } else {
        format!(
            "{function}(): Failed opening '{path}' for inclusion (include_path='{include_path}')"
        )
    };
    if !is_require {
        report_include_warning(eg, frame, op_array, opline, function, &second)?;
        if let Some(exception) = eg.exception.take() {
            return Ok(match throw_in_frame(eg, frame, exception)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
        write_include_result(frame, opline, Value::bool(false));
        // SAFETY: `op_include` supplies the live frame for this exact include
        // instruction. The dispatcher owns the instruction slice and the
        // include opcode has a following dispatch position, so advancing once
        // is the same bounded transition used by the ordinary include miss.
        unsafe { (*frame).opline = (*frame).opline.add(1) };
        return Ok(ColdResult::Continue);
    }

    let exception = make_error_value("Error", &second);
    attach_throwable_origin(
        &exception,
        eg,
        frame,
        op_array,
        include_origin_index(op_array, opline),
    );
    Ok(match throw_in_frame(eg, frame, exception)? {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
    })
}

/// Returns true if the caller should `continue` (skip opline advance).
#[inline(never)]
fn op_include<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &crate::vm::instruction::Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: opcode dispatch supplies a live frame and an operand descriptor
    // belonging to this op-array. Clone the dereferenced value before any
    // object conversion callback can re-enter and mutate the source slot.
    let path_val = unsafe {
        (&*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array))
            .dereferenced()
            .clone()
    };
    let path_str = if path_val.value_type() == ValueType::Object {
        let class_name = path_val
            .as_object()
            .map(|object| object.class_name.to_string())
            .unwrap_or_else(|| "object".to_string());
        let rendered = call_object_string_conversion(eg, &path_val)?;
        if let Some(exception) = eg.exception.take() {
            return Ok(match throw_in_frame(eg, frame, exception)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
        let Some(rendered) = rendered else {
            let error = make_error_value(
                "Error",
                &format!("Object of class {class_name} could not be converted to string"),
            );
            return Ok(match throw_in_frame(eg, frame, error)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        };
        rendered
            .as_str()
            .expect("canonical object string conversion returns String")
            .to_string()
    } else {
        path_val.echo_to_string()
    };
    let is_require = (opline.extended_value & 1) != 0;
    let is_once = (opline.extended_value & 2) != 0;

    #[cfg(feature = "stream-registry")]
    let mut wrapper_candidate = crate::stdlib::user_wrapper::definition_for_url(
        eg,
        &path_str,
    )
    .map(|_| path_str.clone());
    #[cfg(feature = "stream-registry")]
    let mut searched_user_include_path = false;

    #[cfg(all(feature = "stream-registry", feature = "include-path"))]
    if wrapper_candidate.is_none()
        && !std::path::Path::new(&path_str).is_absolute()
        && !path_str.starts_with("./")
        && !path_str.starts_with("../")
        && !path_str.contains("://")
    {
        for candidate in crate::stdlib::include_path::search_candidates(eg, &path_str) {
            match crate::stdlib::user_wrapper::url_stat(eg, &candidate, 6)? {
                Some(true) => {
                    searched_user_include_path = true;
                    wrapper_candidate = Some(candidate);
                    break;
                }
                Some(false) => {
                    searched_user_include_path = true;
                    if let Some(exception) = eg.exception.take() {
                        return Ok(match throw_in_frame(eg, frame, exception)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                ColdResult::NewFrame(new_frame, new_op_array)
                            }
                            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                        });
                    }
                }
                None if std::path::Path::new(&candidate).exists() => break,
                None => {}
            }
        }
    }

    #[cfg(feature = "stream-registry")]
    if let Some(candidate) = wrapper_candidate {
        let opened =
            crate::stdlib::user_wrapper::open_include_source(eg, &candidate)?;
        if let Some(exception) = eg.exception.take() {
            return Ok(match throw_in_frame(eg, frame, exception)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
        match opened {
            crate::stdlib::user_wrapper::IncludeOpenResult::Opened {
                source,
                canonical,
            } => {
                let outcome = execute_included_user_source(
                    eg,
                    source,
                    &candidate,
                    canonical,
                    is_once,
                    Some((frame, op_array)),
                )?;
                return match outcome {
                    IncludeFileOutcome::Executed(value) => {
                        write_include_result(frame, opline, value);
                        Ok(ColdResult::Done)
                    }
                    IncludeFileOutcome::AlreadyIncluded => {
                        write_include_result(frame, opline, Value::bool(true));
                        Ok(ColdResult::Done)
                    }
                    IncludeFileOutcome::Thrown(exception) => {
                        Ok(match throw_in_frame(eg, frame, exception)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                ColdResult::NewFrame(new_frame, new_op_array)
                            }
                            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                        })
                    }
                    IncludeFileOutcome::Missing(_) => unreachable!(
                        "user-wrapper source is already materialized before compilation"
                    ),
                };
            }
            crate::stdlib::user_wrapper::IncludeOpenResult::Declined { class } => {
                return include_failure(
                    eg,
                    frame,
                    op_array,
                    opline,
                    &path_str,
                    Some(&class),
                    is_require,
                    is_once,
                );
            }
            crate::stdlib::user_wrapper::IncludeOpenResult::NotRegistered => {}
        }
    }

    #[cfg(not(feature = "include-path"))]
    let resolved_path = if std::path::Path::new(&path_str).is_absolute() {
        path_str.clone()
    } else {
        let base_dir = source_unit_filesystem_directory(eg, frame, op_array);
        base_dir.join(&path_str).to_string_lossy().into_owned()
    };
    #[cfg(feature = "include-path")]
    let resolved_path = if let Some(path) = crate::stdlib::include_path::resolve_existing(eg, &path_str) {
        path
    } else if std::path::Path::new(&path_str).is_absolute() {
        path_str.clone()
    } else {
        let base_dir = source_unit_filesystem_directory(eg, frame, op_array);
        base_dir.join(&path_str).to_string_lossy().into_owned()
    };

    let outcome = execute_included_file(eg, &resolved_path, is_once, Some((frame, op_array)))?;
    #[cfg(feature = "stream-registry")]
    if searched_user_include_path && matches!(outcome, IncludeFileOutcome::Missing(_)) {
        return include_failure(
            eg,
            frame,
            op_array,
            opline,
            &path_str,
            None,
            is_require,
            is_once,
        );
    }
    match outcome {
        IncludeFileOutcome::Executed(value) => {
            write_include_result(frame, opline, value);
            Ok(ColdResult::Done)
        }
        IncludeFileOutcome::AlreadyIncluded => {
            write_include_result(frame, opline, Value::bool(true));
            Ok(ColdResult::Done)
        }
        IncludeFileOutcome::Thrown(exception) => Ok(match throw_in_frame(eg, frame, exception)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        }),
        IncludeFileOutcome::Missing(error) => {
            // Preserve the backend error payload for callers which inspect
            // IncludeFileOutcome while presenting PHP's canonical reason here.
            let _kind = error.kind();
            include_failure(
                eg,
                frame,
                op_array,
                opline,
                &path_str,
                None,
                is_require,
                is_once,
            )
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
    let (val, instruction_index, valid_throwable) = unsafe {
        let source = (*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array);
        let valid_throwable = (&*source).as_object().is_some_and(|object| {
            eg.class_is_a(&object.class_name, "Throwable")
        });
        let consume_temporary = opline._pad & THROW_FLAG_UNHANDLED_MATCH == 0
            && matches!(opline.op1_type, OpType::Tmp | OpType::Var)
            && valid_throwable;
        let value = if consume_temporary {
            // A throw consumes its temporary expression owner. Keeping that
            // slot alive until frame teardown would postpone a no-variable
            // catch's last Throwable release until after the catch body.
            let writable_source = (*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
            frame_tmp_take!(frame, writable_source)
        } else {
            (&*source).clone()
        };
        (
            value,
            (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize,
            valid_throwable,
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
        return Ok(match throw_in_frame(eg, frame, error)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }
    // PHP validates the operand through a normal catchable Error at the throw
    // opcode. Scalar types and class names are intentionally absent from the
    // public PHP 8.2 messages.
    if !valid_throwable {
        let message = if val.value_type() == ValueType::Object {
            "Cannot throw objects that do not implement Throwable"
        } else {
            "Can only throw objects"
        };
        let error = make_error_value("Error", message);
        attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
        return Ok(match throw_in_frame(eg, frame, error)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }
    let thrown = val;
    attach_throwable_origin(&thrown, eg, frame, op_array, instruction_index);

    match throw_in_frame(eg, frame, thrown)? {
        ThrowResult::Handled(new_frame, new_op_array) => {
            Ok(ColdResult::NewFrame(new_frame, new_op_array))
        }
        ThrowResult::Unhandled(exc) => {
            Ok(ColdResult::Unhandled(exc))
        }
    }
}
