use std::collections::HashMap;
#[cfg(feature = "vm-stats")]
use std::sync::OnceLock;
use std::sync::atomic::Ordering;

use super::frame::{CALL_FRAME_SLOTS, ExecuteData, HeapSlotIter};
use super::function::{
    BinaryLongRecursionPlan, CallStrategy, ComposedScalarDoubleFunctionPlan,
    ComposedScalarDoubleOp, ComposedScalarLongFunctionPlan, ComposedScalarLongOp,
    ComposedTypedLongFunctionPlan, ComposedTypedLongOp, FUNC_HOT_THRESHOLD, Function,
    FunctionCommon, FunctionType, HotStatus, LongPlanSource, LongPropertyMethodPlan,
    LongPropertyOp, LongRecursiveBase, LongRecursiveCombine, LongRecursiveCondition,
    ObjectArrayFunctionPlan, ObjectArrayLongCall, ObjectArrayLongOp, ObjectArraySource,
    ObjectLongFunctionPlan, ObjectLongObjectSource, ObjectLongOp, ObjectLongSource, ParamTypeHint,
    PropertyGetterMethodPlan, PropertyInitMethodPlan, ReturnStrategy, ScalarDoubleFunctionPlan,
    ScalarDoubleOpKind, ScalarDoubleProgram, ScalarDoubleSource, ScalarLongCall,
    ScalarLongCallGuard, ScalarLongConditionKind, ScalarLongConditionOperand,
    ScalarLongFunctionPlan, ScalarLongOp, ScalarLongOpKind, ScalarLongProgram, ScalarLongSource,
    ScalarStringFunctionPlan, ScalarStringSource, UserFunction,
};
#[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
use super::function::{
    CapturedTypedLongFunctionPlan, IndirectScalarLongCallable, IndirectScalarLongFunctionPlan,
};
use super::instruction::{
    ARRAY_ELEMENT_REFERENCE, ARRAY_INIT_DYNAMIC_CALL_CLASS, ARRAY_INIT_HASH_HINT,
    ARRAY_UNPACK_CONSTANT_EXPRESSION, ASSIGN_CV_MOVE_SOURCE, ASSIGN_CV_REBIND,
    ASSIGN_DIM_KEY_ALREADY_NORMALIZED, ASSIGN_DIM_UNSET_REBUILD, ASSIGN_OBJ_CLONE_WITH,
    ASSIGN_OBJ_MODIFY, ASSIGN_PROP_MOVE_SOURCE, CALL_FLAG_CALLBACK_ARRAY_PIPELINE,
    CALL_FLAG_CALLBACK_ARRAY_PIPELINE_FILTER_FIRST, CALL_FLAG_CALLBACK_ARRAY_PIPELINE_JSON_SINK,
    CALL_FLAG_CALLBACK_ARRAY_PIPELINE_STAGED_METADATA, CALL_FLAG_DEFERRED_SCALAR_CANDIDATE,
    CALL_FLAG_DYNAMIC_STATIC_SCOPE, CALL_FLAG_ERROR_SUPPRESS, CALL_FLAG_EXACT_SCALAR_ARGS,
    CALL_FLAG_FILTER_MAP_CALLBACK_ARRAY_PIPELINE, CALL_FLAG_OBJECT_ARRAY_CONSUMERS,
    CALL_FLAG_RETURN_EXPLICITLY_IGNORED, CALL_FLAG_STAGED_CALLBACK_ARRAY_PIPELINE,
    CALL_USER_FUNC_ARRAY_SOURCE_UNPACK, CLASS_CONST_COMPILE_TIME_NAME,
    CLASS_CONST_CONSTANT_EXPRESSION, CLASS_CONST_DYNAMIC_NAME, CLASS_CONST_DYNAMIC_OWNER,
    CLONE_OBJ_WITH_PROPERTIES, FETCH_DIM_DESTRUCTURE, FETCH_DIM_EMPTY, FETCH_DIM_ERROR_SUPPRESS,
    FETCH_DIM_ISSET, FETCH_DIM_MUTABLE, FETCH_DIM_SILENT, FETCH_DYNAMIC_ERROR_SUPPRESS,
    FETCH_DYNAMIC_RETAIN_NAME, FETCH_DYNAMIC_SILENT, FETCH_OBJ_COMPOUND, FETCH_OBJ_ERROR_SUPPRESS,
    FETCH_OBJ_INCDEC, FETCH_OBJ_MODIFY, FETCH_OBJ_REFERENCE_SOURCE, FETCH_OBJ_SILENT,
    INSTANCEOF_DYNAMIC_STATIC_SCOPE, Instruction, KnownScalarType, LATE_STATIC_PROP_EMBEDDED_SCOPE,
    NEW_FLAG_DYNAMIC_CLASS_NAME, NEW_FLAG_DYNAMIC_STATIC_SCOPE, NEW_FLAG_UNPACKED_ARGUMENTS,
    NEW_FLAG_VIRTUAL_DECLARED_READS, NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE,
    OBJ_PROP_REFERENCE_BIND, OpType, PROPERTY_INCDEC_DECREMENT, PROPERTY_INCDEC_INCREMENT,
    REFERENCE_RESULT_INTERNAL, REFERENCE_SOURCE_MAY_BE_NONREFERENCEABLE, SEND_FLAG_GLOBALS,
    SEND_FLAG_NONREFERENCEABLE, STATIC_PROP_DYNAMIC_NAME, STATIC_PROP_DYNAMIC_OWNER,
    STATIC_PROP_INDIRECT_MODIFY, STATIC_PROP_REFERENCE_BIND, STATIC_PROP_REFERENCE_FETCH,
    STATIC_PROP_SILENT, THROW_FLAG_UNHANDLED_MATCH, UNSET_DIM_NESTED,
};
use super::opcode::OpCode;
#[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
use super::quick::QuickIndirectScalarCall;
use super::quick::{
    QUICK_LOOP_COUNTER_STRIDE, QUICK_LOOP_DISABLED, QUICK_LOOP_FAILURE_LIMIT,
    QUICK_LOOP_HOT_THRESHOLD, QUICK_STRING_FETCH_CACHE_LIMIT, QuickArrayIndex,
    QuickDoubleArgumentProgram, QuickDoubleCallAccumulateLoop, QuickDoubleSource,
    QuickIncrementKind, QuickInvariantInput, QuickInvariantPathElement, QuickInvariantValueKind,
    QuickLongAccumulateLoop, QuickLongBound, QuickLongCondition, QuickLongInductionLoop,
    QuickLongOp, QuickLongOperand, QuickLongOpsLoop, QuickLongTarget, QuickLongTerm,
    QuickObjectArrayConsumer, QuickObjectLongArgument, QuickObjectLongMethodCall,
    QuickStringAppendSource, QuickTypedInvariantProducer, QuickTypedMethodCall,
    QuickVirtualDeclaredPropertyRead, QuickVirtualValueSource, ResolvedScalarDoubleProgram,
    compose_quick_scalar_leaf_program, compose_scalar_double_program,
};
#[cfg(all(
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
use crate::jit::{
    NATIVE_QUICK_LONG_MAX_CALL_TARGETS, NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES,
    NATIVE_STRAIGHT_LONG_MAX_OPERATIONS, NativeStraightLongConditionOperand,
    NativeStraightLongLoopConfig, NativeStraightLongLoopOutcome, NativeStraightLongOperation,
    ScalarDoubleJitDispatch, ScalarLongJitDispatch,
};
#[cfg(all(
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
use crate::jit::{
    NativeConditionalLongLoopCondition, NativeConditionalLongLoopConfig, NativeLongAccumulateState,
    QuickLongAccumulateJitOutcome,
};
use crate::parser::Visibility;
use crate::runtime::ExecutorGlobals;
#[cfg(feature = "quick-loops")]
use crate::value::ExactOrderedIntLayout;
use crate::value::{
    ArrayKey, PhpArray, PhpClosure, PhpObject, Value, ValueType, canonical_decimal_array_key,
    make_error_value,
};
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
use crate::value::{NativeIndexedLongLookupContext, NativeLongArraySetContext};
use crate::vm::stats;
// Planner module is kept as scaffolding for future hot-executor architecture.
// Not used in baseline dispatch loop — will be integrated via function-entry dispatch.

#[inline(always)]
fn direct_user_calls_enabled() -> bool {
    #[cfg(feature = "vm-stats")]
    {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var_os("RPHP_DISABLE_DIRECT_USER_CALLS").is_none())
    }
    #[cfg(not(feature = "vm-stats"))]
    {
        true
    }
}

#[inline(always)]
fn deferred_scalar_calls_enabled() -> bool {
    #[cfg(feature = "vm-stats")]
    {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var_os("RPHP_DISABLE_DEFERRED_SCALAR_CALLS").is_none())
    }
    #[cfg(not(feature = "vm-stats"))]
    {
        true
    }
}

#[inline(always)]
fn composed_scalar_calls_enabled() -> bool {
    #[cfg(feature = "vm-stats")]
    {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var_os("RPHP_DISABLE_COMPOSED_SCALAR_CALLS").is_none())
    }
    #[cfg(not(feature = "vm-stats"))]
    {
        true
    }
}

#[inline(always)]
fn composed_scalar_bodies_enabled() -> bool {
    #[cfg(feature = "vm-stats")]
    {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var_os("RPHP_DISABLE_COMPOSED_SCALAR_BODIES").is_none())
    }
    #[cfg(not(feature = "vm-stats"))]
    {
        true
    }
}

#[inline(always)]
fn direct_property_getters_enabled() -> bool {
    #[cfg(feature = "vm-stats")]
    {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var_os("RPHP_DISABLE_DIRECT_PROPERTY_GETTERS").is_none())
    }
    #[cfg(not(feature = "vm-stats"))]
    {
        true
    }
}

#[inline(always)]
fn composed_property_calls_enabled() -> bool {
    #[cfg(feature = "vm-stats")]
    {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var_os("RPHP_DISABLE_COMPOSED_PROPERTY_CALLS").is_none())
    }
    #[cfg(not(feature = "vm-stats"))]
    {
        true
    }
}

/// Resolve an operand whose exact, non-reference representation was proven by
/// the compiler. Unlike `get_op_ptr`, a CV does not need a reference-tag test.
#[inline(always)]
unsafe fn proven_scalar_op_ptr(
    frame: *const ExecuteData,
    op_array: &crate::compiler::OpArray,
    operand: u16,
    op_type: OpType,
) -> *const Value {
    match op_type {
        OpType::Const => &op_array.literals()[operand as usize] as *const Value,
        OpType::Cv => (*frame).cv(operand as u32) as *const Value,
        OpType::Tmp | OpType::Var => {
            (frame as *const Value).add(CALL_FRAME_SLOTS + operand as usize)
        }
        OpType::Unused => unreachable!("proven scalar operand cannot be unused"),
    }
}

/// Get the current caller's **lexical** (declaring) class name from the frame.
/// Uses the `method_declaring_class` map on EG rather than runtime $this,
/// so that `private` checks use the class that defines the code, not the
/// dynamic receiver.  Returns None if in top-level code or a plain function.
#[inline]
fn get_caller_class(frame: *mut ExecuteData, eg: &ExecutorGlobals) -> Option<String> {
    if frame.is_null() {
        return None;
    }
    // SAFETY: callers pass the live executing frame. Its function pointer and
    // compiler-sized CV range remain valid for this non-reentrant scope probe.
    unsafe {
        let func = (*frame).func;
        if func.is_null() {
            return None;
        }
        if let Some(class) = eg.declaring_class_of(func) {
            let is_trait = eg
                .class_table
                .get(class)
                .is_some_and(|definition| definition.is_trait);
            if !is_trait {
                return Some(class.to_string());
            }

            // Trait op arrays are shared by every consuming class. Their lexical
            // visibility scope is the nearest class that composed the trait, not
            // whichever consumer happened to register last.
            let receiver_class = if (*frame).num_cvs == 0 {
                None
            } else {
                let receiver = (*frame).cv(0);
                (receiver.value_type() == ValueType::Object)
                    .then(|| receiver.object_class_name_unchecked().to_string())
            };
            if let Some(receiver_class) = receiver_class
                && let Some(scope) = eg.trait_composition_scope(&receiver_class, class)
            {
                return Some(scope.to_string());
            }
        }
    }

    // Closure::bind() may assign a lexical visibility scope to a closure that
    // was declared outside any class. Dynamic-closure initialization publishes
    // that explicit scope on the closure frame; do not recover a class from an
    // ordinary caller here, because plain functions never inherit visibility.
    let embedded = frame_embedded_late_static_class_id(frame);
    let class_id = if embedded != 0 {
        embedded
    } else {
        eg.late_static_scope_class_id(frame as usize)
    };
    eg.class_by_id(class_id).map(|class| class.name.clone())
}

/// Resolve the class part of a static call without rewriting its bytecode
/// literal. The literal must stay `self`/`parent`: late-static return checks
/// recover forwarding call scope from that exact call-site spelling.
#[cold]
fn resolve_static_call_class(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    class: &str,
    dynamic_scope: bool,
) -> Option<String> {
    let lexical_or_dynamic_scope = || {
        if dynamic_scope {
            let class_id = called_class_id_for_frame(eg, frame, 0);
            eg.class_by_id(class_id)
                .map(|definition| definition.name.clone())
        } else {
            get_caller_class(frame, eg)
        }
    };
    if class.eq_ignore_ascii_case("self") {
        lexical_or_dynamic_scope()
    } else if class.eq_ignore_ascii_case("parent") {
        lexical_or_dynamic_scope().and_then(|caller| {
            eg.class_table
                .get(caller.as_str())
                .and_then(|definition| definition.parent.clone())
        })
    } else if class.eq_ignore_ascii_case("static") {
        let class_id = late_static_call_class_id(eg, frame);
        eg.class_by_id(class_id)
            .map(|definition| definition.name.clone())
    } else {
        Some(class.to_string())
    }
}

/// Recover a called class for a participating call site. If the frame already
/// owns the sparse late-static sidecar required by a relative return contract,
/// reuse it; otherwise walk the saved call sites without publishing state that
/// an ordinary return path would have to clean up.
#[inline(always)]
fn late_static_call_class_id(eg: &ExecutorGlobals, frame: *mut ExecuteData) -> u32 {
    let embedded = frame_embedded_late_static_class_id(frame);
    if embedded != 0 {
        return embedded;
    }
    recover_late_static_call_class_id(eg, frame)
}

/// Resolve the late-called class of the user frame that invoked an internal
/// function. This reuses the canonical direct/forwarding/callback/generator
/// recovery path instead of giving individual builtins a second LSB model.
pub(crate) fn called_class_name_for_internal_call<'a>(
    eg: &'a ExecutorGlobals,
    internal_frame: *mut ExecuteData,
) -> Option<&'a str> {
    let caller = caller_frame_for_internal_call(internal_frame)?;
    let class_id = late_static_call_class_id(eg, caller);
    eg.class_by_id(class_id).map(|class| class.name.as_str())
}

/// Resolve the lexical class scope of the user frame that invoked an internal
/// function, including the consuming class of shared trait bytecode.
pub(crate) fn lexical_class_name_for_internal_call(
    eg: &ExecutorGlobals,
    internal_frame: *mut ExecuteData,
) -> Option<String> {
    get_caller_class(caller_frame_for_internal_call(internal_frame)?, eg)
}

fn caller_frame_for_internal_call(internal_frame: *mut ExecuteData) -> Option<*mut ExecuteData> {
    if internal_frame.is_null() {
        return None;
    }
    // SAFETY: an internal handler receives its own live frame; its saved caller
    // remains live for the duration of the handler invocation.
    let caller = unsafe { (*internal_frame).prev_execute_data };
    (!caller.is_null()).then_some(caller)
}

#[inline(always)]
fn frame_embedded_late_static_class_id(frame: *mut ExecuteData) -> u32 {
    // SAFETY: every caller has already established a live VM frame for the
    // current execution boundary; this only reads its compact scope field.
    unsafe { (*frame).embedded_late_static_class_id() }
}

#[inline]
fn initialize_bound_this_frame(
    frame: *mut ExecuteData,
    func_ptr: *const FunctionCommon,
    bound_this: Option<Value>,
) {
    let Some(bound_this) = bound_this else {
        return;
    };
    // SAFETY: func_ptr and frame come from the same live call-frame creation
    // boundary. User op-array CV metadata identifies an allocated frame slot,
    // and frame_slot_init publishes exactly one previously uninitialized value.
    unsafe {
        if (*func_ptr).fn_type != FunctionType::User {
            return;
        }
        let function = &*(func_ptr as *const UserFunction);
        if let Some((this_cv, _)) = function
            .op_array
            .all_cvs
            .iter()
            .find(|(_, name)| name == "this")
        {
            let destination = (*frame).cv_mut(*this_cv) as *mut Value;
            frame_slot_init(frame, destination, bound_this);
        }
    }
}

#[inline]
fn closure_bound_this(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    is_static: bool,
) -> Option<Value> {
    if is_static {
        return None;
    }
    // SAFETY: CreateClosure runs with a live frame whose function and CV
    // metadata match op_array. The selected slot is read-only and cloned
    // before the closure can outlive the frame.
    unsafe {
        let parent = &*(*frame).func;
        let this_cv = if parent.sig.this_offset == 1 {
            Some(0)
        } else {
            op_array
                .all_cvs
                .iter()
                .find(|(_, name)| name == "this")
                .map(|(index, _)| *index)
        };
        this_cv.and_then(|index| {
            let value = &*(*frame).get_op_ptr(index, OpType::Cv, op_array);
            (value.value_type() == ValueType::Object).then(|| value.clone())
        })
    }
}

#[inline]
fn write_array_union_result(
    frame: *mut ExecuteData,
    result: u16,
    left: &PhpArray,
    right: &PhpArray,
) {
    // SAFETY: Add's compiler-owned result is a fresh TMP slot in this live
    // frame; frame_tmp_set records the new array owner in the cleanup bitmap.
    unsafe {
        let result_ptr = (frame as *mut Value).add(CALL_FRAME_SLOTS + result as usize);
        frame_tmp_set(frame, result_ptr, Value::array(left.union(right)));
    }
}

#[inline]
fn write_fetch_dim_result(frame: *mut ExecuteData, result_ptr: *mut Value, value: Value) {
    // SAFETY: FetchDimR always publishes into its compiler-owned TMP result in
    // this live frame; frame_tmp_set handles first write and later overwrite.
    unsafe { frame_tmp_set(frame, result_ptr, value) }
}

/// Materialize PHP's object-to-array projection. Declared properties retain
/// their visibility-mangled keys, dynamic properties keep insertion order and
/// uninitialized typed slots remain absent from the result.
#[cold]
pub(crate) fn cast_object_to_array(value: &Value, eg: &ExecutorGlobals) -> Value {
    let proxy_instance = eg.lazy_proxy_instance(value);
    let value = proxy_instance.as_ref().unwrap_or(value);
    let object = value
        .as_object()
        .expect("object-to-array cast requires an object value");
    let mut result = PhpArray::new();

    if let Some(class) = eg.class_by_id(object.class_id) {
        for (slot, definition) in class.properties.iter().enumerate() {
            let Some(property) = object.get_property_slot(slot) else {
                continue;
            };
            if property.value_type() == ValueType::Undef {
                continue;
            }
            let key = match definition.visibility {
                Visibility::Public => definition.name.clone(),
                Visibility::Protected => format!("\0*\0{}", definition.name),
                Visibility::Private => {
                    format!("\0{}\0{}", definition.declaring_class, definition.name)
                }
            };
            result.set_str(&key, property.clone());
        }
        object.for_each_dynamic_property(|key, property| {
            result.set_str(key, property.clone());
        });
    } else {
        object.for_each_property(|key, property| {
            if property.value_type() != ValueType::Undef {
                result.set_str(key, property.clone());
            }
        });
    }

    Value::array(result)
}

#[cold]
#[inline(never)]
fn recover_late_static_call_class_id(eg: &ExecutorGlobals, frame: *mut ExecuteData) -> u32 {
    let cached = eg.late_static_scope_class_id(frame as usize);
    if cached != 0 {
        cached
    } else {
        called_class_id_for_frame(eg, frame, 0)
    }
}

/// Publish called-class state directly in compact participating frames. Only
/// wide frames fall back to the sparse cold sidecar.
#[inline(always)]
fn publish_late_static_call_class_id(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    class_id: u32,
) {
    if class_id != 0 && !unsafe { (*frame).try_set_embedded_late_static_class_id(class_id) } {
        eg.push_late_static_scope(frame as usize, class_id);
    }
}

#[cold]
fn resolve_static_method_owner(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    owner: &str,
) -> Option<String> {
    let (class, method) = owner.rsplit_once("::")?;
    // A pseudo owner reaches this path only when the compiler could not bind
    // it lexically (currently shared trait bytecode).
    resolve_static_call_class(eg, frame, class, true).map(|class| format!("{}::{}", class, method))
}

/// Check a value against a parameter type hint. Returns true if the value satisfies the hint.
/// Check a value against a type hint.
/// `callee_class`: the declaring class of the function whose hint is being checked.
/// Used to resolve `self`, `parent`, `static` pseudo-types.
/// Pass `None` for global functions.
pub(crate) fn check_type_hint(
    val: &Value,
    hint: &crate::vm::function::ParamTypeHint,
    eg: &ExecutorGlobals,
    strict: bool,
    callee_class: Option<&str>,
) -> bool {
    let val = val.dereferenced();
    check_type_hint_in_scopes(val, hint, eg, strict, callee_class, callee_class)
}

/// Check a type hint with distinct lexical and late-static class scopes.
fn check_type_hint_in_scopes(
    val: &Value,
    hint: &crate::vm::function::ParamTypeHint,
    eg: &ExecutorGlobals,
    strict: bool,
    callee_class: Option<&str>,
    called_class: Option<&str>,
) -> bool {
    use crate::vm::function::ParamTypeHint;
    match hint {
        ParamTypeHint::None => true,
        ParamTypeHint::Int => val.value_type() == ValueType::Long,
        ParamTypeHint::Float => {
            if strict {
                val.value_type() == ValueType::Double
            } else {
                matches!(val.value_type(), ValueType::Double | ValueType::Long)
            }
        }
        ParamTypeHint::String => val.value_type() == ValueType::String,
        ParamTypeHint::Bool => matches!(val.value_type(), ValueType::True | ValueType::False),
        ParamTypeHint::Array => val.value_type() == ValueType::Array,
        ParamTypeHint::Callable => {
            matches!(
                val.value_type(),
                ValueType::String | ValueType::Array | ValueType::Closure
            ) || val.as_object().is_some_and(|object| {
                eg.find_method_info(&object.class_name, "__invoke")
                    .is_some_and(|(visibility, is_static, _)| {
                        visibility == crate::parser::Visibility::Public && !is_static
                    })
            })
        }
        ParamTypeHint::ClassName(class_name) => {
            if class_name.eq_ignore_ascii_case("false") {
                return val.value_type() == ValueType::False;
            }
            if class_name.eq_ignore_ascii_case("true") {
                return val.value_type() == ValueType::True;
            }
            if val.value_type() == ValueType::Closure
                && (class_name.eq_ignore_ascii_case("Closure")
                    || class_name.eq_ignore_ascii_case("object"))
            {
                return true;
            }
            if class_name.eq_ignore_ascii_case("iterable") {
                return val.as_array().is_some()
                    || val
                        .as_object()
                        .is_some_and(|object| eg.class_is_a(&object.class_name, "Traversable"));
            }
            if let Some(obj) = val.as_object() {
                if class_name.eq_ignore_ascii_case("object") {
                    return true;
                }
                // `self`/`parent` are lexical; `static` is the runtime called class.
                let resolved = match class_name.as_str() {
                    "self" => callee_class.unwrap_or(class_name.as_str()),
                    "static" => called_class.unwrap_or(class_name.as_str()),
                    "parent" => {
                        if let Some(decl) = callee_class {
                            if let Some(class_def) = eg.class_table.get(decl) {
                                class_def.parent.as_deref().unwrap_or(class_name.as_str())
                            } else {
                                class_name.as_str()
                            }
                        } else {
                            class_name.as_str()
                        }
                    }
                    _ => class_name.as_str(),
                };
                eg.class_is_a(&obj.class_name, resolved)
            } else {
                false
            }
        }
        ParamTypeHint::Nullable(inner) => {
            if val.value_type() == ValueType::Null {
                true
            } else if matches!(inner.as_ref(), ParamTypeHint::None) {
                false
            } else {
                check_type_hint_in_scopes(val, inner, eg, strict, callee_class, called_class)
            }
        }
        ParamTypeHint::Void => false,
        ParamTypeHint::Mixed => true,
        ParamTypeHint::Never => false,
        ParamTypeHint::Union(types) => types
            .iter()
            .any(|t| check_type_hint_in_scopes(val, t, eg, strict, callee_class, called_class)),
        ParamTypeHint::Intersection(types) => types
            .iter()
            .all(|t| check_type_hint_in_scopes(val, t, eg, strict, callee_class, called_class)),
    }
}

pub(crate) enum CallArgumentPreparation {
    Exact,
    Coerced(Value),
    Invalid,
}

/// Apply the object-to-string argument conversion used by weak PHP call sites.
/// Exact union members win before conversion and strict callers never invoke
/// `__toString()` implicitly.
pub(crate) fn prepare_call_argument(
    value: &Value,
    hint: &ParamTypeHint,
    eg: &mut ExecutorGlobals,
    strict: bool,
    callee_class: Option<&str>,
) -> Result<CallArgumentPreparation, VmError> {
    // Test exact members first even for weak callers. In particular, an int
    // remains an int for `int|float`; widening is considered only when no
    // member already matches the runtime value.
    if check_type_hint(value, hint, eg, true, callee_class) {
        return Ok(CallArgumentPreparation::Exact);
    }
    if strict {
        return Ok(coerce_property_value(value, hint, false).map_or(
            CallArgumentPreparation::Invalid,
            CallArgumentPreparation::Coerced,
        ));
    }

    // Calls and typed-property writes share PHP's weak scalar conversion
    // table. Reuse the canonical conversion before the object-only string
    // hook below; exact union members have already won in check_type_hint().
    if let Some(coerced) = coerce_property_value(value, hint, true) {
        return Ok(CallArgumentPreparation::Coerced(coerced));
    }

    let coerced = match hint {
        ParamTypeHint::String if value.value_type() == ValueType::Object => {
            call_magic_method(eg, value, "__tostring", &[])?.and_then(|rendered| {
                (rendered.value_type() == ValueType::String)
                    .then(|| Value::string(rendered.as_str().unwrap()))
            })
        }
        ParamTypeHint::Nullable(inner)
            if value.value_type() != ValueType::Null
                && !matches!(inner.as_ref(), ParamTypeHint::None) =>
        {
            return prepare_call_argument(value, inner, eg, false, callee_class);
        }
        ParamTypeHint::Union(parts) => {
            for part in parts {
                match prepare_call_argument(value, part, eg, false, callee_class)? {
                    CallArgumentPreparation::Exact => {
                        return Ok(CallArgumentPreparation::Exact);
                    }
                    CallArgumentPreparation::Coerced(value) => {
                        return Ok(CallArgumentPreparation::Coerced(value));
                    }
                    CallArgumentPreparation::Invalid => {}
                }
            }
            None
        }
        _ => None,
    };
    Ok(coerced.map_or(
        CallArgumentPreparation::Invalid,
        CallArgumentPreparation::Coerced,
    ))
}

#[inline]
fn check_return_type_hint(
    value: &Value,
    hint: &crate::vm::function::ParamTypeHint,
    eg: &ExecutorGlobals,
    strict: bool,
    frame: *mut ExecuteData,
    callee_class: Option<&str>,
) -> bool {
    let lexical_scope = get_caller_class(frame, eg);
    let lexical_scope = lexical_scope.as_deref().or(callee_class);
    if !hint.uses_late_static() {
        return check_type_hint(value, hint, eg, strict, lexical_scope);
    }
    let common = unsafe { &*(*frame).func };
    let receiver_scope = if common.sig.this_offset == 1 {
        let receiver = unsafe { &*(*frame).cv(0) };
        (receiver.value_type() == ValueType::Object)
            .then(|| unsafe { receiver.object_class_name_unchecked() })
    } else {
        None
    };
    let called_scope = receiver_scope.or_else(|| {
        eg.class_by_id(late_static_call_class_id(eg, frame))
            .map(|class| class.name.as_str())
    });
    check_type_hint_in_scopes(value, hint, eg, strict, lexical_scope, called_scope)
}

/// Validate hints supported by the compact scalar call/return protocol.
/// `None` means the hint needs the canonical class/union/callable checker.
#[inline(always)]
pub(crate) fn check_fast_scalar_type_hint(
    value: &Value,
    hint: &ParamTypeHint,
    strict: bool,
) -> Option<bool> {
    Some(match hint {
        ParamTypeHint::None | ParamTypeHint::Mixed => true,
        ParamTypeHint::Int => value.value_type() == ValueType::Long,
        ParamTypeHint::Float => {
            value.value_type() == ValueType::Double
                || (!strict && value.value_type() == ValueType::Long)
        }
        ParamTypeHint::String => value.value_type() == ValueType::String,
        ParamTypeHint::Bool => {
            matches!(value.value_type(), ValueType::True | ValueType::False)
        }
        ParamTypeHint::Array => value.value_type() == ValueType::Array,
        _ => return None,
    })
}

/// Whether a compiler-proven representation makes a scalar return check
/// redundant. Unknown facts never bypass the canonical validator.
#[inline(always)]
pub(crate) fn known_scalar_satisfies_type_hint(
    known: KnownScalarType,
    hint: &ParamTypeHint,
    strict: bool,
) -> bool {
    match hint {
        ParamTypeHint::None | ParamTypeHint::Mixed => true,
        ParamTypeHint::Int => known == KnownScalarType::Long,
        ParamTypeHint::Float => {
            known == KnownScalarType::Double || (!strict && known == KnownScalarType::Long)
        }
        ParamTypeHint::String => known == KnownScalarType::String,
        ParamTypeHint::Bool => known == KnownScalarType::Bool,
        ParamTypeHint::Nullable(inner) => known_scalar_satisfies_type_hint(known, inner, strict),
        ParamTypeHint::Union(types) => types
            .iter()
            .any(|member| known_scalar_satisfies_type_hint(known, member, strict)),
        ParamTypeHint::Intersection(types) => types
            .iter()
            .all(|member| known_scalar_satisfies_type_hint(known, member, strict)),
        _ => false,
    }
}

#[inline(always)]
fn exact_method_return_matches(hint: &ParamTypeHint, expected: KnownScalarType) -> bool {
    matches!(
        (hint, expected),
        (ParamTypeHint::Int, KnownScalarType::Long)
            | (ParamTypeHint::String, KnownScalarType::String)
            | (ParamTypeHint::Bool, KnownScalarType::Bool)
    )
}

/// Validate the exact return contract attached to a statically typed method
/// call against the method selected by the receiver-class inline cache. This
/// is the single dispatch guard that licenses all downstream scalar rewrites.
#[inline(always)]
pub(crate) fn method_return_dispatch_contract_matches(
    initializer: &Instruction,
    common: &FunctionCommon,
) -> bool {
    let expected = initializer.method_return_guard_type();
    let return_contract_matches = expected == KnownScalarType::Unknown
        || (common.fn_type == FunctionType::User
            && exact_method_return_matches(&common.sig.return_type_hint, expected));
    let argument_contract_matches = !initializer.has_method_long_args_guard()
        || (common.fn_type == FunctionType::User
            && common.sig.ref_args == 0
            && common.sig.public_arity() == initializer.extended_value
            && !common.sig.param_type_hints.is_empty()
            && common
                .sig
                .param_type_hints
                .iter()
                .all(|hint| matches!(hint, ParamTypeHint::Int)));
    return_contract_matches && argument_contract_matches
}

/// Validate the already-bound public arguments for compact user-call ABIs.
/// A failed guard leaves the frame untouched so the canonical call path can
/// report or coerce the value according to normal PHP rules.
#[inline(always)]
pub(crate) unsafe fn compact_scalar_call_types_match(
    eg: &ExecutorGlobals,
    call: *mut ExecuteData,
    common: &FunctionCommon,
    strict: bool,
) -> bool {
    let hints = &common.sig.param_type_hints;
    let check_count = std::cmp::min((*call).num_args as usize, hints.len());
    let mut class_guard = 0u64;
    let mut class_count = 0usize;
    let mut class_guard_cacheable = true;
    for (index, hint) in hints.iter().take(check_count).enumerate() {
        if !matches!(hint, ParamTypeHint::ClassName(_)) {
            continue;
        }
        if class_count == 2 {
            class_guard_cacheable = false;
            break;
        }
        let value = &*(*call).cv(common.sig.param_cv_index(index as u32));
        if value.value_type() != ValueType::Object {
            class_guard_cacheable = false;
            break;
        }
        let class_id = value.object_class_id_unchecked();
        if class_id == 0 {
            class_guard_cacheable = false;
            break;
        }
        class_guard |= (class_id as u64) << (class_count * 32);
        class_count += 1;
    }
    class_guard_cacheable &= class_count != 0;
    debug_assert!(common.fn_type == FunctionType::User);
    let user = &*(common as *const FunctionCommon as *const UserFunction);
    let class_guard_matches =
        class_guard_cacheable && user.compact_class_guard.get() == class_guard;

    for (index, hint) in hints.iter().take(check_count).enumerate() {
        if matches!(hint, ParamTypeHint::None | ParamTypeHint::Mixed) {
            continue;
        }
        let value = &*(*call).cv(common.sig.param_cv_index(index as u32));
        let matches = match check_fast_scalar_type_hint(value, hint, strict) {
            Some(matches) => matches,
            None if matches!(hint, ParamTypeHint::ClassName(_)) => {
                class_guard_matches
                    || check_type_hint(
                        value,
                        hint,
                        eg,
                        strict,
                        eg.declaring_class_of(common as *const FunctionCommon),
                    )
            }
            None => false,
        };
        if !matches {
            return false;
        }
    }
    if class_guard_cacheable && !class_guard_matches {
        user.compact_class_guard.set(class_guard);
    }
    true
}

/// VM error — replaces panic! in all runtime paths
#[derive(Debug)]
pub enum VmError {
    Fatal(String),
    /// An uncaught object whose exact runtime class is ParseError. PHP renders
    /// this diagnostic without the ordinary "Uncaught" throwable envelope.
    Parse(String),
    UnimplementedOpcode(OpCode),
    /// `exit($code)` / `die($msg)` — clean script termination.
    Exit(i32),
}

impl std::fmt::Display for VmError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fatal(message) | Self::Parse(message) => formatter.write_str(message),
            Self::UnimplementedOpcode(opcode) => {
                write!(formatter, "Unimplemented opcode {opcode:?}")
            }
            Self::Exit(code) => write!(formatter, "Script exited with status {code}"),
        }
    }
}

impl std::error::Error for VmError {}

#[cfg(test)]
mod vm_error_display_tests {
    use super::VmError;

    #[test]
    fn fatal_display_does_not_leak_the_rust_enum_wrapper() {
        let error =
            VmError::Fatal("Uncaught Error: Call to undefined function missing_function()".into());

        assert_eq!(
            error.to_string(),
            "Uncaught Error: Call to undefined function missing_function()"
        );
        assert!(!error.to_string().contains("Fatal(\""));
    }
}

include!("execute/frame_runtime.rs");
include!("execute/scalar_calls.rs");
include!("execute/object_calls.rs");
include!("execute/composed_calls.rs");
include!("execute/call_frames.rs");
include!("execute/baseline_entry.rs");
include!("execute/callback_array_pipeline.rs");
include!("execute/baseline_control_ops.rs");
include!("execute/baseline_object_calls.rs");
include!("execute/baseline_iteration.rs");
include!("execute/baseline_named_args.rs");
include!("execute/baseline_object_values.rs");
include!("execute/baseline_concat.rs");

#[cfg(feature = "quick-loops")]
pub(super) enum QuickLoopOutcome {
    Completed,
    Deoptimized,
    GuardFailed,
}

#[inline(always)]
#[cfg(all(
    feature = "quick-loops",
    feature = "vm-stats",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
fn record_native_quick_outcome(kind: stats::JitRegionKind, outcome: &QuickLoopOutcome) {
    stats::inc_jit_native_execution(kind);
    if matches!(outcome, QuickLoopOutcome::Deoptimized) {
        stats::inc_jit_native_side_exit(kind);
    }
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
pub(super) unsafe fn quick_loop_slot_has_heap(frame: *mut ExecuteData, slot: u16) -> bool {
    (*frame).heap_bitmap & (1u64 << slot) != 0
}

include!("execute/quick_induction_runtime.rs");
include!("execute/quick_scalar_runtime.rs");
include!("execute/quick_double_runtime.rs");
include!("execute/quick_json_runtime.rs");

include!("execute/quick_object_resolution.rs");

include!("execute/quick_native_accumulate.rs");
include!("execute/quick_accumulate_runtime.rs");

#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn commit_quick_long_ops_slots(
    slot_base: *mut Value,
    slots: &[i64; 64],
    mut dirty_long_mask: u64,
    mut dirty_bool_mask: u64,
) {
    while dirty_long_mask != 0 {
        let slot = dirty_long_mask.trailing_zeros() as usize;
        dirty_long_mask &= dirty_long_mask - 1;
        Value::write_long(slot_base.add(slot), slots[slot]);
    }
    while dirty_bool_mask != 0 {
        let slot = dirty_bool_mask.trailing_zeros() as usize;
        dirty_bool_mask &= dirty_bool_mask - 1;
        Value::write_bool(slot_base.add(slot), slots[slot] != 0);
    }
}

include!("execute/quick_array_state.rs");
include!("execute/quick_string_state.rs");
include!("execute/quick_virtual_pipeline.rs");

include!("execute/quick_kernel_model.rs");
include!("execute/quick_array_access.rs");
include!("execute/quick_kernel_plan.rs");
include!("execute/quick_kernel_common.rs");
include!("execute/quick_string_runtime.rs");
include!("execute/quick_array_push_runtime.rs");
include!("execute/quick_array_runtime.rs");
include!("execute/quick_conditional_runtime.rs");
include!("execute/quick_invariant_property_runtime.rs");

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
include!("execute/native_mixed_core.rs");
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
include!("execute/native_mixed_virtual.rs");
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
include!("execute/native_mixed_scalar.rs");
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
include!("execute/native_mixed_property.rs");
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
include!("execute/native_mixed_typed.rs");

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
include!("execute/native_conditional_add.rs");

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
include!("execute/native_mixed_kernel.rs");

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    target_arch = "x86_64",
    target_os = "linux"
))]
const NATIVE_LONG_SAFEPOINT_INTERVAL: u64 = 1024;

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
fn native_quick_long_straight_kernel(
    plan: &QuickLongOpsLoop,
) -> Option<NativeQuickLongStraightKernel> {
    if plan.entry_op != 0
        || plan.ops.len() < 3
        || plan.ops.len() > NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 2
    {
        return None;
    }

    let (header_lhs, header_rhs, header_condition_tmp, header_false_target, header_next_target) =
        match *plan.ops.first()? {
            QuickLongOp::BranchUnlessLt {
                lhs,
                rhs,
                condition_tmp,
                false_target,
                next_target,
                ..
            } => (lhs, rhs, condition_tmp, false_target, next_target),
            _ => return None,
        };
    header_false_target.exit_ip()?;

    let (
        post_value,
        post_result,
        post_condition_lhs,
        post_condition_rhs,
        post_condition_tmp,
        body_target,
        exit_target,
        post_resume_ip,
    ) = match *plan.ops.last()? {
        QuickLongOp::PostIncLoopLt {
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        } => (
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        ),
        _ => return None,
    };
    let body_end = plan.ops.len() - 1;
    if header_lhs != post_value
        || header_next_target.op_index() != Some(1)
        || body_target.op_index() != Some(1)
        || exit_target != header_false_target
        || post_condition_lhs != header_lhs
        || post_condition_rhs != header_rhs
        || post_condition_tmp != header_condition_tmp
        || post_result == Some(post_value)
    {
        return None;
    }

    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut operation_resume_ips = [0usize; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut trace_guard_operation_indices = [0u8; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut trace_guard_condition_slots = [0u8; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut trace_guard_expected = [false; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut trace_guard_count = 0usize;
    let operation_count = std::cell::Cell::new(0usize);
    let mut has_materialized_arithmetic = false;
    let mut plan_to_native = vec![u8::MAX; plan.ops.len()];
    let mut pending_branches = Vec::new();
    let mut pending_jumps = Vec::new();
    let mut append_operation =
        |operation: NativeStraightLongOperation, resume_ip: usize| -> Option<u8> {
            let index = operation_count.get();
            if index == NATIVE_STRAIGHT_LONG_MAX_OPERATIONS {
                return None;
            }
            operations[index] = operation;
            operation_resume_ips[index] = resume_ip;
            operation_count.set(index + 1);
            Some(index as u8)
        };
    let mut plan_index = 1usize;
    while plan_index < body_end {
        plan_to_native[plan_index] = u8::try_from(operation_count.get()).ok()?;
        let operation = plan.ops[plan_index];
        let next_target = match operation {
            QuickLongOp::BranchUnlessLt {
                lhs,
                rhs,
                false_target,
                next_target,
                resume_ip,
                ..
            } => {
                let target = false_target.op_index()?;
                let native_index = append_operation(
                    NativeStraightLongOperation::BranchUnless {
                        kind: ScalarLongConditionKind::LessThan,
                        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(
                            lhs,
                        )),
                        rhs: NativeStraightLongConditionOperand::Source(rhs),
                        false_target: 0,
                    },
                    resume_ip,
                )?;
                pending_branches.push((native_index, target));
                next_target
            }
            QuickLongOp::BranchUnlessEq {
                lhs,
                rhs,
                false_target,
                next_target,
                resume_ip,
                ..
            } => {
                let target = false_target.op_index()?;
                let native_index = append_operation(
                    NativeStraightLongOperation::BranchUnless {
                        kind: ScalarLongConditionKind::Equal,
                        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(
                            lhs,
                        )),
                        rhs: NativeStraightLongConditionOperand::Source(rhs),
                        false_target: 0,
                    },
                    resume_ip,
                )?;
                pending_branches.push((native_index, target));
                next_target
            }
            QuickLongOp::BranchUnlessLe {
                lhs,
                rhs,
                false_target,
                next_target,
                resume_ip,
                ..
            } => {
                let target = false_target.op_index()?;
                let native_index = append_operation(
                    NativeStraightLongOperation::BranchUnless {
                        kind: ScalarLongConditionKind::LessThanOrEqual,
                        lhs: NativeStraightLongConditionOperand::Source(lhs),
                        rhs: NativeStraightLongConditionOperand::Source(rhs),
                        false_target: 0,
                    },
                    resume_ip,
                )?;
                pending_branches.push((native_index, target));
                next_target
            }
            QuickLongOp::ConditionalAddAssign {
                condition,
                lhs,
                rhs,
                result,
                destination,
                next_target,
                condition_resume_ip,
                add_resume_ip,
                ..
            } => {
                let [branch, add] = native_conditional_add_operations(
                    condition,
                    lhs,
                    rhs,
                    result,
                    destination,
                    next_target,
                    plan_index + 1,
                    post_value,
                )?;
                let branch_index = append_operation(branch, condition_resume_ip)?;
                // The fused quick operation represents a forward branch over
                // the add. Resolve its false edge to the next quick operation
                // after both native operations have been appended.
                pending_branches.push((branch_index, plan_index + 1));
                append_operation(add, add_resume_ip)?;
                has_materialized_arithmetic = true;
                next_target
            }
            QuickLongOp::Jump { target } => {
                let native_index = append_operation(
                    NativeStraightLongOperation::Jump { target: 0 },
                    plan.target_ip(target)?,
                )?;
                pending_jumps.push((native_index, target.op_index()?));
                plan_index += 1;
                continue;
            }
            QuickLongOp::JsonProjectionStep { next_target, .. } => next_target,
            QuickLongOp::ModConst {
                value,
                divisor,
                result,
                next_target,
                resume_ip,
            } if result != post_value => {
                append_operation(
                    NativeStraightLongOperation::Modulo {
                        value: QuickLongOperand::Slot(value),
                        divisor,
                        result,
                    },
                    resume_ip,
                )?;
                next_target
            }
            QuickLongOp::Binary {
                kind,
                lhs,
                rhs,
                result,
                next_target,
                resume_ip,
            } if matches!(
                kind,
                ScalarLongOpKind::Add | ScalarLongOpKind::Subtract | ScalarLongOpKind::Multiply
            ) && result != post_value =>
            {
                append_operation(
                    NativeStraightLongOperation::Binary {
                        kind,
                        lhs,
                        rhs,
                        result,
                    },
                    resume_ip,
                )?;
                next_target
            }
            QuickLongOp::BinaryAssign {
                kind,
                lhs,
                rhs,
                result,
                destination,
                next_target,
                resume_ip,
            } if matches!(
                kind,
                ScalarLongOpKind::Add | ScalarLongOpKind::Subtract | ScalarLongOpKind::Multiply
            ) && result != post_value
                && destination != post_value =>
            {
                append_operation(
                    NativeStraightLongOperation::BinaryAssign {
                        kind,
                        lhs,
                        rhs,
                        result,
                        destination,
                    },
                    resume_ip,
                )?;
                has_materialized_arithmetic = true;
                next_target
            }
            QuickLongOp::Add {
                lhs,
                rhs,
                result,
                next_target,
                resume_ip,
            } if result != post_value => {
                append_operation(
                    NativeStraightLongOperation::Binary {
                        kind: ScalarLongOpKind::Add,
                        lhs: QuickLongOperand::Slot(lhs),
                        rhs: QuickLongOperand::Slot(rhs),
                        result,
                    },
                    resume_ip,
                )?;
                next_target
            }
            QuickLongOp::AddAssign {
                lhs,
                rhs,
                result,
                destination,
                next_target,
                add_resume_ip,
            } if result != post_value && destination != post_value => {
                append_operation(
                    NativeStraightLongOperation::BinaryAssign {
                        kind: ScalarLongOpKind::Add,
                        lhs: QuickLongOperand::Slot(lhs),
                        rhs: QuickLongOperand::Slot(rhs),
                        result,
                        destination,
                    },
                    add_resume_ip,
                )?;
                has_materialized_arithmetic = true;
                next_target
            }
            QuickLongOp::AddAddAssign {
                first_lhs,
                first_rhs,
                first_result,
                second_lhs,
                second_rhs,
                second_result,
                destination,
                next_target,
                first_resume_ip,
                second_resume_ip,
            } if first_result != post_value
                && second_result != post_value
                && destination != post_value =>
            {
                append_operation(
                    NativeStraightLongOperation::Binary {
                        kind: ScalarLongOpKind::Add,
                        lhs: QuickLongOperand::Slot(first_lhs),
                        rhs: QuickLongOperand::Slot(first_rhs),
                        result: first_result,
                    },
                    first_resume_ip,
                )?;
                append_operation(
                    NativeStraightLongOperation::BinaryAssign {
                        kind: ScalarLongOpKind::Add,
                        lhs: QuickLongOperand::Slot(second_lhs),
                        rhs: QuickLongOperand::Slot(second_rhs),
                        result: second_result,
                        destination,
                    },
                    second_resume_ip,
                )?;
                has_materialized_arithmetic = true;
                next_target
            }
            QuickLongOp::TraceGuard {
                kind,
                lhs,
                rhs,
                expected,
                condition_tmp: Some(condition_tmp),
                next_target,
                resume_ip,
            } => {
                let operation_index = append_operation(
                    NativeStraightLongOperation::Guard {
                        kind,
                        lhs: NativeStraightLongConditionOperand::Source(lhs),
                        rhs: NativeStraightLongConditionOperand::Source(rhs),
                        expected,
                    },
                    resume_ip,
                )?;
                trace_guard_operation_indices[trace_guard_count] = operation_index;
                trace_guard_condition_slots[trace_guard_count] =
                    u8::try_from(condition_tmp).ok()?;
                trace_guard_expected[trace_guard_count] = expected;
                trace_guard_count += 1;
                next_target
            }
            _ => return None,
        };
        if next_target.op_index() != Some(plan_index + 1) {
            return None;
        }
        plan_index += 1;
    }
    plan_to_native[body_end] = u8::try_from(operation_count.get()).ok()?;
    for (native_index, target_plan) in pending_branches {
        let false_target = *plan_to_native.get(target_plan)?;
        if false_target == u8::MAX {
            return None;
        }
        let NativeStraightLongOperation::BranchUnless { kind, lhs, rhs, .. } =
            operations[native_index as usize]
        else {
            return None;
        };
        operations[native_index as usize] = NativeStraightLongOperation::BranchUnless {
            kind,
            lhs,
            rhs,
            false_target,
        };
    }
    for (native_index, target_plan) in pending_jumps {
        let target = *plan_to_native.get(target_plan)?;
        if target == u8::MAX {
            return None;
        }
        operations[native_index as usize] = NativeStraightLongOperation::Jump { target };
    }
    if !has_materialized_arithmetic {
        return None;
    }

    let operation_count = operation_count.get() as u8;
    let config = NativeStraightLongLoopConfig {
        induction_slot: post_value,
        bound: header_rhs,
        operations,
        operation_count,
        post_result,
    };
    let mut mutable_mask = config.body_output_mask() | (1u64 << post_value);
    if let Some(slot) = post_result {
        mutable_mask |= 1u64 << slot;
    }
    if matches!(header_rhs, QuickLongOperand::Slot(slot) if mutable_mask & (1u64 << slot) != 0) {
        return None;
    }

    let mut mutable_slots = [0u8; NATIVE_QUICK_LONG_SLOT_CAPACITY];
    let mut mutable_slot_count = 0usize;
    while mutable_mask != 0 {
        if mutable_slot_count == mutable_slots.len() {
            return None;
        }
        let slot = mutable_mask.trailing_zeros() as u8;
        mutable_mask &= mutable_mask - 1;
        mutable_slots[mutable_slot_count] = slot;
        mutable_slot_count += 1;
    }

    Some(NativeQuickLongStraightKernel {
        config,
        header_condition_tmp,
        body_target,
        exit_target,
        post_resume_ip,
        operation_resume_ips,
        trace_guard_operation_indices,
        trace_guard_condition_slots,
        trace_guard_expected,
        trace_guard_count: trace_guard_count as u8,
        mutable_slots,
        mutable_slot_count: mutable_slot_count as u8,
    })
}

#[inline(always)]
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
fn publish_native_quick_long_trace_guards(
    kernel: &NativeQuickLongStraightKernel,
    slots: &mut [i64; 64],
    dirty_bool_mask: &mut u64,
    before_operation: Option<u8>,
) {
    for index in 0..kernel.trace_guard_count as usize {
        if before_operation
            .is_some_and(|limit| kernel.trace_guard_operation_indices[index] >= limit)
        {
            continue;
        }
        let slot = kernel.trace_guard_condition_slots[index] as usize;
        slots[slot] = i64::from(kernel.trace_guard_expected[index]);
        *dirty_bool_mask |= 1u64 << slot;
    }
}

#[inline(never)]
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
unsafe fn run_native_quick_long_straight_kernel(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    slots: &mut [i64; 64],
    kernel: &NativeQuickLongStraightKernel,
) -> Result<Option<QuickLoopOutcome>, VmError> {
    let config = &kernel.config;
    let bound = quick_long_operand(slots, config.bound);
    let cache = plan.native_jit();
    let remaining_range_proof = cache.prove_straight_remaining_range(config, slots);
    let remaining_range_proven = remaining_range_proof.is_some();
    let cv_mask = if op_array.num_cvs == 64 {
        u64::MAX
    } else {
        (1u64 << op_array.num_cvs) - 1
    };
    let publication_mask = config.body_output_mask() & cv_mask;
    let program = if let Some(range_proof) = remaining_range_proof {
        cache.prepare_range_proven_straight_program(
            config,
            NATIVE_LONG_SAFEPOINT_INTERVAL as u16,
            publication_mask,
            range_proof.carried_mask,
        )
    } else {
        cache.prepare_straight_program(config)
    };
    let Some(program) = program else {
        return Ok(None);
    };
    let interrupt_flag = eg.vm_interrupt.as_ptr() as *const bool;
    let body_output_mask = if remaining_range_proven {
        publication_mask
    } else {
        config.body_output_mask()
    };
    let post_result_mask = config.post_result.map_or(0, |slot| 1u64 << slot);
    let mut iterations = 0u64;
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut entered_native = false;

    loop {
        let before_induction = slots[config.induction_slot as usize];
        let mut before_values = [0i64; NATIVE_QUICK_LONG_SLOT_CAPACITY];
        for index in 0..kernel.mutable_slot_count as usize {
            before_values[index] = slots[kernel.mutable_slots[index] as usize];
        }

        let native_result = if remaining_range_proven {
            let Some(result) = cache.dispatch_prepared_proven_straight_remaining(
                program,
                config,
                slots,
                interrupt_flag,
                NATIVE_LONG_SAFEPOINT_INTERVAL as u16,
            ) else {
                return Ok(None);
            };
            result
        } else {
            cache.dispatch_prepared_straight_chunk(program, slots, NATIVE_LONG_SAFEPOINT_INTERVAL)
        };
        let mut result = match native_result {
            Ok(result) => {
                if !entered_native {
                    cache.record_region_entry();
                    entered_native = true;
                }
                result
            }
            Err(_) => {
                for index in 0..kernel.mutable_slot_count as usize {
                    slots[kernel.mutable_slots[index] as usize] = before_values[index];
                }
                if iterations != 0 {
                    publish_native_quick_long_trace_guards(
                        kernel,
                        slots,
                        &mut dirty_bool_mask,
                        None,
                    );
                }
                if let Some(slot) = kernel.header_condition_tmp {
                    slots[slot as usize] = 1;
                    dirty_bool_mask |= 1u64 << slot;
                }
                commit_quick_long_ops_slots(slot_base, slots, dirty_long_mask, dirty_bool_mask);
                let next_ip = plan.target_ip(kernel.body_target).unwrap_unchecked();
                (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
        };

        let induction = slots[config.induction_slot as usize];
        let completed_in_chunk = (induction as u64).wrapping_sub(before_induction as u64);
        iterations = iterations.saturating_add(completed_in_chunk);
        if completed_in_chunk != 0 {
            dirty_long_mask |=
                (1u64 << config.induction_slot) | body_output_mask | post_result_mask;
        }

        if result.outcome == NativeStraightLongLoopOutcome::ChunkExhausted && induction >= bound {
            result.outcome = NativeStraightLongLoopOutcome::Completed;
        }
        let completed = result.outcome == NativeStraightLongLoopOutcome::Completed;
        if let Some(slot) = kernel.header_condition_tmp {
            slots[slot as usize] = i64::from(!completed);
            dirty_bool_mask |= 1u64 << slot;
        }

        match result.outcome {
            NativeStraightLongLoopOutcome::Completed => {
                if iterations != 0 {
                    publish_native_quick_long_trace_guards(
                        kernel,
                        slots,
                        &mut dirty_bool_mask,
                        None,
                    );
                }
                commit_quick_long_ops_slots(slot_base, slots, dirty_long_mask, dirty_bool_mask);
                let next_ip = kernel.exit_target.exit_ip().unwrap_unchecked();
                (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
                stats::inc_quick_loop_completed(iterations);
                return Ok(Some(QuickLoopOutcome::Completed));
            }
            NativeStraightLongLoopOutcome::ChunkExhausted => {
                debug_assert_ne!(completed_in_chunk, 0);
                debug_assert_eq!(completed_in_chunk % NATIVE_LONG_SAFEPOINT_INTERVAL, 0);
                if eg.vm_interrupt.load(Ordering::Relaxed) {
                    publish_native_quick_long_trace_guards(
                        kernel,
                        slots,
                        &mut dirty_bool_mask,
                        None,
                    );
                    commit_quick_long_ops_slots(slot_base, slots, dirty_long_mask, dirty_bool_mask);
                    let next_ip = plan.target_ip(kernel.body_target).unwrap_unchecked();
                    (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
                    handle_interrupt(eg)?;
                }
            }
            NativeStraightLongLoopOutcome::OperationSideExit => {
                let failed_operation = result
                    .failed_operation
                    .expect("operation side exit carries its operation index");
                dirty_long_mask |= config.output_mask_before(failed_operation) & body_output_mask;
                if iterations != 0 {
                    publish_native_quick_long_trace_guards(
                        kernel,
                        slots,
                        &mut dirty_bool_mask,
                        None,
                    );
                } else {
                    publish_native_quick_long_trace_guards(
                        kernel,
                        slots,
                        &mut dirty_bool_mask,
                        Some(failed_operation),
                    );
                }
                commit_quick_long_ops_slots(slot_base, slots, dirty_long_mask, dirty_bool_mask);
                (*frame).opline = op_array
                    .instructions
                    .as_ptr()
                    .add(kernel.operation_resume_ips[failed_operation as usize]);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
            NativeStraightLongLoopOutcome::IncrementOverflow => {
                dirty_long_mask |= body_output_mask;
                publish_native_quick_long_trace_guards(kernel, slots, &mut dirty_bool_mask, None);
                commit_quick_long_ops_slots(slot_base, slots, dirty_long_mask, dirty_bool_mask);
                (*frame).opline = op_array.instructions.as_ptr().add(kernel.post_resume_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
        }
    }
}

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
include!("execute/native_mixed_runtime.rs");

// Apple builds isolate the large hot dispatcher as a child module to keep its
// code generation stable when unrelated cold handlers are added. Linux keeps
// the original textual include because that is its admitted x86 layout.
#[cfg(all(feature = "quick-loops", target_vendor = "apple"))]
#[path = "execute/quick_dispatch.rs"]
mod quick_dispatch;

#[cfg(all(feature = "quick-loops", target_vendor = "apple"))]
use quick_dispatch::run_quick_long_ops_loop_entry as run_quick_long_ops_loop;

#[cfg(all(feature = "quick-loops", not(target_vendor = "apple")))]
include!("execute/quick_dispatch.rs");

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn execute_quick_region_entry(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<bool, VmError> {
    let block_idx = opline.extended_value as usize - 1;
    let Some(super::planner::BlockPlan::QuickLongOps(plan)) = op_array.block_plans.get(block_idx)
    else {
        return Ok(false);
    };
    if plan.header_ip
        != (opline as *const Instruction).offset_from(op_array.instructions().as_ptr()) as usize
    {
        return Ok(false);
    }

    let hot_counter = &op_array.block_counters[block_idx];
    let count = hot_counter.get();
    if count == QUICK_LOOP_DISABLED {
        return Ok(false);
    }
    let hot_progress = count % QUICK_LOOP_COUNTER_STRIDE;
    if hot_progress < QUICK_LOOP_HOT_THRESHOLD {
        hot_counter.set(count + 1);
        return Ok(false);
    }

    #[cfg(feature = "vm-stats")]
    stats::inc_jit_region_execution(stats::JitRegionKind::StraightArrayRegion);
    match run_quick_long_ops_loop(eg, frame, op_array, plan)? {
        QuickLoopOutcome::Completed => {
            hot_counter.set(QUICK_LOOP_HOT_THRESHOLD);
            Ok(true)
        }
        QuickLoopOutcome::Deoptimized => {
            let failures = count / QUICK_LOOP_COUNTER_STRIDE + 1;
            hot_counter.set(if failures >= QUICK_LOOP_FAILURE_LIMIT {
                QUICK_LOOP_DISABLED
            } else {
                failures * QUICK_LOOP_COUNTER_STRIDE
            });
            Ok(true)
        }
        QuickLoopOutcome::GuardFailed => {
            let failures = count / QUICK_LOOP_COUNTER_STRIDE + 1;
            hot_counter.set(if failures >= QUICK_LOOP_FAILURE_LIMIT {
                QUICK_LOOP_DISABLED
            } else {
                failures * QUICK_LOOP_COUNTER_STRIDE
            });
            Ok(false)
        }
    }
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn execute_quick_loop_backedge(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    let target = opline.op1 as usize;
    let block_idx = opline.extended_value as usize - 1;

    if let Some(plan) = op_array.block_plans.get(block_idx) {
        let hot_counter = &op_array.block_counters[block_idx];
        let count = hot_counter.get();
        if count == QUICK_LOOP_DISABLED {
            (*frame).opline = op_array.instructions().as_ptr().add(target);
            return Ok(());
        }
        let hot_progress = count % QUICK_LOOP_COUNTER_STRIDE;
        if hot_progress >= QUICK_LOOP_HOT_THRESHOLD {
            let outcome = match plan {
                super::planner::BlockPlan::QuickLongInduction(plan) => {
                    #[cfg(feature = "vm-stats")]
                    stats::inc_jit_region_execution(stats::JitRegionKind::LongInduction);
                    run_quick_long_induction_loop(eg, frame, op_array, *plan)?
                }
                super::planner::BlockPlan::QuickLongAccumulate(plan) => {
                    #[cfg(feature = "vm-stats")]
                    stats::inc_jit_region_execution(stats::JitRegionKind::LongAccumulate);
                    run_quick_long_accumulate_loop(eg, frame, op_array, plan)?
                }
                super::planner::BlockPlan::QuickDoubleCallAccumulate(plan) => {
                    #[cfg(feature = "vm-stats")]
                    stats::inc_jit_region_execution(stats::JitRegionKind::DoubleCallAccumulate);
                    run_quick_double_call_accumulate_loop(eg, frame, op_array, plan)?
                }
                super::planner::BlockPlan::QuickForeachLongAccumulate(plan) => {
                    #[cfg(feature = "vm-stats")]
                    stats::inc_jit_region_execution(stats::JitRegionKind::ForeachLongAccumulate);
                    super::quick_foreach::run_quick_foreach_long_accumulate_loop(
                        eg, frame, op_array, *plan,
                    )?
                }
                super::planner::BlockPlan::QuickForeachObjectPropertyAccumulate(plan) => {
                    #[cfg(feature = "vm-stats")]
                    stats::inc_jit_region_execution(
                        stats::JitRegionKind::ForeachObjectPropertyAccumulate,
                    );
                    super::quick_foreach::run_quick_foreach_object_property_accumulate_loop(
                        eg, frame, op_array, *plan,
                    )?
                }
                super::planner::BlockPlan::QuickLongOps(plan) => {
                    #[cfg(feature = "vm-stats")]
                    stats::inc_jit_region_execution(stats::JitRegionKind::TypedOpsLoop);
                    run_quick_long_ops_loop(eg, frame, op_array, plan)?
                }
                _ => {
                    (*frame).opline = op_array.instructions().as_ptr().add(target);
                    return Ok(());
                }
            };
            match outcome {
                QuickLoopOutcome::Completed => {
                    hot_counter.set(QUICK_LOOP_HOT_THRESHOLD);
                    return Ok(());
                }
                QuickLoopOutcome::Deoptimized => {
                    let failures = count / QUICK_LOOP_COUNTER_STRIDE + 1;
                    hot_counter.set(if failures >= QUICK_LOOP_FAILURE_LIMIT {
                        QUICK_LOOP_DISABLED
                    } else {
                        failures * QUICK_LOOP_COUNTER_STRIDE
                    });
                    return Ok(());
                }
                QuickLoopOutcome::GuardFailed => {
                    let failures = count / QUICK_LOOP_COUNTER_STRIDE + 1;
                    hot_counter.set(if failures >= QUICK_LOOP_FAILURE_LIMIT {
                        QUICK_LOOP_DISABLED
                    } else {
                        failures * QUICK_LOOP_COUNTER_STRIDE
                    });
                }
            }
        } else {
            hot_counter.set(count + 1);
        }
    }

    (*frame).opline = op_array.instructions().as_ptr().add(target);
    Ok(())
}

// Fuse the cache-hit method protocol while its transition cost is material
// relative to the FastScalar body; longer methods keep the normal DoFcall path.
const FAST_SCALAR_METHOD_FUSION_MAX_OPS: usize = 16;

/// Enter a fixed-signature user method after InitMethodCall already created
/// its frame and bound every scalar argument. Inlining lets the compiler merge
/// the adjacent DoFcall setup into the cache-hit InitMethodCall path.
#[inline(always)]
fn execute_fast_scalar_method_call<'a>(
    eg: &mut ExecutorGlobals,
    caller: *mut ExecuteData,
    call: *mut ExecuteData,
    func_ptr: *const FunctionCommon,
    do_fcall: &Instruction,
    do_fcall_ptr: *const Instruction,
) -> Result<ColdResult<'a>, VmError> {
    unsafe { (*caller).call = (*call).call };
    stats::inc_do_fcall_fast();

    let func_common = unsafe { &*func_ptr };
    let cc = func_common.call_count.get();
    if cc < u32::MAX {
        func_common.call_count.set(cc + 1);
    }
    if cc == FUNC_HOT_THRESHOLD
        && func_common.hot_status.get() == HotStatus::Cold
        && func_common.can_promote_to_hot()
    {
        func_common.hot_status.set(HotStatus::Hot);
    }

    let return_value_ptr = match do_fcall.result_type {
        OpType::Tmp | OpType::Var => unsafe {
            (caller as *mut Value).add(CALL_FRAME_SLOTS + do_fcall.result as usize)
        },
        OpType::Unused => std::ptr::null_mut(),
        _ => unsafe { (*caller).get_op_mut(do_fcall.result as u32, do_fcall.result_type) },
    };
    let user = unsafe { &*(func_ptr as *const UserFunction) };

    unsafe {
        (*call).return_value = return_value_ptr;
        (*call).opline = user.op_array.instructions.as_ptr();
        (*caller).opline = do_fcall_ptr.add(1);
    }
    eg.current_execute_data.set(call);

    if func_common.hot_status.get() == HotStatus::Hot {
        match super::hot::execute_hot_frame(eg, call)? {
            super::hot::HotResult::Completed => Ok(ColdResult::Continue),
            super::hot::HotResult::Bailout => {
                match super::hot::resume_after_long_comparison(eg, call)? {
                    super::hot::HotResult::Completed => Ok(ColdResult::Continue),
                    super::hot::HotResult::Bailout => {
                        // Promotion happens only after caches are warm. If both
                        // the hot executor and its comparison resume reject this
                        // frame, keep later calls on the canonical baseline path
                        // instead of paying the same failed tier entry forever.
                        func_common.hot_status.set(HotStatus::Cold);
                        let active = eg.current_execute_data.get();
                        Ok(ColdResult::NewFrame(active, unsafe {
                            (*active).op_array()
                        }))
                    }
                }
            }
        }
    } else {
        Ok(ColdResult::NewFrame(call, unsafe { (*call).op_array() }))
    }
}

/// Find the call initializer paired with one DoFcall while ignoring complete
/// nested calls used to build its arguments.
#[cold]
fn call_initializer_before<'a>(
    op_array: &'a crate::compiler::OpArray,
    do_fcall_ptr: *const Instruction,
) -> Option<&'a Instruction> {
    let instructions = &op_array.instructions;
    let base = instructions.as_ptr();
    let boundary = unsafe { do_fcall_ptr.offset_from(base) };
    if boundary <= 0 || boundary as usize >= instructions.len() {
        return None;
    }

    let mut index = boundary as usize;
    let mut nested_calls = 0usize;
    while index > 0 {
        index -= 1;
        let instruction = &instructions[index];
        match instruction.opcode {
            OpCode::DoFcall => nested_calls += 1,
            OpCode::InitFcall
            | OpCode::InitUserCall
            | OpCode::InitMethodCall
            | OpCode::InitStaticCall
            | OpCode::InitLateStaticCall
            | OpCode::InitDynamicCall
            | OpCode::NewObj => {
                if nested_calls == 0 {
                    return Some(instruction);
                }
                nested_calls -= 1;
            }
            _ => {}
        }
    }
    None
}

#[cold]
fn static_site_called_class_id(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    do_fcall_ptr: *const Instruction,
    depth: usize,
) -> u32 {
    let Some(initializer) = call_initializer_before(caller_op_array, do_fcall_ptr) else {
        return 0;
    };
    if !matches!(
        initializer.opcode,
        OpCode::InitStaticCall | OpCode::InitLateStaticCall
    ) || initializer.op1_type != OpType::Const
    {
        return 0;
    }
    let Some(class_name) = caller_op_array
        .literals
        .get(initializer.op1 as usize)
        .and_then(Value::as_str)
    else {
        return 0;
    };
    if matches!(
        class_name.to_ascii_lowercase().as_str(),
        "self" | "parent" | "static"
    ) {
        called_class_id_for_frame(eg, caller, depth + 1)
    } else {
        eg.class_id_of(class_name)
    }
}

/// Recover forwarding `self`/`parent`/`static` calls only when a callee has an
/// actual late-static return contract. This walks saved call sites lazily, so
/// ordinary static frames need no called-scope field or side-table entry.
#[cold]
fn called_class_id_for_frame(eg: &ExecutorGlobals, frame: *mut ExecuteData, depth: usize) -> u32 {
    if frame.is_null() || depth >= 64 {
        return 0;
    }
    // SAFETY: every non-null frame comes from the live executor call chain.
    // Its function, CV storage, predecessor, opline and OpArray remain valid
    // until this cold metadata walk returns; the bounds check protects the
    // only backwards instruction lookup.
    unsafe {
        let common = &*(*frame).func;
        if common.sig.this_offset == 1 {
            let receiver = &*(*frame).cv(0);
            if receiver.value_type() == ValueType::Object {
                return receiver.object_class_id_unchecked();
            }
        }

        let lexical_class_id = eg
            .declaring_class_of((*frame).func)
            .map_or(0, |class| eg.class_id_of(class));
        let previous = (*frame).prev_execute_data;
        if previous.is_null() {
            return lexical_class_id;
        }
        let previous_op_array = (*previous).op_array();
        let resume_ptr = (*previous).opline;
        let base = previous_op_array.instructions.as_ptr();
        let resume_index = resume_ptr.offset_from(base);
        if resume_index <= 0 || resume_index as usize > previous_op_array.instructions.len() {
            return lexical_class_id;
        }
        let do_fcall_ptr = resume_ptr.sub(1);
        if (*do_fcall_ptr).opcode != OpCode::DoFcall {
            return lexical_class_id;
        }
        let resolved =
            static_site_called_class_id(eg, previous, previous_op_array, do_fcall_ptr, depth + 1);
        if resolved == 0 {
            lexical_class_id
        } else {
            resolved
        }
    }
}

/// Complete a call that could not use one of the compact DoFcall protocols.
///
/// Argument diagnostics, named variadics, dynamic `__invoke`, generators and
/// internal handlers are intentionally kept out of `execute_ex`. These paths
/// are important for PHP semantics but cold for ordinary fixed-signature user
/// calls, so outlining them keeps the baseline dispatch working set smaller.
fn registered_function_name(eg: &ExecutorGlobals, function: *const FunctionCommon) -> &str {
    // User functions retain their declaration spelling in the OpArray.  The
    // executor's lookup table cannot provide it because PHP function lookup is
    // case-insensitive and its keys are deliberately normalized to lowercase.
    // SAFETY: registered call targets have a stable allocation and their
    // common header discriminant identifies the enclosing function layout.
    unsafe {
        if (*function).fn_type == FunctionType::User {
            return &(*(function as *const UserFunction)).op_array.name;
        }
    }
    if let Some(name) = eg.internal_function_display_name(function) {
        return name;
    }
    eg.function_table
        .iter()
        .find_map(|(name, pointer)| std::ptr::eq(*pointer, function).then_some(name.as_str()))
        .unwrap_or("internal function")
}

pub(crate) fn displayed_function_name(
    eg: &ExecutorGlobals,
    function: *const FunctionCommon,
) -> String {
    let registered_name = registered_function_name(eg, function);
    if let Some((_, hook)) = registered_name.split_once("::$") {
        return eg
            .declaring_class_of(function)
            .map(|class| format!("{class}::${hook}"))
            .unwrap_or_else(|| registered_name.to_string());
    }
    if registered_name.starts_with("__closure_")
        || registered_name
            .rsplit_once("::")
            .map_or(registered_name, |(_, method)| method)
            .starts_with("__closure_")
    {
        registered_name
            .split_once('@')
            .map(|(_, public_name)| public_name.to_string())
            .unwrap_or_else(|| {
                eg.declaring_class_of(function)
                    .map(|class| format!("{class}::{{closure}}"))
                    .unwrap_or_else(|| "{closure}".to_string())
            })
    } else if let Some((_, method)) = registered_name.rsplit_once("::") {
        eg.declaring_class_of(function)
            .map(|class| format!("{class}::{method}"))
            .unwrap_or_else(|| registered_name.to_string())
    } else {
        registered_name.to_string()
    }
}

/// Enum engine methods are compiled to ordinary op-arrays today, but their
/// public argument failures use PHP's internal-method diagnostic wording.
/// Source enums cannot redeclare these reserved names, so the owner/name pair
/// is an unambiguous cold-call marker without widening FunctionCommon.
fn is_synthesized_enum_method(eg: &ExecutorGlobals, function: *const FunctionCommon) -> bool {
    let Some(class_name) = eg.declaring_class_of(function) else {
        return false;
    };
    if !eg
        .find_class(class_name)
        .is_some_and(|definition| definition.is_enum)
    {
        return false;
    }
    registered_function_name(eg, function)
        .rsplit_once("::")
        .is_some_and(|(_, method)| {
            method.eq_ignore_ascii_case("cases")
                || method.eq_ignore_ascii_case("from")
                || method.eq_ignore_ascii_case("tryFrom")
        })
}

fn too_few_arguments_error(
    eg: &ExecutorGlobals,
    function: *const FunctionCommon,
    common: &FunctionCommon,
    supplied: u32,
    caller_op_array: &crate::compiler::OpArray,
    call_instruction: &Instruction,
) -> Value {
    let name = displayed_function_name(eg, function);
    let required = common.sig.required_num_args;
    let internal_diagnostic =
        common.fn_type == FunctionType::Internal || is_synthesized_enum_method(eg, function);
    let relation = if internal_diagnostic {
        if common.sig.is_variadic || common.sig.public_arity() > required {
            "at least"
        } else {
            "exactly"
        }
    } else if common.sig.public_arity() > required {
        "at least"
    } else {
        "exactly"
    };
    let message = if internal_diagnostic {
        let noun = if required == 1 {
            "argument"
        } else {
            "arguments"
        };
        format!("{name}() expects {relation} {required} {noun}, {supplied} given")
    } else {
        let instruction_index = caller_op_array
            .instructions
            .iter()
            .position(|instruction| std::ptr::eq(instruction, call_instruction))
            .unwrap_or(0);
        let line = caller_op_array.source_line(instruction_index).unwrap_or(0);
        let file = if caller_op_array.source_file.is_empty() {
            caller_op_array.name.as_str()
        } else {
            caller_op_array.source_file.as_str()
        };
        format!(
            "Too few arguments to function {name}(), {supplied} passed in {file} on line {line} and {relation} {required} expected"
        )
    };
    make_error_value("ArgumentCountError", &message)
}

fn argument_type_error(
    eg: &ExecutorGlobals,
    function: *const FunctionCommon,
    common: &FunctionCommon,
    parameter_index: usize,
    hint: &ParamTypeHint,
    value: &Value,
    caller_op_array: &crate::compiler::OpArray,
    call_instruction: &Instruction,
) -> Value {
    let name = displayed_function_name(eg, function);
    let parameter = common
        .sig
        .param_names
        .get(parameter_index)
        .map(String::as_str)
        .unwrap_or("unknown");
    let mut message = format!(
        "{name}(): Argument #{} (${parameter}) must be of type {}, {} given",
        parameter_index + 1,
        hint.diagnostic_display_name(),
        declared_type_error_value_name(value)
    );
    if common.fn_type == FunctionType::User && !is_synthesized_enum_method(eg, function) {
        let instruction_index = caller_op_array
            .instructions
            .iter()
            .position(|instruction| std::ptr::eq(instruction, call_instruction))
            .unwrap_or(0);
        let line = caller_op_array.source_line(instruction_index).unwrap_or(0);
        let file = if caller_op_array.source_file.is_empty() {
            caller_op_array.name.as_str()
        } else {
            caller_op_array.source_file.as_str()
        };
        message.push_str(&format!(", called in {file} on line {line}"));
    }
    make_error_value("TypeError", &message)
}

fn declared_type_error_value_name(value: &Value) -> String {
    match value.value_type() {
        ValueType::True => "true".to_string(),
        ValueType::False => "false".to_string(),
        _ => value.diagnostic_type_name().into_owned(),
    }
}

fn too_many_internal_arguments_error(
    eg: &ExecutorGlobals,
    function: *const FunctionCommon,
    signature: &crate::vm::function::SignatureInfo,
    supplied: u32,
) -> Value {
    debug_assert!(!signature.is_variadic);
    let maximum = signature.public_arity();
    let relation = if maximum == signature.required_num_args {
        "exactly"
    } else {
        "at most"
    };
    let noun = if maximum == 1 {
        "argument"
    } else {
        "arguments"
    };
    let name = displayed_function_name(eg, function);
    make_error_value(
        "ArgumentCountError",
        &format!("{name}() expects {relation} {maximum} {noun}, {supplied} given"),
    )
}

#[cold]
#[inline(never)]
fn pack_pending_magic_call(
    call: *mut ExecuteData,
    method: Value,
    pending_named: &mut Option<Vec<(String, Value)>>,
) {
    // SAFETY: the magic initializer reserves the two declared public slots
    // plus the complete original send prefix in this live pending frame.
    unsafe {
        let original_num_args = (*call).num_args;
        let mut arguments = PhpArray::with_packed_capacity(original_num_args as usize);
        for index in 0..original_num_args {
            arguments.push((*call).cv(index + 1).clone());
        }
        if let Some(named) = pending_named.take() {
            for (name, value) in named {
                arguments.set_str(&name, value);
            }
        }

        let method_slot = (*call).cv_mut(1) as *mut Value;
        if original_num_args == 0 {
            frame_slot_init(call, method_slot, method);
        } else {
            frame_slot_set(call, method_slot, method);
        }
        let arguments_slot = (*call).cv_mut(2) as *mut Value;
        if original_num_args < 2 {
            frame_slot_init(call, arguments_slot, Value::array(arguments));
        } else {
            frame_slot_set(call, arguments_slot, Value::array(arguments));
        }
        for index in 2..original_num_args {
            frame_slot_set(call, (*call).cv_mut(index + 1), Value::undef());
        }
        (*call).num_args = 2;
    }
}

#[cold]
#[inline(never)]
fn execute_full_call<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    opline_ptr: *const Instruction,
    call: *mut ExecuteData,
    generic_member_contract: Option<std::rc::Rc<crate::generics::GenericMethodContract>>,
) -> Result<ColdResult<'a>, VmError> {
    stats::inc_do_fcall_full();

    // SAFETY: both frames are live and the optional result operand names a
    // compiler-owned slot in the caller.
    let return_value_ptr = unsafe {
        let result = if opline.result_type != OpType::Unused {
            (*frame).get_op_mut(opline.result as u32, opline.result_type)
        } else {
            std::ptr::null_mut()
        };
        (*call).return_value = result;
        result
    };

    // Extract named variadic args eagerly so no error path can leak them.
    let call_key = call as usize;
    let mut pending_named = eg.pending_named_variadic.remove(&call_key);
    let pending_closure_captures = eg.pending_closure_captures.remove(&call_key);

    // SendVal filled CV 0..N-1 for a dynamically resolved invokable object.
    // Make room for the hidden method receiver before validating arguments.
    if let Some(this_val) = take_pending_invoke_this(eg, call_key) {
        // A named send binds the receiver and shifts its positional prefix
        // eagerly. Undef is the internal marker for that already-bound state.
        if this_val.is_undef() {
            // Nothing left to move.
        } else {
            let num = unsafe { (*call).num_args };
            for i in (0..num).rev() {
                let val = unsafe { (*call).cv(i).clone_closure_capture() };
                let dst = unsafe { (*call).cv_mut(i + 1) };
                // SAFETY: CVs below `num` contain the sent positional prefix.
                // CV `num` is the single new destination and must be initialized;
                // lower destinations are initialized values being overwritten.
                unsafe {
                    if i + 1 == num {
                        frame_slot_init(call, dst as *mut Value, val);
                    } else {
                        frame_slot_set(call, dst as *mut Value, val);
                    }
                }
            }
            let this_slot = unsafe { (*call).cv_mut(0) };
            // SAFETY: a zero-argument frame has not written CV 0; otherwise it
            // contains the first positional value and is an overwrite target.
            unsafe {
                if num == 0 {
                    frame_slot_init(call, this_slot as *mut Value, this_val);
                } else {
                    frame_slot_set(call, this_slot as *mut Value, this_val);
                }
            }
        }
    }

    let pending_magic_method = take_pending_magic_call(eg, call_key);
    if let Some(method) = pending_magic_method {
        pack_pending_magic_call(call, method, &mut pending_named);
    }
    // SAFETY: `call` is the live compiler-sized frame linked from `frame`; its
    // registered function descriptor remains valid for the synchronous call.
    let (func_common, num_args) = unsafe { (&*(*call).func, (*call).num_args) };
    let public_max = func_common.sig.public_arity();
    if func_common.fn_type != FunctionType::User
        && !func_common.sig.is_variadic
        && num_args > public_max
    {
        let error = too_many_internal_arguments_error(
            eg,
            func_common as *const FunctionCommon,
            &func_common.sig,
            num_args,
        );
        // SAFETY: `call` is the live pending call owned by `frame`; its
        // compiler-sized slots were initialized by the preceding sends.
        unsafe { cleanup_frame_slots(call) };
        pop_vm_call_frame(eg, call);
        return Ok(match throw_in_frame(eg, frame, error) {
            ThrowResult::Handled(nf, no) => ColdResult::NewFrame(nf, no),
            ThrowResult::Unhandled(t) => ColdResult::Unhandled(t),
        });
    }

    if func_common.plan.needs_late_static_scope() {
        let receiver_is_object = func_common.sig.this_offset == 1
            && unsafe { (*call).cv(0) }.value_type() == ValueType::Object;
        let scope_is_missing = unsafe { (*call).embedded_late_static_class_id() == 0 }
            && eg.late_static_scope_class_id(call_key) == 0;
        if !receiver_is_object && scope_is_missing {
            let class_id = static_site_called_class_id(eg, frame, op_array, opline_ptr, 0);
            publish_late_static_call_class_id(eg, call, class_id);
        }
    }

    let callee_class = unsafe {
        let mut resolved = eg.declaring_class_of((*call).func).map(str::to_string);
        if let Some(declared) = resolved.as_deref()
            && eg
                .find_class(declared)
                .is_some_and(|definition| definition.is_trait)
            && func_common.sig.this_offset == 1
        {
            let receiver = (*call).cv(0);
            if receiver.value_type() == ValueType::Object
                && let Some(scope) =
                    eg.trait_composition_scope(receiver.object_class_name_unchecked(), declared)
            {
                resolved = Some(scope.to_string());
            }
        }
        resolved
    };
    let callee_class_ref = callee_class.as_deref();

    if !func_common.sig.param_type_hints.is_empty() {
        let mut type_error = None;
        // SAFETY: `call` is the live callee frame and every param_cv_index in
        // this bounded loop names an initialized supplied-argument slot.
        unsafe {
            for (i, hint) in func_common.sig.param_type_hints.iter().enumerate() {
                if matches!(hint, ParamTypeHint::None) {
                    continue;
                }
                if (i as u32) >= num_args {
                    break;
                }
                let cv_idx = func_common.sig.param_cv_index(i as u32);
                let value = (&*(*call).cv(cv_idx)).dereferenced().clone();
                if value.is_undef() {
                    continue;
                }
                match prepare_call_argument(
                    &value,
                    hint,
                    eg,
                    op_array.strict_types,
                    callee_class_ref,
                )? {
                    CallArgumentPreparation::Exact => continue,
                    CallArgumentPreparation::Coerced(prepared) => {
                        let slot = (*call).cv_mut(cv_idx) as *mut Value;
                        if (*slot).is_reference() {
                            slot_set((*slot).as_ref_ptr(), prepared);
                        } else {
                            frame_slot_set(call, slot, prepared);
                        }
                        continue;
                    }
                    CallArgumentPreparation::Invalid => {}
                }
                type_error = Some(argument_type_error(
                    eg,
                    (*call).func,
                    func_common,
                    i,
                    hint,
                    &value,
                    op_array,
                    opline,
                ));
                break;
            }
            if let Some(err) = type_error {
                let function = Function::from_common_ptr((*call).func);
                if function.fn_type() == FunctionType::User
                    && !is_synthesized_enum_method(eg, (*call).func)
                {
                    let callee_op_array = &function.as_user().op_array;
                    if let Some(declaration_line) = callee_op_array.declaration_line()
                        && !callee_op_array.source_file.is_empty()
                    {
                        let ignore_arguments =
                            crate::stdlib::ini_default(eg, "zend.exception_ignore_args")
                                .as_deref()
                                .is_some_and(crate::stdlib::ini_boolean);
                        let trace_options = if ignore_arguments { 2 } else { 0 };
                        let trace = crate::stdlib::collect_debug_backtrace(
                            call,
                            trace_options,
                            0,
                            eg,
                            true,
                        );
                        attach_argument_type_error_origin(
                            &err,
                            callee_op_array.source_file.clone(),
                            declaration_line,
                            trace,
                            op_array,
                            opline,
                        );
                    }
                }
                cleanup_frame_slots(call);
                pop_vm_call_frame(eg, call);
                return Ok(match throw_in_frame(eg, frame, err) {
                    ThrowResult::Handled(nf, no) => ColdResult::NewFrame(nf, no),
                    ThrowResult::Unhandled(t) => ColdResult::Unhandled(t),
                });
            }
        }
    }

    // PHP validates every supplied argument before reporting that a later
    // required argument is absent. This is observable when the first value has
    // the wrong type and a subsequent positional parameter was not supplied.
    if num_args < func_common.sig.required_num_args {
        let error = too_few_arguments_error(
            eg,
            func_common as *const FunctionCommon,
            func_common,
            num_args,
            op_array,
            opline,
        );
        // SAFETY: `call` is the live pending frame owned by `frame`; every
        // initialized send slot must be released before the frame is popped.
        unsafe { cleanup_frame_slots(call) };
        pop_vm_call_frame(eg, call);
        return Ok(match throw_in_frame(eg, frame, error) {
            ThrowResult::Handled(nf, no) => ColdResult::NewFrame(nf, no),
            ThrowResult::Unhandled(t) => ColdResult::Unhandled(t),
        });
    }

    // Named arguments can leave holes even when the public count is correct.
    // As with positional arity, validate every supplied value before reporting
    // the missing required parameter.
    for i in 0..func_common.sig.required_num_args {
        let cv_idx = func_common.sig.param_cv_index(i);
        // SAFETY: required parameter CV indices are part of the compiler-sized
        // live call frame, including Undef holes left by named sends.
        let val = unsafe { &*(*call).cv(cv_idx) };
        if val.is_undef() {
            // SAFETY: the live pending frame retains its registered function
            // descriptor for the complete synchronous call attempt.
            let function_name = registered_function_name(eg, unsafe { (*call).func });
            let parameter_name = func_common
                .sig
                .param_names
                .get(i as usize)
                .map(String::as_str)
                .unwrap_or("unknown");
            let error = make_error_value(
                "ArgumentCountError",
                &format!(
                    "{function_name}(): Argument #{} (${parameter_name}) not passed",
                    i + 1
                ),
            );
            // SAFETY: `call` is still the live pending frame and all initialized
            // slots must be released before removing it from the VM stack.
            unsafe { cleanup_frame_slots(call) };
            pop_vm_call_frame(eg, call);
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(nf, no) => ColdResult::NewFrame(nf, no),
                ThrowResult::Unhandled(t) => ColdResult::Unhandled(t),
            });
        }
    }

    let original_user_arguments = (func_common.fn_type == FunctionType::User
        && !func_common.sig.is_variadic
        && num_args > public_max)
        .then(|| {
            let mut arguments = Vec::with_capacity(num_args as usize);
            for index in 0..num_args {
                let cv_index = if func_common.sig.is_variadic && index >= public_max {
                    func_common.sig.variadic_cv_index + index - public_max
                } else {
                    func_common.sig.param_cv_index(index)
                };
                // SAFETY: the live call frame reserves the complete supplied
                // argument prefix, and cv_index maps one member of that prefix.
                let value = unsafe { (*call).cv(cv_index) };
                let value = if value.is_reference() {
                    unsafe { &*value.as_ref_ptr() }
                } else {
                    value
                };
                arguments.push(if value.is_undef() {
                    Value::null()
                } else {
                    value.clone()
                });
            }
            arguments
        });

    if func_common.sig.is_variadic {
        let extra_count = num_args.saturating_sub(public_max);
        let mut variadic_arr = PhpArray::new();
        let cv_start = func_common.sig.variadic_cv_index;
        let variadic_by_reference = func_common.sig.is_param_by_ref(public_max);
        for i in 0..extra_count {
            // SAFETY: the compiler-sized pending call frame contains the
            // complete supplied variadic prefix through `num_args`.
            let argument = unsafe { (*call).cv(cv_start + i) };
            let arg = if variadic_by_reference && argument.is_owned_reference() {
                argument.clone_owned_reference_alias()
            } else if variadic_by_reference && argument.is_reference() {
                // SAFETY: the source call-frame argument remains live while
                // the packed variadic array is built and invoked synchronously.
                Value::reference(unsafe { argument.as_ref_ptr() })
            } else {
                argument.clone()
            };
            variadic_arr.push(arg);
        }
        if let Some(named_extras) = pending_named {
            let variadic_hint = func_common.sig.param_type_hints.get(public_max as usize);
            for (name, val) in named_extras {
                if let Some(hint) = variadic_hint {
                    if !matches!(hint, ParamTypeHint::None)
                        && !check_type_hint(&val, hint, eg, op_array.strict_types, callee_class_ref)
                    {
                        let type_err = make_error_value(
                            "TypeError",
                            &format!(
                                "Named parameter ${} must be of type {}, {} given",
                                name,
                                hint.display_name(),
                                val.type_name()
                            ),
                        );
                        unsafe { cleanup_frame_slots(call) };
                        pop_vm_call_frame(eg, call);
                        return Ok(match throw_in_frame(eg, frame, type_err) {
                            ThrowResult::Handled(nf, no) => ColdResult::NewFrame(nf, no),
                            ThrowResult::Unhandled(t) => ColdResult::Unhandled(t),
                        });
                    }
                }
                variadic_arr.set_str(&name, val);
            }
        }
        let variadic_slot = unsafe { (*call).cv_mut(cv_start) };
        unsafe {
            frame_slot_set(
                call,
                variadic_slot as *mut Value,
                Value::array(variadic_arr),
            );
        }
    }

    if let Some(captures) = pending_closure_captures {
        let capture_offset = func_common.sig.parameter_cv_count();
        for (index, value) in captures.into_iter().enumerate() {
            // SAFETY: closure frame sizing includes every capture after the
            // declared parameter CVs; each destination is initialized once.
            let destination = unsafe { (*call).cv_mut(capture_offset + index as u32) };
            unsafe { frame_slot_set(call, destination as *mut Value, value) };
        }
    }

    if let Some(arguments) = original_user_arguments {
        eg.function_arguments.insert(call_key, arguments);
    }

    match unsafe { (*(*call).func).fn_type } {
        FunctionType::User => {
            let user = unsafe { &*((*call).func as *const UserFunction) };
            if user.op_array.is_generator {
                use crate::vm::generator::{Generator, new_generator_ref};

                let mut args = Vec::with_capacity(user.op_array.num_cvs as usize);
                for i in 0..user.op_array.num_cvs {
                    args.push(unsafe { (*call).cv(i).clone() });
                }
                let mut generator = Generator::new(
                    unsafe { (*call).func },
                    args,
                    user.op_array.num_cvs,
                    user.op_array.num_temps,
                );
                generator.trace_num_args = Value::long(i64::from(num_args));
                generator.called_scope_class_id = late_static_call_class_id(eg, call);
                generator.closure_static_vars = eg.closure_static_vars(call as usize);
                #[cfg(feature = "php-generics-reified")]
                {
                    generator.reified_context = eg.generator_reified_context(call as usize);
                }
                #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                {
                    generator.generic_member_contract = generic_member_contract;
                }
                #[cfg(not(any(
                    feature = "php-generics-erased",
                    feature = "php-generics-reified"
                )))]
                let _ = generic_member_contract;
                let gen_ref = new_generator_ref(generator);
                let mut gen_obj = PhpObject::dynamic("Generator".to_string(), 0, HashMap::new());
                gen_obj.generator = Some(gen_ref);
                let generator_value = Value::object(gen_obj);
                let return_hint = &func_common.sig.return_type_hint;
                if !check_type_hint(
                    &generator_value,
                    return_hint,
                    eg,
                    op_array.strict_types,
                    callee_class_ref,
                ) {
                    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                    eg.discard_generic_member_call(call as usize);
                    #[cfg(feature = "php-generics-reified")]
                    eg.discard_active_reified_binding_scope(call as usize);
                    unsafe { cleanup_frame_slots(call) };
                    pop_vm_call_frame(eg, call);
                    let error = make_error_value(
                        "TypeError",
                        &format!(
                            "Generator return type must be a supertype of Generator, {} given",
                            return_hint.display_name()
                        ),
                    );
                    return Ok(match throw_in_frame(eg, frame, error) {
                        ThrowResult::Handled(new_frame, new_op_array) => {
                            ColdResult::NewFrame(new_frame, new_op_array)
                        }
                        ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
                    });
                }
                if !return_value_ptr.is_null() {
                    unsafe {
                        frame_result_set(
                            frame,
                            return_value_ptr,
                            opline.result_type,
                            generator_value,
                        )
                    };
                }
                unsafe { cleanup_frame_slots(call) };
                pop_vm_call_frame(eg, call);
                Ok(ColdResult::Done)
            } else {
                #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                if let Some(contract) = generic_member_contract {
                    eg.activate_generic_member_call(call as usize, contract);
                }
                #[cfg(not(any(
                    feature = "php-generics-erased",
                    feature = "php-generics-reified"
                )))]
                let _ = generic_member_contract;
                if user.op_array.may_access_globals {
                    let vars_to_sync = if !op_array.main_scope_vars.is_empty() {
                        &op_array.main_scope_vars
                    } else {
                        &op_array.global_vars
                    };
                    for (cv_idx, var_name) in vars_to_sync {
                        let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                        let val = unsafe { (*cv_ptr).clone() };
                        globals_set(&mut eg.globals, var_name, val);
                    }
                }
                unsafe {
                    (*call).opline = user.op_array.instructions.as_ptr();
                    (*frame).opline = opline_ptr.add(1);
                }
                eg.current_execute_data.set(call);
                Ok(ColdResult::NewFrame(call, unsafe { (*call).op_array() }))
            }
        }
        FunctionType::Internal => {
            let internal = unsafe { &*((*call).func as *const super::function::InternalFunction) };
            if !return_value_ptr.is_null() {
                unsafe {
                    frame_result_prepare_external_write(frame, return_value_ptr, opline.result_type)
                };
            }
            let handler_result = (internal.handler)(call, return_value_ptr, eg);
            if !return_value_ptr.is_null() {
                unsafe {
                    frame_result_finish_external_write(frame, return_value_ptr, opline.result_type)
                };
            }
            let internal_exception = eg.exception.take();
            if let Some(exception) = internal_exception.as_ref() {
                attach_internal_call_trace_if_missing(exception, call, frame, eg);
            }
            unsafe { cleanup_frame_slots(call) };
            pop_vm_call_frame(eg, call);
            if let Some(exc) = internal_exception {
                return Ok(match throw_in_frame(eg, frame, exc) {
                    ThrowResult::Handled(nf, no) => ColdResult::NewFrame(nf, no),
                    ThrowResult::Unhandled(t) => ColdResult::Unhandled(t),
                });
            }
            handler_result?;
            Ok(ColdResult::Done)
        }
        FunctionType::Undef => {
            let err = make_error_value("Error", "Call to undefined function");
            Ok(match throw_in_frame(eg, frame, err) {
                ThrowResult::Handled(nf, no) => ColdResult::NewFrame(nf, no),
                ThrowResult::Unhandled(t) => ColdResult::Unhandled(t),
            })
        }
    }
}

include!("execute/property_types.rs");
include!("execute/baseline_dispatch_cold.rs");
include!("execute/baseline_dispatch.rs");

#[derive(Clone, Copy)]
enum ArrayKeyRef<'a> {
    Int(i64),
    String(&'a str),
}

#[derive(Clone, Copy)]
enum ArrayKeyError {
    Illegal,
    DeprecatedNull,
    DeprecatedFloat(i64),
    NonRepresentableFloat { integer: i64, also_deprecated: bool },
    Resource(i64),
}

const PHP_LONG_UPPER_BOUND: f64 = 9_223_372_036_854_775_808.0;
const PHP_ULONG_MODULUS: f64 = 18_446_744_073_709_551_616.0;

/// Match Zend's finite double-to-long modulo conversion outside the signed
/// range. Non-finite doubles become zero.
fn php_float_to_long(value: f64) -> i64 {
    if !value.is_finite() {
        return 0;
    }
    let mut reduced = value % PHP_ULONG_MODULUS;
    if reduced >= PHP_LONG_UPPER_BOUND {
        reduced -= PHP_ULONG_MODULUS;
    } else if reduced < -PHP_LONG_UPPER_BOUND {
        reduced += PHP_ULONG_MODULUS;
    }
    reduced as i64
}

/// Normalize an array offset while borrowing string storage from the source
/// `Value`. Read paths can therefore probe `PhpArray` without allocating an
/// owned `ArrayKey` for every access.
fn value_to_array_key_ref(val: &Value) -> Result<ArrayKeyRef<'_>, ArrayKeyError> {
    match val.value_type() {
        ValueType::Long => Ok(ArrayKeyRef::Int(val.as_long().unwrap())),
        ValueType::String => {
            let value = val.as_str().unwrap();
            match canonical_decimal_array_key(value) {
                Some(value) => Ok(ArrayKeyRef::Int(value)),
                None => Ok(ArrayKeyRef::String(value)),
            }
        }
        ValueType::Null => Err(ArrayKeyError::DeprecatedNull),
        ValueType::True => Ok(ArrayKeyRef::Int(1)),
        ValueType::False => Ok(ArrayKeyRef::Int(0)),
        ValueType::Double => {
            let value = val.as_double().unwrap();
            let integer = php_float_to_long(value);
            if !value.is_finite() || !(-PHP_LONG_UPPER_BOUND..PHP_LONG_UPPER_BOUND).contains(&value)
            {
                Err(ArrayKeyError::NonRepresentableFloat {
                    integer,
                    also_deprecated: value.is_nan(),
                })
            } else if value == integer as f64 {
                Ok(ArrayKeyRef::Int(integer))
            } else {
                Err(ArrayKeyError::DeprecatedFloat(integer))
            }
        }
        ValueType::Resource => Err(ArrayKeyError::Resource(val.as_resource_id().unwrap())),
        _ => Err(ArrayKeyError::Illegal),
    }
}

#[cfg(test)]
mod array_key_normalization_tests {
    use super::canonical_decimal_array_key;

    #[test]
    fn canonical_decimal_keys_match_php_array_rules_without_allocation() {
        for (source, expected) in [
            ("0", Some(0)),
            ("1", Some(1)),
            ("-3", Some(-3)),
            ("9223372036854775807", Some(i64::MAX)),
            ("-9223372036854775808", Some(i64::MIN)),
            ("", None),
            ("01", None),
            ("-0", None),
            ("+1", None),
            (" 1", None),
            ("1a", None),
            ("9223372036854775808", None),
        ] {
            assert_eq!(canonical_decimal_array_key(source), expected, "{source}");
        }
    }
}

/// Convert a Value to an ArrayKey.
fn value_to_array_key(val: &Value) -> Result<ArrayKey, ArrayKeyError> {
    match value_to_array_key_ref(val)? {
        ArrayKeyRef::Int(value) => Ok(ArrayKey::Int(value)),
        ArrayKeyRef::String(value) => Ok(ArrayKey::String(value.to_string())),
    }
}

/// `$GLOBALS` dimensions name variables through scalar string conversion,
/// unlike ordinary array dimensions. Unsupported containers retain the
/// existing error boundary until their conversion diagnostics are modeled.
fn value_to_global_name(val: &Value) -> Result<String, VmError> {
    match val.value_type() {
        ValueType::Undef | ValueType::Null | ValueType::False => Ok(String::new()),
        ValueType::True => Ok("1".to_string()),
        ValueType::Long | ValueType::Double | ValueType::String | ValueType::Resource => {
            Ok(val.echo_to_string())
        }
        _ => Err(VmError::Fatal("Illegal offset type".into())),
    }
}

const MAX_COMPARISON_DEPTH: usize = 512;

#[derive(Default)]
struct ComparisonContext {
    active_left: std::collections::HashSet<usize>,
    active_right: std::collections::HashSet<usize>,
}

/// PHP == comparison for compound values. Recursive structures raise a
/// catchable Error through the checked entry point rather than overflowing the
/// host stack. Scalar leaves retain PHP's ordinary loose behavior.
pub(crate) fn values_equal_checked(a: &Value, b: &Value) -> Result<bool, ()> {
    fn equal_inner(
        a: &Value,
        b: &Value,
        context: &mut ComparisonContext,
        depth: usize,
    ) -> Result<bool, ()> {
        let a = a.dereferenced();
        let b = b.dereferenced();

        if matches!(a.value_type(), ValueType::True | ValueType::False)
            || matches!(b.value_type(), ValueType::True | ValueType::False)
            || matches!(a.value_type(), ValueType::Null | ValueType::Undef)
            || matches!(b.value_type(), ValueType::Null | ValueType::Undef)
        {
            return Ok(a.is_truthy() == b.is_truthy());
        }

        Ok(match (a.value_type(), b.value_type()) {
            (ValueType::Long, ValueType::Long) => a.as_long() == b.as_long(),
            (ValueType::Long | ValueType::Double, ValueType::Long | ValueType::Double) => {
                a.to_double() == b.to_double()
            }
            (ValueType::String, ValueType::String) => {
                let left = a.as_str().unwrap();
                let right = b.as_str().unwrap();
                match (left.trim().parse::<f64>(), right.trim().parse::<f64>()) {
                    (Ok(left), Ok(right)) => left == right,
                    _ => left == right,
                }
            }
            (ValueType::Array, ValueType::Array) => {
                let left_identity = a.array_identity().unwrap();
                let right_identity = b.array_identity().unwrap();
                if left_identity == right_identity {
                    return Ok(true);
                }
                if depth >= MAX_COMPARISON_DEPTH
                    || !context.active_left.insert(left_identity)
                    || !context.active_right.insert(right_identity)
                {
                    return Err(());
                }
                let left = a.as_array().unwrap();
                let right = b.as_array().unwrap();
                let result = if left.len() != right.len() {
                    Ok(false)
                } else {
                    let mut equal = true;
                    for (key, value) in left.iter() {
                        let other = match key {
                            ArrayKey::Int(key) => right.get_int(key),
                            ArrayKey::String(key) => right.get_str(&key),
                        };
                        let Some(other) = other else {
                            equal = false;
                            break;
                        };
                        if !equal_inner(value, other, context, depth + 1)? {
                            equal = false;
                            break;
                        }
                    }
                    Ok(equal)
                };
                context.active_left.remove(&left_identity);
                context.active_right.remove(&right_identity);
                return result;
            }
            (ValueType::Object, ValueType::Object) => {
                let left_identity = a.object_identity().unwrap();
                let right_identity = b.object_identity().unwrap();
                if left_identity == right_identity {
                    return Ok(true);
                }
                if depth >= MAX_COMPARISON_DEPTH
                    || !context.active_left.insert(left_identity)
                    || !context.active_right.insert(right_identity)
                {
                    return Err(());
                }

                let left = a.as_object().unwrap();
                let right = b.as_object().unwrap();
                let same_class = if left.class_id != 0 || right.class_id != 0 {
                    left.class_id == right.class_id
                } else {
                    left.class_name.eq_ignore_ascii_case(&right.class_name)
                };
                if !same_class {
                    context.active_left.remove(&left_identity);
                    context.active_right.remove(&right_identity);
                    return Ok(false);
                }

                let mut left_count = 0usize;
                let mut properties_equal = true;
                let mut comparison_error = false;
                left.for_each_property(|name, value| {
                    left_count += 1;
                    if properties_equal && !comparison_error {
                        match right.get_property(name) {
                            Some(other) => match equal_inner(value, other, context, depth + 1) {
                                Ok(equal) => properties_equal = equal,
                                Err(()) => comparison_error = true,
                            },
                            None => properties_equal = false,
                        }
                    }
                });
                let mut right_count = 0usize;
                right.for_each_property(|_, _| right_count += 1);
                context.active_left.remove(&left_identity);
                context.active_right.remove(&right_identity);
                if comparison_error {
                    return Err(());
                }
                properties_equal && left_count == right_count
            }
            (ValueType::Closure, ValueType::Closure) => a
                .as_closure()
                .zip(b.as_closure())
                .is_some_and(|(left, right)| left.same_identity(right)),
            (ValueType::Resource, ValueType::Resource) => a.as_resource_id() == b.as_resource_id(),
            _ => false,
        })
    }

    equal_inner(a, b, &mut ComparisonContext::default(), 0)
}

/// PHP three-way comparison for compound values. Object and array tables are
/// compared by key without requiring insertion-order identity, and recursive
/// dependencies use the same bounded error contract as loose equality.
pub(crate) fn values_compare_checked(a: &Value, b: &Value) -> Result<i32, ()> {
    #[inline]
    fn ordering(value: std::cmp::Ordering) -> i32 {
        match value {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }

    fn compare_inner(
        a: &Value,
        b: &Value,
        context: &mut ComparisonContext,
        depth: usize,
    ) -> Result<i32, ()> {
        let a = a.dereferenced();
        let b = b.dereferenced();

        if matches!(a.value_type(), ValueType::True | ValueType::False)
            || matches!(b.value_type(), ValueType::True | ValueType::False)
            || matches!(a.value_type(), ValueType::Null | ValueType::Undef)
            || matches!(b.value_type(), ValueType::Null | ValueType::Undef)
        {
            return Ok(ordering(a.is_truthy().cmp(&b.is_truthy())));
        }

        match (a.value_type(), b.value_type()) {
            (ValueType::Long, ValueType::Long) => {
                Ok(ordering(a.as_long().unwrap().cmp(&b.as_long().unwrap())))
            }
            (ValueType::Long | ValueType::Double, ValueType::Long | ValueType::Double) => {
                let left = a.to_double().unwrap();
                let right = b.to_double().unwrap();
                Ok(left.partial_cmp(&right).map_or(0, ordering))
            }
            (ValueType::String, ValueType::String) => {
                let left = a.as_str().unwrap();
                let right = b.as_str().unwrap();
                Ok(
                    match (left.trim().parse::<f64>(), right.trim().parse::<f64>()) {
                        (Ok(left), Ok(right)) => left.partial_cmp(&right).map_or(0, ordering),
                        _ => ordering(left.cmp(right)),
                    },
                )
            }
            (ValueType::Array, ValueType::Array) => {
                let left_identity = a.array_identity().unwrap();
                let right_identity = b.array_identity().unwrap();
                if left_identity == right_identity {
                    return Ok(0);
                }
                if depth >= MAX_COMPARISON_DEPTH
                    || !context.active_left.insert(left_identity)
                    || !context.active_right.insert(right_identity)
                {
                    return Err(());
                }
                let left = a.as_array().unwrap();
                let right = b.as_array().unwrap();
                let mut result = ordering(left.len().cmp(&right.len()));
                let mut comparison_error = false;
                if result == 0 {
                    for (key, value) in left.iter() {
                        let other = match key {
                            ArrayKey::Int(key) => right.get_int(key),
                            ArrayKey::String(key) => right.get_str(&key),
                        };
                        let Some(other) = other else {
                            result = 1;
                            break;
                        };
                        match compare_inner(value, other, context, depth + 1) {
                            Ok(cmp) if cmp != 0 => {
                                result = cmp;
                                break;
                            }
                            Ok(_) => {}
                            Err(()) => {
                                comparison_error = true;
                                break;
                            }
                        }
                    }
                }
                context.active_left.remove(&left_identity);
                context.active_right.remove(&right_identity);
                if comparison_error {
                    Err(())
                } else {
                    Ok(result)
                }
            }
            (ValueType::Object, ValueType::Object) => {
                let left_identity = a.object_identity().unwrap();
                let right_identity = b.object_identity().unwrap();
                if left_identity == right_identity {
                    return Ok(0);
                }
                if depth >= MAX_COMPARISON_DEPTH
                    || !context.active_left.insert(left_identity)
                    || !context.active_right.insert(right_identity)
                {
                    return Err(());
                }

                let left = a.as_object().unwrap();
                let right = b.as_object().unwrap();
                let same_class = if left.class_id != 0 || right.class_id != 0 {
                    left.class_id == right.class_id
                } else {
                    left.class_name.eq_ignore_ascii_case(&right.class_name)
                };
                if !same_class {
                    context.active_left.remove(&left_identity);
                    context.active_right.remove(&right_identity);
                    return Ok(1);
                }

                let mut left_count = 0usize;
                left.for_each_property(|_, _| left_count += 1);
                let mut right_count = 0usize;
                right.for_each_property(|_, _| right_count += 1);
                let mut result = ordering(left_count.cmp(&right_count));
                let mut comparison_error = false;
                if result == 0 {
                    left.for_each_property(|name, value| {
                        if result != 0 || comparison_error {
                            return;
                        }
                        let Some(other) = right.get_property(name) else {
                            result = 1;
                            return;
                        };
                        match compare_inner(value, other, context, depth + 1) {
                            Ok(cmp) => result = cmp,
                            Err(()) => comparison_error = true,
                        }
                    });
                }
                context.active_left.remove(&left_identity);
                context.active_right.remove(&right_identity);
                if comparison_error {
                    Err(())
                } else {
                    Ok(result)
                }
            }
            (ValueType::Closure, ValueType::Closure) => Ok(
                if a.as_closure()
                    .zip(b.as_closure())
                    .is_some_and(|(left, right)| left.same_identity(right))
                {
                    0
                } else {
                    1
                },
            ),
            (ValueType::Resource, ValueType::Resource) => Ok(ordering(
                a.as_resource_id()
                    .unwrap()
                    .cmp(&b.as_resource_id().unwrap()),
            )),
            _ => Ok(1),
        }
    }

    compare_inner(a, b, &mut ComparisonContext::default(), 0)
}

/// PHP === comparison: same type and same value (recursive for arrays).
pub(crate) fn values_identical_checked(a: &Value, b: &Value) -> Result<bool, ()> {
    fn identical_inner(
        a: &Value,
        b: &Value,
        context: &mut ComparisonContext,
        depth: usize,
    ) -> Result<bool, ()> {
        let a = a.dereferenced();
        let b = b.dereferenced();
        if matches!(a.value_type(), ValueType::Undef | ValueType::Null)
            && matches!(b.value_type(), ValueType::Undef | ValueType::Null)
        {
            return Ok(true);
        }
        if a.value_type() != b.value_type() {
            return Ok(false);
        }
        Ok(match a.value_type() {
            ValueType::Undef | ValueType::Null => true,
            ValueType::True | ValueType::False => true,
            ValueType::Long => a.as_long() == b.as_long(),
            ValueType::Double => a.as_double() == b.as_double(),
            ValueType::String => a.as_str() == b.as_str(),
            ValueType::Array => {
                let left_identity = a.array_identity().unwrap();
                let right_identity = b.array_identity().unwrap();
                if left_identity == right_identity {
                    return Ok(true);
                }
                if depth >= MAX_COMPARISON_DEPTH
                    || !context.active_left.insert(left_identity)
                    || !context.active_right.insert(right_identity)
                {
                    return Err(());
                }
                let arr_a = a.as_array().unwrap();
                let arr_b = b.as_array().unwrap();
                if arr_a.len() != arr_b.len() {
                    context.active_left.remove(&left_identity);
                    context.active_right.remove(&right_identity);
                    return Ok(false);
                }
                let mut identical = true;
                for ((ka, va), (kb, vb)) in arr_a.iter().zip(arr_b.iter()) {
                    if ka != kb || !identical_inner(va, vb, context, depth + 1)? {
                        identical = false;
                        break;
                    }
                }
                context.active_left.remove(&left_identity);
                context.active_right.remove(&right_identity);
                identical
            }
            ValueType::Object => {
                // Objects are identical if they are the same instance (same Rc pointer)
                let rc_a = a.as_object_rc().unwrap();
                let rc_b = b.as_object_rc().unwrap();
                std::rc::Rc::ptr_eq(&rc_a, &rc_b)
            }
            ValueType::Closure => {
                // Closures are PHP objects too. Their immutable payload address is
                // the request-local object identity retained by Value::clone.
                std::ptr::eq(a.as_closure().unwrap(), b.as_closure().unwrap())
            }
            ValueType::Resource => a.as_resource_id() == b.as_resource_id(),
            _ => false,
        })
    }

    identical_inner(a, b, &mut ComparisonContext::default(), 0)
}

pub(crate) fn values_identical(a: &Value, b: &Value) -> bool {
    values_identical_checked(a, b).unwrap_or(false)
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
pub(super) fn handle_interrupt(eg: &ExecutorGlobals) -> Result<(), VmError> {
    eg.vm_interrupt.store(false, Ordering::Relaxed);

    if eg.timed_out.load(Ordering::Relaxed) {
        eg.timed_out.store(false, Ordering::Relaxed);
        return Err(VmError::Fatal("Maximum execution time exceeded".into()));
    }

    Ok(())
}
