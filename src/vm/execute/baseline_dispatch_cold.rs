// Included in the execute module so cold opcode helpers keep private access to
// the canonical frame machinery without adding abstractions to hot dispatch.

/// Publish globals dirtied by a recursively invoked callback back into the
/// suspended caller's tracked CVs. Ordinary VM calls do this while unwinding;
/// callbacks entered directly from an opcode or stdlib handler return across
/// an execution boundary and need the same synchronization explicitly.
pub(crate) fn sync_dirty_globals_to_frame(eg: &mut ExecutorGlobals, frame: &mut ExecuteData) {
    if eg.dirty_globals.is_empty() {
        return;
    }
    // SAFETY: `frame` is borrowed from the active executor and every `cv` was
    // published by that same frame's immutable op-array. The canonical slot
    // writer keeps heap cleanup metadata synchronized with the replacement.
    unsafe {
        let vars = {
            let op_array = frame.op_array();
            if !op_array.main_scope_vars.is_empty() {
                op_array.main_scope_vars.clone()
            } else {
                op_array.global_vars.clone()
            }
        };
        for (cv, name) in &vars {
            if eg.dirty_globals.contains(name)
                && let Some(value) = eg.globals.get(name).cloned()
            {
                let slot = frame.cv_mut(*cv) as *mut Value;
                if (*slot).is_reference() {
                    slot_set((*slot).as_ref_ptr(), value);
                } else {
                    frame_slot_set(frame, slot, value);
                }
            }
        }
        if !vars.is_empty() {
            eg.dirty_globals.clear();
        }
    }
}

/// Emit the PHP 8.2 undefined-local diagnostic for one already-snapshotted
/// read. The caller owns control-flow handling when a user handler throws.
fn report_undefined_variable_read(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    name_literal: u16,
    suppressed: bool,
) -> Result<(), VmError> {
    let name = op_array.literals()[name_literal as usize]
        .as_str()
        .unwrap_or("");
    report_undefined_variable_name(eg, frame, op_array, opline, name, suppressed)
}

fn report_undefined_variable_name(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    name: &str,
    suppressed: bool,
) -> Result<(), VmError> {
    report_php_warning(
        eg,
        frame,
        op_array,
        opline,
        &format!("Undefined variable ${name}"),
        suppressed,
    )
}

fn report_php_warning(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    message: &str,
    suppressed: bool,
) -> Result<(), VmError> {
    report_php_diagnostic(eg, frame, op_array, opline, message, 2, "Warning", suppressed)
}

fn report_php_diagnostic(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    message: &str,
    level: i64,
    label: &str,
    suppressed: bool,
) -> Result<(), VmError> {
    let instruction_index = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let line = op_array.source_line(instruction_index).unwrap_or(0);
    let file = if op_array.source_file.is_empty() {
        op_array.name.as_str()
    } else {
        op_array.source_file.as_str()
    };
    if suppressed {
        eg.begin_error_suppression(frame as usize);
    }
    let handled = crate::stdlib::dispatch_php_error(eg, frame, level, message, file, line);
    // Decide whether the built-in diagnostic is visible while the suppression
    // scope is still active. Restoring the caller's mask first would make an
    // ordinary `@$missing` warn merely because the outer mask contains
    // E_WARNING. A handler may explicitly re-enable E_WARNING inside `@`, in
    // which case PHP does expose the declined built-in diagnostic.
    let report_builtin = eg.error_reporting & level != 0;
    if suppressed {
        eg.end_error_suppression(frame as usize);
    }
    let handled = handled?;
    if !handled {
        eg.record_last_error(level, message, file, line);
    }
    if !handled && report_builtin {
        eg.write_output(format!("\n{label}: {message} in {file} on line {line}\n").as_bytes());
    }
    Ok(())
}

fn deprecated_use_site(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> crate::stdlib::reflection::DeprecatedUseSite {
    let instruction_index = op_array
        .instructions
        .iter()
        .position(|instruction| std::ptr::eq(instruction, opline))
        .expect("active deprecated-use instruction belongs to its op array");
    crate::stdlib::reflection::DeprecatedUseSite {
        frame,
        file: if op_array.source_file.is_empty() {
            op_array.name.clone()
        } else {
            op_array.source_file.to_string()
        },
        line: op_array.source_line(instruction_index).unwrap_or(0),
    }
}

fn report_php_notice(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    message: &str,
) -> Result<(), VmError> {
    report_php_diagnostic(eg, frame, op_array, opline, message, 8, "Notice", false)
}

fn report_php_deprecation(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    message: &str,
) -> Result<(), VmError> {
    report_php_diagnostic(
        eg,
        frame,
        op_array,
        opline,
        message,
        8192,
        "Deprecated",
        false,
    )
}

#[cold]
fn report_user_call_diagnostic(
    eg: &mut ExecutorGlobals,
    caller: *mut ExecuteData,
    source_override: Option<(&str, usize)>,
    diagnostic: &str,
    level: i64,
    label: &str,
) -> Result<(), VmError> {
    if source_override.is_none() {
        // Detached callbacks may suspend an internal handler. Walk to the
        // nearest user frame so callback-forwarded calls retain their PHP
        // source site unless the caller supplied a synthetic origin.
        let mut source_frame = caller;
        // SAFETY: `caller` and every predecessor are live synchronous frames.
        // Registered metadata outlives them, and an opline is dereferenced
        // only after its index is proven inside that user op-array.
        unsafe {
            while !source_frame.is_null() {
                let source_function = (*source_frame).func;
                if !source_function.is_null() && (*source_function).fn_type == FunctionType::User {
                    let source_user = &*(source_function as *const UserFunction);
                    let source_op_array = &source_user.op_array;
                    let source_opline = (*source_frame).opline;
                    if !source_opline.is_null() {
                        let index = source_opline.offset_from(source_op_array.instructions.as_ptr());
                        if index >= 0 && (index as usize) < source_op_array.instructions.len() {
                            return report_php_diagnostic(
                                eg,
                                source_frame,
                                source_op_array,
                                &*source_opline,
                                diagnostic,
                                level,
                                label,
                                false,
                            );
                        }
                    }
                }
                source_frame = (*source_frame).prev_execute_data;
            }
        }
    }

    let (file, line) = source_override.unwrap_or(("Unknown", 0));
    let handled = crate::stdlib::dispatch_php_error(
        eg,
        caller,
        level,
        diagnostic,
        file,
        line,
    )?;
    if !handled {
        eg.record_last_error(level, diagnostic, file, line);
    }
    if !handled && eg.error_reporting & level != 0 {
        eg.write_output(
            format!("\n{label}: {diagnostic} in {file} on line {line}\n").as_bytes(),
        );
    }
    Ok(())
}

/// Materialize PHP's built-in `#[Deprecated]` attribute and report the
/// declaration-specific E_USER_DEPRECATED diagnostic before call validation.
/// Attribute arguments may depend on runtime constants, so this stays on the
/// cold attempted-call boundary rather than being folded into declaration
/// registration.
#[cold]
fn report_deprecated_user_call(
    eg: &mut ExecutorGlobals,
    caller: *mut ExecuteData,
    function: *const FunctionCommon,
    call_key: Option<usize>,
    source_override: Option<(&str, usize)>,
) -> Result<(), VmError> {
    if function.is_null() {
        return Ok(());
    }
    // SAFETY: both entry paths pass a registered function descriptor whose
    // storage outlives the synchronous diagnostic attempt.
    let descriptor = unsafe { Function::from_common_ptr(function) };
    if descriptor.fn_type() != FunctionType::User {
        return Ok(());
    }
    let Some((definition, repeated)) = descriptor.dispatch(
        |user| {
            let mut definitions = user
                .attributes
                .iter()
                .filter(|attribute| attribute.name.eq_ignore_ascii_case("Deprecated"));
            definitions
                .next()
                .cloned()
                .map(|definition| (definition, definitions.next().is_some()))
        },
        |_| None,
    ) else {
        return Ok(());
    };

    let mut instance = Value::undef();
    crate::stdlib::reflection::instantiate_attribute_definition(
        caller,
        &mut instance,
        &definition,
        repeated,
        eg,
    )?;
    if eg.exception.is_some() {
        return Ok(());
    }

    let (message, since) = instance
        .as_object()
        .map(|object| {
            let message = object
                .get_property("message")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            let since = object
                .get_property("since")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            (message, since)
        })
        .unwrap_or((None, None));
    let mut name = displayed_function_name(eg, function);
    if let Some(method) = call_key.and_then(|key| pending_magic_call_name(eg, key))
        && let Some((class, implementation)) = name.rsplit_once("::")
        && matches!(implementation.to_ascii_lowercase().as_str(), "__call" | "__callstatic")
    {
        name = format!("{class}::{method}");
    }
    let noun = if name.contains("{closure") || !name.contains("::") {
        "Function"
    } else {
        "Method"
    };
    let mut diagnostic = format!("{noun} {name}() is deprecated");
    if let Some(since) = since {
        diagnostic.push_str(" since ");
        diagnostic.push_str(&since);
    }
    if let Some(message) = message {
        diagnostic.push_str(", ");
        diagnostic.push_str(&message);
    }

    report_user_call_diagnostic(
        eg,
        caller,
        source_override,
        &diagnostic,
        16_384,
        "Deprecated",
    )
}

/// Materialize PHP's built-in `#[NoDiscard]` attribute before entering a user
/// callable whose result is syntactically unused. Attribute type errors and a
/// throwing E_USER_WARNING handler therefore stop the call before its body.
#[cold]
fn report_no_discard_user_call(
    eg: &mut ExecutorGlobals,
    caller: *mut ExecuteData,
    user: &UserFunction,
    call_key: Option<usize>,
    source_override: Option<(&str, usize)>,
) -> Result<(), VmError> {
    let mut definitions = user
        .attributes
        .iter()
        .filter(|attribute| attribute.name.eq_ignore_ascii_case("NoDiscard"));
    let Some(definition) = definitions.next().cloned() else {
        return Ok(());
    };
    let repeated = definitions.next().is_some();

    let mut instance = Value::undef();
    crate::stdlib::reflection::instantiate_attribute_definition(
        caller,
        &mut instance,
        &definition,
        repeated,
        eg,
    )?;
    if eg.exception.is_some() {
        return Ok(());
    }

    let message = instance.as_object().and_then(|object| {
        object
            .get_property("message")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    });
    let function = &user.common as *const FunctionCommon;
    let mut name = if user.op_array.name.starts_with("__closure_")
        || user
            .op_array
            .name
            .rsplit_once("::")
            .is_some_and(|(_, method)| method.starts_with("__closure_"))
    {
        displayed_function_name(eg, function)
    } else {
        user.op_array.name.clone()
    };
    if let Some(method) = call_key.and_then(|key| pending_magic_call_name(eg, key))
        && let Some((class, implementation)) = name.rsplit_once("::")
        && matches!(implementation.to_ascii_lowercase().as_str(), "__call" | "__callstatic")
    {
        name = format!("{class}::{method}");
    }
    let noun = if name.contains("{closure") || !name.contains("::") {
        "function"
    } else {
        "method"
    };
    let mut diagnostic = format!(
        "The return value of {noun} {name}() should either be used or intentionally ignored by casting it as (void)"
    );
    if let Some(message) = message {
        diagnostic.push_str(", ");
        diagnostic.push_str(&message);
    }
    report_user_call_diagnostic(
        eg,
        caller,
        source_override,
        &diagnostic,
        512,
        "Warning",
    )
}

fn scalar_dynamic_variable_name(value: &Value) -> Result<String, VmError> {
    Ok(match value.value_type() {
        ValueType::Undef | ValueType::Null | ValueType::False => String::new(),
        ValueType::True => "1".to_string(),
        ValueType::Long | ValueType::Double | ValueType::String | ValueType::Resource => {
            value.echo_to_string()
        }
        ValueType::Array => "Array".to_string(),
        ValueType::Object => unreachable!("object names require VM re-entry"),
        other => return Err(VmError::Fatal(format!("Cannot convert {other:?} to string"))),
    })
}

fn dynamic_scope_frame(eg: &ExecutorGlobals, frame: *mut ExecuteData) -> *mut ExecuteData {
    eg.dynamic_scope_owner(frame as usize) as *mut ExecuteData
}

fn dynamic_scope_cv(
    frame: *mut ExecuteData,
    name: &str,
) -> Option<u32> {
    if frame.is_null() {
        return None;
    }
    if name == "this" {
        let function = unsafe { (*frame).func };
        if !function.is_null() && unsafe { (*function).sig.this_offset == 1 } {
            return Some(0);
        }
    }
    // SAFETY: dynamic-scope owners are live call frames. Their user op-array
    // metadata outlives execution and each advertised CV belongs to the same
    // frame allocation.
    let op_array = unsafe { (*frame).op_array() };
    op_array
        .all_cvs
        .iter()
        .find(|(_, candidate)| candidate == name)
        .map(|(cv, _)| *cv)
}

fn dynamic_scope_is_global(frame: *mut ExecuteData) -> bool {
    // Included frames are first resolved to their owner. The remaining root
    // frame is the request-global script scope; ordinary function frames have
    // a live predecessor.
    unsafe { !frame.is_null() && (*frame).prev_execute_data.is_null() }
}

enum CallerScopeOperation<'a> {
    Read(&'a str),
    Write(&'a str, Value),
    Snapshot,
}

enum CallerScopeResult {
    Value(Option<Value>),
    Written(bool),
    Variables(PhpArray),
}

/// Execute every caller-symbol-table operation behind one audited raw-frame
/// boundary. The caller frame and its dynamic-scope owner remain live for the
/// synchronous internal call; all_cvs proves CV indices and frame_slot_set
/// maintains heap ownership metadata for writes.
fn caller_scope_operation(
    eg: &mut ExecutorGlobals,
    internal_frame: *mut ExecuteData,
    operation: CallerScopeOperation<'_>,
) -> CallerScopeResult {
    // SAFETY: the invariant above covers every dereference in this block. No
    // frame pointer or borrowed slot escapes the operation.
    unsafe {
        let caller = (*internal_frame).prev_execute_data;
        if caller.is_null() {
            return match operation {
                CallerScopeOperation::Read(_) => CallerScopeResult::Value(None),
                CallerScopeOperation::Write(name, _) => {
                    CallerScopeResult::Written(name != "this")
                }
                CallerScopeOperation::Snapshot => {
                    CallerScopeResult::Variables(PhpArray::new())
                }
            };
        }
        let owner = dynamic_scope_frame(eg, caller);
        match operation {
            CallerScopeOperation::Read(name) => {
                let value = if let Some(cv) = dynamic_scope_cv(owner, name) {
                    let value = &*(*owner).get_op_ptr(cv, OpType::Cv, (*owner).op_array());
                    (!value.is_undef()).then(|| value.clone())
                } else if dynamic_scope_is_global(owner) {
                    eg.globals
                        .get(name)
                        .filter(|value| !value.is_undef())
                        .cloned()
                } else {
                    eg.dynamic_variables
                        .get(&(owner as usize))
                        .and_then(|variables| variables.get(name))
                        .filter(|value| !value.is_undef())
                        .cloned()
                };
                CallerScopeResult::Value(value)
            }
            CallerScopeOperation::Write(name, value) => {
                if name == "this" {
                    return CallerScopeResult::Written(false);
                }
                if let Some(cv) = dynamic_scope_cv(owner, name) {
                    frame_slot_set(owner, (*owner).cv_mut(cv), value);
                } else if dynamic_scope_is_global(owner) {
                    globals_assign(&mut eg.globals, name, value);
                    eg.dirty_globals.insert(name.to_string());
                } else {
                    globals_assign(
                        eg.dynamic_variables.entry(owner as usize).or_default(),
                        name,
                        value,
                    );
                }
                CallerScopeResult::Written(true)
            }
            CallerScopeOperation::Snapshot => {
                let mut result = PhpArray::new();
                let op_array = (*owner).op_array();
                for (cv, name) in &op_array.all_cvs {
                    let value = &*(*owner).get_op_ptr(*cv, OpType::Cv, op_array);
                    if !value.is_undef() {
                        result.set_str(name, value.clone());
                    }
                }
                let extra = if dynamic_scope_is_global(owner) {
                    Some(&eg.globals)
                } else {
                    eg.dynamic_variables.get(&(owner as usize))
                };
                if let Some(extra) = extra {
                    for (name, value) in extra {
                        if !value.is_undef() && result.get_str(name).is_none() {
                            result.set_str(name, value.clone());
                        }
                    }
                }
                CallerScopeResult::Variables(result)
            }
        }
    }
}

pub(crate) fn caller_scope_variable(
    eg: &mut ExecutorGlobals,
    internal_frame: *mut ExecuteData,
    name: &str,
) -> Option<Value> {
    let CallerScopeResult::Value(value) = caller_scope_operation(
        eg,
        internal_frame,
        CallerScopeOperation::Read(name),
    ) else {
        unreachable!();
    };
    value
}

pub(crate) fn set_caller_scope_variable(
    eg: &mut ExecutorGlobals,
    internal_frame: *mut ExecuteData,
    name: &str,
    value: Value,
) -> bool {
    let CallerScopeResult::Written(written) = caller_scope_operation(
        eg,
        internal_frame,
        CallerScopeOperation::Write(name, value),
    ) else {
        unreachable!();
    };
    written
}

pub(crate) fn caller_scope_variables(
    eg: &mut ExecutorGlobals,
    internal_frame: *mut ExecuteData,
) -> PhpArray {
    let CallerScopeResult::Variables(variables) =
        caller_scope_operation(eg, internal_frame, CallerScopeOperation::Snapshot)
    else {
        unreachable!();
    };
    variables
}

#[inline(never)]
fn op_dynamic_variable<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: `frame` is the active execute frame and `opline.op1` names a
    // compiler-allocated live operand. The retained-name path additionally
    // writes only its compiler-owned TMP before any handler can reuse it.
    let name = unsafe {
        let raw_key =
            (&*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)).clone();
        let key = reference_initial_value(raw_key);
        let name = if key.value_type() == ValueType::Object {
        let class_name = key
            .as_object()
            .map(|object| object.class_name.to_string())
            .unwrap_or_else(|| "object".to_string());
        let rendered = call_magic_method(eg, &key, "__tostring", &[])?;
        if let Some(exception) = eg.exception.take() {
            return Ok(match throw_in_frame(eg, frame, exception)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        }
        let Some(rendered) = rendered else {
            return Ok(static_property_throw(
                eg,
                frame,
                "Error",
                format!("Object of class {class_name} could not be converted to string"),
            )?);
        };
        let Some(rendered) = rendered.as_str() else {
            return Ok(static_property_throw(
                eg,
                frame,
                "TypeError",
                format!(
                    "{class_name}::__toString(): Return value must be of type string"
                ),
            )?);
        };
            rendered.to_string()
        } else {
            if key.value_type() == ValueType::Array {
                report_php_warning(
                    eg,
                    frame,
                    op_array,
                    opline,
                    "Array to string conversion",
                    opline._pad & FETCH_DYNAMIC_ERROR_SUPPRESS != 0,
                )?;
            }
            scalar_dynamic_variable_name(&key)?
        };
        if opline.opcode == OpCode::FetchDynamicVar
            && opline._pad & FETCH_DYNAMIC_RETAIN_NAME != 0
        {
            debug_assert_eq!(opline.op1_type, OpType::Tmp);
            let retained = (*frame).get_op_mut(opline.op1 as u32, OpType::Tmp);
            frame_slot_set(frame, retained, Value::string(name.clone()));
        }
        name
    };
    let owner = dynamic_scope_frame(eg, frame);
    let direct_cv = dynamic_scope_cv(owner, &name);
    let global_scope = dynamic_scope_is_global(owner);

    if name == "this"
        && matches!(
            opline.opcode,
            OpCode::AssignDynamicVar
                | OpCode::UnsetDynamicVar
                | OpCode::BindDynamicVarRef
                | OpCode::AssignDynamicVarRef
                | OpCode::BindDynamicGlobal
        )
    {
        return Ok(static_property_throw(
            eg,
            frame,
            "Error",
            "Cannot re-assign $this".to_string(),
        )?);
    }

    match opline.opcode {
        OpCode::FetchDynamicVar => {
            let value = if let Some(cv) = direct_cv {
                unsafe { (&*(*owner).get_op_ptr(cv, OpType::Cv, (*owner).op_array())).clone() }
            } else if global_scope {
                eg.globals.get(&name).cloned().unwrap_or_else(Value::undef)
            } else {
                eg.dynamic_variables
                    .get(&(owner as usize))
                    .and_then(|variables| variables.get(&name))
                    .cloned()
                    .unwrap_or_else(Value::undef)
            };
            let value = if opline._pad & FETCH_DIM_ISSET != 0 {
                Value::bool(!matches!(value.value_type(), ValueType::Null | ValueType::Undef))
            } else if value.is_undef() {
                if opline._pad & FETCH_DYNAMIC_SILENT == 0 {
                    report_undefined_variable_name(
                        eg,
                        frame,
                        op_array,
                        opline,
                        &name,
                        opline._pad & FETCH_DYNAMIC_ERROR_SUPPRESS != 0,
                    )?;
                }
                Value::null()
            } else {
                value
            };
            let result = unsafe {
                (*frame).get_op_mut(opline.result as u32, opline.result_type)
            };
            write_fetch_dim_result(frame, result, value);
        }
        OpCode::AssignDynamicVar => {
            let mut value = unsafe {
                (&*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)).clone()
            };
            if let Some(cv) = direct_cv {
                // SAFETY: direct_cv was resolved from this live owner frame;
                // validation finishes before the dereferenced target write.
                unsafe {
                    let raw = (*owner).cv_mut(cv);
                    if raw.is_reference() {
                        let constraints = raw.reference_property_constraints();
                        value = match prepare_reference_assignment(
                            value,
                            &constraints,
                            eg,
                            op_array.strict_types,
                        ) {
                            Ok(value) => value,
                            Err(message) => {
                                return Ok(static_property_throw(
                                    eg,
                                    frame,
                                    "TypeError",
                                    message,
                                )?);
                            }
                        };
                        slot_set((*owner).get_op_mut(cv, OpType::Cv), value);
                    } else {
                        frame_slot_set(owner, raw, value);
                    }
                }
            } else if global_scope {
                let constraints = eg
                    .globals
                    .get(&name)
                    .map(Value::reference_property_constraints)
                    .unwrap_or_default();
                value = match prepare_reference_assignment(
                    value,
                    &constraints,
                    eg,
                    op_array.strict_types,
                ) {
                    Ok(value) => value,
                    Err(message) => {
                        return Ok(static_property_throw(
                            eg,
                            frame,
                            "TypeError",
                            message,
                        )?);
                    }
                };
                globals_assign(&mut eg.globals, &name, value);
                eg.dirty_globals.insert(name);
            } else {
                let constraints = eg
                    .dynamic_variables
                    .get(&(owner as usize))
                    .and_then(|variables| variables.get(&name))
                    .map(Value::reference_property_constraints)
                    .unwrap_or_default();
                value = match prepare_reference_assignment(
                    value,
                    &constraints,
                    eg,
                    op_array.strict_types,
                ) {
                    Ok(value) => value,
                    Err(message) => {
                        return Ok(static_property_throw(
                            eg,
                            frame,
                            "TypeError",
                            message,
                        )?);
                    }
                };
                let variables = eg.dynamic_variables.entry(owner as usize).or_default();
                globals_assign(variables, &name, value);
            }
        }
        OpCode::UnsetDynamicVar => {
            if let Some(cv) = direct_cv {
                unsafe { frame_slot_set(owner, (*owner).cv_mut(cv), Value::undef()) };
            } else if global_scope {
                globals_set(&mut eg.globals, &name, Value::undef());
                eg.dirty_globals.insert(name);
            } else if let Some(variables) = eg.dynamic_variables.get_mut(&(owner as usize)) {
                variables.remove(&name);
            }
        }
        OpCode::BindDynamicVarRef => {
            let mut binding = if let Some(cv) = direct_cv {
                unsafe {
                    let slot = (*owner).cv_mut(cv);
                    if slot.is_owned_reference() {
                        slot.clone_owned_reference_alias()
                    } else {
                        let owned = Value::owned_reference(reference_initial_value(slot.clone()));
                        let alias = owned.clone_owned_reference_alias();
                        frame_slot_set(owner, slot, owned);
                        alias
                    }
                }
            } else if global_scope {
                let binding = eg.globals.get(&name).map_or_else(
                    || Value::owned_reference(Value::null()),
                    |value| {
                        if value.is_owned_reference() {
                            value.clone_owned_reference_alias()
                        } else {
                            Value::owned_reference(reference_initial_value(value.clone()))
                        }
                    },
                );
                globals_set(&mut eg.globals, &name, binding.clone_owned_reference_alias());
                binding
            } else {
                let variables = eg.dynamic_variables.entry(owner as usize).or_default();
                let binding = variables.get(&name).map_or_else(
                    || Value::owned_reference(Value::null()),
                    |value| {
                        if value.is_owned_reference() {
                            value.clone_owned_reference_alias()
                        } else {
                            Value::owned_reference(reference_initial_value(value.clone()))
                        }
                    },
                );
                globals_set(variables, &name, binding.clone_owned_reference_alias());
                binding
            };
            if opline._pad & REFERENCE_RESULT_INTERNAL != 0 {
                binding.mark_internal_reference_alias();
            }
            let destination = unsafe {
                (*frame).get_op_mut(opline.result as u32, opline.result_type)
            };
            unsafe { frame_slot_set(frame, destination, binding) };
        }
        OpCode::AssignDynamicVarRef => {
            let source = unsafe { (*frame).get_op_mut(opline.op2 as u32, opline.op2_type) };
            let binding = unsafe {
                if (*source).is_owned_reference() {
                    (*source).clone_owned_reference_alias()
                } else {
                    let owned = Value::owned_reference(reference_initial_value((*source).clone()));
                    let alias = owned.clone_owned_reference_alias();
                    if opline.op2_type == OpType::Cv {
                        frame_slot_set(frame, source, owned);
                    } else {
                        frame_tmp_set(frame, source, owned);
                    }
                    alias
                }
            };
            if let Some(cv) = direct_cv {
                unsafe { frame_slot_set(owner, (*owner).cv_mut(cv), binding.clone_owned_reference_alias()) };
            } else if global_scope {
                globals_set(&mut eg.globals, &name, binding.clone_owned_reference_alias());
                eg.dirty_globals.insert(name);
            } else {
                globals_set(
                    eg.dynamic_variables.entry(owner as usize).or_default(),
                    &name,
                    binding.clone_owned_reference_alias(),
                );
            }
        }
        OpCode::BindDynamicGlobal => {
            let binding = eg.globals.get(&name).map_or_else(
                || Value::owned_reference(Value::null()),
                |value| {
                    if value.is_owned_reference() {
                        value.clone_owned_reference_alias()
                    } else {
                        Value::owned_reference(reference_initial_value(value.clone()))
                    }
                },
            );
            globals_set(&mut eg.globals, &name, binding.clone_owned_reference_alias());
            // The local binding can mutate the shared reference cell without
            // executing another global-table opcode. Publish it back into a
            // suspended request-scope CV when this function returns.
            eg.dirty_globals.insert(name.clone());
            if let Some(cv) = direct_cv {
                unsafe { frame_slot_set(owner, (*owner).cv_mut(cv), binding.clone_owned_reference_alias()) };
            } else if !global_scope {
                globals_set(
                    eg.dynamic_variables.entry(owner as usize).or_default(),
                    &name,
                    binding.clone_owned_reference_alias(),
                );
            }
        }
        _ => unreachable!("dynamic-variable helper called for another opcode"),
    }
    Ok(ColdResult::Done)
}

/// Snapshot a runtime-resolved send operand. A by-reference caller bypasses
/// this helper; every by-value path consumes null before invoking the handler,
/// so a re-entrant assignment cannot change the current argument value.
fn snapshot_runtime_send_rvalue(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<Value, VmError> {
    let source = unsafe {
        &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
    };
    if !source.is_undef() {
        return Ok(source.clone());
    }

    let snapshot = Value::null();
    if opline._pad & crate::vm::instruction::SEND_FLAG_FETCH_CV_R != 0 {
        report_undefined_variable_read(
            eg,
            frame,
            op_array,
            opline,
            opline.result,
            opline._pad & crate::vm::instruction::SEND_FLAG_ERROR_SUPPRESS != 0,
        )?;
    }
    Ok(snapshot)
}

#[inline(never)]
fn op_check_generic_args(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    op_check_generic_args_impl::<false>(eg, frame, op_array, opline)
}

#[inline(never)]
fn op_check_late_static_generic_args(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    op_check_generic_args_impl::<true>(eg, frame, op_array, opline)
}

#[inline(always)]
fn op_check_generic_args_impl<const LATE_STATIC: bool>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    // SAFETY: dispatch supplies an instruction from this op-array and a live
    // frame whose compiler-emitted owner operand remains valid for this check.
    // The cache has exactly one stable entry per instruction.
    let (cache, raw_owner) = unsafe {
        let ip = (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize;
        (
            &mut *(op_array.cache.as_ptr().add(ip)
                as *mut crate::vm::instruction::InlineCache),
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
        )
    };

    let kind = crate::generics::GenericDeclarationKind::from_tag(opline._pad)
        .ok_or_else(|| VmError::Fatal("Invalid generic declaration kind".into()))?;
    let owner_value = if raw_owner.is_reference() {
        // SAFETY: a Reference-tagged owner points at a live VM slot for the
        // duration of this non-reentrant opcode.
        unsafe { &*raw_owner.as_ref_ptr() }
    } else {
        raw_owner
    };

    if let Some(declaration) = cache.generic_declaration() {
        let cache_hit = if LATE_STATIC {
            let class_id = late_static_call_class_id(eg, frame);
            class_id != 0 && cache.class_id == class_id
        } else if kind == crate::generics::GenericDeclarationKind::Method
            && opline.op2_type == OpType::Const
        {
            owner_value
                .as_object()
                .is_some_and(|object| object.class_id == cache.class_id)
        } else if opline.op1_type == OpType::Const {
            true
        } else {
            owner_value
                .as_closure()
                .is_some_and(|closure| closure.func == cache.func)
        };
        if cache_hit {
            let binding = crate::generics::ReifiedBinding {
                declaration,
                use_site: opline.extended_value,
            };
            #[cfg(feature = "php-generics-reified")]
            {
                if cache.generic_signature_uses_class_scope() {
                    let class_id = generic_scope_class_id(eg, frame, kind, owner_value);
                    eg.push_reified_binding_scope_with_class(
                        frame as usize,
                        binding,
                        class_id,
                    );
                } else {
                    eg.push_reified_binding_scope(frame as usize, binding);
                }
            }
            #[cfg(not(feature = "php-generics-reified"))]
            let _ = binding;
            return Ok(());
        }
    }

    resolve_generic_args_cache_miss::<LATE_STATIC>(
        eg,
        frame,
        op_array,
        opline,
        cache,
        kind,
        owner_value,
    )
}

#[cold]
#[inline(never)]
fn resolve_generic_args_cache_miss<const LATE_STATIC: bool>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    cache: &mut crate::vm::instruction::InlineCache,
    mut kind: crate::generics::GenericDeclarationKind,
    owner_value: &Value,
) -> Result<(), VmError> {
    let mut cacheable = opline.op1_type == OpType::Const;
    let mut receiver_class_id = 0;
    let mut callable = std::ptr::null();

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    let mut static_receiver_scope = None;
    let mut owner = if kind == crate::generics::GenericDeclarationKind::Method
        && opline.op2_type == OpType::Const
    {
        let object = owner_value.as_object().ok_or_else(|| {
            VmError::Fatal("Generic method arguments require an object receiver".into())
        })?;
        let method = op_array.literals[opline.op2 as usize]
            .as_str()
            .unwrap_or("");
        cacheable = true;
        receiver_class_id = object.class_id;
        let target_class = object.class_name.to_string();
        let caller_class = get_caller_class(frame, eg);
        let dispatch_class = if let Some(ref caller) = caller_class {
            if let Some((Visibility::Private, defining)) =
                eg.find_method_visibility(caller, method)
            {
                if defining.eq_ignore_ascii_case(caller)
                    && eg.class_is_a(&target_class, caller)
                {
                    caller.clone()
                } else {
                    target_class
                }
            } else {
                target_class
            }
        } else {
            target_class
        };
        format!("{}::{}", dispatch_class, method)
    } else if let Some(name) = owner_value.as_str() {
        if kind == crate::generics::GenericDeclarationKind::Method {
            if name
                .rsplit_once("::")
                .is_some_and(|(class, _)| {
                    class.eq_ignore_ascii_case("self")
                        || class.eq_ignore_ascii_case("parent")
                        || class.eq_ignore_ascii_case("static")
                })
            {
                // Late-static and shared-trait bytecode can resolve to
                // multiple declarations. Its dedicated opcode validates the
                // cached declaration against the recovered called class;
                // legacy/unmarked pseudo owners remain safely uncached.
                cacheable = LATE_STATIC;
            }
            let resolved = resolve_static_method_owner(eg, frame, name)
                .unwrap_or_else(|| name.to_string());
            receiver_class_id = resolved
                .rsplit_once("::")
                .map_or(0, |(class, _)| eg.class_id_of(class));
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            {
                static_receiver_scope = resolved
                    .rsplit_once("::")
                    .map(|(class, _)| class.to_string());
            }
            resolved
        } else {
            name.to_string()
        }
    } else if let Some(closure) = owner_value.as_closure() {
        let Some(user) = closure.user_function() else {
            return Err(VmError::Fatal(
                "Generic arguments are not supported for this callable".into(),
            ));
        };
        kind = crate::generics::GenericDeclarationKind::Closure;
        cacheable = true;
        callable = closure.func;
        user.op_array.name.clone()
    } else {
        return Err(VmError::Fatal(
            "Generic arguments require a named function, method, class, or closure".into(),
        ));
    };

    // Metadata belongs to the concrete body declaration, not to aliases
    // installed for inheritance or trait composition. Resolve it by the same
    // function pointer the subsequent method call will execute. This also
    // prevents a non-generic override from falling through to generic parent
    // metadata merely because the names match.
    if kind == crate::generics::GenericDeclarationKind::Method {
        let method = owner
            .rsplit_once("::")
            .map(|(_, method)| method.to_string());
        if let Some(method) = method {
            if let Some(function) = eg.find_function(&owner) {
                if let Some(definition_owner) =
                    eg.method_definition_owner(function, &method)
                {
                    owner = format!("{}::{}", definition_owner, method);
                }
            }
        }
    }

    // Unqualified namespaced function calls retain PHP's global fallback.
    if kind == crate::generics::GenericDeclarationKind::Function
        && eg.generic_metadata.find(kind, &owner).is_none()
        && opline.op2_type == OpType::Const
    {
        if let Some(fallback) = op_array.literals[opline.op2 as usize].as_str() {
            owner = fallback.to_string();
        }
    }

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    let receiver_scope = if kind == crate::generics::GenericDeclarationKind::Method
        && owner_value.value_type() == ValueType::Object
    {
        Some(unsafe { owner_value.object_class_name_unchecked() })
    } else if kind == crate::generics::GenericDeclarationKind::Method {
        static_receiver_scope.as_deref()
    } else {
        None
    };
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    let declaration_scope = eg.generic_declaration_scope(&owner, receiver_scope);
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    let binding = eg
        .generic_metadata
        .resolve_binding(kind, &owner, opline.extended_value, |actual, bound| {
            eg.class_is_a_in_generic_scopes(
                actual,
                bound,
                declaration_scope,
                receiver_scope,
            )
        })
        .map_err(VmError::Fatal)?;
    #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
    let binding = eg
        .generic_metadata
        .resolve_binding(kind, &owner, opline.extended_value, |actual, bound| {
            eg.class_is_a(actual, bound)
        })
        .map_err(VmError::Fatal)?;

    let uses_class_scope = eg
        .generic_metadata
        .declaration(binding)
        .is_some_and(|declaration| declaration.signature_uses_class_pseudo);

    if cacheable {
        cache.set_generic_declaration(
            binding.declaration,
            receiver_class_id,
            callable,
            uses_class_scope,
        );
    }

    #[cfg(feature = "php-generics-reified")]
    {
        if uses_class_scope {
            let class_id = generic_scope_class_id(eg, frame, kind, owner_value);
            eg.push_reified_binding_scope_with_class(frame as usize, binding, class_id);
        } else {
            eg.push_reified_binding_scope(frame as usize, binding);
        }
    }

    #[cfg(not(feature = "php-generics-reified"))]
    let _ = binding;

    Ok(())
}

#[cfg(feature = "php-generics-reified")]
#[inline]
fn generic_scope_class_id(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    kind: crate::generics::GenericDeclarationKind,
    owner: &Value,
) -> u32 {
    if kind != crate::generics::GenericDeclarationKind::Method {
        return 0;
    }
    if owner.value_type() == ValueType::Object {
        return unsafe { owner.object_class_id_unchecked() };
    }
    let class = owner
        .as_str()
        .map(|name| name.split_once("::").map_or(name, |(class, _)| class));
    class.map_or(0, |class| {
        if class.eq_ignore_ascii_case("self")
            || class.eq_ignore_ascii_case("parent")
            || class.eq_ignore_ascii_case("static")
        {
            resolve_static_call_class(eg, frame, class, true)
                .map_or(0, |class| eg.class_id_of(&class))
        } else {
            eg.class_id_of(class)
        }
    })
}

#[cfg(feature = "php-generics-reified")]
#[inline]
fn generic_call_class_is_a(
    eg: &ExecutorGlobals,
    call: *mut ExecuteData,
    scope_owner: usize,
    actual: &str,
    expected: &str,
    declared_scope: &str,
) -> bool {
    let common = unsafe { &*(*call).func };
    let receiver_scope = if common.sig.this_offset == 1 {
        let receiver = unsafe { &*(*call).cv(0) };
        if receiver.value_type() == ValueType::Object {
            Some(unsafe { receiver.object_class_name_unchecked() })
        } else {
            None
        }
    } else {
        None
    };
    let called_scope = receiver_scope.or_else(|| {
        eg.class_by_id(eg.reified_binding_scope_class_id(scope_owner))
            .map(|class| class.name.as_str())
    });
    let scope = eg.generic_declaration_scope(declared_scope, called_scope);
    eg.class_is_a_in_generic_scopes(actual, expected, scope, called_scope)
}

#[cfg(feature = "php-generics-reified")]
#[inline]
fn generic_call_reified_arguments_match(
    eg: &ExecutorGlobals,
    call: *mut ExecuteData,
    scope_owner: usize,
    value: &Value,
    expected: &str,
    arguments: &[crate::generics::GenericType],
    declaration: &crate::generics::GenericDeclaration,
    site: &crate::generics::GenericUseSite,
    declared_scope: &str,
) -> bool {
    let common = unsafe { &*(*call).func };
    let receiver_scope = if common.sig.this_offset == 1 {
        let receiver = unsafe { &*(*call).cv(0) };
        (receiver.value_type() == ValueType::Object)
            .then(|| unsafe { receiver.object_class_name_unchecked() })
    } else {
        None
    };
    let called_scope = receiver_scope.or_else(|| {
        eg.class_by_id(eg.reified_binding_scope_class_id(scope_owner))
            .map(|class| class.name.as_str())
    });
    let scope = eg.generic_declaration_scope(declared_scope, called_scope);
    eg.reified_object_arguments_match_binding(
        value,
        expected,
        arguments,
        declaration,
        site,
        scope,
        called_scope,
    )
}

#[cfg(feature = "php-generics-reified")]
#[inline]
fn value_matches_reified_default(
    eg: &ExecutorGlobals,
    scope_owner: usize,
    value: &Value,
    expected: &crate::generics::GenericType,
    binding: crate::generics::ReifiedBinding,
    declared_scope: &str,
) -> bool {
    let class_id = eg.reified_binding_scope_class_id(scope_owner);
    let receiver_scope = eg.class_by_id(class_id).map(|class| class.name.as_str());
    let scope = eg.generic_declaration_scope(declared_scope, receiver_scope);
    eg.generic_metadata.value_matches_binding_reified(
        value,
        expected,
        binding,
        |actual, bound| {
            eg.class_is_a_in_generic_scopes(actual, bound, scope, receiver_scope)
        },
        |value, name, arguments, declaration, site| {
            eg.reified_object_arguments_match_binding(
                value,
                name,
                arguments,
                declaration,
                site,
                scope,
                receiver_scope,
            )
        },
    )
}

#[inline(never)]
fn op_check_reified_args(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
) -> Result<(), VmError> {
    #[cfg(not(feature = "php-generics-reified"))]
    {
        let _ = (eg, frame);
        return Err(VmError::Fatal(
            "Reified generic check emitted without php-generics-reified".into(),
        ));
    }

    #[cfg(feature = "php-generics-reified")]
    {
        let binding = *eg
            .reified_bindings
            .last()
            .ok_or_else(|| VmError::Fatal("Missing reified generic binding".into()))?;
        let declaration = eg
            .generic_metadata
            .declaration(binding)
            .ok_or_else(|| VmError::Fatal("Invalid reified generic declaration".into()))?;
        let call = unsafe { (*frame).call };
        if call.is_null() {
            return Err(VmError::Fatal(
                "Reified generic call has no pending call frame".into(),
            ));
        }
        let common = unsafe { &*(*call).func };
        let declared_scope = eg
            .generic_metadata
            .symbol(declaration.owner)
            .unwrap_or("?");
        let fixed = declaration
            .value_parameters
            .len()
            .saturating_sub(usize::from(common.sig.is_variadic));
        for index in 0..fixed {
            let expected = &declaration.value_parameters[index];
            let Some(expected) = expected else {
                continue;
            };
            if index >= unsafe { (*call).num_args as usize } {
                continue;
            }
            let slot = common.sig.param_cv_index(index as u32);
            if slot >= unsafe { (*call).num_cvs } {
                break;
            }
            let value = unsafe { (*call).cv(slot) };
            if value.is_undef() {
                continue;
            }
            if !eg.generic_metadata.value_matches_binding_reified(
                value,
                expected,
                binding,
                |actual, bound| {
                    generic_call_class_is_a(
                        eg,
                        call,
                        frame as usize,
                        actual,
                        bound,
                        declared_scope,
                    )
                },
                |value, name, arguments, declaration, site| {
                    generic_call_reified_arguments_match(
                        eg,
                        call,
                        frame as usize,
                        value,
                        name,
                        arguments,
                        declaration,
                        site,
                        declared_scope,
                    )
                },
            ) {
                let owner = eg
                    .generic_metadata
                    .symbol(declaration.owner)
                    .unwrap_or("?");
                return Err(VmError::Fatal(format!(
                    "Argument #{} passed to {} does not match its reified generic type",
                    index + 1,
                    owner
                )));
            }
        }
        if common.sig.is_variadic
            && let Some(expected) = declaration
                .value_parameters
                .last()
                .and_then(Option::as_ref)
        {
            let public_max = common.sig.public_arity();
            let extra = unsafe { (*call).num_args }.saturating_sub(public_max);
            for index in 0..extra {
                let value = unsafe { &*(*call).cv(common.sig.variadic_cv_index + index) };
                if !eg.generic_metadata.value_matches_binding_reified(
                    value,
                    expected,
                    binding,
                    |actual, bound| {
                        generic_call_class_is_a(
                            eg,
                            call,
                            frame as usize,
                            actual,
                            bound,
                            declared_scope,
                        )
                    },
                    |value, name, arguments, declaration, site| {
                        generic_call_reified_arguments_match(
                            eg,
                            call,
                            frame as usize,
                            value,
                            name,
                            arguments,
                            declaration,
                            site,
                            declared_scope,
                        )
                    },
                ) {
                    let owner = eg
                        .generic_metadata
                        .symbol(declaration.owner)
                        .unwrap_or("?");
                    return Err(VmError::Fatal(format!(
                        "Variadic argument #{} passed to {} does not match its reified generic type",
                        fixed + index as usize + 1,
                        owner
                    )));
                }
            }
            if let Some(named) = eg.pending_named_variadic.get(&(call as usize)) {
                for (name, value) in named {
                    if !eg.generic_metadata.value_matches_binding_reified(
                        value,
                        expected,
                        binding,
                        |actual, bound| {
                            generic_call_class_is_a(
                                eg,
                                call,
                                frame as usize,
                                actual,
                                bound,
                                declared_scope,
                            )
                        },
                        |value, name, arguments, declaration, site| {
                            generic_call_reified_arguments_match(
                                eg,
                                call,
                                frame as usize,
                                value,
                                name,
                                arguments,
                                declaration,
                                site,
                                declared_scope,
                            )
                        },
                    ) {
                        let owner = eg
                            .generic_metadata
                            .symbol(declaration.owner)
                            .unwrap_or("?");
                        return Err(VmError::Fatal(format!(
                            "Named variadic argument ${} passed to {} does not match its reified generic type",
                            name, owner
                        )));
                    }
                }
            }
        }
        eg.activate_reified_binding_scope(frame as usize, call as usize);
        Ok(())
    }
}

#[cold]
#[inline(never)]
fn op_check_generic_default(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    opline: &Instruction,
) -> Result<(), VmError> {
    #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
    {
        let _ = (eg, frame, opline);
        return Err(VmError::Fatal(
            "Generic default check emitted without generic runtime support".into(),
        ));
    }

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    {
        let parameter_index = opline.extended_value as usize;
        let value = unsafe { &*(*frame).cv(opline.op1 as u32) };

        #[cfg(feature = "php-generics-reified")]
        if let Some((scope_owner, binding)) =
            eg.active_reified_binding_scope(frame as usize)
        {
            let declaration = eg
                .generic_metadata
                .declaration(binding)
                .ok_or_else(|| VmError::Fatal("Invalid reified generic declaration".into()))?;
            if let Some(expected) = declaration
                .value_parameters
                .get(parameter_index)
                .and_then(Option::as_ref)
            {
                let declared_scope = eg
                    .generic_metadata
                    .symbol(declaration.owner)
                    .unwrap_or("?");
                if !value_matches_reified_default(
                    eg,
                    scope_owner,
                    value,
                    expected,
                    binding,
                    declared_scope,
                ) {
                    return Err(VmError::Fatal(format!(
                        "Default value for argument #{} of {} does not match its reified generic type",
                        parameter_index + 1,
                        declared_scope
                    )));
                }
            }
        }

        if let Some(contract) = eg.active_generic_member_call(frame as usize)
            && let Some(expected) = contract
                .value_parameters
                .get(parameter_index)
                .and_then(Option::as_ref)
            && !eg.value_matches_generic_method_contract(value, expected, contract)
        {
            return Err(VmError::Fatal(format!(
                "Default value for argument #{} of {}::{}() does not match its {}",
                parameter_index + 1,
                contract.owner,
                contract.method,
                generic_method_contract_kind(contract.runtime_mode)
            )));
        }
        Ok(())
    }
}

#[inline(never)]
fn op_check_reified_return(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    #[cfg(not(feature = "php-generics-reified"))]
    {
        let _ = (eg, frame, op_array, opline);
        return Err(VmError::Fatal(
            "Reified generic return check emitted without php-generics-reified".into(),
        ));
    }

    #[cfg(feature = "php-generics-reified")]
    {
        let binding = *eg
            .reified_bindings
            .last()
            .ok_or_else(|| VmError::Fatal("Missing reified generic binding".into()))?;
        let declaration = eg
            .generic_metadata
            .declaration(binding)
            .ok_or_else(|| VmError::Fatal("Invalid reified generic declaration".into()))?;
        let declared_scope = eg
            .generic_metadata
            .symbol(declaration.owner)
            .unwrap_or("?");
        if let Some(expected) = declaration.return_type.as_ref() {
            let value = unsafe {
                &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
            };
            let matches_return = matches!(expected, crate::generics::GenericType::Void)
                || eg.generic_metadata.value_matches_binding_reified(
                    value,
                    expected,
                    binding,
                    |actual, bound| {
                        let class_id = eg.reified_binding_scope_class_id(frame as usize);
                        let receiver_scope = eg
                            .class_by_id(class_id)
                            .map(|class| class.name.as_str());
                        let scope =
                            eg.generic_declaration_scope(declared_scope, receiver_scope);
                        eg.class_is_a_in_generic_scopes(actual, bound, scope, receiver_scope)
                    },
                    |value, name, arguments, declaration, site| {
                        let class_id = eg.reified_binding_scope_class_id(frame as usize);
                        let receiver_scope = eg
                            .class_by_id(class_id)
                            .map(|class| class.name.as_str());
                        let scope =
                            eg.generic_declaration_scope(declared_scope, receiver_scope);
                        eg.reified_object_arguments_match_binding(
                            value,
                            name,
                            arguments,
                            declaration,
                            site,
                            scope,
                            receiver_scope,
                        )
                    },
                );
            if !matches_return {
                let owner = eg
                    .generic_metadata
                    .symbol(declaration.owner)
                    .unwrap_or("?");
                return Err(VmError::Fatal(format!(
                    "Return value of {} does not match its reified generic type",
                    owner
                )));
            }
        }
        eg.finish_reified_binding_scope(frame as usize);
        Ok(())
    }
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[inline(never)]
fn validate_generic_member_arguments(
    eg: &ExecutorGlobals,
    call: *mut ExecuteData,
    contract: &crate::generics::GenericMethodContract,
) -> Result<(), VmError> {
    let contract_kind = generic_method_contract_kind(contract.runtime_mode);
    let common = unsafe { &*(*call).func };
    let fixed = contract
        .value_parameters
        .len()
        .saturating_sub(usize::from(contract.is_variadic));
    for index in 0..fixed {
        let Some(expected) = contract
            .value_parameters
            .get(index)
            .and_then(Option::as_ref)
        else {
            continue;
        };
        let value = unsafe { &*(*call).cv(common.sig.param_cv_index(index as u32)) };
        if value.is_undef() {
            continue;
        }
        if !eg.value_matches_generic_method_contract(value, expected, contract) {
            return Err(VmError::Fatal(format!(
                "Argument #{} passed to {}::{}() does not match its {}",
                index + 1,
                contract.owner,
                contract.method,
                contract_kind
            )));
        }
    }

    if contract.is_variadic {
        let expected = contract
            .value_parameters
            .last()
            .and_then(Option::as_ref);
        if let Some(expected) = expected {
            let public_max = common.sig.public_arity();
            let extra = unsafe { (*call).num_args }.saturating_sub(public_max);
            for index in 0..extra {
                let value = unsafe { &*(*call).cv(common.sig.variadic_cv_index + index) };
                if !eg.value_matches_generic_method_contract(value, expected, contract) {
                    return Err(VmError::Fatal(format!(
                        "Variadic argument #{} passed to {}::{}() does not match its {}",
                        fixed + index as usize + 1,
                        contract.owner,
                        contract.method,
                        contract_kind
                    )));
                }
            }
            if let Some(named) = eg.pending_named_variadic.get(&(call as usize)) {
                for (name, value) in named {
                    if !eg.value_matches_generic_method_contract(value, expected, contract) {
                        return Err(VmError::Fatal(format!(
                            "Named variadic argument ${} passed to {}::{}() does not match its {}",
                            name, contract.owner, contract.method, contract_kind
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[inline(never)]
fn validate_generic_member_return(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    contract: &crate::generics::GenericMethodContract,
) -> Result<(), VmError> {
    let Some(expected) = contract.return_type.as_ref() else {
        return Ok(());
    };
    if matches!(expected, crate::generics::GenericType::Void) {
        return if opline.extended_value == 0 {
            Ok(())
        } else {
            Err(VmError::Fatal(format!(
                "Return value of {}::{}() does not match its {}",
                contract.owner,
                contract.method,
                generic_method_contract_kind(contract.runtime_mode)
            )))
        };
    }
    let implicit_null;
    let value = if opline.op1_type == OpType::Unused {
        implicit_null = Value::null();
        &implicit_null
    } else {
        unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) }
    };
    if eg.value_matches_generic_method_contract(value, expected, contract) {
        return Ok(());
    }
    Err(VmError::Fatal(format!(
        "Return value of {}::{}() does not match its {}",
        contract.owner,
        contract.method,
        generic_method_contract_kind(contract.runtime_mode)
    )))
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[inline(always)]
fn generic_method_contract_kind(mode: crate::generics::GenericRuntimeMode) -> &'static str {
    match mode {
        crate::generics::GenericRuntimeMode::BoundErased => "linked generic class type",
        crate::generics::GenericRuntimeMode::Reified => "reified class type",
    }
}

#[inline(never)]
fn op_call_user_func_array<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: dispatch supplies a live frame and compiler-emitted operands.
    // Reference targets remain live through this non-reentrant call setup.
    let (callback, args) = unsafe {
        let callback = &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array);
        let callback = if callback.is_reference() {
            &*callback.as_ref_ptr()
        } else {
            callback
        };
        let args = &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array);
        let args = if args.is_reference() {
            &*args.as_ref_ptr()
        } else {
            args
        };
        (callback, args)
    };
    let callback = if opline.extended_value != 0
        && callback
            .as_str()
            .is_some_and(|name| eg.find_function(name).is_none())
    {
        op_array
            .literals
            .get(opline.extended_value as usize)
            .unwrap_or(callback)
    } else {
        callback
    };

    // SAFETY: `opline` belongs to this op-array, whose cache has one stable
    // entry per instruction.
    let cache_slot = unsafe {
        let ip = (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize;
        op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache
    };
    let caller_class = get_caller_class(frame, eg);
    let uses_legacy_scope = crate::stdlib::callback_uses_legacy_scope(callback);
    let receiver = if uses_legacy_scope {
        closure_bound_this(frame, op_array, false)
    } else {
        None
    };
    let called_class = if uses_legacy_scope {
        receiver
            .as_ref()
            .and_then(Value::as_object)
            .map(|object| object.class_name.to_string())
            .or_else(|| {
                eg.class_by_id(late_static_call_class_id(eg, frame))
                    .map(|class| class.name.clone())
            })
    } else {
        None
    };
    let legacy = if uses_legacy_scope {
        crate::stdlib::resolve_legacy_callback(
            callback,
            eg,
            caller_class.as_deref(),
            called_class.as_deref(),
            receiver.as_ref(),
        )
    } else {
        crate::stdlib::LegacyCallbackResolution::NotLegacy
    };
    let result = if let crate::stdlib::LegacyCallbackResolution::Legacy {
        resolved,
        deprecation,
    } = legacy
    {
        let args = args.clone();
        let invalid_reason = resolved.is_none().then(|| {
            crate::stdlib::legacy_callback_invalid_reason(
                callback,
                eg,
                caller_class.as_deref(),
                called_class.as_deref(),
                receiver.as_ref(),
            )
        });
        if let Some(deprecation) = deprecation {
            report_php_deprecation(eg, frame, op_array, opline, &deprecation)?;
        }
        if opline._pad & CALL_USER_FUNC_ARRAY_SOURCE_UNPACK == 0
            && let Some(previous) = eg.exception.take()
        {
            eg.exception = Some(legacy_callback_deprecation_type_error(
                eg,
                "call_user_func_array",
                previous,
            ));
        }
        if eg.exception.is_some() {
            Value::null()
        } else if let Some(resolved) = resolved {
            if opline._pad & CALL_USER_FUNC_ARRAY_SOURCE_UNPACK != 0 {
                let source_file = if op_array.source_file.is_empty() {
                    op_array.name.as_str()
                } else {
                    op_array.source_file.as_str()
                };
                crate::stdlib::invoke_resolved_source_unpacked_call(
                    resolved,
                    &args,
                    eg,
                    source_file,
                    op_array.strict_types,
                )?
            } else {
                crate::stdlib::invoke_resolved_call_user_func_array(resolved, &args, eg)?
            }
        } else {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "call_user_func_array(): Argument #1 ($callback) must be a valid callback, {}",
                    invalid_reason.unwrap_or_else(|| "no array or string given".to_string())
                ),
            ));
            Value::null()
        }
    } else if opline._pad & CALL_USER_FUNC_ARRAY_SOURCE_UNPACK != 0 {
        let source_file = if op_array.source_file.is_empty() {
            op_array.name.as_str()
        } else {
            op_array.source_file.as_str()
        };
        crate::stdlib::invoke_source_unpacked_call(
            callback,
            args,
            eg,
            caller_class.as_deref(),
            Some(cache_slot),
            source_file,
            op_array.strict_types,
        )?
    } else {
        crate::stdlib::invoke_call_user_func_array(
            callback,
            args,
            eg,
            caller_class.as_deref(),
            Some(cache_slot),
        )?
    };

    if let Some(exc) = eg.exception.take() {
        return Ok(match throw_in_frame(eg, frame, exc)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }

    if opline.result_type != OpType::Unused {
        // SAFETY: the compiler emitted a writable result for this call and the
        // frame remains live until the opcode returns. CallUserFuncArray
        // results are fresh TMP slots, whose reused stack bytes are not
        // necessarily an initialized Value; use the frame-aware first-write
        // path instead of dropping stale bytes through slot_set().
        unsafe {
            let result_ptr = (*frame).get_op_mut(opline.result as u32, opline.result_type);
            frame_tmp_set(frame, result_ptr, result);
        }
    }
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_fetch_static_prop<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_fetch_static_prop_impl::<false>(eg, frame, op_array, opline)
}

#[inline(never)]
fn op_fetch_late_static_prop<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_fetch_static_prop_impl::<true>(eg, frame, op_array, opline)
}

/// Static storage is commonly scalar. Copy that 16-byte representation
/// directly and reserve refcount/reference cloning for the uncommon heap path.
#[inline(always)]
fn clone_static_property_value(value: &Value) -> Value {
    if value.needs_cleanup() || value.is_reference() {
        clone_heap_static_property_value(value)
    } else {
        let mut cloned = std::mem::MaybeUninit::<Value>::uninit();
        unsafe {
            Value::raw_copy(value as *const Value, cloned.as_mut_ptr());
            cloned.assume_init()
        }
    }
}

/// Keep reference counting and reference-wrapper cloning out of scalar static
/// property dispatch. Inlining `Value::clone` here noticeably grows both
/// monomorphized static-property write handlers and perturbs their hot layout.
#[inline(never)]
fn clone_heap_static_property_value(value: &Value) -> Value {
    value.clone()
}

fn dynamic_static_property_owner(
    eg: &ExecutorGlobals,
    raw_value: &Value,
) -> Result<(String, u32), VmError> {
    let value = if raw_value.is_reference() {
        // SAFETY: the reference tag retains either the owned cell or the live
        // frame slot reached by `as_ref_ptr()` for this synchronous dispatch.
        unsafe { &*raw_value.as_ref_ptr() }
    } else {
        raw_value
    };
    if let Some(object) = value.as_object() {
        let class_name = object.class_name.to_string();
        let class_id = if object.class_id == 0 {
            eg.class_id_of(&class_name)
        } else {
            object.class_id
        };
        return Ok((class_name, class_id));
    }
    if let Some(class_name) = value.as_str() {
        let class_name = class_name.strip_prefix('\\').unwrap_or(class_name);
        return Ok((class_name.to_string(), eg.class_id_of(class_name)));
    }
    Err(VmError::Fatal(
        "Class name must be a valid object or a string".to_string(),
    ))
}

#[inline]
fn static_property_throw<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    class: &str,
    message: String,
) -> Result<ColdResult<'a>, VmError> {
    let error = make_error_value(class, &message);
    Ok(match throw_in_frame(eg, frame, error)? {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
    })
}

#[inline(always)]
fn op_fetch_static_prop_impl<'a, const LATE_STATIC: bool>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: dispatch supplies an instruction from this op-array and a live
    // frame with compiler-emitted operand/result slots. The cache has one
    // stable entry per instruction and execution owns its mutable access.
    let (class_name_val, prop_name_val, result_ptr, cache) = unsafe {
        let ip = (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize;
        (
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array),
            (*frame).get_op_mut(opline.result as u32, opline.result_type),
            &mut *(op_array.cache.as_ptr().add(ip)
                as *mut crate::vm::instruction::InlineCache),
        )
    };

    let dynamic_owner = opline._pad & STATIC_PROP_DYNAMIC_OWNER != 0;
    let dynamic_owner_value = dynamic_owner
        .then(|| dynamic_static_property_owner(eg, class_name_val))
        .transpose()?;
    let raw_class = dynamic_owner_value
        .as_ref()
        .map_or_else(|| class_name_val.as_str().unwrap_or(""), |(name, _)| name);
    let prop = prop_name_val.as_str().unwrap_or("");
    let class_id = dynamic_owner_value.as_ref().map_or_else(
        || static_property_class_id::<LATE_STATIC>(eg, frame, opline, cache, raw_class),
        |(_, class_id)| *class_id,
    );

    if opline._pad & STATIC_PROP_REFERENCE_FETCH != 0 {
        return resolve_static_property_reference_fetch(
            eg,
            frame,
            opline,
            result_ptr,
            cache,
            class_id,
            raw_class,
            prop,
        );
    }

    if opline._pad & STATIC_PROP_DYNAMIC_NAME == 0
        && class_id != 0
        && cache.class_id == class_id
        && cache.property_flags() == 1
    {
        // SAFETY: the class/cache guards prove the storage slot is valid; the
        // compiler-emitted result pointer was validated above.
        unsafe {
            let value = clone_static_property_value(
                eg.static_property_value_unchecked(cache.property_slot()),
            );
            frame_tmp_set(frame, result_ptr, value);
        }
        return Ok(ColdResult::Done);
    }

    resolve_static_property_read_cache_miss(
        eg,
        frame,
        result_ptr,
        cache,
        class_id,
        raw_class,
        prop,
        opline._pad,
    )
}

#[inline(always)]
fn static_property_class_id<const LATE_STATIC: bool>(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    opline: &Instruction,
    cache: &crate::vm::instruction::InlineCache,
    raw_class: &str,
) -> u32 {
    if LATE_STATIC {
        if opline._pad & LATE_STATIC_PROP_EMBEDDED_SCOPE != 0 {
            unsafe { ((*frame).heap_bitmap >> 32) as u32 }
        } else if raw_class.eq_ignore_ascii_case("parent") {
            eg.class_by_id(late_static_call_class_id(eg, frame))
                .and_then(|class| class.parent.as_deref())
                .map_or(0, |parent| eg.class_id_of(parent))
        } else if raw_class.eq_ignore_ascii_case("static")
            || raw_class.eq_ignore_ascii_case("self")
        {
            late_static_call_class_id(eg, frame)
        } else {
            eg.class_id_of(raw_class)
        }
    } else if cache.class_id != 0 && cache.property_flags() != 0 {
        cache.class_id
    } else {
        eg.class_id_of(raw_class)
    }
}

#[inline(never)]
fn op_fetch_class_const<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_fetch_class_const_impl::<false>(eg, frame, op_array, opline)
}

#[inline(never)]
fn op_fetch_late_class_const<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_fetch_class_const_impl::<true>(eg, frame, op_array, opline)
}

#[inline(never)]
fn op_fetch_dynamic_class_const<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_fetch_class_const_impl::<false>(eg, frame, op_array, opline)
}

#[inline(never)]
fn op_fetch_late_dynamic_class_const<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_fetch_class_const_impl::<true>(eg, frame, op_array, opline)
}

#[inline(always)]
fn op_fetch_class_const_impl<'a, const LATE_STATIC: bool>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: dispatch supplies a live frame and an instruction from this
    // op-array; its operand kinds and writable result slot were emitted by the
    // compiler and remain valid until this opcode completes.
    let (raw_class_value, raw_constant_value, result_ptr) = unsafe {
        (
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array),
            (*frame).get_op_mut(opline.result as u32, opline.result_type),
        )
    };
    // SAFETY: a Reference-tagged operand was created from a live VM slot; the
    // class/name borrows do not outlive this opcode or mutate that slot.
    let (class_value, constant_value) = unsafe {
        (
            if raw_class_value.is_reference() {
                &*raw_class_value.as_ref_ptr()
            } else {
                raw_class_value
            },
            if raw_constant_value.is_reference() {
                &*raw_constant_value.as_ref_ptr()
            } else {
                raw_constant_value
            },
        )
    };
    let set_result = |value| {
        // SAFETY: `result_ptr` is the initialized writable result slot proven
        // above, and every call transfers exactly one owned Value into it.
        unsafe { frame_result_set(frame, result_ptr, opline.result_type, value) };
    };
    let dynamic_owner = opline._pad & CLASS_CONST_DYNAMIC_OWNER != 0;
    let dynamic_name = opline._pad & CLASS_CONST_DYNAMIC_NAME != 0;
    let compile_time_name = opline._pad & CLASS_CONST_COMPILE_TIME_NAME != 0;
    let constant_expression = opline._pad & CLASS_CONST_CONSTANT_EXPRESSION != 0;
    let dynamic_call_owner = opline._pad & CLASS_CONST_DYNAMIC_CALL_OWNER != 0;
    let raw_class = class_value.as_str().unwrap_or("");
    let constant = constant_value.as_str();

    if dynamic_owner && class_value.as_object().is_none() && class_value.as_str().is_none() {
        return Ok(static_property_throw(
            eg,
            frame,
            "Error",
            "Class name must be a valid object or a string".to_string(),
        )?);
    }
    if dynamic_owner
        && !dynamic_name
        && class_value.as_str().is_some()
        && constant.is_some_and(|name| name.eq_ignore_ascii_case("class"))
    {
        return Ok(static_property_throw(
            eg,
            frame,
            "TypeError",
            "Cannot use \"::class\" on string".to_string(),
        )?);
    }
    if dynamic_owner
        && !dynamic_name
        && constant.is_some_and(|name| name.eq_ignore_ascii_case("class"))
        && let Some(object) = class_value.as_object()
    {
        let class_name = object.class_name.clone();
        drop(object);
        set_result(Value::string(class_name.to_string()));
        return Ok(ColdResult::Done);
    }
    let scoped_owner = raw_class.eq_ignore_ascii_case("self")
        || raw_class.eq_ignore_ascii_case("static")
        || raw_class.eq_ignore_ascii_case("parent");
    if class_value.as_object().is_none()
        && !raw_class.is_empty()
        && !scoped_owner
        && eg.find_class(raw_class).is_none()
    {
        let _ = crate::stdlib::autoload::ensure_symbol_loaded(eg, raw_class)?;
        if let Some(exception) = eg.exception.take() {
            return Ok(match throw_in_frame(eg, frame, exception)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
    }
    // SAFETY: `opline` belongs to this op-array, and cache has one stable entry
    // per instruction. Execution is single-threaded, so this opcode owns the
    // mutable cache access for the duration of the lookup.
    let cache = unsafe {
        let ip = (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize;
        &mut *(op_array.cache.as_ptr().add(ip)
            as *mut crate::vm::instruction::InlineCache)
    };
    let class_id = if dynamic_owner && !LATE_STATIC {
        class_value
            .as_object()
            .map(|object| {
                if object.class_id == 0 {
                    eg.class_id_of(&object.class_name)
                } else {
                    object.class_id
                }
            })
            .unwrap_or_else(|| eg.class_id_of(raw_class))
    } else {
        static_property_class_id::<LATE_STATIC>(eg, frame, opline, cache, raw_class)
    };

    if class_id == 0 && scoped_owner {
        let message = if constant.is_some_and(|name| name.eq_ignore_ascii_case("class"))
            && !dynamic_call_owner
        {
            format!(
                "Cannot use \"{}\" in the global scope",
                raw_class.to_ascii_lowercase()
            )
        } else {
            format!(
                "Cannot access \"{}\" when no class scope is active",
                raw_class.to_ascii_lowercase()
            )
        };
        let error = make_error_value("Error", &message);
        let instruction_index = unsafe {
            (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
        };
        attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
        return Ok(match throw_in_frame(eg, frame, error)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }

    let Some(class) = eg.class_by_id(class_id) else {
        return Ok(static_property_throw(
            eg,
            frame,
            "Error",
            format!("Class \"{}\" not found", raw_class),
        )?);
    };
    let Some(constant) = constant else {
        return Ok(static_property_throw(
            eg,
            frame,
            "TypeError",
            format!(
                "Cannot use value of type {} as class constant name",
                constant_value.type_name()
            ),
        )?);
    };
    if constant.eq_ignore_ascii_case("class") && (!dynamic_name || !compile_time_name) {
        set_result(Value::string(class.name.clone()));
        return Ok(ColdResult::Done);
    }

    let cached_constant_flags = cache.property_flags();
    if class_id != 0
        && cache.class_id == class_id
        && matches!(cached_constant_flags, 1 | 3)
    {
        let class = eg
            .class_by_id(class_id)
            .expect("cached class constant owner must stay registered");
        let definition = class
            .constants
            .get(cache.property_slot())
            .expect("cached class constant index must stay valid");
        if !dynamic_name || definition.name == constant {
            if cached_constant_flags == 1 {
                set_result(definition.value.clone());
                return Ok(ColdResult::Done);
            }
            let definition = definition.clone();
            let display_class = class.name.clone();
            let use_site = deprecated_use_site(frame, op_array, opline);
            crate::stdlib::reflection::report_deprecated_class_constant_use(
                &display_class,
                &definition,
                &use_site,
                eg,
            )?;
            if let Some(exception) = eg.exception.take() {
                return Ok(match throw_in_frame(eg, frame, exception)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
            let value = if definition.value_is_deferred {
                let Some(value) =
                    crate::stdlib::reflection::evaluate_deferred_class_constant_value(
                        &definition,
                        eg,
                    )?
                else {
                    let exception = eg
                        .exception
                        .take()
                        .expect("deferred class constant failure sets an exception");
                    let instruction_index = op_array
                        .instructions
                        .iter()
                        .position(|instruction| std::ptr::eq(instruction, opline))
                        .expect("class-constant opcode belongs to the active op-array");
                    attach_constant_expression_trace(
                        &exception,
                        eg,
                        frame,
                        op_array,
                        instruction_index,
                    );
                    return Ok(match throw_in_frame(eg, frame, exception)? {
                        ThrowResult::Handled(new_frame, new_op_array) => {
                            ColdResult::NewFrame(new_frame, new_op_array)
                        }
                        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                    });
                };
                value
            } else {
                definition.value.clone()
            };
            set_result(value);
            return Ok(ColdResult::Done);
        }
    }
    if !dynamic_name && class_id != 0 && cache.class_id == class_id && cached_constant_flags == 2 {
        if cache.enum_case_requires_deprecated_use_check() {
            let resolved_case = eg.class_by_id(class_id).and_then(|class| {
                class
                    .static_properties
                    .iter()
                    .enumerate()
                    .find(|(index, _)| {
                        eg.static_property_storage_slot(class_id, *index)
                            == Some(cache.property_slot())
                    })
                    .map(|(_, case)| (class.name.clone(), case.clone()))
            });
            let Some((class_name, case)) = resolved_case else {
                return Ok(static_property_throw(
                    eg,
                    frame,
                    "Error",
                    "Cached enum case metadata is unavailable".to_string(),
                )?);
            };
            let use_site = deprecated_use_site(frame, op_array, opline);
            crate::stdlib::reflection::report_deprecated_enum_case_use(
                &class_name,
                &case,
                &use_site,
                eg,
            )?;
            if let Some(exception) = eg.exception.take() {
                return Ok(match throw_in_frame(eg, frame, exception)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
        }
        let stored = eg
            .static_property_value(cache.property_slot())
            .expect("cached enum-case storage slot must stay valid");
        let value = clone_static_property_value(stored);
        set_result(value);
        return Ok(ColdResult::Done);
    }

    if class.is_trait {
        let message = format!("Cannot access trait constant {}::{} directly", class.name, constant);
        return Ok(static_property_throw(eg, frame, "Error", message)?);
    }
    let display_class = class.name.clone();
    let Some((constant_index, definition)) = eg.find_class_constant(class_id, constant) else {
        // Enum cases occupy immutable static storage but share PHP's
        // `Enum::Case` syntax with class constants. Preserve their existing
        // representation while keeping a distinct cache tag.
        if class.is_enum
            && let Some(case_index) = class
                .static_properties
                .iter()
                .position(|case| case.name == constant)
            && let Some(storage_slot) = eg.static_property_storage_slot(class_id, case_index)
        {
            let called_from_cases = op_array.name.rsplit_once("::").is_some_and(
                |(owner, method)| {
                    owner.eq_ignore_ascii_case(&class.name)
                        && method.eq_ignore_ascii_case("cases")
                },
            );
            if !called_from_cases
                && !constant_expression
                && let Some(error) = class.enum_backing_error.as_ref()
            {
                return Ok(static_property_throw(
                    eg,
                    frame,
                    error.exception_class(),
                    error.message().to_string(),
                )?);
            }
            let case = class.static_properties[case_index].clone();
            let class_name = class.name.clone();
            let requires_deprecated_use_check = case
                .attributes
                .iter()
                .any(|attribute| attribute.name.eq_ignore_ascii_case("Deprecated"));
            if requires_deprecated_use_check {
                let use_site = deprecated_use_site(frame, op_array, opline);
                crate::stdlib::reflection::report_deprecated_enum_case_use(
                    &class_name,
                    &case,
                    &use_site,
                    eg,
                )?;
                if let Some(exception) = eg.exception.take() {
                    return Ok(match throw_in_frame(eg, frame, exception)? {
                        ThrowResult::Handled(new_frame, new_op_array) => {
                            ColdResult::NewFrame(new_frame, new_op_array)
                        }
                        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                    });
                }
            }
            let stored = eg
                .static_property_value(storage_slot)
                .expect("resolved enum-case storage slot must stay valid");
            let value = clone_static_property_value(stored);
            cache.set_enum_case(class_id, storage_slot, requires_deprecated_use_check);
            set_result(value);
            return Ok(ColdResult::Done);
        }
        return Ok(static_property_throw(
            eg,
            frame,
            "Error",
            format!("Undefined constant {}::{}", display_class, constant),
        )?);
    };
    // Fetching any declared constant from a backed enum makes Zend update the
    // class constants and build the backing lookup table. Undefined constants
    // and `Enum::class` return earlier; constant-expression materialization is
    // the intentional per-constant bypass mirrored above for enum cases.
    if class.is_enum
        && !constant_expression
        && let Some(error) = class.enum_backing_error.as_ref()
    {
        return Ok(static_property_throw(
            eg,
            frame,
            error.exception_class(),
            error.message().to_string(),
        )?);
    }
    let caller = get_caller_class(frame, eg);
    if !eg.check_visibility(
        caller.as_deref(),
        &definition.declaring_class,
        definition.visibility,
    ) {
        let visibility = match definition.visibility {
            Visibility::Private => "private",
            Visibility::Protected => "protected",
            Visibility::Public => unreachable!(),
        };
        return Ok(static_property_throw(
            eg,
            frame,
            "Error",
            format!(
                "Cannot access {} constant {}::{}",
                visibility, display_class, constant
            ),
        )?);
    }
    if let Some(error) =
        crate::stdlib::reflection::class_constant_evaluation_error_value(&definition)
    {
        let instruction_index = op_array
            .instructions
            .iter()
            .position(|instruction| std::ptr::eq(instruction, opline))
            .expect("class-constant opcode belongs to the active op-array");
        attach_constant_expression_trace(&error, eg, frame, op_array, instruction_index);
        return Ok(match throw_in_frame(eg, frame, error)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }
    if !definition.requires_deprecated_use_check() {
        let value = definition.value.clone();
        cache.set_property(class_id, constant_index, 1);
        set_result(value);
        return Ok(ColdResult::Done);
    }
    let definition = definition.clone();
    let use_site = deprecated_use_site(frame, op_array, opline);
    crate::stdlib::reflection::report_deprecated_class_constant_use(
        &display_class,
        &definition,
        &use_site,
        eg,
    )?;
    if let Some(exception) = eg.exception.take() {
        return Ok(match throw_in_frame(eg, frame, exception)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }
    let value = if definition.value_is_deferred {
        let Some(value) = crate::stdlib::reflection::evaluate_deferred_class_constant_value(
            &definition,
            eg,
        )? else {
            let exception = eg
                .exception
                .take()
                .expect("deferred class constant failure sets an exception");
            let instruction_index = op_array
                .instructions
                .iter()
                .position(|instruction| std::ptr::eq(instruction, opline))
                .expect("class-constant opcode belongs to the active op-array");
            attach_constant_expression_trace(
                &exception,
                eg,
                frame,
                op_array,
                instruction_index,
            );
            return Ok(match throw_in_frame(eg, frame, exception)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        };
        value
    } else {
        definition.value.clone()
    };
    cache.set_property(class_id, constant_index, 3);
    set_result(value);
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_assign_static_prop<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_assign_static_prop_impl::<false>(eg, frame, op_array, opline)
}

#[inline(never)]
fn op_assign_late_static_prop<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_assign_static_prop_impl::<true>(eg, frame, op_array, opline)
}

#[inline(never)]
fn op_unset_static_prop<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: both compiler operands belong to the active frame/op-array and
    // remain live for this cold opcode; no mutable slot access occurs here.
    let (class_value, property_value) = unsafe {
        (
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array),
        )
    };
    let dynamic_owner = opline._pad & STATIC_PROP_DYNAMIC_OWNER != 0;
    let dynamic_owner_value = dynamic_owner
        .then(|| dynamic_static_property_owner(eg, class_value))
        .transpose()?;
    let raw_class = dynamic_owner_value
        .as_ref()
        .map_or_else(|| class_value.as_str().unwrap_or(""), |(name, _)| name);
    // SAFETY: dispatch passes an opline from this immutable instruction slice.
    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    // SAFETY: every instruction owns one aligned cache record at the same
    // index; the op-array outlives this frame and the returned shared borrow.
    let cache = unsafe {
        &*(op_array.cache.as_ptr().add(ip) as *const crate::vm::instruction::InlineCache)
    };
    let late_static = opline.extended_value != 0;
    let class_id = dynamic_owner_value.as_ref().map_or_else(
        || {
            if late_static {
                static_property_class_id::<true>(eg, frame, opline, cache, raw_class)
            } else {
                static_property_class_id::<false>(eg, frame, opline, cache, raw_class)
            }
        },
        |(_, class_id)| *class_id,
    );
    if class_id == 0
        && matches!(
            raw_class.to_ascii_lowercase().as_str(),
            "self" | "parent" | "static"
        )
    {
        return Ok(static_property_throw(
            eg,
            frame,
            "Error",
            format!(
                "Cannot access \"{}\" when no class scope is active",
                raw_class.to_ascii_lowercase()
            ),
        )?);
    }
    let Some(class) = eg.class_by_id(class_id) else {
        return Ok(static_property_throw(
            eg,
            frame,
            "Error",
            format!("Class \"{}\" not found", raw_class),
        )?);
    };
    let property = property_value.as_str().unwrap_or("");
    Ok(static_property_throw(
        eg,
        frame,
        "Error",
        format!("Attempt to unset static property {}::${}", class.name, property),
    )?)
}

#[cold]
#[inline(never)]
fn prepare_other_static_reference_constraints(
    eg: &ExecutorGlobals,
    storage_slot: usize,
    value: Value,
    strict: bool,
) -> Result<Value, String> {
    let mut constraints = eg
        .static_property_value(storage_slot)
        .map(Value::reference_property_constraints)
        .unwrap_or_default();
    constraints.retain(|constraint| constraint.owner != storage_slot);
    prepare_reference_assignment(value, &constraints, eg, strict)
}

#[cold]
#[inline(never)]
fn commit_constrained_static_property_value<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    storage_slot: usize,
    value: Value,
    strict: bool,
) -> Result<ColdResult<'a>, VmError> {
    let value = match prepare_other_static_reference_constraints(
        eg,
        storage_slot,
        value,
        strict,
    ) {
        Ok(value) => value,
        Err(message) => {
            return Ok(static_property_throw(
                eg,
                frame,
                "TypeError",
                message,
            )?);
        }
    };
    // SAFETY: every caller supplies a checked inline-cache storage slot owned
    // by this executor; reference validation completes before mutation.
    unsafe { eg.set_static_property_value_unchecked(storage_slot, value) };
    Ok(ColdResult::Done)
}

#[inline(always)]
fn commit_cached_static_property_value<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    storage_slot: usize,
    value: Value,
    strict: bool,
) -> Result<ColdResult<'a>, VmError> {
    if !eg
        .static_property_value(storage_slot)
        .is_some_and(Value::is_owned_reference)
    {
        // SAFETY: cache-hit callers provide a published append-only storage
        // slot; the non-reference guard preserves the ordinary direct write.
        unsafe { eg.set_static_property_value_unchecked(storage_slot, value) };
        return Ok(ColdResult::Done);
    }
    commit_constrained_static_property_value(eg, frame, storage_slot, value, strict)
}

#[inline(always)]
fn op_assign_static_prop_impl<'a, const LATE_STATIC: bool>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    if opline._pad & STATIC_PROP_REFERENCE_BIND != 0 {
        return assign_static_property_reference::<LATE_STATIC>(eg, frame, op_array, opline);
    }
    let source = unsafe {
        &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array)
    };
    let source = if source.is_reference() {
        unsafe { &*source.as_ref_ptr() }
    } else {
        source
    };
    let mut value = if opline._pad & ASSIGN_PROP_MOVE_SOURCE != 0
        && matches!(opline.result_type, OpType::Tmp | OpType::Var)
    {
        unsafe {
            let source = (*frame).get_op_mut(opline.result as u32, opline.result_type);
            if (&*source).is_reference() {
                clone_static_property_value(&*source)
            } else {
                frame_tmp_take!(frame, source)
            }
        }
    } else {
        clone_static_property_value(source)
    };
    // Compact late-static frames already carry the called class ID. Check the
    // monomorphic untyped cache before decoding the two constant string
    // operands; a cache miss still takes the canonical resolver below.
    if LATE_STATIC && opline._pad & LATE_STATIC_PROP_EMBEDDED_SCOPE != 0 {
        let ip = unsafe {
            (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
        };
        let cache = unsafe {
            &*(op_array.cache.as_ptr().add(ip) as *const crate::vm::instruction::InlineCache)
        };
        let class_id = unsafe { ((*frame).heap_bitmap >> 32) as u32 };
        let flags = cache.property_flags();
        let exact_int = flags == 1
            && cache.typed_static_property_tag()
                == crate::vm::instruction::InlineCache::TYPED_PROPERTY_INT;
        if class_id != 0 && cache.class_id == class_id && (flags == 3 || exact_int) {
            if flags == 3 || value.value_type() == ValueType::Long {
                return commit_cached_static_property_value(
                    eg,
                    frame,
                    cache.property_slot(),
                    value,
                    op_array.strict_types,
                );
            }
        }
    }

    let class_name =
        unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let property_name =
        unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let dynamic_owner = opline._pad & STATIC_PROP_DYNAMIC_OWNER != 0;
    let dynamic_owner_value = dynamic_owner
        .then(|| dynamic_static_property_owner(eg, class_name))
        .transpose()?;
    let raw_class = dynamic_owner_value
        .as_ref()
        .map_or_else(|| class_name.as_str().unwrap_or(""), |(name, _)| name);
    let property = property_name.as_str().unwrap_or("");
    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let cache = unsafe {
        &mut *(op_array.cache.as_ptr().add(ip)
            as *mut crate::vm::instruction::InlineCache)
    };
    let class_id = dynamic_owner_value.as_ref().map_or_else(
        || static_property_class_id::<LATE_STATIC>(eg, frame, opline, cache, raw_class),
        |(_, class_id)| *class_id,
    );
    if opline._pad & STATIC_PROP_DYNAMIC_NAME == 0
        && class_id != 0
        && cache.class_id == class_id
    {
        if cache.property_flags() == 3 {
            return commit_cached_static_property_value(
                eg,
                frame,
                cache.property_slot(),
                value,
                op_array.strict_types,
            );
        }
        if cache.property_flags() == 1 {
            let tag = cache.typed_static_property_tag();
            let value_type = value.value_type();
            if tag == crate::vm::instruction::InlineCache::TYPED_PROPERTY_INT
                && value_type == ValueType::Long
            {
                return commit_cached_static_property_value(
                    eg,
                    frame,
                    cache.property_slot(),
                    value,
                    op_array.strict_types,
                );
            }
            #[cfg(feature = "php-generics-reified")]
            let reified_contract = if tag
                == crate::vm::instruction::InlineCache::TYPED_PROPERTY_REIFIED
            {
                cache.reified_static_property_contract()
            } else {
                std::ptr::null()
            };
            #[cfg(feature = "php-generics-reified")]
            if !reified_contract.is_null()
                && unsafe {
                    eg.static_generic_property_contract_remembers(reified_contract, &value)
                }
            {
                return commit_cached_static_property_value(
                    eg,
                    frame,
                    cache.property_slot(),
                    value,
                    op_array.strict_types,
                );
            }
            if tag == crate::vm::instruction::InlineCache::TYPED_PROPERTY_FLOAT
                && value_type == ValueType::Long
            {
                value = Value::double(value.as_long().unwrap() as f64);
            }
            let fast_match = match tag {
                crate::vm::instruction::InlineCache::TYPED_PROPERTY_FLOAT
                    if matches!(value_type, ValueType::Double | ValueType::Long) => true,
                crate::vm::instruction::InlineCache::TYPED_PROPERTY_STRING
                    if value_type == ValueType::String => true,
                crate::vm::instruction::InlineCache::TYPED_PROPERTY_BOOL
                    if matches!(value_type, ValueType::True | ValueType::False) =>
                {
                    true
                }
                crate::vm::instruction::InlineCache::TYPED_PROPERTY_ARRAY
                    if value_type == ValueType::Array => true,
                _ => false,
            };
            if fast_match {
                return commit_cached_static_property_value(
                    eg,
                    frame,
                    cache.property_slot(),
                    value,
                    op_array.strict_types,
                );
            }
            return validate_cached_typed_static_property(
                eg,
                frame,
                op_array,
                cache,
                class_id,
                raw_class,
                opline._pad,
                value,
            );
        }
    }

    assign_static_property_cache_miss(
        eg,
        frame,
        op_array,
        cache,
        class_id,
        raw_class,
        property,
        value,
        opline._pad,
    )
}

#[cold]
#[inline(never)]
fn assign_static_property_reference<'a, const LATE_STATIC: bool>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: dispatch supplies a live frame and its current op-array/opline;
    // compiler validation guarantees both property operands and the source CV
    // are in bounds. The materialized cell remains owned by the source CV and
    // static-property alias before this frame can release either reference.
    unsafe {
    let class_name = &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array);
    let property_name = &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array);
        let source = if opline.result_type == OpType::Cv {
            (*frame).cv_mut(opline.result as u32) as *mut Value
        } else {
            (*frame).get_op_mut(opline.result as u32, opline.result_type)
        };
        if opline.result_type != OpType::Cv && !(&*source).is_reference() {
            report_php_notice(
                eg,
                frame,
                op_array,
                opline,
                "Only variables should be assigned by reference",
            )?;
        }
    let dynamic_owner = opline._pad & STATIC_PROP_DYNAMIC_OWNER != 0;
    let dynamic_owner_value = dynamic_owner
        .then(|| dynamic_static_property_owner(eg, class_name))
        .transpose()?;
    let raw_class = dynamic_owner_value
        .as_ref()
        .map_or_else(|| class_name.as_str().unwrap_or(""), |(name, _)| name);
    let property = property_name.as_str().unwrap_or("");
    let ip = (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize;
    let cache = &mut *(op_array.cache.as_ptr().add(ip)
        as *mut crate::vm::instruction::InlineCache);
    let class_id = dynamic_owner_value.as_ref().map_or_else(
        || static_property_class_id::<LATE_STATIC>(eg, frame, opline, cache, raw_class),
        |(_, class_id)| *class_id,
    );
    let resolved = match resolve_static_property(
        eg,
        frame,
        class_id,
        raw_class,
        property,
        Some("indirectly modify"),
    ) {
        Ok(resolved) => resolved,
        Err(VmError::Fatal(message)) => {
            return Ok(static_property_throw(eg, frame, "Error", message)?);
        }
        Err(error) => return Err(error),
    };
    let definition = &*resolved.definition;
    let called_class = eg
        .class_by_id(class_id)
        .map_or_else(|| raw_class.to_string(), |class| class.name.clone());
    let binding = materialize_reference_alias(frame, source);
    let constraints = binding.reference_property_constraints();
    let current = (&*binding.as_ref_ptr()).clone();
    let prepared = match prepare_typed_property_reference_attachment(
        current,
        definition,
        &constraints,
        eg,
        op_array.strict_types,
        &called_class,
    ) {
        Ok(value) => value,
        Err(message) => {
            return Ok(static_property_throw(eg, frame, "TypeError", message)?);
        }
    };

    let destructor = eg
        .static_property_value(resolved.storage_slot)
        .and_then(|value| {
            (!value.owned_reference_is_aliased())
                .then(|| prepare_replaced_value_destructor(eg, value))
                .flatten()
        });
    if let Some(previous) = eg.static_property_value(resolved.storage_slot) {
        previous.remove_reference_property_constraint(resolved.storage_slot);
    }
    let target = binding.as_ref_ptr();
    std::ptr::drop_in_place(target);
    target.write(prepared);
    let property_binding = if binding.is_owned_reference() {
        binding.clone_owned_reference_alias()
    } else {
        Value::reference(binding.as_ref_ptr())
    };
    if definition.is_typed() && property_binding.is_owned_reference() {
        property_binding.add_reference_property_constraint(
            crate::value::ReferencePropertyConstraint {
                owner: resolved.storage_slot,
                declaring_class: definition.declaring_class.clone(),
                property: definition.name.clone(),
                type_scope: definition.type_scope.clone(),
                called_class: called_class.clone(),
                type_hint: definition.type_hint.clone(),
            },
        );
    }
    if !eg.rebind_static_property_value(resolved.storage_slot, property_binding) {
        return Err(VmError::Fatal("Invalid static property storage slot".into()));
    }
    run_prepared_value_destructor(eg, destructor)?;
    if let Some(result) = take_magic_exception(eg, frame)? {
        return Ok(result);
    }
    if definition
        .set_visibility
        .is_none_or(|visibility| visibility == Visibility::Public)
    {
        cache.set_property(class_id, resolved.storage_slot, 1);
    }
    Ok(ColdResult::Done)
    }
}

#[inline(never)]
fn validate_cached_typed_static_property<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    cache: &mut crate::vm::instruction::InlineCache,
    class_id: u32,
    raw_class: &str,
    assignment_flags: u16,
    mut value: Value,
) -> Result<ColdResult<'a>, VmError> {
    #[cfg(feature = "php-generics-reified")]
    let reified_contract = if cache.typed_static_property_tag()
        == crate::vm::instruction::InlineCache::TYPED_PROPERTY_REIFIED
    {
        cache.reified_static_property_contract()
    } else {
        std::ptr::null()
    };
    #[cfg(feature = "php-generics-reified")]
    let definition = if reified_contract.is_null() {
        cache
            .typed_static_property_definition()
            .expect("typed static cache must retain its definition")
    } else {
        // SAFETY: tag 6 was published from an executor-owned boxed contract
        // and remains stable until executor teardown.
        unsafe { &*eg.static_generic_property_contract_definition(reified_contract) }
    };
    #[cfg(not(feature = "php-generics-reified"))]
    let definition = cache
        .typed_static_property_definition()
        .expect("typed static cache must retain its definition");
    let called_class = eg
        .class_by_id(class_id)
        .map_or(raw_class, |class| class.name.as_str());
    if let Some(overflow) =
        PropertyIncDecOverflow::from_assignment_flags(assignment_flags)
        && let Some(stored) = eg.static_property_value(cache.property_slot())
        && let Some(message) =
            property_incdec_overflow_message(stored, definition, eg, called_class, overflow)
    {
        return Ok(static_property_throw(eg, frame, "TypeError", message)?);
    }
    value = match prepare_property_assignment(
        value,
        definition,
        eg,
        op_array.strict_types,
        called_class,
    ) {
        Ok(value) => value,
        Err(message) => {
            return Ok(static_property_throw(
                eg,
                frame,
                "TypeError",
                message,
            )?);
        }
    };
    #[cfg(feature = "php-generics-reified")]
    if definition.requires_reified_check
        && let Err(message) = eg.check_reified_static_property_value(
            called_class,
            &definition.name,
            &value,
        )
    {
        return Ok(static_property_throw(
            eg,
            frame,
            "TypeError",
            message,
        )?);
    }
    #[cfg(feature = "php-generics-reified")]
    if !reified_contract.is_null() {
        unsafe { eg.remember_static_generic_property_contract(reified_contract, &value) };
    }
    commit_cached_static_property_value(
        eg,
        frame,
        cache.property_slot(),
        value,
        op_array.strict_types,
    )
}

#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn assign_static_property_cache_miss<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    cache: &mut crate::vm::instruction::InlineCache,
    class_id: u32,
    raw_class: &str,
    property: &str,
    mut value: Value,
    assignment_flags: u16,
) -> Result<ColdResult<'a>, VmError> {
    let indirect = assignment_flags & STATIC_PROP_INDIRECT_MODIFY != 0;
    let action = if indirect { "indirectly modify" } else { "modify" };
    let resolved = match resolve_static_property(
        eg,
        frame,
        class_id,
        raw_class,
        property,
        Some(action),
    ) {
        Ok(resolved) => resolved,
        Err(VmError::Fatal(message)) => {
            return Ok(static_property_throw(eg, frame, "Error", message)?);
        }
        Err(error) => return Err(error),
    };
    let definition = unsafe { &*resolved.definition };
    if definition.is_typed() {
        let called_class = eg
            .class_by_id(class_id)
            .map_or(raw_class, |class| class.name.as_str());
        if let Some(overflow) =
            PropertyIncDecOverflow::from_assignment_flags(assignment_flags)
            && let Some(stored) = eg.static_property_value(resolved.storage_slot)
            && let Some(message) =
                property_incdec_overflow_message(stored, definition, eg, called_class, overflow)
        {
            return Ok(static_property_throw(eg, frame, "TypeError", message)?);
        }
        value = match prepare_property_assignment(
            value,
            definition,
            eg,
            op_array.strict_types,
            called_class,
        ) {
            Ok(value) => value,
            Err(message) => {
                return Ok(static_property_throw(
                    eg,
                    frame,
                    "TypeError",
                    message,
                )?);
            }
        };
        #[cfg(feature = "php-generics-reified")]
        if definition.requires_reified_check
            && let Err(message) = eg.check_reified_static_property_value(
                called_class,
                &definition.name,
                &value,
            )
        {
            return Ok(static_property_throw(
                eg,
                frame,
                "TypeError",
                message,
            )?);
        }
    }
    #[cfg(feature = "php-generics-reified")]
    let reified_contract = if definition.requires_reified_check {
        eg.cache_static_generic_property_contract(resolved.definition, &value)
    } else {
        std::ptr::null()
    };
    value = match prepare_other_static_reference_constraints(
        eg,
        resolved.storage_slot,
        value,
        op_array.strict_types,
    ) {
        Ok(value) => value,
        Err(message) => {
            return Ok(static_property_throw(
                eg,
                frame,
                "TypeError",
                message,
            )?);
        }
    };
    if !eg.set_static_property_value(resolved.storage_slot, value) {
        return Err(VmError::Fatal("Invalid static property storage slot".into()));
    }
    let cacheable_write = definition
        .set_visibility
        .is_none_or(|visibility| visibility == Visibility::Public);
    #[cfg(feature = "php-generics-reified")]
    if cacheable_write && !reified_contract.is_null() {
        cache.set_reified_static_property(reified_contract, class_id, resolved.storage_slot);
    } else if cacheable_write && definition.is_typed() {
        cache.set_typed_static_property(definition, class_id, resolved.storage_slot);
    } else if cacheable_write {
        cache.set_property(class_id, resolved.storage_slot, 3);
    }
    #[cfg(not(feature = "php-generics-reified"))]
    if cacheable_write && definition.is_typed() {
        cache.set_typed_static_property(definition, class_id, resolved.storage_slot);
    } else if cacheable_write {
        cache.set_property(class_id, resolved.storage_slot, 3);
    }
    Ok(ColdResult::Done)
}

#[cold]
#[inline(never)]
fn resolve_static_property_read_cache_miss<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    result_ptr: *mut Value,
    cache: &mut crate::vm::instruction::InlineCache,
    class_id: u32,
    raw_class: &str,
    property: &str,
    flags: u16,
) -> Result<ColdResult<'a>, VmError> {
    let silent = flags & STATIC_PROP_SILENT != 0;
    let indirect = flags & STATIC_PROP_INDIRECT_MODIFY != 0;
    let write_action = indirect.then_some("indirectly modify");
    let resolved = match resolve_static_property(
        eg,
        frame,
        class_id,
        raw_class,
        property,
        write_action,
    ) {
        Ok(resolved) => resolved,
        Err(VmError::Fatal(message)) => {
            if silent
                && (message.starts_with("Cannot access private property ")
                    || message.starts_with("Cannot access protected property ")
                    || message.starts_with("Access to undeclared static property "))
            {
                // `isset()` treats inaccessible and undeclared properties as
                // absent, while invalid class resolution remains an Error.
                // SAFETY: result_ptr is the compiler-owned output slot for
                // this live fetch instruction.
                unsafe { frame_tmp_set(frame, result_ptr, Value::null()) };
                return Ok(ColdResult::Done);
            }
            return Ok(static_property_throw(eg, frame, "Error", message)?);
        }
        Err(error) => return Err(error),
    };
    let definition = unsafe { &*resolved.definition };
    let stored = eg
        .static_property_value(resolved.storage_slot)
        .ok_or_else(|| VmError::Fatal("Invalid static property storage slot".into()))?;
    if stored.is_undef() {
        if silent {
            // SAFETY: result_ptr is the compiler-owned output slot for this
            // live frame and has been prepared for one result write.
            unsafe { frame_tmp_set(frame, result_ptr, Value::null()) };
            return Ok(ColdResult::Done);
        }
        return Ok(static_property_throw(
            eg,
            frame,
            "Error",
            format!(
                "Typed static property {}::${} must not be accessed before initialization",
                definition.declaring_class, definition.name
            ),
        )?);
    }
    let value = clone_static_property_value(stored);
    if !indirect
        || definition
            .set_visibility
            .is_none_or(|visibility| visibility == Visibility::Public)
    {
        cache.set_property(class_id, resolved.storage_slot, 1);
    }
    unsafe { frame_tmp_set(frame, result_ptr, value) };
    Ok(ColdResult::Done)
}

#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn resolve_static_property_reference_fetch<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    opline: &Instruction,
    result_ptr: *mut Value,
    cache: &mut crate::vm::instruction::InlineCache,
    class_id: u32,
    raw_class: &str,
    property: &str,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: the resolved definition belongs to executor-global class
    // storage, and result_ptr is the compiler-owned CV output slot in the live
    // frame. Both stay valid for the duration of this cold dispatch.
    unsafe {
    let resolved = match resolve_static_property(
        eg,
        frame,
        class_id,
        raw_class,
        property,
        Some("indirectly modify"),
    ) {
        Ok(resolved) => resolved,
        Err(VmError::Fatal(message)) => {
            return Ok(static_property_throw(eg, frame, "Error", message)?);
        }
        Err(error) => return Err(error),
    };
    let definition = &*resolved.definition;
    let initialize_null = eg
        .static_property_value(resolved.storage_slot)
        .is_some_and(Value::is_undef);
    if initialize_null
        && !property_type_matches_exact(
            &Value::null(),
            &definition.type_hint,
            eg,
            &definition.type_scope,
            raw_class,
        )
    {
        return Ok(static_property_throw(
            eg,
            frame,
            "Error",
            format!(
                "Cannot access uninitialized non-nullable property {}::${} by reference",
                definition.declaring_class, definition.name
            ),
        )?);
    }

    let slot = eg
        .static_property_value_mut(resolved.storage_slot)
        .ok_or_else(|| VmError::Fatal("Invalid static property storage slot".into()))?;
    if initialize_null {
        *slot = Value::null();
    }
    let mut binding = if slot.is_owned_reference() {
        slot.clone_owned_reference_alias()
    } else {
        let current = std::mem::replace(slot, Value::undef());
        let current = reference_initial_value(current);
        let binding = Value::owned_reference(current);
        *slot = binding.clone_owned_reference_alias();
        binding
    };
    if opline._pad & REFERENCE_RESULT_INTERNAL != 0 {
        binding.mark_internal_reference_alias();
    }
    if definition.is_typed() && binding.is_owned_reference() {
        let called_class = eg
            .class_by_id(class_id)
            .map_or(raw_class, |class| class.name.as_str());
        binding.add_reference_property_constraint(crate::value::ReferencePropertyConstraint {
            owner: resolved.storage_slot,
            declaring_class: definition.declaring_class.clone(),
            property: definition.name.clone(),
            type_scope: definition.type_scope.clone(),
            called_class: called_class.to_string(),
            type_hint: definition.type_hint.clone(),
        });
    }
    if definition
        .set_visibility
        .is_none_or(|visibility| visibility == Visibility::Public)
    {
        cache.set_property(class_id, resolved.storage_slot, 1);
    }
    frame_slot_set(frame, result_ptr, binding);
    Ok(ColdResult::Done)
    }
}

struct ResolvedStaticProperty {
    storage_slot: usize,
    definition: *const crate::compiler::compile::PropertyDefinition,
}

#[cold]
#[inline(never)]
fn resolve_static_property(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    class_id: u32,
    raw_class: &str,
    property: &str,
    write_action: Option<&str>,
) -> Result<ResolvedStaticProperty, VmError> {
    if class_id == 0
        && matches!(
            raw_class.to_ascii_lowercase().as_str(),
            "self" | "parent" | "static"
        )
    {
        return Err(VmError::Fatal(format!(
            "Cannot access \"{}\" when no class scope is active",
            raw_class.to_ascii_lowercase()
        )));
    }
    let class = eg.class_by_id(class_id).ok_or_else(|| {
        VmError::Fatal(format!("Class \"{}\" not found", raw_class))
    })?;
    let Some((property_index, definition)) = class
        .static_properties
        .iter()
        .enumerate()
        .find(|(_, definition)| definition.name == property)
    else {
        return Err(VmError::Fatal(format!(
            "Access to undeclared static property {}::${}",
            class.name, property
        )));
    };
    if write_action.is_some() && class.is_enum {
        return Err(VmError::Fatal(format!(
            "Cannot modify readonly property {}::${}",
            class.name, property
        )));
    }
    let caller = get_caller_class(frame, eg);
    let visibility = write_action
        .and_then(|_| definition.set_visibility)
        .unwrap_or(definition.visibility);
    if !eg.check_visibility(
        caller.as_deref(),
        &definition.declaring_class,
        visibility,
    ) {
        let visibility_name = match visibility {
            Visibility::Private => "private",
            Visibility::Protected => "protected",
            Visibility::Public => unreachable!(),
        };
        if definition.set_visibility.is_some()
            && let Some(action) = write_action
        {
            return Err(VmError::Fatal(format!(
                "Cannot {action} {visibility_name}(set) property {}::${property} from {}",
                definition.declaring_class,
                caller
                    .as_deref()
                    .map_or_else(|| "global scope".to_string(), |scope| format!("scope {scope}")),
            )));
        }
        return Err(VmError::Fatal(format!(
            "Cannot access {} property {}::${}",
            visibility_name, definition.declaring_class, property
        )));
    }
    let storage_slot = eg
        .static_property_storage_slot(class_id, property_index)
        .ok_or_else(|| VmError::Fatal("Invalid static property storage mapping".into()))?;
    Ok(ResolvedStaticProperty {
        storage_slot,
        definition,
    })
}

#[inline(never)]
fn op_instanceof(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) {
    let obj_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let class_name = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    // PHP accepts an object on the right side and uses its canonical runtime
    // class. This matters for aliases: the object's layout retains the
    // declared class identity rather than the spelling used at construction.
    let object_target = class_name
        .as_object()
        .map(|object| object.class_name.clone());
    let raw_target = object_target
        .as_deref()
        .or_else(|| class_name.as_str())
        .unwrap_or("");
    let dynamic_target = (opline._pad & INSTANCEOF_DYNAMIC_STATIC_SCOPE != 0)
        .then(|| resolve_static_call_class(eg, frame, raw_target, true))
        .flatten();
    let target = dynamic_target.as_deref().unwrap_or(raw_target);
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
    let is_instance = if obj_val.value_type() == ValueType::Closure {
        eg.class_is_a("Closure", target)
    } else {
        obj_val
            .as_object()
            .is_some_and(|object| eg.class_is_a(&object.class_name, target))
    };
    unsafe { frame_result_set(frame, result_ptr, opline.result_type, Value::bool(is_instance)) };
}

#[inline(never)]
fn op_fetch_const(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    if opline.extended_value == 1 {
        // SAFETY: both compiler-emitted operands belong to the live frame and
        // remain immutable while define() clones their name and value.
        let (name_val, value_val) = unsafe {
            (
                &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
                &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array),
            )
        };
        let name = name_val.as_str().unwrap_or("").to_string();
        if eg.find_constant(&name).is_some() {
            report_php_warning(
                eg,
                frame,
                op_array,
                opline,
                &crate::runtime::constant_redefinition_message(&name),
                false,
            )?;
        } else {
            eg.define_constant(&name, value_val.clone())
                .map_err(VmError::Fatal)?;
        }
    } else {
        let name_val =
            unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
        let name = name_val.as_str().unwrap_or("");
        let mut value = eg.find_constant(name);
        if value.is_none() && opline.extended_value == 2 {
            // SAFETY: the compiler emits operand 2 as an in-bounds constant
            // literal exactly when the namespace-fallback marker is set.
            let fallback = unsafe {
                &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)
            };
            let fallback = fallback.as_str().unwrap_or("");
            value = eg.find_constant(fallback);
        }
        let value =
            value.ok_or_else(|| VmError::Fatal(format!("Undefined constant \"{}\"", name)))?;
        if eg.constant_deprecation_metadata_present {
            // SAFETY: `opline` belongs to this op-array and its same-index
            // cache entry remains stable for single-threaded opcode execution.
            let cache = unsafe {
                let ip = (opline as *const Instruction)
                    .offset_from(op_array.instructions.as_ptr()) as usize;
                &mut *(op_array.cache.as_ptr().add(ip)
                    as *mut crate::vm::instruction::InlineCache)
            };
            let generation = eg.constant_deprecation_generation;
            let requires_deprecated_use_check = if cache.class_id == generation {
                cache.property_flags() == 2
            } else {
                let requires_deprecated_use_check = eg.constant_requires_deprecated_use_check(name)
                    || (opline.extended_value == 2
                        && op_array
                            .literals()
                            .get(opline.op2 as usize)
                            .and_then(Value::as_str)
                            .is_some_and(|fallback| {
                                eg.constant_requires_deprecated_use_check(fallback)
                            }));
                cache.set_property(
                    generation,
                    0,
                    if requires_deprecated_use_check { 2 } else { 1 },
                );
                requires_deprecated_use_check
            };
            if requires_deprecated_use_check {
                let resolved_name = if eg.find_constant(name).is_some() {
                    name
                } else {
                    op_array
                        .literals()
                        .get(opline.op2 as usize)
                        .and_then(Value::as_str)
                        .unwrap_or(name)
                };
                let use_site = deprecated_use_site(frame, op_array, opline);
                crate::stdlib::reflection::report_deprecated_global_constant_use(
                    resolved_name,
                    &use_site,
                    eg,
                )?;
            }
        }
        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
        unsafe { frame_result_set(frame, result_ptr, opline.result_type, value) };
    }
    Ok(())
}

#[inline(never)]
fn op_report_deprecated_trait_uses(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    let consumer = op_array
        .literals()
        .get(opline.op1 as usize)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let uses = eg.find_class(&consumer).map_or_else(Vec::new, |class| {
        class
            .uses
            .iter()
            .filter_map(|trait_name| {
                eg.find_class(trait_name).map(|trait_definition| {
                    (trait_definition.name.clone(), trait_definition.attributes.clone())
                })
            })
            .collect::<Vec<_>>()
    });
    if uses.is_empty() {
        return Ok(());
    }
    let use_site = deprecated_use_site(frame, op_array, opline);
    for (trait_name, attributes) in uses {
        crate::stdlib::reflection::report_deprecated_trait_use(
            &trait_name,
            &consumer,
            &attributes,
            &use_site,
            eg,
        )?;
        if eg.exception.is_some() {
            break;
        }
    }
    Ok(())
}

#[inline(never)]
fn op_bind_default_param(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> bool {
    let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };
    if unsafe { (*cv_ptr).is_undef() } {
        return false;
    }
    unsafe {
        (*frame).opline = op_array.instructions.as_ptr().add(opline.op2 as usize);
    }
    true
}

#[inline(never)]
fn op_bind_global(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) {
    // SAFETY: the compiler validated the global-name operand for this live
    // frame and its op array before dispatch reached this opcode.
    let name_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let name = name_val.as_str().unwrap_or("").to_string();
    // SAFETY: BindGlobal's destination is compiler-validated as a CV in this frame.
    let cv_ptr = unsafe { (*frame).cv_mut(opline.op1 as u32) as *mut Value };
    // At top level `global $name` binds the symbol table to the CV that is
    // already the same global-scope variable. Do not replace an initialized
    // value with null merely because the detached globals snapshot has not
    // been populated yet. A function-local CV, by contrast, must discard its
    // prior local value and acquire the global binding.
    if !op_array.main_scope_vars.is_empty() && !unsafe { (*cv_ptr).is_undef() } {
        return;
    }
    let value = eg.globals.get(&name).cloned().unwrap_or_else(Value::null);
    let binding = if value.is_owned_reference() {
        value.clone_owned_reference_alias()
    } else {
        Value::owned_reference(reference_initial_value(value))
    };
    globals_set(
        &mut eg.globals,
        &name,
        binding.clone_owned_reference_alias(),
    );
    // SAFETY: cv_ptr is the live BindGlobal destination derived above.
    unsafe { frame_slot_set(frame, cv_ptr, binding) };
}

#[inline(never)]
fn set_global_snapshot_entry(snapshot: &mut PhpArray, name: &str, value: Value) {
    if let Some(key) = canonical_decimal_array_key(name) {
        snapshot.set_int(key, value);
    } else {
        snapshot.set_str(name, value);
    }
}

#[inline(never)]
fn op_global_dimension<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let scope_vars = if !op_array.main_scope_vars.is_empty() {
        &op_array.main_scope_vars
    } else {
        &op_array.global_vars
    };

    // SAFETY: each dedicated global opcode is emitted only with validated
    // operands belonging to this live frame/op-array pair. CV indices in
    // scope metadata come from the same compiler allocation, and every slot
    // replacement goes through the frame bitmap helpers before old owners
    // are dropped.
    unsafe {
        if opline.opcode == OpCode::FetchGlobals {
            let mut snapshot = PhpArray::with_hash_capacity(eg.globals.len() + scope_vars.len());

            for (name, value) in &eg.globals {
                if name != "GLOBALS" && value.value_type() != ValueType::Undef {
                    set_global_snapshot_entry(&mut snapshot, name, value.clone());
                }
            }
            for (cv, name) in scope_vars {
                if name == "GLOBALS" {
                    continue;
                }
                let value = (&*(*frame).get_op_ptr(*cv, OpType::Cv, op_array)).clone();
                if value.value_type() == ValueType::Undef {
                    let key = canonical_decimal_array_key(name)
                        .map(ArrayKey::Int)
                        .unwrap_or_else(|| ArrayKey::String(name.clone()));
                    snapshot.remove(&key);
                } else {
                    set_global_snapshot_entry(&mut snapshot, name, value);
                }
            }

            let result = (*frame).get_op_mut(opline.result as u32, opline.result_type);
            write_fetch_dim_result(frame, result, Value::array(snapshot));
            return Ok(ColdResult::Done);
        }

        let key = &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array);
        let name = value_to_global_name(key)?;
        match opline.opcode {
            OpCode::FetchGlobal => {
                let local = scope_vars
                    .iter()
                    .find(|(_, variable)| variable == &name)
                    .map(|(cv, _)| {
                        (&*(*frame).get_op_ptr(*cv, OpType::Cv, op_array)).clone()
                    });
                let value = local
                    .or_else(|| eg.globals.get(&name).cloned())
                    .unwrap_or_else(Value::undef);
                let value = if opline._pad & FETCH_DIM_ISSET != 0 {
                    Value::bool(!matches!(
                        value.value_type(),
                        ValueType::Null | ValueType::Undef
                    ))
                } else {
                    value
                };
                let result = (*frame).get_op_mut(opline.result as u32, opline.result_type);
                write_fetch_dim_result(frame, result, value);
            }
            OpCode::AssignGlobal => {
                let mut value =
                    (&*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)).clone();
                let mut constraints = eg
                    .globals
                    .get(&name)
                    .map(Value::reference_property_constraints)
                    .unwrap_or_default();
                if constraints.is_empty()
                    && let Some((cv, _)) = scope_vars.iter().find(|(_, variable)| variable == &name)
                {
                    constraints = (*frame).cv(*cv).reference_property_constraints();
                }
                value = match prepare_reference_assignment(
                    value,
                    &constraints,
                    eg,
                    op_array.strict_types,
                ) {
                    Ok(value) => value,
                    Err(message) => {
                        return Ok(static_property_throw(
                            eg,
                            frame,
                            "TypeError",
                            message,
                        )?);
                    }
                };
                globals_assign(&mut eg.globals, &name, value.clone());
                eg.dirty_globals.insert(name.clone());
                if let Some((cv, _)) = scope_vars.iter().find(|(_, variable)| variable == &name) {
                    let is_reference = (*frame).cv(*cv).is_reference();
                    let destination = (*frame).get_op_mut(*cv, OpType::Cv);
                    if is_reference {
                        slot_set(destination, value);
                    } else {
                        frame_slot_set(frame, destination, value);
                    }
                }
            }
            OpCode::UnsetGlobal => {
                globals_set(&mut eg.globals, &name, Value::undef());
                eg.dirty_globals.insert(name.clone());
                if let Some((cv, _)) = scope_vars.iter().find(|(_, variable)| variable == &name) {
                    frame_slot_set(frame, (*frame).cv_mut(*cv), Value::undef());
                }
            }
            OpCode::BindGlobalRef => {
                let current_cv = scope_vars
                    .iter()
                    .find(|(_, variable)| variable == &name)
                    .map(|(cv, _)| *cv);
                let binding = if let Some(cv) = current_cv {
                    let slot = (*frame).cv_mut(cv);
                    if slot.is_owned_reference() {
                        slot.clone_owned_reference_alias()
                    } else {
                        let owned = Value::owned_reference(reference_initial_value(slot.clone()));
                        let alias = owned.clone_owned_reference_alias();
                        frame_slot_set(frame, slot, owned);
                        alias
                    }
                } else if let Some(value) = eg.globals.get(&name) {
                    if value.is_owned_reference() {
                        value.clone_owned_reference_alias()
                    } else {
                        Value::owned_reference(reference_initial_value(value.clone()))
                    }
                } else {
                    Value::owned_reference(Value::null())
                };
                let mut destination_binding = binding.clone_owned_reference_alias();
                if opline._pad & REFERENCE_RESULT_INTERNAL != 0 {
                    destination_binding.mark_internal_reference_alias();
                }
                globals_set(
                    &mut eg.globals,
                    &name,
                    binding.clone_owned_reference_alias(),
                );
                eg.dirty_globals.insert(name.clone());
                frame_slot_set(
                    frame,
                    (*frame).cv_mut(opline.result as u32),
                    destination_binding,
                );
            }
            OpCode::AssignGlobalRef => {
                let source = (*frame).cv_mut(opline.op2 as u32);
                let binding = if source.is_owned_reference() {
                    source.clone_owned_reference_alias()
                } else {
                    let owned = Value::owned_reference(reference_initial_value(source.clone()));
                    let alias = owned.clone_owned_reference_alias();
                    frame_slot_set(frame, source, owned);
                    alias
                };
                globals_set(
                    &mut eg.globals,
                    &name,
                    binding.clone_owned_reference_alias(),
                );
                eg.dirty_globals.insert(name.clone());
                if let Some((cv, _)) = scope_vars.iter().find(|(_, variable)| variable == &name) {
                    frame_slot_set(
                        frame,
                        (*frame).cv_mut(*cv),
                        binding.clone_owned_reference_alias(),
                    );
                }
            }
            _ => unreachable!("op_global_dimension called for a non-global opcode"),
        }
    }
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_check_static(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> bool {
    // SAFETY: CheckStatic carries compiler-validated operands and a jump
    // target within this live op array. The frame remains live throughout;
    // replacing its raw CV intentionally installs the request-owned cell.
    unsafe {
        let name_val = &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array);
        let var_name = name_val.as_str().unwrap_or("").to_string();
        let func_name = op_array.literals[opline.extended_value as usize]
            .as_str()
            .unwrap_or("")
            .to_string();
        if let Some((_, global_name)) = op_array
            .global_vars
            .iter()
            .find(|(cv, _)| *cv == u32::from(opline.op1))
        {
            let current = &*(*frame).cv_mut(u32::from(opline.op1));
            let value = if current.is_owned_reference() {
                current.clone_owned_reference_alias()
            } else {
                current.clone()
            };
            globals_set(&mut eg.globals, global_name, value);
            eg.dirty_globals.insert(global_name.clone());
        }
        let binding = eg.with_function_static_vars_mut(
            frame as usize,
            &func_name,
            |statics| {
                if let Some(stored) = statics.get(&var_name) {
                    if stored.is_static_initializer_in_progress() {
                        // A recursive call entered while the initializer is
                        // still evaluating. PHP evaluates it recursively too.
                        return None;
                    }
                    let binding = if stored.is_owned_reference() {
                        stored.clone_owned_reference_alias()
                    } else {
                        Value::owned_reference(stored.clone())
                    };
                    if !stored.is_owned_reference() {
                        statics.insert(var_name.clone(), binding.clone_owned_reference_alias());
                    }
                    return Some(binding);
                }

                let binding = Value::owned_reference(Value::null());
                let mut stored = binding.clone_owned_reference_alias();
                stored.mark_static_initializer_in_progress();
                statics.insert(var_name.clone(), stored);
                None
            },
        );
        if let Some(binding) = binding {
            slot_set((*frame).cv_mut(opline.op1 as u32), binding);
            (*frame).opline = op_array.instructions.as_ptr().add(opline.result as usize);
            return true;
        }
        false
    }
}

#[inline(never)]
fn op_bind_static(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    // SAFETY: BindStatic carries compiler-validated operands for this live
    // frame/op-array pair. Both raw writes replace initialized Values, and
    // the request-owned reference cell outlives the installed CV alias.
    unsafe {
        let name_val = &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array);
        let var_name = name_val.as_str().unwrap_or("").to_string();
        let func_name = op_array.literals[opline.extended_value as usize]
            .as_str()
            .unwrap_or("")
            .to_string();
        if let Some((_, global_name)) = op_array
            .global_vars
            .iter()
            .find(|(cv, _)| *cv == u32::from(opline.op1))
        {
            let current = &*(*frame).cv_mut(u32::from(opline.op1));
            let value = if current.is_owned_reference() {
                current.clone_owned_reference_alias()
            } else {
                current.clone()
            };
            globals_set(&mut eg.globals, global_name, value);
            eg.dirty_globals.insert(global_name.clone());
        }
        let initial = if opline.result_type != OpType::Unused {
            (&*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array)).clone()
        } else {
            Value::null()
        };

        let binding = eg.with_function_static_vars_mut(
            frame as usize,
            &func_name,
            |statics| {
                let stored = statics
                    .entry(var_name)
                    .or_insert_with(|| Value::owned_reference(Value::null()));
                if !stored.is_owned_reference() {
                    *stored = Value::owned_reference(stored.clone());
                }
                if stored.is_static_initializer_in_progress() {
                    slot_set(stored.as_ref_ptr(), initial);
                    stored.clear_static_initializer_in_progress();
                }
                stored.clone_owned_reference_alias()
            },
        );
        let destination = (*frame).cv_mut(opline.op1 as u32);
        let destructor = prepare_replaced_value_destructor(eg, &*destination);
        slot_set(destination, binding);
        run_prepared_value_destructor(eg, destructor)?;
    }
    Ok(())
}

#[inline(never)]
fn op_create_closure(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) {
    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let cached = op_array.cache[ip].func;
    let func_ptr = if !cached.is_null() {
        cached
    } else {
        let name_val =
            unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
        let name = name_val
            .as_str()
            .expect("CreateClosure: op1 must be a function name string");
        let ptr = eg
            .find_function(name)
            .unwrap_or_else(|| panic!("CreateClosure: closure function {} not found", name));
        unsafe {
            (*(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache)).func =
                ptr;
        }
        ptr
    };
    let common = unsafe { &*func_ptr };
    let trait_scope_class_id = if opline._pad
        & crate::vm::instruction::CLOSURE_FLAG_TRAIT_LEXICAL_SCOPE
        != 0
    {
        // SAFETY: this compiler-only flag accompanies an op2 that names the
        // initialized hidden trait-scope TMP in the live frame.
        unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) }
            .as_str()
            .map_or(0, |class| eg.class_id_of(class))
    } else {
        0
    };
    let called_scope_class_id = if common.plan.needs_late_static_scope() {
        late_static_call_class_id(eg, frame)
    } else if trait_scope_class_id != 0 {
        trait_scope_class_id
    } else if opline.op2_type == OpType::Const {
        op_array.literals[opline.op2 as usize]
            .as_str()
            .map_or(0, |class| eg.class_id_of(class))
    } else {
        get_caller_class(frame, eg)
            .as_deref()
            .map_or(0, |class| eg.class_id_of(class))
    };
    let is_static = (opline._pad & crate::vm::instruction::CLOSURE_FLAG_STATIC) != 0;
    let bound_this = closure_bound_this(frame, op_array, is_static);
    let static_vars = ((opline._pad & crate::vm::instruction::CLOSURE_FLAG_HAS_STATICS) != 0)
        .then(|| {
            std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new()))
        });
    let closure = PhpClosure {
        object_handle: 0,
        func: func_ptr,
        called_scope_class_id,
        trait_scope_class_id,
        is_static,
        bound_this,
        captures: Vec::with_capacity(opline.extended_value as usize),
        static_vars,
        has_heap_captures: false,
    };
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
    unsafe { frame_tmp_set(frame, result_ptr, Value::closure(closure)) };
}

#[inline(never)]
fn op_create_first_class_callable<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: opline operands and result identify compiler-allocated slots in
    // this live frame; the read is cloned before callback resolution mutates VM state.
    let (callable, instruction_index) = unsafe {
        (
            (&*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)).clone(),
            (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize,
        )
    };
    let existing_closure = if callable.value_type() == ValueType::Closure {
        Some(callable.clone())
    } else {
        callable.as_array().and_then(|array| {
            (array.len() == 2
                && array
                    .get_value_at(1)
                    .and_then(Value::as_str)
                    .is_some_and(|method| method.eq_ignore_ascii_case("__invoke")))
            .then(|| array.get_value_at(0))
            .flatten()
            .filter(|receiver| receiver.value_type() == ValueType::Closure)
            .cloned()
        })
    };
    if let Some(closure) = existing_closure {
        // SAFETY: the result operand names the prepared compiler-owned slot. A
        // first-class callable made from Closure or Closure::__invoke is that
        // same PHP object, so cloning retains identity and ownership.
        unsafe {
            let result_ptr = (*frame).get_op_mut(opline.result as u32, opline.result_type);
            frame_tmp_set(frame, result_ptr, closure);
        }
        return Ok(ColdResult::Done);
    }
    let caller_class = get_caller_class(frame, eg);
    let resolved = crate::stdlib::resolve_callback_with_cache(
        &callable,
        eg,
        caller_class.as_deref(),
        None,
    )
    .or_else(|| {
        if opline.extended_value == 0 {
            return None;
        }
        let fallback = &op_array.literals[opline.extended_value as usize];
        crate::stdlib::resolve_callback_with_cache(
            fallback,
            eg,
            caller_class.as_deref(),
            None,
        )
    });
    let Some(resolved) = resolved else {
        let message = crate::stdlib::first_class_callable_error(
            &callable,
            eg,
            caller_class.as_deref(),
        );
        let error = make_error_value("Error", &message);
        attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
        return Ok(match throw_in_frame(eg, frame, error)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
        });
    };

    let closure = crate::stdlib::resolved_callback_into_closure(resolved, eg);
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
    // SAFETY: `get_op_mut` returned the prepared compiler-owned result slot for
    // this frame; it is initialized exactly once with the newly owned closure.
    unsafe { frame_tmp_set(frame, result_ptr, closure) };
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_closure_use_var(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) {
    let cloned_value = if opline._pad & crate::vm::instruction::CLOSURE_USE_REFERENCE != 0 {
        // Closure use variables are compiler-guaranteed CVs. Promote an
        // ordinary local to a request-owned cell so both the active frame and
        // every closure copy can retain it after this frame returns.
        let source = unsafe { (*frame).cv_mut(opline.op2 as u32) as *mut Value };
        unsafe {
            if (*source).is_owned_reference() {
                (*source).clone_owned_reference_alias()
            } else if (*source).is_reference() {
                Value::reference((*source).as_ref_ptr())
            } else {
                let current = reference_initial_value(std::mem::replace(
                    &mut *source,
                    Value::undef(),
                ));
                let binding = Value::owned_reference(current);
                frame_slot_set(frame, source, binding.clone_owned_reference_alias());
                binding
            }
        }
    } else {
        let value = unsafe {
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)
        };
        value.clone()
    };
    let closure_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
    // SAFETY: ClosureUseVar targets the live CreateClosure result TMP, which
    // remains initialized and exclusively owned during this bytecode sequence.
    unsafe { &mut *closure_ptr }.push_closure_capture(cloned_value);
}
