//! Generic Reflection built-ins.
//!
//! Reflection consumes the permanent interned generic graph. Keeping these
//! cold handlers outside the main stdlib unit prevents metadata-facing API
//! growth from obscuring unrelated built-ins or entering their hot paths.

mod ancestry;
mod functions;
mod generic_parameters;
mod registry;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use ancestry::reflected_arguments;
use functions::reflection_function_target;

use crate::compiler::compile::{ClassConstantDefinition, Compiler, PropertyDefinition};
use crate::generics::{GenericDeclarationKind, GenericRuntimeCapabilities};
use crate::parser::{Expr, Visibility};
use crate::runtime::{
    ExecutorGlobals, LazyObjectStrategy, ReflectionAttributeDeclaration,
    ReflectionAttributeDeclarationKind, ReflectionPropertyMetadata,
};
use crate::value::{
    ArrayKey, DynamicPropertyMap, PhpArray, PhpClosure, PhpObject, ReferencePropertyConstraint,
    Value, ValueType, make_error_value,
};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{
    ATTRIBUTE_PUBLIC_TARGET_MASK, ATTRIBUTE_TARGET_PROPERTY_HOOK, AttributeArgument,
    AttributeDefinition, AttributeEvaluationScope, FunctionCommon, FunctionType, InternalFunction,
    InternalFunctionDeprecation, ParamTypeHint, UserFunction,
};

pub(super) use registry::register;

fn with_raw_argument<R>(ed: *mut ExecuteData, index: u32, visit: impl FnOnce(&Value) -> R) -> R {
    // SAFETY: every reflection handler receives a live internal ExecuteData
    // frame, and its registered fixed/variadic arity guarantees this CV slot.
    let value = unsafe { (*ed).cv(index) };
    visit(value)
}

fn with_argument<R>(ed: *mut ExecuteData, index: u32, visit: impl FnOnce(&Value) -> R) -> R {
    with_raw_argument(ed, index, |value| {
        let value = if value.is_reference() {
            unsafe { &*value.as_ref_ptr() }
        } else {
            value
        };
        visit(value)
    })
}

fn argument_string(ed: *mut ExecuteData, index: u32) -> String {
    with_argument(ed, index, |value| {
        value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.echo_to_string())
    })
}

fn reflection_argument_type_name(value: &Value) -> String {
    match value.dereferenced().value_type() {
        ValueType::True => "true".to_string(),
        ValueType::False => "false".to_string(),
        _ => value.dereferenced().diagnostic_type_name().into_owned(),
    }
}

fn return_value(rv: *mut Value, value: Value) -> Result<(), VmError> {
    if !rv.is_null() {
        unsafe { rv.write(value) };
    }
    Ok(())
}

fn object_value(
    class_name: &str,
    properties: impl IntoIterator<Item = (&'static str, Value)>,
) -> Value {
    let properties = properties
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect::<HashMap<_, _>>();
    Value::object(PhpObject::dynamic(class_name.to_string(), 0, properties))
}

fn named_reflected_type(name: &str) -> Value {
    object_value(
        "ReflectionNamedType",
        [
            ("__generic_name", Value::string(name)),
            ("__generic_arguments", Value::array(PhpArray::new())),
            ("__generic_string", Value::string(name)),
        ],
    )
}

fn reflected_signature_type(hint: &ParamTypeHint) -> Value {
    match hint {
        ParamTypeHint::Union(parts) | ParamTypeHint::Intersection(parts) => {
            let mut types = PhpArray::with_packed_capacity(parts.len());
            for part in parts {
                types.push(reflected_signature_type(part));
            }
            object_value(
                if matches!(hint, ParamTypeHint::Union(_)) {
                    "ReflectionUnionType"
                } else {
                    "ReflectionIntersectionType"
                },
                [
                    ("__generic_types", Value::array(types)),
                    ("__generic_string", Value::string(hint.display_name())),
                    ("__reflection_allows_null", Value::bool(false)),
                ],
            )
        }
        ParamTypeHint::Nullable(inner) => object_value(
            "ReflectionNamedType",
            [
                ("__generic_name", Value::string(inner.display_name())),
                ("__generic_arguments", Value::array(PhpArray::new())),
                ("__generic_string", Value::string(hint.display_name())),
                ("__reflection_allows_null", Value::bool(true)),
            ],
        ),
        _ => object_value(
            "ReflectionNamedType",
            [
                ("__generic_name", Value::string(hint.display_name())),
                ("__generic_arguments", Value::array(PhpArray::new())),
                ("__generic_string", Value::string(hint.display_name())),
                ("__reflection_allows_null", Value::bool(false)),
            ],
        ),
    }
}

fn reflected_property(ed: *mut ExecuteData, name: &str) -> Option<Value> {
    with_argument(ed, 0, |value| {
        value.as_object()?.get_property(name).cloned()
    })
}

fn reflection_property_metadata<'a>(
    ed: *mut ExecuteData,
    eg: &'a ExecutorGlobals,
) -> Option<&'a ReflectionPropertyMetadata> {
    let receiver = with_argument(ed, 0, Clone::clone);
    eg.reflection_property_metadata(&receiver)
}

fn with_reflected_property<R>(
    ed: *mut ExecuteData,
    name: &str,
    visit: impl FnOnce(Option<&Value>) -> R,
) -> R {
    with_argument(ed, 0, |value| {
        let object = value.as_object();
        visit(object.as_ref().and_then(|object| object.get_property(name)))
    })
}

fn reflection_type_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__generic_name").unwrap_or_else(|| Value::string("")),
    )
}

fn reflection_type_to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__generic_string").unwrap_or_else(|| Value::string("")),
    )
}

fn reflection_type_has_generic_arguments(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let found = reflected_property(ed, "__generic_arguments")
        .and_then(|arguments| arguments.as_array().map(|arguments| !arguments.is_empty()))
        .unwrap_or(false);
    return_value(rv, Value::bool(found))
}

fn reflection_type_generic_arguments(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__generic_arguments")
            .unwrap_or_else(|| Value::array(PhpArray::new())),
    )
}

fn reflection_compound_types(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__generic_types").unwrap_or_else(|| Value::array(PhpArray::new())),
    )
}

fn reflection_exception(eg: &mut ExecutorGlobals, message: impl AsRef<str>) {
    eg.exception = Some(make_error_value("ReflectionException", message.as_ref()));
}

fn set_target(ed: *mut ExecuteData, kind: &str, owner: String) {
    with_argument(ed, 0, |value| {
        if let Some(mut object) = value.as_object_mut() {
            object.set_property("__generic_kind", Value::string(kind));
            object.set_property("__generic_owner", Value::string(owner));
        }
    });
}

fn function_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = with_argument(ed, 1, Clone::clone);
    let (kind, owner) = reflection_function_target(&target)?;
    let function = target
        .as_closure()
        .map(|closure| closure.func)
        .or_else(|| eg.find_function(&owner));
    let requested_name = owner.trim_start_matches('\\');
    let public_name = crate::builtin_metadata::internal_function_alias(requested_name)
        .map(|alias| alias.alias.to_string())
        .unwrap_or_else(|| {
            function.map_or_else(
                || requested_name.to_string(),
                |function| {
                    crate::vm::execute::displayed_function_name(
                        eg,
                        function as *const FunctionCommon,
                    )
                },
            )
        });
    let is_anonymous = target
        .as_closure()
        .and_then(PhpClosure::user_function)
        .is_some_and(|function| function.op_array.name.starts_with("__closure_"));
    let closure_this = target
        .as_closure()
        .and_then(|closure| closure.bound_this.clone())
        .unwrap_or_else(Value::null);
    let called_class = target.as_closure().and_then(|closure| {
        (closure.called_scope_class_id != 0)
            .then(|| eg.class_by_id(closure.called_scope_class_id))
            .flatten()
            .map(|class| class.name.clone())
            .or_else(|| {
                closure
                    .bound_this
                    .as_ref()
                    .and_then(Value::as_object)
                    .map(|object| object.class_name.to_string())
            })
    });
    let closure_scope_class = target.as_closure().and_then(|closure| {
        if closure.scope_is_dummy {
            Some("Closure".to_string())
        } else if closure.called_scope_class_id != 0 {
            eg.class_by_id(closure.called_scope_class_id)
                .map(|class| class.name.clone())
        } else {
            None
        }
    });
    set_target(ed, kind, owner.clone());
    with_argument(ed, 0, |value| {
        if let Some(mut object) = value.as_object_mut() {
            object.set_property("name", Value::string(public_name));
            object.set_property("__reflection_is_anonymous", Value::bool(is_anonymous));
            object.set_property("__reflection_closure_this", closure_this);
            object.set_property(
                "__reflection_closure",
                target
                    .as_closure()
                    .map_or_else(Value::null, |_| target.clone()),
            );
            object.set_property(
                "__reflection_closure_called_class",
                called_class.map_or_else(Value::null, Value::string),
            );
            object.set_property(
                "__reflection_closure_scope_class",
                closure_scope_class.map_or_else(Value::null, Value::string),
            );
            object.set_property(
                "__reflection_function_pointer",
                Value::long(function.map_or(0, |function| function as usize as i64)),
            );
        }
    });
    Ok(())
}

fn reflected_function(ed: *mut ExecuteData) -> Option<&'static FunctionCommon> {
    let pointer = reflected_property(ed, "__reflection_function_pointer")?.as_long()? as usize
        as *const FunctionCommon;
    // SAFETY: constructors store only registered FunctionCommon pointers;
    // ExecutorGlobals owns those allocations for the full request lifetime.
    (!pointer.is_null()).then(|| unsafe { &*pointer })
}

fn reflected_user_function(ed: *mut ExecuteData) -> Option<&'static UserFunction> {
    let function = reflected_function(ed)?;
    reflected_user_function_from_common(function)
}

fn reflected_internal_function(ed: *mut ExecuteData) -> Option<&'static InternalFunction> {
    let function = reflected_function(ed)?;
    reflected_invocation_metadata(function, None).1
}

fn internal_deprecated_attribute(deprecation: &InternalFunctionDeprecation) -> AttributeDefinition {
    AttributeDefinition {
        name: "Deprecated".to_string(),
        arguments: vec![
            AttributeArgument {
                name: Some("since".to_string()),
                value: Ok(Value::string(deprecation.since)),
                deferred_expression: None,
            },
            AttributeArgument {
                name: Some("message".to_string()),
                value: Ok(Value::string(deprecation.message)),
                deferred_expression: None,
            },
        ],
        evaluation_scope: Rc::new(AttributeEvaluationScope::default()),
        target: 2,
        source_file: String::new(),
        source_line: 0,
        strict_types: false,
    }
}

fn reflected_function_attributes(ed: *mut ExecuteData) -> Vec<AttributeDefinition> {
    if let Some(function) = reflected_user_function(ed) {
        return function.attributes.clone();
    }
    reflected_internal_function(ed)
        .and_then(|function| function.deprecation)
        .map(internal_deprecated_attribute)
        .into_iter()
        .collect()
}

fn reflected_user_function_from_common(function: &FunctionCommon) -> Option<&UserFunction> {
    reflected_invocation_metadata(function, None).0
}

fn reflected_invocation_metadata<'function, 'receiver>(
    function: &'function FunctionCommon,
    receiver: Option<&'receiver Value>,
) -> (
    Option<&'function UserFunction>,
    Option<&'function InternalFunction>,
    Option<(u32, &'receiver str)>,
) {
    let user = function.fn_type == FunctionType::User;
    let internal = function.fn_type == FunctionType::Internal;
    let object = receiver.filter(|value| value.value_type() == ValueType::Object);
    // SAFETY: FunctionCommon is the first field of every repr(C) UserFunction
    // and InternalFunction, and each discriminant proves its allocation kind.
    // The checked Object tag similarly proves its stable request-owned
    // PhpObject payload.
    unsafe {
        (
            user.then(|| &*(function as *const FunctionCommon as *const UserFunction)),
            internal.then(|| &*(function as *const FunctionCommon as *const InternalFunction)),
            object.map(|object| {
                (
                    object.object_class_id_unchecked(),
                    object.object_class_name_unchecked(),
                )
            }),
        )
    }
}

fn receiver_class_name(ed: *mut ExecuteData) -> Option<String> {
    with_argument(ed, 0, |value| {
        value
            .as_object()
            .map(|object| object.class_name.to_string())
    })
}

fn rebind_attribute_evaluation_scope(
    definitions: &mut [AttributeDefinition],
    class_name: Option<&str>,
    eg: &ExecutorGlobals,
) {
    let Some(class_name) = class_name else {
        return;
    };
    let parent = eg
        .find_class(class_name)
        .and_then(|class| class.parent.clone());
    for definition in definitions {
        let scope = std::rc::Rc::make_mut(&mut definition.evaluation_scope);
        scope.lexical_class = Some(class_name.to_string());
        scope.lexical_parent = parent.clone();
    }
}

fn reflected_function_attribute_scope(ed: *mut ExecuteData) -> Option<String> {
    if parameter_property_bool(ed, "__reflection_closure_method") {
        return reflected_property(ed, "__reflection_closure_called_class")
            .and_then(|value| value.as_str().map(str::to_owned));
    }
    reflected_property(ed, "__reflection_closure_called_class")
        .or_else(|| reflected_property(ed, "__reflection_method_class"))
        .and_then(|value| value.as_str().map(str::to_owned))
}

fn reflected_attribute_definitions(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Vec<AttributeDefinition> {
    match receiver_class_name(ed).as_deref() {
        Some("ReflectionFunction") => {
            let mut definitions = reflected_function_attributes(ed);
            let called_class = reflected_property(ed, "__reflection_closure_called_class")
                .and_then(|value| value.as_str().map(str::to_owned));
            rebind_attribute_evaluation_scope(&mut definitions, called_class.as_deref(), eg);
            definitions
        }
        Some("ReflectionMethod") => {
            let mut definitions = reflected_function_attributes(ed);
            let called_class = reflected_function_attribute_scope(ed);
            rebind_attribute_evaluation_scope(&mut definitions, called_class.as_deref(), eg);
            definitions
        }
        Some("ReflectionClass" | "ReflectionObject" | "ReflectionEnum") => {
            let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
                return Vec::new();
            };
            eg.find_class(&owner)
                .map(|class| class.attributes.clone())
                .unwrap_or_default()
        }
        Some("ReflectionProperty") => {
            let owner =
                reflected_property(ed, "class").and_then(|value| value.as_str().map(str::to_owned));
            let name =
                reflected_property(ed, "name").and_then(|value| value.as_str().map(str::to_owned));
            owner
                .as_deref()
                .zip(name.as_deref())
                .and_then(|(owner, name)| {
                    let class = eg.find_class(owner)?;
                    class
                        .properties
                        .iter()
                        .chain(class.static_properties.iter())
                        .find(|property| property.name == name)
                })
                .map(|property| property.attributes.clone())
                .unwrap_or_default()
        }
        Some("ReflectionClassConstant" | "ReflectionEnumUnitCase" | "ReflectionEnumBackedCase") => {
            let owner =
                reflected_property(ed, "class").and_then(|value| value.as_str().map(str::to_owned));
            let name =
                reflected_property(ed, "name").and_then(|value| value.as_str().map(str::to_owned));
            owner
                .as_deref()
                .zip(name.as_deref())
                .and_then(|(owner, name)| {
                    let class = eg.find_class(owner)?;
                    class
                        .constants
                        .iter()
                        .find(|constant| constant.name == name)
                        .map(|constant| constant.attributes.clone())
                        .or_else(|| {
                            if class.is_enum {
                                class
                                    .static_properties
                                    .iter()
                                    .find(|case| case.name == name)
                                    .map(|case| case.attributes.clone())
                            } else {
                                None
                            }
                        })
                })
                .unwrap_or_default()
        }
        Some("ReflectionConstant") => reflected_property(ed, "name")
            .and_then(|value| value.as_str().map(str::to_owned))
            .and_then(|name| eg.constant_attributes.get(&name).cloned())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

enum DeferredAttributeError {
    Message(String),
    LocatedMessage {
        message: String,
        source_file: String,
        line: usize,
    },
    TypedClassConstant(String),
    /// A nested synthetic constant-expression factory already published the
    /// exact throwable, origin and trace into ExecutorGlobals.
    PendingException,
    Vm(VmError),
}

impl DeferredAttributeError {
    fn with_location_if_missing(self, source_file: &str, line: usize) -> Self {
        if source_file.is_empty() || line == 0 {
            return self;
        }
        match self {
            Self::Message(message) => Self::LocatedMessage {
                message,
                source_file: source_file.to_string(),
                line,
            },
            located => located,
        }
    }
}

fn evaluate_runtime_callable_constant_factory(
    factory: &crate::compiler::compile::RuntimeCallableConstantFactory,
    scope: &AttributeEvaluationScope,
    eg: &mut ExecutorGlobals,
) -> Result<Value, DeferredAttributeError> {
    if let Some(value) = factory.resolved() {
        return Ok(value);
    }
    let function = eg.find_private_function(&factory.name).ok_or_else(|| {
        DeferredAttributeError::Message(
            "Constant-expression callable factory is unavailable".to_string(),
        )
    })?;
    let called_scope_class_id = scope
        .lexical_class
        .as_deref()
        .map_or(0, |class| eg.class_id_of(class));
    let value = crate::vm::execute::call_function_iter_with_context(
        eg,
        function,
        0,
        std::iter::empty::<&Value>(),
        called_scope_class_id,
        None,
        0,
        None,
    )
    .map_err(DeferredAttributeError::Vm)?;
    if eg.exception.is_some() {
        Err(DeferredAttributeError::PendingException)
    } else {
        factory.cache(value.clone());
        Ok(value)
    }
}

impl From<VmError> for DeferredAttributeError {
    fn from(error: VmError) -> Self {
        Self::Vm(error)
    }
}

fn resolve_attribute_class_name(name: &str, scope: &AttributeEvaluationScope) -> String {
    if name.eq_ignore_ascii_case("self") || name.eq_ignore_ascii_case("static") {
        return scope
            .lexical_class
            .clone()
            .unwrap_or_else(|| name.to_string());
    }
    if name.eq_ignore_ascii_case("parent") {
        return scope
            .lexical_parent
            .clone()
            .unwrap_or_else(|| name.to_string());
    }
    if let Some(relative) = name.strip_prefix("namespace\\") {
        return scope
            .namespace
            .as_ref()
            .map_or_else(|| relative.to_string(), |ns| format!("{ns}\\{relative}"));
    }
    if let Some(fully_qualified) = name.strip_prefix('\\') {
        return fully_qualified.to_string();
    }
    let first_segment = name.split('\\').next().unwrap_or(name);
    if let Some(fully_qualified) = scope
        .class_imports
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(first_segment))
        .map(|(_, target)| target)
    {
        return if name.contains('\\') {
            format!("{}{}", fully_qualified, &name[first_segment.len()..])
        } else {
            fully_qualified.clone()
        };
    }
    scope
        .namespace
        .as_ref()
        .map_or_else(|| name.to_string(), |ns| format!("{ns}\\{name}"))
}

fn resolve_attribute_constant_name(
    name: &str,
    scope: &AttributeEvaluationScope,
) -> (String, Option<String>) {
    if let Some(relative) = name.strip_prefix("namespace\\") {
        return (
            scope
                .namespace
                .as_ref()
                .map_or_else(|| relative.to_string(), |ns| format!("{ns}\\{relative}")),
            None,
        );
    }
    if let Some(fully_qualified) = name.strip_prefix('\\') {
        return (fully_qualified.to_string(), None);
    }
    if !name.contains('\\')
        && let Some(imported) = scope.constant_imports.get(name)
    {
        return (imported.clone(), None);
    }
    if let Some(namespace) = &scope.namespace {
        return (
            format!("{namespace}\\{name}"),
            (!name.contains('\\')).then(|| name.to_string()),
        );
    }
    (name.to_string(), None)
}

fn resolve_attribute_function_name(
    name: &str,
    scope: &AttributeEvaluationScope,
) -> (String, Option<String>) {
    if let Some(relative) = name.strip_prefix("namespace\\") {
        return (
            scope
                .namespace
                .as_ref()
                .map_or_else(|| relative.to_string(), |ns| format!("{ns}\\{relative}")),
            None,
        );
    }
    if let Some(fully_qualified) = name.strip_prefix('\\') {
        return (fully_qualified.to_string(), None);
    }
    if !name.contains('\\')
        && let Some(imported) = scope
            .function_imports
            .iter()
            .find(|(alias, _)| alias.eq_ignore_ascii_case(name))
            .map(|(_, target)| target)
    {
        return (imported.clone(), None);
    }
    if let Some(namespace) = &scope.namespace {
        return (
            format!("{namespace}\\{name}"),
            (!name.contains('\\')).then(|| name.to_string()),
        );
    }
    (name.to_string(), None)
}

fn render_attribute_function_name(name: &str, scope: &AttributeEvaluationScope) -> String {
    if let Some(fully_qualified) = name.strip_prefix('\\') {
        return format!("\\{fully_qualified}");
    }
    if let Some(relative) = name.strip_prefix("namespace\\") {
        return scope.namespace.as_ref().map_or_else(
            || format!("\\{relative}"),
            |namespace| format!("\\{namespace}\\{relative}"),
        );
    }
    let first_segment = name.split('\\').next().unwrap_or(name);
    if let Some(imported) = scope
        .function_imports
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(first_segment))
        .map(|(_, target)| target)
    {
        return if name.contains('\\') {
            format!("\\{}{}", imported, &name[first_segment.len()..])
        } else {
            format!("\\{imported}")
        };
    }
    scope.namespace.as_ref().map_or_else(
        || name.to_string(),
        |namespace| format!("{namespace}\\{name}"),
    )
}

fn evaluate_attribute_first_class_callable(
    callable: Value,
    fallback: Option<Value>,
    scope: &AttributeEvaluationScope,
    eg: &mut ExecutorGlobals,
) -> Result<Value, DeferredAttributeError> {
    if let Some(class_name) = callable.as_array().and_then(|array| {
        (array.len() == 2 && array.get_value_at(1).and_then(Value::as_str).is_some())
            .then(|| array.get_value_at(0).and_then(Value::as_str))
            .flatten()
    }) {
        let class_name = class_name.trim_start_matches('\\');
        if eg.find_class(class_name).is_none() {
            let _ = crate::stdlib::autoload::ensure_symbol_loaded(eg, class_name)?;
            if eg.exception.is_some() {
                return Err(DeferredAttributeError::PendingException);
            }
        }
    }

    let caller_class = scope.lexical_class.as_deref();
    let resolved = crate::stdlib::resolve_callback_with_cache(&callable, eg, caller_class, None)
        .or_else(|| {
            fallback.as_ref().and_then(|fallback| {
                crate::stdlib::resolve_callback_with_cache(fallback, eg, caller_class, None)
            })
        });
    let Some(resolved) = resolved else {
        return Err(DeferredAttributeError::Message(
            crate::stdlib::first_class_callable_error(&callable, eg, caller_class),
        ));
    };
    if resolved.is_magic_call {
        return Err(DeferredAttributeError::Message(
            "Creating a callable for the magic __callStatic() method is not supported in constant expressions"
                .to_string(),
        ));
    }
    Ok(crate::stdlib::resolved_callback_into_closure(resolved, eg))
}

fn render_attribute_static_class_name(name: &str, scope: &AttributeEvaluationScope) -> String {
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "self" | "parent" | "static"
    ) {
        return name.to_string();
    }
    format!("\\{}", resolve_attribute_class_name(name, scope))
}

fn deferred_class_constant(
    class_name: &str,
    constant: &str,
    scope: &AttributeEvaluationScope,
    eg: &mut ExecutorGlobals,
) -> Result<Value, DeferredAttributeError> {
    let source_class_name = class_name;
    let class_name = resolve_attribute_class_name(class_name, scope);
    if constant.eq_ignore_ascii_case("class") {
        let public_name = eg
            .find_class(&class_name)
            .and_then(|class| class.anonymous_public_name())
            .unwrap_or(class_name);
        return Ok(Value::string(public_name));
    }
    if eg.find_class(&class_name).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &class_name)?
    {
        return Err(DeferredAttributeError::Message(format!(
            "Class \"{class_name}\" not found"
        )));
    }

    let Some(class) = eg.find_class(&class_name) else {
        return Err(DeferredAttributeError::Message(format!(
            "Class \"{class_name}\" not found"
        )));
    };
    let pseudo_scope = matches!(
        source_class_name.to_ascii_lowercase().as_str(),
        "self" | "parent" | "static"
    );
    if class.is_trait && !pseudo_scope {
        return Err(DeferredAttributeError::Message(format!(
            "Cannot access trait constant {}::{constant} directly",
            class.name
        )));
    }
    let display_class = class.name.clone();
    let definition = class
        .constants
        .iter()
        .find(|definition| definition.name == constant)
        .cloned();
    let enum_value = if definition.is_none() && class.is_enum {
        class
            .static_properties
            .iter()
            .position(|case| case.name == constant)
            .and_then(|index| eg.static_property_storage_slot(class.class_id, index))
            .and_then(|slot| eg.static_property_value(slot))
            .cloned()
    } else {
        None
    };
    if let Some(value) = enum_value {
        return Ok(value);
    }
    let Some(definition) = definition else {
        let display_class = if pseudo_scope {
            source_class_name
        } else {
            &display_class
        };
        return Err(DeferredAttributeError::Message(format!(
            "Undefined constant {display_class}::{constant}"
        )));
    };
    if !eg.check_visibility(
        scope.lexical_class.as_deref(),
        &definition.declaring_class,
        definition.visibility,
    ) {
        let visibility = match definition.visibility {
            Visibility::Private => "private",
            Visibility::Protected => "protected",
            Visibility::Public => unreachable!(),
        };
        return Err(DeferredAttributeError::Message(format!(
            "Cannot access {visibility} constant {display_class}::{constant}"
        )));
    }
    if let Some(error) = definition.evaluation_error {
        return Err(DeferredAttributeError::Message(error));
    }
    if definition.value_is_deferred {
        return evaluate_deferred_class_constant_definition(&definition, eg);
    }
    Ok(definition.value)
}

fn evaluate_deferred_attribute_expression(
    expression: &Expr,
    scope: &AttributeEvaluationScope,
    source_file: &str,
    eg: &mut ExecutorGlobals,
) -> Result<Value, DeferredAttributeError> {
    match expression {
        Expr::Integer(value) => Ok(Value::long(*value)),
        Expr::Float(value) => Ok(Value::double(*value)),
        Expr::StringLiteral(value) => Ok(Value::string(value.clone())),
        Expr::BinaryStringLiteral(value) => Ok(Value::binary_string_from_storage(value.clone())),
        Expr::Bool(value) => Ok(Value::bool(*value)),
        Expr::Null => Ok(Value::null()),
        Expr::Constant { name, .. } => {
            let (primary, fallback) = resolve_attribute_constant_name(name, scope);
            eg.find_constant(&primary)
                .or_else(|| fallback.as_deref().and_then(|name| eg.find_constant(name)))
                .ok_or_else(|| {
                    DeferredAttributeError::Message(format!("Undefined constant \"{primary}\""))
                })
        }
        Expr::MagicConstant { name, line } if name.eq_ignore_ascii_case("__LINE__") => {
            Ok(Value::long(*line as i64))
        }
        Expr::MagicConstant { name, .. } if name.eq_ignore_ascii_case("__FILE__") => {
            Ok(Value::string(source_file))
        }
        Expr::MagicConstant { name, .. } if name.eq_ignore_ascii_case("__DIR__") => {
            Ok(Value::string(scope.source_directory.clone()))
        }
        Expr::MagicConstant { name, .. } if name.eq_ignore_ascii_case("__CLASS__") => Ok(
            Value::string(scope.lexical_class.clone().unwrap_or_default()),
        ),
        Expr::MagicConstant { name, .. } if name.eq_ignore_ascii_case("__PROPERTY__") => Ok(
            Value::string(scope.lexical_property.clone().unwrap_or_default()),
        ),
        Expr::ClassConstant {
            class_name,
            constant,
            line,
        } => deferred_class_constant(class_name, constant, scope, eg)
            .map_err(|error| error.with_location_if_missing(source_file, *line)),
        Expr::FirstClassFunctionCallable { name, .. } => {
            let (primary, fallback) = resolve_attribute_function_name(name, scope);
            evaluate_attribute_first_class_callable(
                Value::string(primary),
                fallback.map(Value::string),
                scope,
                eg,
            )
        }
        Expr::FirstClassCallable { callable, .. } => {
            let callable =
                evaluate_deferred_attribute_expression(callable, scope, source_file, eg)?;
            evaluate_attribute_first_class_callable(callable, None, scope, eg)
        }
        Expr::DynamicNamedClassConstant {
            class_name,
            constant,
        } => {
            let constant =
                evaluate_deferred_attribute_expression(constant, scope, source_file, eg)?;
            let Some(constant) = constant.as_str() else {
                return Err(DeferredAttributeError::Message(format!(
                    "Cannot use value of type {} as class constant name",
                    constant.type_name()
                )));
            };
            deferred_class_constant(class_name, constant, scope, eg)
        }
        Expr::DynamicClassConstant {
            class, constant, ..
        } => {
            let class = evaluate_deferred_attribute_expression(class, scope, source_file, eg)?;
            let Some(class) = class.as_str() else {
                return Err(DeferredAttributeError::Message(format!(
                    "Cannot use value of type {} as class name",
                    class.type_name()
                )));
            };
            let constant =
                evaluate_deferred_attribute_expression(constant, scope, source_file, eg)?;
            let Some(constant) = constant.as_str() else {
                return Err(DeferredAttributeError::Message(format!(
                    "Cannot use value of type {} as class constant name",
                    constant.type_name()
                )));
            };
            let dynamic_scope = AttributeEvaluationScope {
                namespace: None,
                class_imports: HashMap::new(),
                function_imports: HashMap::new(),
                constant_imports: HashMap::new(),
                lexical_class: scope.lexical_class.clone(),
                lexical_parent: scope.lexical_parent.clone(),
                lexical_property: scope.lexical_property.clone(),
                source_directory: scope.source_directory.clone(),
            };
            deferred_class_constant(class, constant, &dynamic_scope, eg)
        }
        Expr::BinaryOp {
            op, left, right, ..
        } => {
            let left = evaluate_deferred_attribute_expression(left, scope, source_file, eg)?;
            match op {
                crate::parser::BinOp::And if !left.is_truthy() => {
                    return Ok(Value::bool(false));
                }
                crate::parser::BinOp::Or if left.is_truthy() => {
                    return Ok(Value::bool(true));
                }
                _ => {}
            }
            let right = evaluate_deferred_attribute_expression(right, scope, source_file, eg)?;
            Compiler::eval_const_binary(*op, &left, &right).map_err(DeferredAttributeError::Message)
        }
        Expr::Not(inner) => Ok(Value::bool(
            !evaluate_deferred_attribute_expression(inner, scope, source_file, eg)?.is_truthy(),
        )),
        Expr::UnaryPlus(inner) => {
            let value = evaluate_deferred_attribute_expression(inner, scope, source_file, eg)?;
            if let Some(value) = value.as_long() {
                Ok(Value::long(value))
            } else if let Some(value) = value.as_double() {
                Ok(Value::double(value))
            } else {
                Err(DeferredAttributeError::Message(
                    "unsupported unary expression".to_string(),
                ))
            }
        }
        Expr::UnaryMinus(inner) => {
            let value = evaluate_deferred_attribute_expression(inner, scope, source_file, eg)?;
            if let Some(value) = value.as_long() {
                Ok(value
                    .checked_neg()
                    .map(Value::long)
                    .unwrap_or_else(|| Value::double(-(value as f64))))
            } else if let Some(value) = value.as_double() {
                Ok(Value::double(-value))
            } else {
                Err(DeferredAttributeError::Message(
                    "unsupported unary expression".to_string(),
                ))
            }
        }
        Expr::BitwiseNot { expr: inner, .. } => {
            let value = evaluate_deferred_attribute_expression(inner, scope, source_file, eg)?;
            if let Some(value) = value.as_long() {
                Ok(Value::long(!value))
            } else if let Some(value) = value.as_str() {
                Ok(Value::string(crate::value::php_byte_string_from_bytes(
                    value.bytes().map(|byte| !byte),
                )))
            } else {
                Err(DeferredAttributeError::Message(format!(
                    "Cannot perform bitwise not on {}",
                    value.type_name()
                )))
            }
        }
        Expr::Ternary {
            condition,
            then_expr,
            else_expr,
        } => {
            if evaluate_deferred_attribute_expression(condition, scope, source_file, eg)?
                .is_truthy()
            {
                evaluate_deferred_attribute_expression(then_expr, scope, source_file, eg)
            } else {
                evaluate_deferred_attribute_expression(else_expr, scope, source_file, eg)
            }
        }
        Expr::Elvis { left, right } => {
            let left = evaluate_deferred_attribute_expression(left, scope, source_file, eg)?;
            if left.is_truthy() {
                Ok(left)
            } else {
                evaluate_deferred_attribute_expression(right, scope, source_file, eg)
            }
        }
        Expr::NullCoalesce { left, right } => {
            let left = evaluate_deferred_attribute_expression(left, scope, source_file, eg)?;
            if left.value_type() == ValueType::Null {
                evaluate_deferred_attribute_expression(right, scope, source_file, eg)
            } else {
                Ok(left)
            }
        }
        Expr::PropertyAccess {
            object,
            property,
            nullsafe,
            line,
        } => {
            let receiver = evaluate_deferred_attribute_expression(object, scope, source_file, eg)?;
            if *nullsafe && receiver.value_type() == ValueType::Null {
                return Ok(Value::null());
            }
            let object = receiver.as_object().ok_or_else(|| {
                DeferredAttributeError::Message(format!(
                    "Attempt to read property \"{property}\" on {}",
                    receiver.type_name()
                ))
            })?;
            if !eg
                .find_class(&object.class_name)
                .is_some_and(|class| class.is_enum)
            {
                return Err(DeferredAttributeError::Message(
                    "Fetching properties on non-enums in constant expressions is not allowed"
                        .to_string(),
                )
                .with_location_if_missing(source_file, *line));
            }
            Ok(object
                .get_property(property)
                .cloned()
                .unwrap_or_else(Value::null))
        }
        Expr::DynamicPropertyAccess {
            object,
            property,
            nullsafe,
            line,
        } => {
            let receiver = evaluate_deferred_attribute_expression(object, scope, source_file, eg)?;
            if *nullsafe && receiver.value_type() == ValueType::Null {
                return Ok(Value::null());
            }
            let property =
                evaluate_deferred_attribute_expression(property, scope, source_file, eg)?;
            let Some(property) = property.as_str() else {
                return Err(DeferredAttributeError::Message(format!(
                    "Cannot use value of type {} as a property name",
                    property.type_name()
                )));
            };
            let object = receiver.as_object().ok_or_else(|| {
                DeferredAttributeError::Message(format!(
                    "Attempt to read property \"{property}\" on {}",
                    receiver.type_name()
                ))
            })?;
            if !eg
                .find_class(&object.class_name)
                .is_some_and(|class| class.is_enum)
            {
                return Err(DeferredAttributeError::Message(
                    "Fetching properties on non-enums in constant expressions is not allowed"
                        .to_string(),
                )
                .with_location_if_missing(source_file, *line));
            }
            Ok(object
                .get_property(property)
                .cloned()
                .unwrap_or_else(Value::null))
        }
        Expr::New {
            class_name,
            args,
            generic_args,
            ..
        } if class_name.eq_ignore_ascii_case("stdClass")
            && args.is_empty()
            && generic_args.is_empty() =>
        {
            Ok(Value::object(PhpObject::dynamic(
                "stdClass".into(),
                0,
                HashMap::new(),
            )))
        }
        Expr::ArrayLiteral(elements) => {
            let mut result = PhpArray::new();
            for element in elements {
                let value =
                    evaluate_deferred_attribute_expression(&element.value, scope, source_file, eg)?;
                if element.unpack {
                    let Some(source) = value.as_array() else {
                        return Err(DeferredAttributeError::Message(
                            "Only arrays and Traversables can be unpacked".to_string(),
                        ));
                    };
                    for (key, value) in source.iter() {
                        match key {
                            ArrayKey::Int(_) => {
                                if !result.try_push(value.dereferenced().clone()) {
                                    return Err(DeferredAttributeError::Message(
                                        "Cannot add element to the array as the next element is already occupied"
                                            .to_string(),
                                    ));
                                }
                            }
                            ArrayKey::String(key) => {
                                result.set_str(&key, value.dereferenced().clone());
                            }
                        }
                    }
                    continue;
                }
                if let Some(key) = &element.key {
                    let key = evaluate_deferred_attribute_expression(key, scope, source_file, eg)?;
                    if let Some(key) = key.as_long() {
                        result.set_int(key, value);
                    } else if let Some(key) = key.as_str() {
                        result.set_str(key, value);
                    } else {
                        return Err(DeferredAttributeError::Message(
                            "unsupported array key type in constant expression".to_string(),
                        ));
                    }
                } else {
                    result.push(value);
                }
            }
            Ok(Value::array(result))
        }
        Expr::ArrayAccess { array, index, line } => {
            let array = evaluate_deferred_attribute_expression(array, scope, source_file, eg)?;
            if matches!(array.value_type(), ValueType::Object | ValueType::Closure) {
                return Err(DeferredAttributeError::Message(
                    crate::compiler::compile::OBJECT_OFFSET_CONSTANT_EXPRESSION_ERROR.to_string(),
                )
                .with_location_if_missing(source_file, *line));
            }
            let index = evaluate_deferred_attribute_expression(index, scope, source_file, eg)?;
            let Some(array) = array.as_array() else {
                return Err(DeferredAttributeError::Message(
                    "constant expression cannot index a non-array".to_string(),
                ));
            };
            let value = if let Some(index) = index.as_long() {
                array.get_int(index)
            } else if let Some(index) = index.as_str() {
                array.get_str(index)
            } else if matches!(index.value_type(), ValueType::True | ValueType::False) {
                array.get_int(i64::from(index.is_truthy()))
            } else if index.value_type() == ValueType::Null {
                array.get_str("")
            } else {
                None
            };
            value.cloned().ok_or_else(|| {
                DeferredAttributeError::Message(
                    "undefined array key in constant expression".to_string(),
                )
            })
        }
        _ => Err(DeferredAttributeError::Message(format!(
            "expression {expression:?} is not a constant expression"
        ))),
    }
}

#[derive(Clone)]
pub(crate) struct DeprecatedUseSite {
    pub frame: *mut ExecuteData,
    pub file: String,
    pub line: usize,
}

fn deprecated_attribute(attributes: &[AttributeDefinition]) -> Option<(AttributeDefinition, bool)> {
    let mut definitions = attributes
        .iter()
        .filter(|attribute| attribute.name.eq_ignore_ascii_case("Deprecated"));
    definitions
        .next()
        .cloned()
        .map(|definition| (definition, definitions.next().is_some()))
}

fn emit_deprecated_symbol_diagnostic(
    attributes: &[AttributeDefinition],
    diagnostic_prefix: &str,
    use_site: &DeprecatedUseSite,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((definition, repeated)) = deprecated_attribute(attributes) else {
        return Ok(());
    };
    let mut instance = Value::undef();
    instantiate_attribute_definition_at_use(
        use_site.frame,
        &mut instance,
        &definition,
        repeated,
        eg,
        Some(use_site),
        None,
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
    let mut diagnostic = format!("{diagnostic_prefix} is deprecated");
    if let Some(since) = since {
        diagnostic.push_str(" since ");
        diagnostic.push_str(&since);
    }
    if let Some(message) = message {
        diagnostic.push_str(", ");
        diagnostic.push_str(&message);
    }
    let handled = crate::stdlib::dispatch_php_error(
        eg,
        use_site.frame,
        16_384,
        &diagnostic,
        &use_site.file,
        use_site.line,
    )?;
    if !handled {
        eg.record_last_error(16_384, &diagnostic, &use_site.file, use_site.line);
    }
    if !handled && eg.error_reporting & 16_384 != 0 {
        eg.write_output(
            format!(
                "\nDeprecated: {diagnostic} in {} on line {}\n",
                use_site.file, use_site.line
            )
            .as_bytes(),
        );
    }
    Ok(())
}

fn guarded_deprecated_symbol(
    identity: String,
    eg: &mut ExecutorGlobals,
    report: impl FnOnce(&mut ExecutorGlobals) -> Result<(), VmError>,
) -> Result<(), VmError> {
    if eg
        .deprecated_symbol_stack
        .iter()
        .any(|active| active.eq_ignore_ascii_case(&identity))
    {
        return Ok(());
    }
    eg.deprecated_symbol_stack.push(identity);
    let result = report(eg);
    eg.deprecated_symbol_stack.pop();
    result
}

pub(crate) fn report_deprecated_global_constant_use(
    name: &str,
    use_site: &DeprecatedUseSite,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if name == "E_STRICT" {
        let identity = "constant:E_STRICT".to_string();
        return guarded_deprecated_symbol(identity, eg, |eg| {
            let message = "Constant E_STRICT is deprecated since 8.4, the error level was removed";
            let handled = crate::stdlib::dispatch_php_error(
                eg,
                use_site.frame,
                8_192,
                message,
                &use_site.file,
                use_site.line,
            )?;
            if !handled {
                eg.record_last_error(8_192, message, &use_site.file, use_site.line);
            }
            if !handled && eg.error_reporting & 8_192 != 0 {
                eg.write_output(
                    format!(
                        "\nDeprecated: {message} in {} on line {}\n",
                        use_site.file, use_site.line
                    )
                    .as_bytes(),
                );
            }
            Ok(())
        });
    }
    let attributes = eg
        .constant_attributes
        .get(name)
        .cloned()
        .unwrap_or_default();
    let expression = eg.constant_expressions.get(name).cloned();
    if attributes.is_empty() && expression.is_none() {
        return Ok(());
    }
    let identity = format!("constant:{name}");
    guarded_deprecated_symbol(identity, eg, |eg| {
        if let Some(expression) = &expression {
            report_deprecated_expression_references(
                &expression.expression,
                &expression.evaluation_scope,
                &expression.source_file,
                use_site,
                eg,
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
        }
        emit_deprecated_symbol_diagnostic(&attributes, &format!("Constant {name}"), use_site, eg)
    })
}

pub(crate) fn report_deprecated_class_constant_use(
    display_class: &str,
    definition: &ClassConstantDefinition,
    use_site: &DeprecatedUseSite,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let identity = format!(
        "class-constant:{}::{}",
        definition.declaring_class, definition.name
    );
    let definition = definition.clone();
    guarded_deprecated_symbol(identity, eg, |eg| {
        if let (Some(expression), Some(scope)) =
            (&definition.source_expression, &definition.evaluation_scope)
        {
            report_deprecated_expression_references(
                expression,
                scope,
                definition.source_file(),
                use_site,
                eg,
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
        }
        emit_deprecated_symbol_diagnostic(
            &definition.attributes,
            &format!("Constant {display_class}::{}", definition.name),
            use_site,
            eg,
        )
    })
}

pub(crate) fn report_deprecated_enum_case_use(
    class_name: &str,
    case: &PropertyDefinition,
    use_site: &DeprecatedUseSite,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let attributes = case.attributes.clone();
    let case_name = case.name.clone();
    guarded_deprecated_symbol(format!("enum-case:{class_name}::{case_name}"), eg, |eg| {
        emit_deprecated_symbol_diagnostic(
            &attributes,
            &format!("Enum case {class_name}::{case_name}"),
            use_site,
            eg,
        )
    })
}

pub(crate) fn report_deprecated_trait_use(
    trait_name: &str,
    consumer_name: &str,
    attributes: &[AttributeDefinition],
    use_site: &DeprecatedUseSite,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let attributes = attributes.to_vec();
    guarded_deprecated_symbol(format!("trait:{trait_name}"), eg, |eg| {
        emit_deprecated_symbol_diagnostic(
            &attributes,
            &format!("Trait {trait_name} used by {consumer_name}"),
            use_site,
            eg,
        )
    })
}

pub(crate) fn evaluate_deferred_class_constant_value(
    definition: &ClassConstantDefinition,
    eg: &mut ExecutorGlobals,
) -> Result<Option<Value>, VmError> {
    match evaluate_deferred_class_constant_definition(definition, eg) {
        Ok(value) => Ok(Some(value)),
        Err(DeferredAttributeError::Message(error)) => {
            eg.exception = Some(make_error_value("Error", &error));
            Ok(None)
        }
        Err(DeferredAttributeError::LocatedMessage {
            message,
            source_file,
            line,
        }) => {
            let error = make_error_value("Error", &message);
            if let Some(mut object) = error.as_object_mut() {
                object.set_property("file", Value::string(source_file));
                object.set_property("line", Value::long(line as i64));
            }
            eg.exception = Some(error);
            Ok(None)
        }
        Err(DeferredAttributeError::TypedClassConstant(error)) => {
            eg.exception = Some(make_error_value("TypeError", &error));
            Ok(None)
        }
        Err(DeferredAttributeError::PendingException) => Ok(None),
        Err(DeferredAttributeError::Vm(error)) => Err(error),
    }
}

pub(crate) fn class_constant_evaluation_error_value(
    definition: &ClassConstantDefinition,
) -> Option<Value> {
    let message = definition.evaluation_error.as_deref()?;
    let error = make_error_value("Error", message);
    if let Some(line) = definition.evaluation_error_line()
        && !definition.source_file().is_empty()
        && let Some(mut object) = error.as_object_mut()
    {
        object.set_property("file", Value::string(definition.source_file()));
        object.set_property("line", Value::long(line as i64));
    }
    Some(error)
}

pub(crate) fn activate_deferred_class_constants(
    class_id: u32,
    eg: &mut ExecutorGlobals,
) -> Result<bool, VmError> {
    if !eg.deferred_class_constants_require_activation(class_id) {
        return Ok(true);
    }
    let definitions = eg
        .class_by_id(class_id)
        .map(|class| {
            class
                .constants
                .iter()
                .filter(|constant| constant.value_is_deferred)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for definition in definitions {
        if evaluate_deferred_class_constant_value(&definition, eg)?.is_none() {
            return Ok(false);
        }
    }
    eg.complete_deferred_class_constant_activation(class_id);
    Ok(true)
}

fn normalize_deferred_class_constant_value(
    value: Value,
    hint: &ParamTypeHint,
    declaring_class: &str,
    eg: &ExecutorGlobals,
) -> Result<Value, Value> {
    if crate::vm::execute::check_type_hint(&value, hint, eg, true, Some(declaring_class)) {
        return Ok(value);
    }
    match hint {
        ParamTypeHint::Float if value.value_type() == ValueType::Long => Ok(Value::double(
            value
                .as_long()
                .expect("checked deferred class-constant integer") as f64,
        )),
        ParamTypeHint::Nullable(inner) => {
            normalize_deferred_class_constant_value(value, inner, declaring_class, eg)
        }
        ParamTypeHint::Union(parts) => {
            for part in parts {
                if let Ok(value) = normalize_deferred_class_constant_value(
                    value.clone(),
                    part,
                    declaring_class,
                    eg,
                ) {
                    return Ok(value);
                }
            }
            Err(value)
        }
        _ => Err(value),
    }
}

fn evaluate_deferred_class_constant_definition(
    definition: &ClassConstantDefinition,
    eg: &mut ExecutorGlobals,
) -> Result<Value, DeferredAttributeError> {
    let (Some(expression), Some(scope)) =
        (&definition.source_expression, &definition.evaluation_scope)
    else {
        return Ok(definition.value.clone());
    };
    let value = if let Some(factory) = &definition.callable_factory {
        evaluate_runtime_callable_constant_factory(factory, scope, eg)?
    } else {
        evaluate_deferred_attribute_expression(expression, scope, definition.source_file(), eg)?
    };
    normalize_deferred_class_constant_value(
        value,
        &definition.type_hint,
        &definition.declaring_class,
        eg,
    )
    .map_err(|value| {
        DeferredAttributeError::TypedClassConstant(format!(
            "Cannot assign {} to class constant {}::{} of type {}",
            value.diagnostic_type_name(),
            definition.declaring_class,
            definition.name,
            definition.type_hint.display_name()
        ))
    })
}

pub(crate) fn evaluate_deferred_property_default_value(
    definition: &crate::compiler::compile::DeferredPropertyDefault,
    eg: &mut ExecutorGlobals,
) -> Result<Option<Value>, VmError> {
    let evaluated = if let Some(factory) = &definition.callable_factory {
        evaluate_runtime_callable_constant_factory(factory, &definition.evaluation_scope, eg)
    } else {
        evaluate_deferred_attribute_expression(
            &definition.expression,
            &definition.evaluation_scope,
            &definition.source_file,
            eg,
        )
    };
    match evaluated {
        Ok(value) => Ok(Some(value)),
        Err(DeferredAttributeError::Message(error)) => {
            // Autoload may already have raised a user exception. Preserve that
            // object and its origin; synthesize Error only for an ordinary
            // unresolved constant-expression dependency.
            if eg.exception.is_none() {
                eg.exception = Some(make_error_value("Error", &error));
            }
            Ok(None)
        }
        Err(DeferredAttributeError::LocatedMessage { message, .. }) => {
            if eg.exception.is_none() {
                eg.exception = Some(make_error_value("Error", &message));
            }
            Ok(None)
        }
        Err(DeferredAttributeError::TypedClassConstant(error)) => {
            if eg.exception.is_none() {
                eg.exception = Some(make_error_value("TypeError", &error));
            }
            Ok(None)
        }
        Err(DeferredAttributeError::PendingException) => Ok(None),
        Err(DeferredAttributeError::Vm(error)) => Err(error),
    }
}

fn report_deprecated_expression_references(
    expression: &Expr,
    scope: &AttributeEvaluationScope,
    source_file: &str,
    use_site: &DeprecatedUseSite,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    match expression {
        Expr::Constant { name, .. } => {
            let (primary, fallback) = resolve_attribute_constant_name(name, scope);
            let resolved = if eg.find_constant(&primary).is_some() {
                Some(primary)
            } else {
                fallback.filter(|name| eg.find_constant(name).is_some())
            };
            if let Some(name) = resolved {
                report_deprecated_global_constant_use(&name, use_site, eg)?;
            }
        }
        Expr::ClassConstant {
            class_name,
            constant,
            ..
        } => {
            let class_name = resolve_attribute_class_name(class_name, scope);
            let resolved = eg.find_class(&class_name).map(|class| {
                let definition = class
                    .constants
                    .iter()
                    .find(|definition| definition.name == *constant)
                    .cloned();
                let case = (definition.is_none() && class.is_enum)
                    .then(|| {
                        class
                            .static_properties
                            .iter()
                            .find(|case| case.name == *constant)
                            .cloned()
                    })
                    .flatten();
                (class.name.clone(), definition, case)
            });
            if let Some((class_name, definition, case)) = resolved {
                if let Some(definition) = definition {
                    report_deprecated_class_constant_use(&class_name, &definition, use_site, eg)?;
                } else if let Some(case) = case {
                    report_deprecated_enum_case_use(&class_name, &case, use_site, eg)?;
                }
            }
        }
        Expr::DynamicNamedClassConstant {
            class_name,
            constant,
        } => {
            let value = evaluate_deferred_attribute_expression(constant, scope, source_file, eg);
            if let Ok(value) = value
                && let Some(constant) = value.as_str()
            {
                report_deprecated_expression_references(
                    &Expr::ClassConstant {
                        class_name: class_name.clone(),
                        constant: constant.to_string(),
                        line: 0,
                    },
                    scope,
                    source_file,
                    use_site,
                    eg,
                )?;
            }
        }
        Expr::DynamicClassConstant {
            class, constant, ..
        } => {
            let class = evaluate_deferred_attribute_expression(class, scope, source_file, eg);
            let constant = evaluate_deferred_attribute_expression(constant, scope, source_file, eg);
            if let (Ok(class), Ok(constant)) = (class, constant)
                && let (Some(class), Some(constant)) = (class.as_str(), constant.as_str())
            {
                let dynamic_scope = AttributeEvaluationScope {
                    namespace: None,
                    class_imports: HashMap::new(),
                    function_imports: HashMap::new(),
                    constant_imports: HashMap::new(),
                    lexical_class: scope.lexical_class.clone(),
                    lexical_parent: scope.lexical_parent.clone(),
                    lexical_property: scope.lexical_property.clone(),
                    source_directory: scope.source_directory.clone(),
                };
                report_deprecated_expression_references(
                    &Expr::ClassConstant {
                        class_name: class.to_string(),
                        constant: constant.to_string(),
                        line: 0,
                    },
                    &dynamic_scope,
                    source_file,
                    use_site,
                    eg,
                )?;
            }
        }
        Expr::BinaryOp {
            op, left, right, ..
        } => {
            report_deprecated_expression_references(left, scope, source_file, use_site, eg)?;
            if eg.exception.is_some() {
                return Ok(());
            }
            let skip_right = match op {
                crate::parser::BinOp::And => {
                    evaluate_deferred_attribute_expression(left, scope, source_file, eg)
                        .is_ok_and(|value| !value.is_truthy())
                }
                crate::parser::BinOp::Or => {
                    evaluate_deferred_attribute_expression(left, scope, source_file, eg)
                        .is_ok_and(|value| value.is_truthy())
                }
                _ => false,
            };
            if !skip_right {
                report_deprecated_expression_references(right, scope, source_file, use_site, eg)?;
            }
        }
        Expr::Not(inner)
        | Expr::UnaryPlus(inner)
        | Expr::UnaryMinus(inner)
        | Expr::BitwiseNot { expr: inner, .. }
        | Expr::ErrorSuppress(inner)
        | Expr::Cast { expr: inner, .. } => {
            report_deprecated_expression_references(inner, scope, source_file, use_site, eg)?;
        }
        Expr::Elvis { left, right } | Expr::NullCoalesce { left, right } => {
            report_deprecated_expression_references(left, scope, source_file, use_site, eg)?;
            if eg.exception.is_some() {
                return Ok(());
            }
            let left_value =
                evaluate_deferred_attribute_expression(left, scope, source_file, eg).ok();
            let skip_right = match expression {
                Expr::Elvis { .. } => left_value.is_some_and(|value| value.is_truthy()),
                Expr::NullCoalesce { .. } => {
                    left_value.is_some_and(|value| value.value_type() != ValueType::Null)
                }
                _ => unreachable!(),
            };
            if !skip_right {
                report_deprecated_expression_references(right, scope, source_file, use_site, eg)?;
            }
        }
        Expr::Ternary {
            condition,
            then_expr,
            else_expr,
        } => {
            report_deprecated_expression_references(condition, scope, source_file, use_site, eg)?;
            if eg.exception.is_some() {
                return Ok(());
            }
            let selected =
                evaluate_deferred_attribute_expression(condition, scope, source_file, eg)
                    .ok()
                    .is_some_and(|value| value.is_truthy());
            report_deprecated_expression_references(
                if selected { then_expr } else { else_expr },
                scope,
                source_file,
                use_site,
                eg,
            )?;
        }
        Expr::ArrayLiteral(elements) => {
            for element in elements {
                if let Some(key) = &element.key {
                    report_deprecated_expression_references(key, scope, source_file, use_site, eg)?;
                }
                report_deprecated_expression_references(
                    &element.value,
                    scope,
                    source_file,
                    use_site,
                    eg,
                )?;
                if eg.exception.is_some() {
                    break;
                }
            }
        }
        Expr::ArrayAccess { array, index, .. } => {
            report_deprecated_expression_references(array, scope, source_file, use_site, eg)?;
            if eg.exception.is_none() {
                report_deprecated_expression_references(index, scope, source_file, use_site, eg)?;
            }
        }
        Expr::PropertyAccess { object, .. } => {
            report_deprecated_expression_references(object, scope, source_file, use_site, eg)?;
        }
        Expr::DynamicPropertyAccess {
            object,
            property,
            nullsafe,
            ..
        } => {
            report_deprecated_expression_references(object, scope, source_file, use_site, eg)?;
            if eg.exception.is_none()
                && (!*nullsafe
                    || evaluate_deferred_attribute_expression(object, scope, source_file, eg)
                        .is_ok_and(|value| value.value_type() != ValueType::Null))
            {
                report_deprecated_expression_references(
                    property,
                    scope,
                    source_file,
                    use_site,
                    eg,
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn evaluate_attribute_arguments(
    definition: &AttributeDefinition,
    eg: &mut ExecutorGlobals,
    deprecated_use_site: Option<&DeprecatedUseSite>,
) -> Result<Option<Value>, VmError> {
    let mut arguments = PhpArray::with_packed_capacity(definition.arguments.len());
    for argument in &definition.arguments {
        let value = match (&argument.value, &argument.deferred_expression) {
            (_, Some(expression)) => {
                if let Some(use_site) = deprecated_use_site {
                    report_deprecated_expression_references(
                        expression,
                        &definition.evaluation_scope,
                        &definition.source_file,
                        use_site,
                        eg,
                    )?;
                    if eg.exception.is_some() {
                        return Ok(None);
                    }
                }
                match evaluate_deferred_attribute_expression(
                    expression,
                    &definition.evaluation_scope,
                    &definition.source_file,
                    eg,
                ) {
                    Ok(value) => value,
                    Err(DeferredAttributeError::Message(error)) => {
                        eg.exception = Some(make_error_value("Error", &error));
                        return Ok(None);
                    }
                    Err(DeferredAttributeError::LocatedMessage { message, .. }) => {
                        eg.exception = Some(make_error_value("Error", &message));
                        return Ok(None);
                    }
                    Err(DeferredAttributeError::TypedClassConstant(error)) => {
                        eg.exception = Some(make_error_value("TypeError", &error));
                        return Ok(None);
                    }
                    Err(DeferredAttributeError::PendingException) => return Ok(None),
                    Err(DeferredAttributeError::Vm(error)) => return Err(error),
                }
            }
            (Ok(value), None) => value.clone(),
            (Err(error), None) => {
                eg.exception = Some(make_error_value("Error", error));
                return Ok(None);
            }
        };
        if let Some(name) = &argument.name {
            arguments.set_str(name, value);
        } else {
            arguments.push(value);
        }
    }
    Ok(Some(Value::array(arguments)))
}

fn reflection_attribute_value(
    definition: &AttributeDefinition,
    repeated: bool,
    declaration: &ReflectionAttributeDeclaration,
    eg: &mut ExecutorGlobals,
) -> Value {
    let object = object_value(
        "ReflectionAttribute",
        [("name", Value::string(definition.name.clone()))],
    );
    eg.register_reflection_attribute(&object, definition.clone(), repeated, declaration.clone());
    object
}

fn reflected_attribute_declaration(ed: *mut ExecuteData) -> ReflectionAttributeDeclaration {
    with_argument(ed, 0, |receiver| {
        let Some(object) = receiver.as_object() else {
            return ReflectionAttributeDeclaration {
                name: Value::null(),
                class_name: None,
                kind: ReflectionAttributeDeclarationKind::Plain,
            };
        };
        let name = object
            .get_property("name")
            .cloned()
            .unwrap_or_else(Value::null);
        let (class_property, kind) = match object.class_name.as_ref() {
            "ReflectionMethod" => (
                Some("__reflection_method_class"),
                ReflectionAttributeDeclarationKind::Method,
            ),
            "ReflectionProperty" => (Some("class"), ReflectionAttributeDeclarationKind::Property),
            "ReflectionClassConstant" | "ReflectionEnumUnitCase" | "ReflectionEnumBackedCase" => (
                Some("class"),
                ReflectionAttributeDeclarationKind::ClassConstant,
            ),
            _ => (None, ReflectionAttributeDeclarationKind::Plain),
        };
        ReflectionAttributeDeclaration {
            name,
            class_name: class_property.and_then(|property| object.get_property(property).cloned()),
            kind,
        }
    })
}

fn reflected_attribute_declaration_name(declaration: &ReflectionAttributeDeclaration) -> String {
    let name = declaration.name.as_str().unwrap_or_default();
    let Some(class) = declaration.class_name.as_ref().and_then(Value::as_str) else {
        return name.to_string();
    };
    match declaration.kind {
        ReflectionAttributeDeclarationKind::Plain => name.to_string(),
        ReflectionAttributeDeclarationKind::Method
        | ReflectionAttributeDeclarationKind::ClassConstant => format!("{class}::{name}"),
        ReflectionAttributeDeclarationKind::Property => format!("{class}::${name}"),
    }
}

fn reflection_attributes(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    definitions: Vec<AttributeDefinition>,
) -> Result<(), VmError> {
    let filter = with_argument(ed, 1, |value| value.as_str().map(str::to_owned));
    let flags = with_argument(ed, 2, Value::as_long).unwrap_or(0);
    if flags != 0 && flags != 2 {
        let owner = match receiver_class_name(ed).as_deref() {
            Some("ReflectionFunction" | "ReflectionMethod") => {
                "ReflectionFunctionAbstract".to_string()
            }
            Some(owner) => owner.to_string(),
            None => "ReflectionClass".to_string(),
        };
        eg.exception = Some(make_error_value(
            "ValueError",
            &format!(
                "{owner}::getAttributes(): Argument #2 ($flags) must be a valid attribute filter flag"
            ),
        ));
        return Ok(());
    }
    if flags == 2 {
        let Some(filter_name) = filter.as_deref() else {
            eg.exception = Some(make_error_value(
                "ValueError",
                "ReflectionFunctionAbstract::getAttributes(): Argument #1 ($name) must be provided when using ReflectionAttribute::IS_INSTANCEOF",
            ));
            return Ok(());
        };
        if eg.find_class(filter_name).is_none()
            && !crate::stdlib::autoload::ensure_symbol_loaded(eg, filter_name)?
        {
            eg.exception = Some(make_error_value(
                "Error",
                &format!("Class \"{filter_name}\" not found"),
            ));
            return Ok(());
        }
    }

    let mut counts = HashMap::<String, usize>::new();
    for definition in &definitions {
        *counts
            .entry(definition.name.to_ascii_lowercase())
            .or_default() += 1;
    }
    let declaration = reflected_attribute_declaration(ed);
    let mut result = PhpArray::with_packed_capacity(definitions.len());
    for definition in &definitions {
        let matches = match filter.as_deref() {
            None => true,
            Some(name) if flags == 0 => definition.name.eq_ignore_ascii_case(name),
            Some(name) => eg.class_is_a(&definition.name, name),
        };
        if !matches {
            continue;
        }
        let repeated = counts
            .get(&definition.name.to_ascii_lowercase())
            .copied()
            .unwrap_or(0)
            > 1;
        result.push(reflection_attribute_value(
            definition,
            repeated,
            &declaration,
            eg,
        ));
    }
    return_value(rv, Value::array(result))
}

fn attribute_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let flags = with_argument(ed, 1, Value::as_long).unwrap_or(127);
    with_argument(ed, 0, |receiver| {
        if let Some(mut object) = receiver.as_object_mut() {
            object.set_property("flags", Value::long(flags));
        }
    });
    Ok(())
}

fn override_construct(
    _ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    Ok(())
}

fn sensitive_parameter_construct(
    _ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    Ok(())
}

fn deprecated_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let message = with_argument(ed, 1, |value| {
        if value.is_undef() {
            Value::null()
        } else {
            value.clone()
        }
    });
    let since = with_argument(ed, 2, |value| {
        if value.is_undef() {
            Value::null()
        } else {
            value.clone()
        }
    });
    with_argument(ed, 0, |receiver| {
        if let Some(mut object) = receiver.as_object_mut() {
            // The built-in readonly properties have no declaration default.
            // A normal NewObj invocation and a reflected object created
            // without its constructor therefore both enter here with Undef
            // slots. After the first successful constructor call, even an
            // omitted nullable argument initializes the slot to Null and a
            // later explicit __construct() call must respect readonly state.
            if object
                .get_property("message")
                .is_some_and(|value| !value.is_undef())
            {
                eg.exception = Some(make_error_value(
                    "Error",
                    "Cannot modify readonly property Deprecated::$message",
                ));
                return;
            }
            object.set_property("message", message);
            object.set_property("since", since);
        }
    });
    Ok(())
}

fn no_discard_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let message = with_argument(ed, 1, |value| {
        if value.is_undef() {
            Value::null()
        } else {
            value.clone()
        }
    });
    with_argument(ed, 0, |receiver| {
        if let Some(mut object) = receiver.as_object_mut() {
            if object
                .get_property("message")
                .is_some_and(|value| !value.is_undef())
            {
                eg.exception = Some(make_error_value(
                    "Error",
                    "Cannot modify readonly property NoDiscard::$message",
                ));
                return;
            }
            object.set_property("message", message);
        }
    });
    Ok(())
}

fn attribute_get_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = with_argument(ed, 0, Value::clone);
    return_value(
        rv,
        eg.reflection_attribute_state(&receiver).map_or_else(
            || reflected_property(ed, "name").unwrap_or_else(|| Value::string("")),
            |state| Value::string(state.definition.name.clone()),
        ),
    )
}

fn attribute_argument_expression_string(
    expression: &Expr,
    definition: &AttributeDefinition,
    declaration_name: &str,
) -> Option<String> {
    match expression {
        Expr::Closure { line, .. } => {
            Some(format!("Closure({{closure:{declaration_name}():{line}}})"))
        }
        Expr::FirstClassFunctionCallable { name, .. } => Some(format!(
            "{}(...)",
            render_attribute_function_name(name, &definition.evaluation_scope)
        )),
        Expr::FirstClassCallable { callable, .. } => {
            let Expr::ArrayLiteral(elements) = callable.as_ref() else {
                return None;
            };
            let [owner, method] = elements.as_slice() else {
                return None;
            };
            let Expr::ClassConstant {
                class_name,
                constant,
                ..
            } = &owner.value
            else {
                return None;
            };
            let Expr::StringLiteral(method) = &method.value else {
                return None;
            };
            if !constant.eq_ignore_ascii_case("class") {
                return None;
            }
            Some(format!(
                "{}::{method}(...)",
                render_attribute_static_class_name(class_name, &definition.evaluation_scope)
            ))
        }
        _ => None,
    }
}

fn attribute_to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = with_argument(ed, 0, Value::clone);
    let Some(state) = eg.reflection_attribute_state(&receiver) else {
        let name = reflected_property(ed, "name")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        return return_value(rv, Value::string(format!("Attribute [ {name} ]\n")));
    };
    let definition = state.definition.clone();
    let declaration_name = reflected_attribute_declaration_name(&state.declaration);
    if definition.arguments.is_empty() {
        return return_value(
            rv,
            Value::string(format!("Attribute [ {} ]\n", definition.name)),
        );
    }

    let mut rendered = format!(
        "Attribute [ {} ] {{\n  - Arguments [{}] {{\n",
        definition.name,
        definition.arguments.len()
    );
    for (index, argument) in definition.arguments.iter().enumerate() {
        let value = argument
            .deferred_expression
            .as_deref()
            .and_then(|expression| {
                attribute_argument_expression_string(expression, &definition, &declaration_name)
            })
            .or_else(|| {
                argument
                    .value
                    .as_ref()
                    .ok()
                    .map(|value| reflection_value_name(value, eg))
            })
            .or_else(|| {
                argument
                    .deferred_expression
                    .as_deref()
                    .and_then(|expression| {
                        crate::compiler::compile::assertion_expression_source(expression).and_then(
                            |source| {
                                source
                                    .strip_prefix("assert(")
                                    .and_then(|source| source.strip_suffix(')'))
                                    .map(str::to_owned)
                            },
                        )
                    })
            })
            .unwrap_or_else(|| "NULL".to_string());
        let name = argument
            .name
            .as_ref()
            .map_or_else(String::new, |name| format!("{name} = "));
        rendered.push_str(&format!("    Argument #{index} [ {name}{value} ]\n"));
    }
    rendered.push_str("  }\n}\n");
    return_value(rv, Value::string(rendered))
}

fn reflection_reference_construct(
    _ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    eg.exception = Some(make_error_value(
        "Error",
        "Call to private ReflectionReference::__construct() from global scope",
    ));
    Ok(())
}

fn reflection_reference_from_array_element(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let key = with_argument(ed, 2, |value| {
        value.as_long().map(ArrayKey::Int).or_else(|| {
            value
                .as_str()
                .map(|value| ArrayKey::String(value.to_string()))
        })
    });
    let Some(key) = key else {
        return return_value(rv, Value::null());
    };
    let reference_identity = with_argument(ed, 1, |value| {
        let array = value.as_array()?;
        let entry = match &key {
            ArrayKey::Int(key) => array.get_int(*key),
            ArrayKey::String(key) => array.get_str(key),
        }?;
        Some(entry.reference_identity())
    });
    let Some(reference_identity) = reference_identity else {
        reflection_exception(eg, "Array key not found");
        return Ok(());
    };
    let Some(reference_identity) = reference_identity else {
        return return_value(rv, Value::null());
    };

    let reflection = object_value("ReflectionReference", []);
    eg.register_reflection_reference(&reflection, reference_identity);
    return_value(rv, reflection)
}

fn reflection_reference_id(reference_identity: usize) -> [u8; 20] {
    // Zend deliberately exposes an opaque 20-byte token. SplitMix-style
    // diffusion prevents the backing allocation address from being published
    // while retaining stable same-reference equality for this request.
    let mut state = (reference_identity as u64) ^ 0x9e37_79b9_7f4a_7c15;
    let mut output = [0_u8; 20];
    for chunk in output.chunks_mut(8) {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut mixed = state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        mixed ^= mixed >> 31;
        let bytes = mixed.to_ne_bytes();
        for (output, byte) in chunk.iter_mut().zip(bytes) {
            const OPAQUE_ALPHABET: &[u8; 64] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
            *output = OPAQUE_ALPHABET[usize::from(byte & 63)];
        }
    }
    output
}

fn reflection_reference_get_id(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = with_argument(ed, 0, Value::clone);
    let Some(identity) = eg.reflection_reference_identity(&receiver) else {
        return return_value(rv, Value::string(""));
    };
    let id = reflection_reference_id(identity);
    return_value(
        rv,
        Value::string(super::filesystem::bytes_to_php_string(&id)),
    )
}

fn reflection_reference_debug_info(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::array(PhpArray::new()))
}

fn attribute_get_arguments(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = with_argument(ed, 0, Value::clone);
    let definition = eg
        .reflection_attribute_state(&receiver)
        .map(|state| state.definition.clone());
    let Some(definition) = definition else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let Some(arguments) = evaluate_attribute_arguments(&definition, eg, None)? else {
        return Ok(());
    };
    return_value(rv, arguments)
}

fn attribute_get_target(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = with_argument(ed, 0, Value::clone);
    return_value(
        rv,
        Value::long(eg.reflection_attribute_state(&receiver).map_or(0, |state| {
            state.definition.target & ATTRIBUTE_PUBLIC_TARGET_MASK
        })),
    )
}

fn attribute_is_repeated(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = with_argument(ed, 0, Value::clone);
    return_value(
        rv,
        Value::bool(
            eg.reflection_attribute_state(&receiver)
                .is_some_and(|state| state.repeated),
        ),
    )
}

fn attribute_target_name(target: i64) -> &'static str {
    match target {
        1 => "class",
        2 => "function",
        4 => "method",
        8 => "property",
        16 => "class constant",
        32 => "parameter",
        64 => "constant",
        _ => "declaration",
    }
}

fn attribute_allowed_targets(flags: i64) -> String {
    [
        (1, "class"),
        (2, "function"),
        (4, "method"),
        (8, "property"),
        (16, "class constant"),
        (32, "parameter"),
        (64, "constant"),
    ]
    .into_iter()
    .filter_map(|(flag, name)| (flags & flag != 0).then_some(name))
    .collect::<Vec<_>>()
    .join(", ")
}

fn attribute_new_instance(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = with_argument(ed, 0, Value::clone);
    let Some((definition, repeated, delayed_builtin_error)) =
        eg.reflection_attribute_state(&receiver).map(|state| {
            (
                state.definition.clone(),
                state.repeated,
                delayed_builtin_class_form_error(
                    &state.definition.name,
                    state.definition.target & ATTRIBUTE_PUBLIC_TARGET_MASK,
                    Some(&state.declaration),
                    eg,
                ),
            )
        })
    else {
        eg.exception = Some(make_error_value(
            "Error",
            "Invalid ReflectionAttribute object",
        ));
        return Ok(());
    };
    instantiate_attribute_definition_at_use(
        ed,
        rv,
        &definition,
        repeated,
        eg,
        None,
        delayed_builtin_error.as_deref(),
    )
}

/// Instantiate one already-resolved attribute at its declaration site.
/// Built-in semantic attributes use the same constructor normalization,
/// strictness and throwable-origin path as ReflectionAttribute::newInstance().
pub(crate) fn instantiate_attribute_definition(
    ed: *mut ExecuteData,
    rv: *mut Value,
    definition: &AttributeDefinition,
    repeated: bool,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    instantiate_attribute_definition_at_use(ed, rv, definition, repeated, eg, None, None)
}

fn delayed_builtin_class_form_error(
    name: &str,
    public_target: i64,
    declaration: Option<&ReflectionAttributeDeclaration>,
    eg: &ExecutorGlobals,
) -> Option<String> {
    if public_target != 1 {
        return None;
    }
    let declaration = declaration?;
    let class_name = declaration.name.as_str()?;
    let class = eg.find_class(class_name)?;
    let target = if name.eq_ignore_ascii_case("Attribute") {
        if class.is_trait {
            Some(format!("trait {}", class.name))
        } else if class.is_interface {
            Some(format!("interface {}", class.name))
        } else if class.is_abstract {
            Some(format!("abstract class {}", class.name))
        } else if class.is_enum {
            Some(format!("enum {}", class.name))
        } else {
            None
        }
    } else if name.eq_ignore_ascii_case("AllowDynamicProperties") {
        if class.is_trait {
            Some(format!("trait {}", class.name))
        } else if class.is_interface {
            Some(format!("interface {}", class.name))
        } else if class.is_readonly {
            Some(format!("readonly class {}", class.name))
        } else if class.is_enum {
            Some(format!("enum {}", class.name))
        } else {
            None
        }
    } else {
        None
    };
    target.map(|target| format!("Cannot apply #[\\{name}] to {target}"))
}

fn instantiate_attribute_definition_at_use(
    ed: *mut ExecuteData,
    rv: *mut Value,
    definition: &AttributeDefinition,
    repeated: bool,
    eg: &mut ExecutorGlobals,
    deprecated_use_site: Option<&DeprecatedUseSite>,
    delayed_builtin_error: Option<&str>,
) -> Result<(), VmError> {
    let Some(arguments) = evaluate_attribute_arguments(definition, eg, deprecated_use_site)? else {
        return Ok(());
    };
    let name = definition.name.clone();
    let target = definition.target;
    let public_target = target & ATTRIBUTE_PUBLIC_TARGET_MASK;
    let source_file = definition.source_file.clone();
    let source_line = definition.source_line;
    let strict = definition.strict_types;
    if eg.find_class(&name).is_none() && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &name)?
    {
        eg.exception = Some(make_error_value(
            "Error",
            &format!("Attribute class \"{name}\" not found"),
        ));
        return Ok(());
    }

    let Some(marker) = eg.find_class(&name).and_then(|class| {
        class
            .attributes
            .iter()
            .find(|attribute| attribute.name.eq_ignore_ascii_case("Attribute"))
            .cloned()
    }) else {
        eg.exception = Some(make_error_value(
            "Error",
            &format!("Attempting to use non-attribute class \"{name}\" as attribute"),
        ));
        return Ok(());
    };
    let Some(marker_arguments) = evaluate_attribute_arguments(&marker, eg, None)? else {
        return Ok(());
    };
    let marker_flag = marker_arguments
        .as_array()
        .and_then(|arguments| arguments.iter().next().map(|(_, value)| value.clone()));
    let flags = match marker_flag.as_ref() {
        None => 127,
        Some(value) => {
            let Some(flags) = value.as_long() else {
                eg.exception = Some(make_error_value(
                    "TypeError",
                    &format!(
                        "Attribute::__construct(): Argument #1 ($flags) must be of type int, {} given",
                        value.diagnostic_type_name()
                    ),
                ));
                return Ok(());
            };
            flags
        }
    };
    if flags & !(127 | 128) != 0 {
        eg.exception = Some(make_error_value(
            "Error",
            "Invalid attribute flags specified",
        ));
        return Ok(());
    }
    if flags & public_target == 0 {
        eg.exception = Some(make_error_value(
            "Error",
            &format!(
                "Attribute \"{name}\" cannot target {} (allowed targets: {})",
                attribute_target_name(public_target),
                attribute_allowed_targets(flags)
            ),
        ));
        return Ok(());
    }
    if repeated && flags & 128 == 0 {
        eg.exception = Some(make_error_value(
            "Error",
            &format!("Attribute \"{name}\" must not be repeated"),
        ));
        return Ok(());
    }
    if let Some(message) = delayed_builtin_error {
        eg.exception = Some(make_error_value("Error", message));
        return Ok(());
    }
    if name.eq_ignore_ascii_case("Deprecated") && public_target == 1 {
        let invalid_target = definition
            .evaluation_scope
            .lexical_class
            .as_deref()
            .and_then(|owner| eg.find_class(owner))
            .and_then(|class| {
                (!class.is_trait).then(|| {
                    let kind = if class.is_interface {
                        "interface"
                    } else if class.is_enum {
                        "enum"
                    } else {
                        "class"
                    };
                    format!("{kind} {}", class.name)
                })
            });
        if let Some(target) = invalid_target {
            eg.exception = Some(make_error_value(
                "Error",
                &format!("Cannot apply #[\\Deprecated] to {target}"),
            ));
            return Ok(());
        }
    }
    if name.eq_ignore_ascii_case("NoDiscard") && target & ATTRIBUTE_TARGET_PROPERTY_HOOK != 0 {
        eg.exception = Some(make_error_value(
            "Error",
            "#[\\NoDiscard] is not supported for property hooks",
        ));
        return Ok(());
    }
    let Some(class) = eg.find_class(&name) else {
        eg.exception = Some(make_error_value(
            "Error",
            &format!("Attribute class \"{name}\" not found"),
        ));
        return Ok(());
    };
    if class.is_interface || class.is_trait || class.is_abstract || class.is_enum {
        eg.exception = Some(make_error_value(
            "Error",
            &format!("Cannot instantiate attribute class {name}"),
        ));
        return Ok(());
    }
    let class_id = class.class_id;
    let object = Value::object(if class_id == 0 {
        PhpObject::dynamic(class.name.clone(), 0, HashMap::new())
    } else {
        PhpObject::with_layout_from_defaults(
            class_id,
            class.property_layout.clone(),
            class.property_defaults.as_ref(),
        )
    });

    let Some((_, visibility, _, _, constructor, _)) =
        find_reflected_method(eg, &name, "__construct")
    else {
        return return_value(rv, object);
    };
    if visibility != Visibility::Public {
        eg.exception = Some(make_error_value(
            "Error",
            &format!(
                "Call to {} {name}::__construct() from global scope",
                if visibility == Visibility::Private {
                    "private"
                } else {
                    "protected"
                }
            ),
        ));
        return Ok(());
    }

    let common = unsafe { &*constructor };
    let parameter_names = common.sig.param_names.clone();
    let parameter_hints = common.sig.param_type_hints.clone();
    let public_arity = common.sig.public_arity() as usize;
    let required = common.sig.required_num_args as usize;
    let is_variadic = common.sig.is_variadic;
    let supplied = arguments.as_array().map_or(0, PhpArray::len);
    let mut normalized = Vec::<Value>::new();
    let mut named_variadic = Vec::<(String, Value)>::new();
    if let Some(arguments) = arguments.as_array() {
        for (key, value) in arguments.iter() {
            match key {
                ArrayKey::Int(_) => normalized.push(value.clone()),
                ArrayKey::String(name) => {
                    if let Some(position) = parameter_names
                        .iter()
                        .position(|parameter| parameter == &name)
                    {
                        if normalized.len() <= position {
                            normalized.resize_with(position + 1, Value::undef);
                        }
                        if !normalized[position].is_undef() {
                            eg.exception = Some(make_error_value(
                                "Error",
                                &format!("Named parameter ${name} overwrites previous argument"),
                            ));
                            return Ok(());
                        }
                        normalized[position] = value.clone();
                    } else if is_variadic {
                        named_variadic.push((name.to_string(), value.clone()));
                    } else {
                        eg.exception = Some(make_error_value(
                            "Error",
                            &format!("Unknown named parameter ${name}"),
                        ));
                        return Ok(());
                    }
                }
            }
        }
    }
    if (0..required).any(|index| normalized.get(index).is_none_or(Value::is_undef)) {
        let relation = if public_arity > required {
            "at least"
        } else {
            "exactly"
        };
        eg.exception = Some(make_error_value(
            "ArgumentCountError",
            &format!(
                "Too few arguments to function {name}::__construct(), {supplied} passed in {source_file} on line {source_line} and {relation} {required} expected"
            ),
        ));
        return Ok(());
    }

    for index in 0..normalized.len().min(parameter_hints.len()) {
        if normalized[index].is_undef()
            || matches!(
                parameter_hints[index],
                ParamTypeHint::None | ParamTypeHint::Mixed
            )
        {
            continue;
        }
        let prepared = crate::vm::execute::prepare_call_argument(
            &normalized[index],
            &parameter_hints[index],
            eg,
            strict,
            Some(&name),
        )?;
        match prepared {
            crate::vm::execute::CallArgumentPreparation::Exact => {}
            crate::vm::execute::CallArgumentPreparation::Coerced(value, _diagnostic) => {
                normalized[index] = value;
            }
            crate::vm::execute::CallArgumentPreparation::Invalid => {
                let parameter = parameter_names.get(index).map_or("unknown", String::as_str);
                let call_site = if common.fn_type == FunctionType::Internal {
                    String::new()
                } else {
                    format!(", called in {source_file} on line {source_line}")
                };
                let error = make_error_value(
                    "TypeError",
                    &format!(
                        "{name}::__construct(): Argument #{} (${parameter}) must be of type {}, {} given{call_site}",
                        index + 1,
                        parameter_hints[index].diagnostic_display_name(),
                        normalized[index].diagnostic_type_name()
                    ),
                );
                crate::vm::execute::attach_detached_argument_type_error_origin(
                    eg,
                    ed,
                    constructor,
                    1 + normalized.len(),
                    std::iter::once(object.clone()).chain(normalized.iter().cloned()),
                    &source_file,
                    source_line,
                    &error,
                )?;
                eg.exception = Some(error);
                return Ok(());
            }
        }
    }
    while normalized.last().is_some_and(Value::is_undef) {
        normalized.pop();
    }
    let num_args = 1 + normalized.len();
    crate::vm::execute::call_function_owned_iter_with_context_and_named_from(
        eg,
        ed,
        constructor,
        num_args,
        std::iter::once(object.clone()).chain(normalized),
        class_id,
        None,
        0,
        None,
        named_variadic,
        (source_file, source_line),
        None,
        false,
    )?;
    if eg.exception.is_some() {
        return Ok(());
    }
    return_value(rv, object)
}

fn function_get_closure(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(closure) = reflected_property(ed, "__reflection_closure")
        .filter(|value| value.value_type() == ValueType::Closure)
    {
        return return_value(rv, closure);
    }
    let Some(function) = reflected_function(ed) else {
        reflection_exception(eg, "ReflectionFunction has no resolved function");
        return Ok(());
    };
    return_value(
        rv,
        Value::closure(PhpClosure {
            object_handle: 0,
            func: function as *const FunctionCommon,
            called_scope_class_id: 0,
            trait_scope_class_id: 0,
            is_static: false,
            bound_this: None,
            captures: vec![],
            static_vars: None,
            has_heap_captures: false,
            scope_is_dummy: false,
        }),
    )
}

fn reflected_function_callback(
    ed: *mut ExecuteData,
    eg: &ExecutorGlobals,
) -> Option<super::ResolvedCallback> {
    if let Some(closure) = reflected_property(ed, "__reflection_closure")
        .filter(|value| value.value_type() == ValueType::Closure)
    {
        return super::resolve_callback(&closure, eg, None);
    }
    let function = reflected_function(ed)?;
    Some(super::ResolvedCallback {
        func_ptr: function as *const FunctionCommon,
        prepend_args: Vec::new(),
        use_vars: Vec::new(),
        called_scope_class_id: 0,
        bound_this: None,
        closure_static_vars: None,
        is_magic_call: false,
    })
}

fn invoke_reflected_function(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arguments =
        with_argument(ed, 1, |value| value.as_array().cloned()).unwrap_or_else(PhpArray::new);
    let Some(callback) = reflected_function_callback(ed, eg) else {
        reflection_exception(eg, "ReflectionFunction has no resolved function");
        return Ok(());
    };
    let result = super::call_resolved_with_php_array(eg, callback, &arguments, true)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    return_value(rv, result)
}

fn function_invoke(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    invoke_reflected_function(ed, rv, eg)
}

fn function_invoke_args(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let valid = with_argument(ed, 1, |value| value.value_type() == ValueType::Array);
    if !valid {
        let given = with_argument(ed, 1, reflection_argument_type_name);
        eg.exception = Some(make_error_value(
            "TypeError",
            &format!(
                "ReflectionFunction::invokeArgs(): Argument #1 ($args) must be of type array, {given} given"
            ),
        ));
        return Ok(());
    }
    invoke_reflected_function(ed, rv, eg)
}

fn hint_metadata(hint: &ParamTypeHint) -> (&'static str, String, bool) {
    match hint {
        ParamTypeHint::None => ("named", String::new(), true),
        ParamTypeHint::Nullable(inner) => ("named", inner.display_name(), true),
        ParamTypeHint::Union(parts) => (
            "union",
            hint.display_name(),
            parts.iter().any(|part| {
                matches!(part, ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("null"))
            }),
        ),
        ParamTypeHint::Intersection(_) => ("intersection", hint.display_name(), false),
        _ => ("named", hint.display_name(), false),
    }
}

fn reflected_magic_call_trampoline(ed: *mut ExecuteData, eg: &ExecutorGlobals) -> bool {
    reflected_property(ed, "__reflection_closure")
        .and_then(|value| {
            value
                .as_closure()
                .map(|closure| super::closure_is_magic_call(closure, eg))
        })
        .unwrap_or(false)
}

fn magic_call_trampoline_parameter(function: &FunctionCommon) -> Value {
    object_value(
        "ReflectionParameter",
        [
            ("name", Value::string("arguments")),
            (
                "__reflection_function_pointer",
                Value::long(function as *const FunctionCommon as usize as i64),
            ),
            ("__reflection_position", Value::long(0)),
            ("__reflection_has_type", Value::bool(true)),
            ("__reflection_type_kind", Value::string("named")),
            ("__reflection_type_name", Value::string("mixed")),
            ("__reflection_allows_null", Value::bool(true)),
            ("__reflection_variadic", Value::bool(true)),
            ("__reflection_passed_by_reference", Value::bool(false)),
            ("__reflection_has_default", Value::bool(false)),
            ("__reflection_declaring_class", Value::null()),
        ],
    )
}

fn populate_magic_call_trampoline_parameter(receiver: &Value, function: &FunctionCommon) {
    if let Some(mut object) = receiver.as_object_mut() {
        object.set_property("name", Value::string("arguments"));
        object.set_property(
            "__reflection_function_pointer",
            Value::long(function as *const FunctionCommon as usize as i64),
        );
        object.set_property("__reflection_position", Value::long(0));
        object.set_property("__reflection_has_type", Value::bool(true));
        object.set_property("__reflection_type_kind", Value::string("named"));
        object.set_property("__reflection_type_name", Value::string("mixed"));
        object.set_property("__reflection_allows_null", Value::bool(true));
        object.set_property("__reflection_variadic", Value::bool(true));
        object.set_property("__reflection_passed_by_reference", Value::bool(false));
        object.set_property("__reflection_has_default", Value::bool(false));
        object.set_property("__reflection_declaring_class", Value::null());
    }
}

fn function_get_parameters(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(function) = reflected_function(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if reflected_magic_call_trampoline(ed, eg) {
        let mut parameters = PhpArray::with_packed_capacity(1);
        parameters.push(magic_call_trampoline_parameter(function));
        return return_value(rv, Value::array(parameters));
    }
    let fixed = function.sig.public_arity();
    let count = fixed + u32::from(function.sig.is_variadic);
    let declaring_class = eg
        .declaring_class_of(function as *const FunctionCommon)
        .map(str::to_owned);
    let attribute_scope_class = reflected_function_attribute_scope(ed);
    let mut parameters = PhpArray::with_packed_capacity(count as usize);
    for index in 0..count {
        let name = function
            .sig
            .param_names
            .get(index as usize)
            .cloned()
            .unwrap_or_else(|| format!("arg{}", index + 1));
        let hint = function
            .sig
            .param_type_hints
            .get(index as usize)
            .unwrap_or(&ParamTypeHint::None);
        let (type_kind, type_name, allows_null) = hint_metadata(hint);
        let has_type = !matches!(hint, ParamTypeHint::None);
        let is_variadic = function.sig.is_variadic && index == fixed;
        let has_default = !is_variadic && index >= function.sig.required_num_args;
        let parameter = object_value(
            "ReflectionParameter",
            [
                ("name", Value::string(name)),
                (
                    "__reflection_function_pointer",
                    Value::long(function as *const FunctionCommon as usize as i64),
                ),
                ("__reflection_position", Value::long(index as i64)),
                ("__reflection_has_type", Value::bool(has_type)),
                ("__reflection_type_kind", Value::string(type_kind)),
                ("__reflection_type_name", Value::string(type_name)),
                ("__reflection_allows_null", Value::bool(allows_null)),
                ("__reflection_variadic", Value::bool(is_variadic)),
                (
                    "__reflection_passed_by_reference",
                    Value::bool(function.sig.is_param_by_ref(index)),
                ),
                ("__reflection_has_default", Value::bool(has_default)),
                (
                    "__reflection_declaring_class",
                    declaring_class
                        .as_deref()
                        .map_or_else(Value::null, Value::string),
                ),
            ],
        );
        if let Some(attribute_scope_class) = &attribute_scope_class {
            eg.register_reflection_parameter_scope(&parameter, attribute_scope_class.clone());
        }
        parameters.push(parameter);
    }
    return_value(rv, Value::array(parameters))
}

fn function_get_number_of_parameters(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if reflected_magic_call_trampoline(ed, eg) {
        return return_value(rv, Value::long(1));
    }
    let count = reflected_function(ed).map_or(0, |function| {
        function.sig.public_arity() + u32::from(function.sig.is_variadic)
    });
    return_value(rv, Value::long(i64::from(count)))
}

fn function_get_number_of_required_parameters(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if reflected_magic_call_trampoline(ed, eg) {
        return return_value(rv, Value::long(0));
    }
    let count = reflected_function(ed).map_or(0, |function| function.sig.required_num_args);
    return_value(rv, Value::long(i64::from(count)))
}

fn function_returns_reference(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(reflected_function(ed).is_some_and(|function| function.sig.returns_reference)),
    )
}

fn function_is_closure(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let is_closure = reflected_property(ed, "__generic_kind")
        .and_then(|value| value.as_str().map(|kind| kind == "closure"))
        .unwrap_or(false);
    return_value(rv, Value::bool(is_closure))
}

fn function_is_deprecated(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let deprecated = reflected_user_function(ed).is_some_and(|function| {
        function
            .attributes
            .iter()
            .any(|attribute| attribute.name.eq_ignore_ascii_case("Deprecated"))
    }) || reflected_internal_function(ed)
        .is_some_and(|function| function.deprecation.is_some());
    return_value(rv, Value::bool(deprecated))
}

fn function_has_return_type(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let has_implicit_setter_return = reflected_property(ed, "name")
        .and_then(|value| value.as_str().map(|name| name.ends_with("::set")))
        .unwrap_or(false);
    return_value(
        rv,
        Value::bool(
            has_implicit_setter_return
                || reflected_function(ed).is_some_and(|function| {
                    !matches!(function.sig.return_type_hint, ParamTypeHint::None)
                }),
        ),
    )
}

fn function_get_return_type(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(function) = reflected_function(ed) else {
        return return_value(rv, Value::null());
    };
    if matches!(function.sig.return_type_hint, ParamTypeHint::None) {
        if reflected_property(ed, "name")
            .and_then(|value| value.as_str().map(|name| name.ends_with("::set")))
            .unwrap_or(false)
        {
            return return_value(rv, reflected_signature_type(&ParamTypeHint::Void));
        }
        return return_value(rv, Value::null());
    }
    return_value(rv, reflected_signature_type(&function.sig.return_type_hint))
}

fn function_has_tentative_return_type(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::bool(false))
}

fn function_get_tentative_return_type(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::null())
}

fn function_is_anonymous(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let anonymous =
        reflected_property(ed, "__reflection_is_anonymous").is_some_and(|value| value.is_truthy());
    return_value(rv, Value::bool(anonymous))
}

fn function_get_short_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = reflected_property(ed, "name")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    let short = if reflected_property(ed, "__reflection_is_anonymous")
        .is_some_and(|value| value.is_truthy())
    {
        name
    } else {
        name.rsplit_once('\\')
            .map_or(name.clone(), |(_, short)| short.to_string())
    };
    return_value(rv, Value::string(short))
}

fn function_get_namespace_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let anonymous =
        reflected_property(ed, "__reflection_is_anonymous").is_some_and(|value| value.is_truthy());
    let namespace = (!anonymous)
        .then(|| reflected_property(ed, "name"))
        .flatten()
        .and_then(|value| value.as_str().map(str::to_owned))
        .and_then(|name| {
            name.rsplit_once('\\')
                .map(|(namespace, _)| namespace.to_string())
        })
        .unwrap_or_default();
    return_value(rv, Value::string(namespace))
}

fn function_in_namespace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let anonymous =
        reflected_property(ed, "__reflection_is_anonymous").is_some_and(|value| value.is_truthy());
    let namespaced = !anonymous
        && reflected_property(ed, "name")
            .and_then(|value| value.as_str().map(|name| name.contains('\\')))
            .unwrap_or(false);
    return_value(rv, Value::bool(namespaced))
}

fn function_get_closure_this(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__reflection_closure_this").unwrap_or_else(Value::null),
    )
}

fn function_get_closure_called_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(name) = reflected_property(ed, "__reflection_closure_called_class")
        .and_then(|name| name.as_str().map(str::to_owned))
    else {
        return return_value(rv, Value::null());
    };
    return_value(
        rv,
        object_value(
            "ReflectionClass",
            [
                ("__generic_kind", Value::string("class")),
                ("__generic_owner", Value::string(name.clone())),
                ("name", Value::string(name)),
            ],
        ),
    )
}

fn function_get_closure_scope_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(name) = reflected_property(ed, "__reflection_closure_scope_class")
        .and_then(|name| name.as_str().map(str::to_owned))
    else {
        return return_value(rv, Value::null());
    };
    return_value(
        rv,
        object_value(
            "ReflectionClass",
            [
                ("__generic_kind", Value::string("class")),
                ("__generic_owner", Value::string(name.clone())),
                ("name", Value::string(name)),
            ],
        ),
    )
}

fn parameter_property_bool(ed: *mut ExecuteData, name: &str) -> bool {
    reflected_property(ed, name)
        .map(|value| value.is_truthy())
        .unwrap_or(false)
}

fn populate_reflection_parameter(
    receiver: &Value,
    function: &FunctionCommon,
    index: u32,
    declaring_class: Option<&str>,
) {
    let fixed = function.sig.public_arity();
    let name = function
        .sig
        .param_names
        .get(index as usize)
        .cloned()
        .unwrap_or_else(|| format!("arg{}", index + 1));
    let hint = function
        .sig
        .param_type_hints
        .get(index as usize)
        .unwrap_or(&ParamTypeHint::None);
    let (type_kind, type_name, allows_null) = hint_metadata(hint);
    let is_variadic = function.sig.is_variadic && index == fixed;
    if let Some(mut object) = receiver.as_object_mut() {
        object.set_property("name", Value::string(name));
        object.set_property(
            "__reflection_function_pointer",
            Value::long(function as *const FunctionCommon as usize as i64),
        );
        object.set_property("__reflection_position", Value::long(index as i64));
        object.set_property(
            "__reflection_has_type",
            Value::bool(!matches!(hint, ParamTypeHint::None)),
        );
        object.set_property("__reflection_type_kind", Value::string(type_kind));
        object.set_property("__reflection_type_name", Value::string(type_name));
        object.set_property("__reflection_allows_null", Value::bool(allows_null));
        object.set_property("__reflection_variadic", Value::bool(is_variadic));
        object.set_property(
            "__reflection_passed_by_reference",
            Value::bool(function.sig.is_param_by_ref(index)),
        );
        object.set_property(
            "__reflection_has_default",
            Value::bool(!is_variadic && index >= function.sig.required_num_args),
        );
        object.set_property(
            "__reflection_declaring_class",
            declaring_class.map_or_else(Value::null, Value::string),
        );
    }
}

fn parameter_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = with_argument(ed, 1, Clone::clone);
    if let Some(closure) = target.as_closure()
        && super::closure_is_magic_call(closure, eg)
    {
        let selector = with_argument(ed, 2, Clone::clone);
        let valid = selector.as_long() == Some(0) || selector.as_str() == Some("arguments");
        if !valid {
            if selector.as_long().is_some_and(|index| index < 0) {
                eg.exception = Some(make_error_value(
                    "ValueError",
                    "ReflectionParameter::__construct(): Argument #2 ($param) must be greater than or equal to 0",
                ));
            } else {
                let kind = if selector.as_str().is_some() {
                    "name"
                } else {
                    "offset"
                };
                reflection_exception(
                    eg,
                    format!("The parameter specified by its {kind} could not be found"),
                );
            }
            return Ok(());
        }
        let receiver = with_argument(ed, 0, Clone::clone);
        let Some(function) = closure.common() else {
            reflection_exception(eg, "ReflectionParameter has no resolved function");
            return Ok(());
        };
        populate_magic_call_trampoline_parameter(&receiver, function);
        return Ok(());
    }
    let mut declaring_class = None;
    let mut type_scope_class = None;
    let function = if let Some(closure) = target.as_closure() {
        type_scope_class = (closure.called_scope_class_id != 0)
            .then(|| eg.class_by_id(closure.called_scope_class_id))
            .flatten()
            .map(|class| class.name.clone())
            .or_else(|| eg.declaring_class_of(closure.func).map(str::to_owned));
        closure.func
    } else {
        let method_target = if let Some(array) = target.as_array() {
            if array.len() != 2 {
                reflection_exception(
                    eg,
                    "Expected array($object, $method) or array($classname, $method)",
                );
                return Ok(());
            }
            let owner = array.get_value_at(0).map(Value::dereferenced);
            let method = array
                .get_value_at(1)
                .map(Value::dereferenced)
                .and_then(Value::as_str)
                .map(str::to_owned);
            let target = owner.zip(method).and_then(|(owner, method)| {
                let class = owner
                    .as_object()
                    .map(|object| object.class_name.to_string())
                    .or_else(|| owner.as_str().map(str::to_owned))?;
                Some((class, method))
            });
            if target.is_none() {
                reflection_exception(
                    eg,
                    "Expected array($object, $method) or array($classname, $method)",
                );
                return Ok(());
            }
            target
        } else if let Some(object) = target.as_object() {
            Some((object.class_name.to_string(), "__invoke".to_string()))
        } else {
            None
        };
        if let Some((class, method)) = method_target {
            if eg.find_class(&class).is_none()
                && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &class)?
            {
                reflection_exception(eg, format!("Class \"{class}\" does not exist"));
                return Ok(());
            }
            let Some((_, _, _, _, function, declared)) = find_reflected_method(eg, &class, &method)
            else {
                reflection_exception(eg, format!("Method {class}::{method}() does not exist"));
                return Ok(());
            };
            declaring_class = Some(declared.clone());
            type_scope_class = Some(declared);
            function
        } else {
            let Some(name) = target.as_str().map(str::to_owned) else {
                reflection_exception(
                    eg,
                    format!(
                        "ReflectionParameter::__construct(): Argument #1 ($function) must be a string, an array(class, method), or a callable object, {} given",
                        reflection_argument_type_name(&target)
                    ),
                );
                return Ok(());
            };
            let function = (!name.contains("::"))
                .then(|| eg.find_function(name.trim_start_matches('\\')))
                .flatten();
            let Some(function) = function else {
                reflection_exception(eg, format!("Function {name}() does not exist"));
                return Ok(());
            };
            function
        }
    };
    if function.is_null() {
        reflection_exception(eg, "ReflectionParameter has no resolved function");
        return Ok(());
    }
    let common = target
        .as_closure()
        .and_then(crate::value::PhpClosure::common)
        .or_else(|| eg.registered_function_common(function));
    let Some(common) = common else {
        reflection_exception(eg, "ReflectionParameter has no resolved function");
        return Ok(());
    };
    let signature = &common.sig;
    let count = signature.public_arity() + u32::from(signature.is_variadic);
    let parameter = with_argument(ed, 2, Clone::clone);
    let index = if let Some(index) = parameter.as_long() {
        if index < 0 {
            eg.exception = Some(make_error_value(
                "ValueError",
                "ReflectionParameter::__construct(): Argument #2 ($param) must be greater than or equal to 0",
            ));
            return Ok(());
        }
        u32::try_from(index).ok().filter(|index| *index < count)
    } else if let Some(name) = parameter.as_str() {
        signature
            .param_names
            .iter()
            .position(|parameter| parameter == name)
            .and_then(|index| u32::try_from(index).ok())
    } else {
        None
    };
    let Some(index) = index else {
        let selector = if parameter.as_str().is_some() {
            "name"
        } else {
            "offset"
        };
        reflection_exception(
            eg,
            format!("The parameter specified by its {selector} could not be found"),
        );
        return Ok(());
    };
    let receiver = with_argument(ed, 0, Clone::clone);
    populate_reflection_parameter(&receiver, common, index, declaring_class.as_deref());
    if let Some(scope) = type_scope_class {
        eg.register_reflection_parameter_scope(&receiver, scope);
    }
    Ok(())
}

fn parameter_get_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "name").unwrap_or_else(|| Value::string("")),
    )
}

fn report_legacy_parameter_type_deprecation(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    method: &str,
) -> Result<bool, VmError> {
    super::report_internal_deprecation(
        eg,
        ed,
        &format!(
            "Method ReflectionParameter::{method}() is deprecated since 8.0, use ReflectionParameter::getType() instead"
        ),
    )?;
    Ok(eg.exception.is_none())
}

fn parameter_is_legacy_named_type(ed: *mut ExecuteData, expected: &str) -> bool {
    reflected_property(ed, "__reflection_type_kind")
        .and_then(|value| value.as_str().map(|kind| kind == "named"))
        .unwrap_or(false)
        && reflected_property(ed, "__reflection_type_name")
            .and_then(|value| {
                value
                    .as_str()
                    .map(|name| name.eq_ignore_ascii_case(expected))
            })
            .unwrap_or(false)
}

fn parameter_is_array(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if !report_legacy_parameter_type_deprecation(ed, eg, "isArray")? {
        return Ok(());
    }
    let legacy_array_union = reflected_property(ed, "__reflection_type_kind")
        .and_then(|value| value.as_str().map(|kind| kind == "union"))
        .unwrap_or(false)
        && reflected_property(ed, "__reflection_type_name")
            .and_then(|value| {
                value.as_str().map(|name| {
                    let parts = name.split('|').map(str::trim).collect::<Vec<_>>();
                    parts.iter().any(|part| part.eq_ignore_ascii_case("array"))
                        && parts.iter().all(|part| {
                            matches!(part.to_ascii_lowercase().as_str(), "array" | "null")
                                || (!part.contains('&')
                                    && !part.contains('(')
                                    && !part.contains(')')
                                    && !matches!(
                                        part.to_ascii_lowercase().as_str(),
                                        "bool"
                                            | "callable"
                                            | "false"
                                            | "float"
                                            | "int"
                                            | "iterable"
                                            | "mixed"
                                            | "never"
                                            | "object"
                                            | "string"
                                            | "true"
                                            | "void"
                                    ))
                        })
                })
            })
            .unwrap_or(false);
    let is_array = parameter_is_legacy_named_type(ed, "array") || legacy_array_union;
    return_value(rv, Value::bool(is_array))
}

fn parameter_is_callable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if !report_legacy_parameter_type_deprecation(ed, eg, "isCallable")? {
        return Ok(());
    }
    return_value(
        rv,
        Value::bool(parameter_is_legacy_named_type(ed, "callable")),
    )
}

fn parameter_get_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if !report_legacy_parameter_type_deprecation(ed, eg, "getClass")? {
        return Ok(());
    }
    if !parameter_property_bool(ed, "__reflection_has_type") {
        return return_value(rv, Value::null());
    }
    let kind = reflected_property(ed, "__reflection_type_kind")
        .and_then(|value| value.as_str().map(str::to_owned));
    let type_name = reflected_property(ed, "__reflection_type_name")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    let is_builtin = |name: &str| {
        matches!(
            name.to_ascii_lowercase().as_str(),
            "array"
                | "bool"
                | "callable"
                | "false"
                | "float"
                | "int"
                | "iterable"
                | "mixed"
                | "never"
                | "null"
                | "object"
                | "string"
                | "true"
                | "void"
        )
    };
    let mut class_names = match kind.as_deref() {
        Some("named") if type_name.eq_ignore_ascii_case("iterable") => {
            vec!["Traversable".to_string()]
        }
        Some("named") => vec![type_name],
        Some("union") => type_name
            .split('|')
            .map(str::trim)
            .filter(|part| !part.contains('&') && !part.contains('(') && !part.contains(')'))
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    };
    class_names.retain(|name| !is_builtin(name));
    if class_names.len() != 1 {
        return return_value(rv, Value::null());
    }
    let mut name = class_names.pop().unwrap();

    let scope = reflected_property(ed, "__reflection_declaring_class")
        .and_then(|value| value.as_str().map(str::to_owned))
        .or_else(|| {
            with_argument(ed, 0, |receiver| {
                eg.reflection_parameter_scope(receiver).map(str::to_owned)
            })
        });
    if name.eq_ignore_ascii_case("self") {
        let Some(scope) = scope else {
            reflection_exception(
                eg,
                "Parameter uses \"self\" as type but function is not a class member",
            );
            return Ok(());
        };
        name = scope;
    } else if name.eq_ignore_ascii_case("parent") {
        let Some(scope) = scope else {
            reflection_exception(
                eg,
                "Parameter uses \"parent\" as type but function is not a class member",
            );
            return Ok(());
        };
        let Some(parent) = eg.find_class(&scope).and_then(|class| class.parent.clone()) else {
            reflection_exception(
                eg,
                "Parameter uses \"parent\" as type although class does not have a parent",
            );
            return Ok(());
        };
        name = parent;
    }

    if eg.find_public_class(&name).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &name)?
    {
        if eg.exception.is_none() {
            reflection_exception(eg, format!("Class \"{name}\" does not exist"));
        }
        return Ok(());
    }
    let canonical = eg
        .find_public_class(&name)
        .map(|class| class.name.clone())
        .unwrap_or(name);
    return_value(
        rv,
        object_value(
            "ReflectionClass",
            [
                ("__generic_kind", Value::string("class")),
                ("__generic_owner", Value::string(canonical.clone())),
                ("name", Value::string(canonical)),
            ],
        ),
    )
}

fn parameter_to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let position = reflected_property(ed, "__reflection_position")
        .and_then(|value| value.as_long())
        .unwrap_or(0);
    let name = reflected_property(ed, "name")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    let variadic = parameter_property_bool(ed, "__reflection_variadic");
    let has_default = parameter_property_bool(ed, "__reflection_has_default");
    let requirement = if variadic || has_default {
        "optional"
    } else {
        "required"
    };
    let type_prefix = if parameter_property_bool(ed, "__reflection_has_type") {
        reflected_property(ed, "__reflection_type_name")
            .and_then(|value| {
                value.as_str().map(|name| {
                    let nullable = parameter_property_bool(ed, "__reflection_allows_null")
                        && reflected_property(ed, "__reflection_type_kind")
                            .and_then(|value| value.as_str().map(|kind| kind == "named"))
                            .unwrap_or(true)
                        && !matches!(name.to_ascii_lowercase().as_str(), "mixed" | "null");
                    format!("{}{name} ", if nullable { "?" } else { "" })
                })
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    let reference = if parameter_property_bool(ed, "__reflection_passed_by_reference") {
        "&"
    } else {
        ""
    };
    let variadic_prefix = if variadic { "..." } else { "" };
    let function = reflected_property(ed, "__reflection_function_pointer")
        .and_then(|value| value.as_long())
        .map(|pointer| pointer as usize as *const FunctionCommon);
    let default = if has_default {
        function
            .and_then(|function| {
                eg.internal_function_parameter_default_diagnostic(function, position as usize)
            })
            .map(|diagnostic| format!(" = {diagnostic}"))
            .or_else(|| {
                function
                    .and_then(|function| {
                        eg.internal_function_parameter_default(function, position as usize)
                    })
                    .map(|value| format!(" = {}", reflection_default_text(value)))
            })
            .unwrap_or_else(|| " = <default>".to_string())
    } else {
        String::new()
    };
    return_value(
        rv,
        Value::string(format!(
            "Parameter #{position} [ <{requirement}> {type_prefix}{reference}{variadic_prefix}${name}{default} ]"
        )),
    )
}

fn parameter_get_type(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let property_metadata = reflection_property_metadata(ed, eg);
    let has_type = property_metadata.map_or_else(
        || parameter_property_bool(ed, "__reflection_has_type"),
        |metadata| metadata.has_type,
    );
    if !has_type {
        return return_value(rv, Value::null());
    }
    let kind = property_metadata.map_or_else(
        || {
            reflected_property(ed, "__reflection_type_kind")
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "named".to_string())
        },
        |metadata| metadata.type_kind.clone(),
    );
    let name = property_metadata.map_or_else(
        || {
            reflected_property(ed, "__reflection_type_name")
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default()
        },
        |metadata| metadata.type_name.clone(),
    );
    let class = match kind.as_str() {
        "union" => "ReflectionUnionType",
        "intersection" => "ReflectionIntersectionType",
        _ => "ReflectionNamedType",
    };
    let allows_null = property_metadata.map_or_else(
        || parameter_property_bool(ed, "__reflection_allows_null"),
        |metadata| metadata.allows_null,
    );
    let rendered = if kind == "named"
        && allows_null
        && !matches!(name.to_ascii_lowercase().as_str(), "mixed" | "null")
    {
        format!("?{name}")
    } else {
        name.clone()
    };
    return_value(
        rv,
        object_value(
            class,
            [
                ("__generic_name", Value::string(name.clone())),
                ("__generic_string", Value::string(rendered)),
                ("__reflection_allows_null", Value::bool(allows_null)),
            ],
        ),
    )
}

fn parameter_has_type(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let has_type = reflection_property_metadata(ed, eg).map_or_else(
        || parameter_property_bool(ed, "__reflection_has_type"),
        |metadata| metadata.has_type,
    );
    return_value(rv, Value::bool(has_type))
}

fn parameter_is_variadic(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_variadic")),
    )
}

fn parameter_is_passed_by_reference(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(
            ed,
            "__reflection_passed_by_reference",
        )),
    )
}

fn parameter_is_optional(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(
            parameter_property_bool(ed, "__reflection_has_default")
                || parameter_property_bool(ed, "__reflection_variadic"),
        ),
    )
}

fn parameter_is_default_available(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_has_default")),
    )
}

fn parameter_get_default_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let function = reflected_property(ed, "__reflection_function_pointer")
        .and_then(|value| value.as_long())
        .map(|pointer| pointer as usize as *const FunctionCommon);
    let position = reflected_property(ed, "__reflection_position")
        .and_then(|value| value.as_long())
        .and_then(|position| usize::try_from(position).ok());
    let default = function
        .zip(position)
        .and_then(|(function, position)| eg.internal_function_parameter_default(function, position))
        .cloned()
        .unwrap_or_else(Value::null);
    return_value(rv, default)
}

fn reflection_default_text(value: &Value) -> String {
    let value = value.dereferenced();
    match value.value_type() {
        ValueType::Null => "null".to_string(),
        ValueType::True => "true".to_string(),
        ValueType::False => "false".to_string(),
        ValueType::Long => value.as_long().unwrap_or_default().to_string(),
        ValueType::Double => value.echo_to_string(),
        ValueType::String => format!("\"{}\"", value.as_str().unwrap_or_default()),
        _ => "<default>".to_string(),
    }
}

fn parameter_allows_null(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_allows_null")),
    )
}

fn parameter_get_attributes(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let position = reflected_property(ed, "__reflection_position")
        .and_then(|value| value.as_long())
        .and_then(|position| usize::try_from(position).ok());
    let mut attributes = reflected_user_function(ed)
        .zip(position)
        .and_then(|(function, position)| function.parameter_attributes.get(position))
        .cloned()
        .unwrap_or_default();
    let called_class = with_argument(ed, 0, |receiver| {
        eg.reflection_parameter_scope(receiver).map(str::to_owned)
    });
    rebind_attribute_evaluation_scope(&mut attributes, called_class.as_deref(), eg);
    reflection_attributes(ed, rv, eg, attributes)
}

fn parameter_get_declaring_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(name) = reflected_property(ed, "__reflection_declaring_class")
        .and_then(|value| value.as_str().map(str::to_owned))
    else {
        return return_value(rv, Value::null());
    };
    return_value(
        rv,
        object_value(
            "ReflectionClass",
            [
                ("__generic_kind", Value::string("class")),
                ("__generic_owner", Value::string(name.clone())),
                ("name", Value::string(name)),
            ],
        ),
    )
}

fn reflection_type_is_builtin(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let builtin = reflected_property(ed, "__generic_name")
        .and_then(|value| value.as_str().map(str::to_ascii_lowercase))
        .is_some_and(|name| {
            matches!(
                name.as_str(),
                "int"
                    | "float"
                    | "string"
                    | "bool"
                    | "array"
                    | "callable"
                    | "iterable"
                    | "object"
                    | "mixed"
                    | "null"
                    | "false"
                    | "true"
            )
        });
    return_value(rv, Value::bool(builtin))
}

fn reflection_type_allows_null(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_allows_null")),
    )
}

fn class_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let owner = with_argument(ed, 1, |value| {
        value
            .as_object()
            .map(|object| object.class_name.to_string())
            .unwrap_or_else(|| argument_string(ed, 1))
    });
    if eg.find_public_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        reflection_exception(eg, format!("Class \"{owner}\" does not exist"));
        return Ok(());
    }
    let owner = eg.find_public_class(&owner).map_or(owner, |class| {
        class
            .anonymous_public_name()
            .unwrap_or_else(|| class.name.clone())
    });
    set_target(ed, "class", owner.clone());
    with_argument(ed, 0, |value| {
        if let Some(mut object) = value.as_object_mut() {
            object.set_property("name", Value::string(owner));
        }
    });
    Ok(())
}

fn constant_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = argument_string(ed, 1);
    if eg.find_constant(&name).is_none() {
        reflection_exception(eg, format!("Constant \"{name}\" does not exist"));
        return Ok(());
    }
    with_argument(ed, 0, |value| {
        if let Some(mut object) = value.as_object_mut() {
            object.set_property("name", Value::string(name));
        }
    });
    Ok(())
}

fn class_constant_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let class_name = with_argument(ed, 1, |value| {
        value
            .as_object()
            .map(|object| object.class_name.to_string())
            .unwrap_or_else(|| argument_string(ed, 1))
    });
    let constant_name = argument_string(ed, 2);
    if eg.find_class(&class_name).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &class_name)?
    {
        reflection_exception(eg, format!("Class \"{class_name}\" does not exist"));
        return Ok(());
    }
    let target = eg.find_class(&class_name).and_then(|class| {
        if let Some(constant) = class
            .constants
            .iter()
            .find(|constant| constant.name == constant_name)
        {
            let visibility = match constant.visibility {
                Visibility::Public => 1,
                Visibility::Protected => 2,
                Visibility::Private => 4,
            };
            return Some((
                constant.declaring_class.clone(),
                constant.value.clone(),
                visibility | if constant.is_final { 32 } else { 0 },
            ));
        }
        if !class.is_enum {
            return None;
        }
        class
            .static_properties
            .iter()
            .position(|case| case.name == constant_name)
            .and_then(|index| enum_case_value(eg, class.class_id, index))
            .map(|value| (class.name.clone(), value, 1))
    });
    let Some((declaring_class, value, modifiers)) = target else {
        reflection_exception(
            eg,
            format!("Constant {class_name}::{constant_name} does not exist"),
        );
        return Ok(());
    };
    with_argument(ed, 0, |receiver| {
        if let Some(mut object) = receiver.as_object_mut() {
            object.set_property("name", Value::string(constant_name));
            object.set_property("class", Value::string(declaring_class.clone()));
            object.set_property(
                "__reflection_declaring_class",
                Value::string(declaring_class),
            );
            object.set_property("__reflection_modifiers", Value::long(modifiers));
            object.set_property("__reflection_value", value);
        }
    });
    Ok(())
}

fn constant_get_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(name) =
        reflected_property(ed, "name").and_then(|value| value.as_str().map(str::to_owned))
    else {
        return return_value(rv, Value::null());
    };
    return_value(rv, eg.find_constant(&name).unwrap_or_else(Value::null))
}

fn constant_to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let rendered = reflected_property(ed, "name")
        .and_then(|name| {
            let name = name.as_str()?;
            let value = eg.find_constant(name)?;
            Some(format!(
                "Constant [ {} {name} ] {{ {} }}\n",
                value.diagnostic_type_name(),
                reflection_constant_value_name(&value)
            ))
        })
        .unwrap_or_default();
    return_value(rv, Value::string(rendered))
}

fn reflection_quoted_string(value: &str) -> String {
    let mut rendered = String::with_capacity(value.len() + 2);
    rendered.push('\'');
    for character in value.chars() {
        match character {
            '\\' => rendered.push_str("\\\\"),
            '\n' => rendered.push_str("\\n"),
            '\r' => rendered.push_str("\\r"),
            '\t' => rendered.push_str("\\t"),
            '\u{000b}' => rendered.push_str("\\v"),
            '\u{000c}' => rendered.push_str("\\f"),
            '\0' => rendered.push_str("\\0"),
            _ => rendered.push(character),
        }
    }
    rendered.push('\'');
    rendered
}

fn reflection_value_name(value: &Value, eg: &ExecutorGlobals) -> String {
    match value.value_type() {
        ValueType::Undef | ValueType::Null => "NULL".to_string(),
        ValueType::True => "true".to_string(),
        ValueType::False => "false".to_string(),
        ValueType::Long => value.as_long().unwrap().to_string(),
        ValueType::Double => value.echo_to_string(),
        ValueType::String => reflection_quoted_string(value.as_str().unwrap()),
        ValueType::Array => {
            let array = value.as_array().unwrap();
            let is_list = array
                .iter()
                .enumerate()
                .all(|(index, (key, _))| key == ArrayKey::Int(index as i64));
            let entries = array
                .iter()
                .map(|(key, value)| {
                    let value = reflection_value_name(value, eg);
                    if is_list {
                        value
                    } else {
                        let key = match key {
                            ArrayKey::Int(key) => key.to_string(),
                            ArrayKey::String(key) => reflection_quoted_string(&key),
                        };
                        format!("{key} => {value}")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{entries}]")
        }
        ValueType::Object => super::enum_case_export(value, eg).unwrap_or_else(|| {
            value.as_object().map_or_else(
                || "NULL".to_string(),
                |object| object.class_name.to_string(),
            )
        }),
        _ => "NULL".to_string(),
    }
}

fn reflection_constant_value_name(value: &Value) -> String {
    match value.value_type() {
        ValueType::Undef | ValueType::Null | ValueType::False => String::new(),
        ValueType::True => "1".to_string(),
        ValueType::Long => value.as_long().unwrap().to_string(),
        ValueType::Double => value.echo_to_string(),
        ValueType::String => value.as_str().unwrap().to_string(),
        ValueType::Array => "Array".to_string(),
        ValueType::Object | ValueType::Closure => value.diagnostic_type_name().into_owned(),
        ValueType::Resource => "Resource".to_string(),
        ValueType::Reference => reflection_constant_value_name(&value.dereferenced()),
    }
}

fn render_reflection_class_constant(constant: &ClassConstantDefinition, value: &Value) -> String {
    let final_modifier = if constant.is_final { "final " } else { "" };
    let type_name = if matches!(constant.type_hint, ParamTypeHint::None) {
        value.diagnostic_type_name().into_owned()
    } else {
        constant.type_hint.display_name()
    };
    format!(
        "Constant [ {final_modifier}{} {type_name} {} ] {{ {} }}",
        reflection_visibility(constant.visibility),
        constant.name,
        reflection_constant_value_name(value)
    )
}

fn reflection_class_constant_definition<'a>(
    ed: *mut ExecuteData,
    eg: &'a ExecutorGlobals,
) -> Option<&'a ClassConstantDefinition> {
    let class = reflected_property(ed, "class")?;
    let name = reflected_property(ed, "name")?;
    eg.find_class(class.as_str()?)?
        .constants
        .iter()
        .find(|constant| constant.name == name.as_str().unwrap_or_default())
}

fn class_constant_to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let rendered =
        reflection_class_constant_definition(ed, eg).map_or_else(String::new, |constant| {
            let value = reflected_property(ed, "__reflection_value")
                .unwrap_or_else(|| constant.value.clone());
            format!("{}\n", render_reflection_class_constant(constant, &value))
        });
    return_value(rv, Value::string(rendered))
}

fn reflection_visibility(visibility: Visibility) -> &'static str {
    match visibility {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        Visibility::Private => "private",
    }
}

fn render_reflection_property(
    property: &PropertyDefinition,
    is_static: bool,
    eg: &ExecutorGlobals,
) -> String {
    let mut declaration = String::new();
    if property.is_final() {
        declaration.push_str("final ");
    }
    if property.abstract_get_hook() || property.abstract_set_hook() {
        declaration.push_str("abstract ");
    }
    declaration.push_str(reflection_visibility(property.visibility));
    declaration.push(' ');
    let set_visibility = property.set_visibility.or_else(|| {
        (!eg.class_is_internal(&property.declaring_class)
            && eg
                .find_class(&property.declaring_class)
                .is_some_and(|class| class.is_enum))
        .then_some(Visibility::Protected)
    });
    if let Some(set_visibility) = set_visibility {
        declaration.push_str(reflection_visibility(set_visibility));
        declaration.push_str("(set) ");
    }
    if is_static {
        declaration.push_str("static ");
    }
    if property.is_readonly {
        declaration.push_str("readonly ");
    }
    if property.is_virtual_hook_property() {
        declaration.push_str("virtual ");
    }
    if !matches!(property.type_hint, ParamTypeHint::None) {
        declaration.push_str(&property.type_hint.display_name());
        declaration.push(' ');
    }
    declaration.push('$');
    declaration.push_str(&property.name);
    if property.has_default() {
        declaration.push_str(" = ");
        declaration.push_str(&property.default.as_ref().map_or_else(
            || "NULL".to_string(),
            |value| reflection_value_name(value, eg),
        ));
    }
    if property.has_get_hook || property.has_set_hook {
        declaration.push_str(" {");
        if property.has_get_hook {
            declaration.push_str(" get;");
        }
        if property.has_set_hook {
            declaration.push_str(" set;");
        }
        declaration.push_str(" }");
    }
    format!("Property [ {declaration} ]")
}

fn reflection_property_definition<'a>(
    ed: *mut ExecuteData,
    eg: &'a ExecutorGlobals,
) -> Option<(&'a PropertyDefinition, bool)> {
    let metadata = reflection_property_metadata(ed, eg);
    let class_name = metadata
        .and_then(|metadata| {
            metadata
                .target
                .as_object()
                .map(|object| object.class_name.to_string())
                .or_else(|| metadata.target.as_str().map(str::to_owned))
        })
        .or_else(|| {
            reflected_property(ed, "class").and_then(|class| class.as_str().map(str::to_owned))
        })?;
    let name = metadata.map_or_else(
        || reflected_property(ed, "name").and_then(|name| name.as_str().map(str::to_owned)),
        |metadata| Some(metadata.property.clone()),
    )?;
    let class = eg.find_class(&class_name)?;
    class
        .properties
        .iter()
        .find(|property| property.name == name)
        .map(|property| (property, false))
        .or_else(|| {
            class
                .static_properties
                .iter()
                .find(|property| property.name == name)
                .map(|property| (property, true))
        })
}

const PROPERTY_HOOK_TYPE: &str = "PropertyHookType";

fn property_hook_type_case(eg: &ExecutorGlobals, backing_value: &str) -> Option<Value> {
    let class = eg.find_class(PROPERTY_HOOK_TYPE)?;
    let index = class.static_properties.iter().position(|case| {
        case.default.as_ref().is_some_and(|value| {
            value.as_object().is_some_and(|object| {
                object
                    .get_property("value")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == backing_value)
            })
        })
    })?;
    let storage = eg.static_property_storage_slot(class.class_id, index)?;
    eg.static_property_value(storage).cloned()
}

fn property_hook_type_cases(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let mut cases = PhpArray::with_packed_capacity(2);
    for value in ["get", "set"] {
        if let Some(case) = property_hook_type_case(eg, value) {
            cases.push(case);
        }
    }
    return_value(rv, Value::array(cases))
}

fn property_hook_type_try_from(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = argument_string(ed, 1);
    return_value(
        rv,
        property_hook_type_case(eg, &value).unwrap_or_else(Value::null),
    )
}

fn property_hook_type_from(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = argument_string(ed, 1);
    let Some(case) = property_hook_type_case(eg, &value) else {
        eg.exception = Some(make_error_value(
            "ValueError",
            &format!("\"{value}\" is not a valid backing value for enum PropertyHookType"),
        ));
        return Ok(());
    };
    return_value(rv, case)
}

fn property_hook_kind(ed: *mut ExecuteData) -> Option<String> {
    with_argument(ed, 1, |value| {
        let object = value.as_object()?;
        object
            .get_property("value")
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
}

fn reflected_property_hook(
    ed: *mut ExecuteData,
    eg: &ExecutorGlobals,
    hook: &str,
) -> Option<Value> {
    let (property, is_static) = reflection_property_definition(ed, eg)?;
    if is_static
        || match hook {
            "get" => !property.has_get_hook,
            "set" => !property.has_set_hook,
            _ => true,
        }
    {
        return None;
    }
    let owner = property.declaring_class.clone();
    let method = format!("${}::{hook}", property.name);
    let (name, visibility, is_static, is_final, function, declaring_class) =
        find_reflected_method(eg, &owner, &method)?;
    Some(reflected_method_value(
        name,
        visibility,
        is_static,
        is_final,
        function,
        declaring_class,
    ))
}

fn property_get_hook(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = property_hook_kind(ed)
        .as_deref()
        .and_then(|hook| reflected_property_hook(ed, eg, hook))
        .unwrap_or_else(Value::null);
    return_value(rv, value)
}

fn property_has_hook(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let found = property_hook_kind(ed).as_deref().is_some_and(|hook| {
        let Some((property, is_static)) = reflection_property_definition(ed, eg) else {
            return false;
        };
        !is_static
            && match hook {
                "get" => property.has_get_hook,
                "set" => property.has_set_hook,
                _ => false,
            }
    });
    return_value(rv, Value::bool(found))
}

fn property_get_hooks(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let mut hooks = PhpArray::with_hash_capacity(2);
    for hook in ["get", "set"] {
        if let Some(method) = reflected_property_hook(ed, eg, hook) {
            hooks.set_str(hook, method);
        }
    }
    return_value(rv, Value::array(hooks))
}

fn property_to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = reflection_property_metadata(ed, eg)
        .map(|metadata| metadata.property.clone())
        .or_else(|| {
            reflected_property(ed, "name").and_then(|name| name.as_str().map(str::to_owned))
        })
        .unwrap_or_default();
    let rendered = reflection_property_definition(ed, eg).map_or_else(
        || format!("Property [ <dynamic> public ${name} ]\n"),
        |(property, is_static)| {
            format!("{}\n", render_reflection_property(property, is_static, eg))
        },
    );
    return_value(rv, Value::string(rendered))
}

fn render_reflection_enum_builtin_method(name: &str) -> Option<String> {
    let (prototype, parameters, return_type) = if name.eq_ignore_ascii_case("cases") {
        ("UnitEnum", Vec::new(), "array")
    } else if name.eq_ignore_ascii_case("from") {
        (
            "BackedEnum",
            vec!["Parameter #0 [ <required> string|int $value ]"],
            "static",
        )
    } else if name.eq_ignore_ascii_case("tryFrom") {
        (
            "BackedEnum",
            vec!["Parameter #0 [ <required> string|int $value ]"],
            "?static",
        )
    } else {
        return None;
    };
    let mut rendered =
        format!("Method [ <internal, prototype {prototype}> static public method {name} ] {{\n\n");
    rendered.push_str(&format!("      - Parameters [{}] {{\n", parameters.len()));
    for parameter in parameters {
        rendered.push_str("        ");
        rendered.push_str(parameter);
        rendered.push('\n');
    }
    rendered.push_str("      }\n");
    rendered.push_str(&format!("      - Return [ {return_type} ]\n"));
    rendered.push_str("    }");
    Some(rendered)
}

fn render_reflection_signature_parameter(
    function: &FunctionCommon,
    index: u32,
    variadic: bool,
    eg: &ExecutorGlobals,
) -> String {
    let name = function
        .sig
        .param_names
        .get(index as usize)
        .cloned()
        .unwrap_or_else(|| format!("arg{}", index + 1));
    let hint = function
        .sig
        .param_type_hints
        .get(index as usize)
        .unwrap_or(&ParamTypeHint::None);
    let type_prefix = if matches!(hint, ParamTypeHint::None) {
        String::new()
    } else {
        format!("{} ", hint.display_name())
    };
    let requirement = if variadic || index >= function.sig.required_num_args {
        "optional"
    } else {
        "required"
    };
    let reference = if function.sig.is_param_by_ref(index) {
        "&"
    } else {
        ""
    };
    let variadic_prefix = if variadic { "..." } else { "" };
    let default = if !variadic && index >= function.sig.required_num_args {
        eg.internal_function_parameter_default_diagnostic(
            function as *const FunctionCommon,
            index as usize,
        )
        .map(|diagnostic| format!(" = {diagnostic}"))
        .or_else(|| {
            eg.internal_function_parameter_default(
                function as *const FunctionCommon,
                index as usize,
            )
            .map(|value| format!(" = {}", reflection_default_text(value)))
        })
        .unwrap_or_else(|| " = <default>".to_string())
    } else {
        String::new()
    };
    format!(
        "Parameter #{index} [ <{requirement}> {type_prefix}{reference}{variadic_prefix}${name}{default} ]"
    )
}

fn reflection_user_source_span(user: &UserFunction) -> Option<(usize, usize)> {
    let start = user.op_array.declaration_line().or_else(|| {
        user.op_array
            .source_lines
            .iter()
            .find_map(|(index, line)| (*index != u32::MAX).then_some(*line as usize))
    })?;
    let end = user
        .op_array
        .source_lines
        .iter()
        .filter_map(|(index, line)| (*index != u32::MAX).then_some(*line as usize))
        .max()
        .unwrap_or(start);
    Some((start, end))
}

#[allow(clippy::too_many_arguments)]
fn render_reflection_method_details(
    function: &FunctionCommon,
    user: Option<&UserFunction>,
    name: &str,
    visibility: Visibility,
    is_static: bool,
    is_final: bool,
    is_abstract: bool,
    closure_method: bool,
    eg: &ExecutorGlobals,
) -> String {
    let mut modifiers = String::new();
    if is_final {
        modifiers.push_str("final ");
    }
    if is_abstract {
        modifiers.push_str("abstract ");
    }
    modifiers.push_str(reflection_visibility(visibility));
    modifiers.push(' ');
    if is_static {
        modifiers.push_str("static ");
    }
    let provenance = if function.fn_type == FunctionType::User && !closure_method {
        "user"
    } else {
        "internal"
    };
    let callable_kind = if name.eq_ignore_ascii_case("__construct") {
        format!("{provenance}, ctor")
    } else {
        provenance.to_string()
    };
    let mut rendered = format!("Method [ <{callable_kind}> {modifiers}method {name} ] {{\n");
    if closure_method {
        rendered.push('\n');
    }
    if !closure_method
        && let Some(user) = user
        && !user.op_array.source_file.is_empty()
        && let Some((start, end)) = reflection_user_source_span(user)
    {
        rendered.push_str(&format!(
            "  @@ {} {start} - {end}\n\n",
            user.op_array.source_file
        ));
    }
    let fixed = function.sig.public_arity();
    let parameter_count = fixed + u32::from(function.sig.is_variadic);
    let implicit_setter_return = name.ends_with("::set");
    let has_return =
        !matches!(function.sig.return_type_hint, ParamTypeHint::None) || implicit_setter_return;
    if parameter_count != 0 || has_return {
        rendered.push_str(&format!("  - Parameters [{parameter_count}] {{\n"));
        for index in 0..parameter_count {
            let variadic = function.sig.is_variadic && index == fixed;
            rendered.push_str("    ");
            rendered.push_str(&render_reflection_signature_parameter(
                function, index, variadic, eg,
            ));
            rendered.push('\n');
        }
        rendered.push_str("  }\n");
    }
    if !matches!(function.sig.return_type_hint, ParamTypeHint::None) {
        rendered.push_str(&format!(
            "  - Return [ {} ]\n",
            function.sig.return_type_hint.display_name()
        ));
    } else if implicit_setter_return {
        rendered.push_str("  - Return [ void ]\n");
    }
    if parameter_count == 0 && !has_return && rendered.ends_with("\n\n") {
        rendered.pop();
    }
    rendered.push_str("}\n");
    rendered
}

fn function_to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(function) = reflected_function(ed) else {
        return return_value(rv, Value::string(""));
    };
    let name = reflected_property(ed, "name")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    let is_closure = reflected_property(ed, "__reflection_closure")
        .is_some_and(|value| value.value_type() == ValueType::Closure);
    let kind = if is_closure { "Closure" } else { "Function" };
    let provenance = if function.fn_type == FunctionType::User {
        "user".to_string()
    } else {
        format!(
            "internal:{}",
            eg.internal_function_extension(function as *const FunctionCommon)
                .unwrap_or("Core")
        )
    };
    let reference = if function.sig.returns_reference {
        "&"
    } else {
        ""
    };
    let mut rendered = format!("{kind} [ <{provenance}> function {reference}{name} ] {{\n");

    if let Some(user) = reflected_user_function(ed)
        && !user.op_array.source_file.is_empty()
        && let Some((start, end)) = reflection_user_source_span(user)
    {
        rendered.push_str(&format!(
            "  @@ {} {start} - {end}\n\n",
            user.op_array.source_file
        ));
    } else {
        rendered.push('\n');
    }

    let fixed = function.sig.public_arity();
    let parameter_count = fixed + u32::from(function.sig.is_variadic);
    if parameter_count != 0 {
        rendered.push_str(&format!("  - Parameters [{parameter_count}] {{\n"));
        for index in 0..parameter_count {
            let variadic = function.sig.is_variadic && index == fixed;
            rendered.push_str("    ");
            rendered.push_str(&render_reflection_signature_parameter(
                function, index, variadic, eg,
            ));
            rendered.push('\n');
        }
        rendered.push_str("  }\n");
    }
    if !matches!(function.sig.return_type_hint, ParamTypeHint::None) {
        rendered.push_str(&format!(
            "  - Return [ {} ]\n",
            function.sig.return_type_hint.display_name()
        ));
    }
    if parameter_count == 0
        && matches!(function.sig.return_type_hint, ParamTypeHint::None)
        && rendered.ends_with("\n\n")
    {
        rendered.pop();
    }
    rendered.push_str("}\n");
    return_value(rv, Value::string(rendered))
}

fn method_to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(function) = reflected_function(ed) else {
        return return_value(rv, Value::string(""));
    };
    let name = reflected_property(ed, "name")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    let visibility = reflected_property(ed, "__reflection_method_visibility")
        .and_then(|value| value.as_long())
        .unwrap_or(1);
    let visibility = match visibility {
        2 => Visibility::Protected,
        4 => Visibility::Private,
        _ => Visibility::Public,
    };
    let is_final = parameter_property_bool(ed, "__reflection_method_final");
    let is_abstract = parameter_property_bool(ed, "__reflection_method_abstract");
    let is_static = parameter_property_bool(ed, "__reflection_method_static");
    let closure_method = parameter_property_bool(ed, "__reflection_closure_method");
    let rendered = render_reflection_method_details(
        function,
        reflected_user_function(ed),
        &name,
        visibility,
        is_static,
        is_final,
        is_abstract,
        closure_method,
        eg,
    );
    return_value(rv, Value::string(rendered))
}

fn class_to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::string(""));
    };
    let Some(class) = eg.find_class(&owner) else {
        return return_value(rv, Value::string(""));
    };
    let is_user_enum = class.is_enum && !eg.class_is_internal(&owner);
    let is_object = with_argument(ed, 0, |value| {
        value
            .as_object()
            .is_some_and(|object| object.class_name.eq_ignore_ascii_case("ReflectionObject"))
    });
    let mut modifiers = String::new();
    if class.is_final && !is_user_enum {
        modifiers.push_str("final ");
    }
    if class.is_abstract && !class.is_interface {
        modifiers.push_str("abstract ");
    }
    if class.is_readonly {
        modifiers.push_str("readonly ");
    }
    let kind = if class.is_interface {
        "interface"
    } else if class.is_trait {
        "trait"
    } else if class.is_enum {
        "enum"
    } else {
        "class"
    };
    let provenance = if eg.class_is_internal(&owner) {
        "internal"
    } else {
        "user"
    };
    let title = if is_object {
        "Object of class"
    } else if class.is_interface {
        "Interface"
    } else if class.is_trait {
        "Trait"
    } else if class.is_enum {
        "Enum"
    } else {
        "Class"
    };
    let iterateable = if class
        .properties
        .iter()
        .any(|property| property.has_get_hook || property.has_set_hook)
    {
        " <iterateable>"
    } else {
        ""
    };
    let enum_backing_type = is_user_enum.then(|| {
        class
            .properties
            .iter()
            .find(|property| property.name == "value")
            .map(|property| property.type_hint.display_name())
    });
    let enum_backing_type = enum_backing_type.flatten();
    let backing_declaration = enum_backing_type
        .as_deref()
        .map(|backing_type| format!(": {backing_type}"))
        .unwrap_or_default();
    let implements_declaration = if is_user_enum && !class.implements.is_empty() {
        format!(" implements {}", class.implements.join(", "))
    } else {
        String::new()
    };
    let mut rendered = format!(
        "{title} [ <{provenance}>{iterateable} {modifiers}{kind} {}{backing_declaration}{implements_declaration} ] {{\n",
        class.name
    );
    if let Some(source_file) = &class.source_file {
        let member_end = class
            .methods
            .iter()
            .filter_map(|(_, _, _, _, method)| {
                reflection_user_source_span(method).map(|(_, end)| end)
            })
            .chain(
                class
                    .properties
                    .iter()
                    .map(|property| property.reflection_order),
            )
            .max()
            .unwrap_or(class.declaration_line);
        let declaration_end = if member_end > class.declaration_line {
            member_end + 1
        } else {
            class.declaration_line
        };
        rendered.push_str(&format!(
            "  @@ {source_file} {}-{declaration_end}\n\n",
            class.declaration_line
        ));
    }

    if is_user_enum && !class.static_properties.is_empty() {
        rendered.push_str(&format!(
            "  - Enum cases [{}] {{\n",
            class.static_properties.len()
        ));
        for case in &class.static_properties {
            rendered.push_str("    Case ");
            rendered.push_str(&case.name);
            if let Some(value) = case
                .default
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|case| case.get_property("value").cloned())
            {
                rendered.push_str(" = ");
                rendered.push_str(&value.echo_to_string_with_precision(eg.precision));
            }
            rendered.push('\n');
        }
        rendered.push_str("  }\n\n");
    }

    rendered.push_str(&format!("  - Constants [{}] {{\n", class.constants.len()));
    for constant in &class.constants {
        rendered.push_str("    ");
        rendered.push_str(&render_reflection_class_constant(constant, &constant.value));
        rendered.push('\n');
    }
    rendered.push_str("  }\n\n");

    let reflected_static_properties = if is_user_enum {
        &[][..]
    } else {
        class.static_properties.as_slice()
    };
    rendered.push_str(&format!(
        "  - Static properties [{}] {{\n",
        reflected_static_properties.len()
    ));
    for property in reflected_static_properties {
        rendered.push_str("    ");
        rendered.push_str(&render_reflection_property(property, true, eg));
        rendered.push('\n');
    }
    rendered.push_str("  }\n\n");

    let mut methods = Vec::new();
    collect_reflected_methods(eg, &owner, &mut methods, &mut HashSet::new());
    methods.retain(|(name, ..)| !name.starts_with('$'));
    if is_user_enum {
        methods.sort_by_key(|(name, ..)| {
            if name.eq_ignore_ascii_case("cases") {
                1
            } else if name.eq_ignore_ascii_case("from") {
                2
            } else if name.eq_ignore_ascii_case("tryFrom") {
                3
            } else {
                0
            }
        });
    }
    let static_method_count = methods
        .iter()
        .filter(|(_, _, is_static, ..)| *is_static)
        .count();
    rendered.push_str(&format!("  - Static methods [{static_method_count}] {{\n"));
    let mut rendered_static_method = false;
    for (name, visibility, is_static, is_final, function, declaring_class) in &methods {
        if !is_static {
            continue;
        }
        if is_user_enum && rendered_static_method {
            rendered.push('\n');
        }
        if is_user_enum && let Some(method) = render_reflection_enum_builtin_method(name) {
            rendered.push_str("    ");
            rendered.push_str(&method);
            rendered.push('\n');
        } else if let Some(function) = eg.registered_function_common(*function) {
            let method = render_reflection_method_details(
                function,
                reflected_user_function_from_common(function),
                name,
                *visibility,
                true,
                *is_final,
                eg.find_class(declaring_class)
                    .is_some_and(|class| class.method_is_abstract(name)),
                false,
                eg,
            );
            for line in method.lines() {
                if line.is_empty() {
                    rendered.push('\n');
                } else {
                    rendered.push_str("    ");
                    rendered.push_str(line);
                    rendered.push('\n');
                }
            }
        }
        rendered_static_method = true;
    }
    rendered.push_str("  }\n\n");

    rendered.push_str(&format!("  - Properties [{}] {{\n", class.properties.len()));
    for property in &class.properties {
        rendered.push_str("    ");
        rendered.push_str(&render_reflection_property(property, false, eg));
        rendered.push('\n');
    }
    rendered.push_str("  }\n\n");

    if is_object {
        let mut dynamic_names = Vec::new();
        if let Some(target) = reflected_property(ed, "__generic_object")
            && let Some(object) = target.as_object()
        {
            object.for_each_dynamic_property(|name, _| dynamic_names.push(name.to_string()));
        }
        rendered.push_str(&format!(
            "  - Dynamic properties [{}] {{\n",
            dynamic_names.len()
        ));
        for name in dynamic_names {
            rendered.push_str(&format!("    Property [ <dynamic> public ${name} ]\n"));
        }
        rendered.push_str("  }\n\n");
    }

    let instance_method_count = methods.len() - static_method_count;
    rendered.push_str(&format!("  - Methods [{instance_method_count}] {{\n"));
    for (name, visibility, is_static, is_final, function, declaring_class) in &methods {
        if *is_static {
            continue;
        }
        let Some(function) = eg.registered_function_common(*function) else {
            continue;
        };
        let method = render_reflection_method_details(
            function,
            reflected_user_function_from_common(function),
            name,
            *visibility,
            false,
            *is_final,
            eg.find_class(declaring_class)
                .is_some_and(|class| class.method_is_abstract(name)),
            false,
            eg,
        );
        for line in method.lines() {
            if line.is_empty() {
                rendered.push('\n');
            } else {
                rendered.push_str("    ");
                rendered.push_str(line);
                rendered.push('\n');
            }
        }
    }
    rendered.push_str("  }\n}\n");
    return_value(rv, Value::string(rendered))
}

fn class_get_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed)
        && let Some(public_name) = eg
            .find_class(&owner)
            .and_then(|class| class.anonymous_public_name())
    {
        return return_value(rv, Value::string(public_name));
    }
    return_value(
        rv,
        reflected_property(ed, "name").unwrap_or_else(|| Value::string("")),
    )
}

fn class_debug_info(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let mut properties = PhpArray::with_hash_capacity(1);
    properties.set_str(
        "name",
        reflected_property(ed, "name").unwrap_or_else(|| Value::string("")),
    );
    return_value(rv, Value::array(properties))
}

fn class_get_attributes(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let attributes = reflected_attribute_definitions(ed, eg);
    reflection_attributes(ed, rv, eg, attributes)
}

fn reflection_get_doc_comment(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if matches!(
        receiver_class_name(ed).as_deref(),
        Some("ReflectionClassConstant")
    ) {
        let owner =
            reflected_property(ed, "class").and_then(|value| value.as_str().map(str::to_owned));
        let name =
            reflected_property(ed, "name").and_then(|value| value.as_str().map(str::to_owned));
        let comment = owner
            .as_deref()
            .zip(name.as_deref())
            .and_then(|(owner, name)| {
                eg.find_class(owner)?
                    .constants
                    .iter()
                    .find(|constant| constant.name == name)?
                    .doc_comment()
                    .map(str::to_owned)
            });
        if let Some(comment) = comment {
            return return_value(rv, Value::string(comment));
        }
    }
    // Other declaration kinds still deliberately discard comments. Returning
    // false is PHP's truthful "no retained doc comment" result.
    return_value(rv, Value::bool(false))
}

fn class_get_parent(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::bool(false));
    }
    let Some(parent) = eg
        .find_class(&owner)
        .and_then(|class| class.parent.as_ref())
        .cloned()
    else {
        return return_value(rv, Value::bool(false));
    };
    return_value(
        rv,
        object_value(
            "ReflectionClass",
            [
                ("__generic_kind", Value::string("class")),
                ("__generic_owner", Value::string(parent.clone())),
                ("name", Value::string(parent)),
            ],
        ),
    )
}

fn class_is_internal(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::bool(false));
    }
    return_value(rv, Value::bool(eg.class_is_internal(&owner)))
}

fn class_is_user_defined(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::bool(false));
    }
    return_value(rv, Value::bool(!eg.class_is_internal(&owner)))
}

fn class_is_subclass_of(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    let target = with_argument(ed, 1, |value| {
        value
            .as_object()
            .and_then(|object| object.get_property("name").cloned())
            .and_then(|name| name.as_str().map(str::to_owned))
            .or_else(|| value.as_str().map(str::to_owned))
    });
    let Some(target) = target else {
        return return_value(rv, Value::bool(false));
    };
    if (eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?)
        || (eg.find_class(&target).is_none()
            && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &target)?)
    {
        return return_value(rv, Value::bool(false));
    }
    let same_identity = eg
        .find_class(&owner)
        .zip(eg.find_class(&target))
        .is_some_and(|(owner, target)| std::ptr::eq(owner, target));
    return_value(
        rv,
        Value::bool(!same_identity && eg.class_is_a(&owner, &target)),
    )
}

fn class_implements_interface(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    let target = argument_string(ed, 1);
    if (eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?)
        || (eg.find_class(&target).is_none()
            && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &target)?)
    {
        return return_value(rv, Value::bool(false));
    }
    return_value(
        rv,
        Value::bool(eg.class_is_interface(&target) && eg.class_is_a(&owner, &target)),
    )
}

fn class_get_interface_names(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let names = eg.class_interface_names(&owner);
    let mut result = PhpArray::with_packed_capacity(names.len());
    for name in names {
        result.push(Value::string(name));
    }
    return_value(rv, Value::array(result))
}

fn reflected_class_map(names: impl IntoIterator<Item = String>) -> Value {
    let mut result = PhpArray::new();
    for name in names {
        result.set_str(
            &name,
            object_value(
                "ReflectionClass",
                [
                    ("__generic_kind", Value::string("class")),
                    ("__generic_owner", Value::string(name.clone())),
                    ("name", Value::string(name.clone())),
                ],
            ),
        );
    }
    Value::array(result)
}

fn class_get_interfaces(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let names = eg.class_interface_names(&owner);
    return_value(rv, reflected_class_map(names))
}

fn class_get_trait_names(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let Some(class) = eg.find_class(&owner) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let mut result = PhpArray::with_packed_capacity(class.uses.len());
    for name in &class.uses {
        result.push(Value::string(name.clone()));
    }
    return_value(rv, Value::array(result))
}

fn class_get_traits(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let names = eg
        .find_class(&owner)
        .map(|class| class.uses.clone())
        .unwrap_or_default();
    return_value(rv, reflected_class_map(names))
}

fn class_get_trait_aliases(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let Some(class) = eg.find_class(&owner) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let mut result = PhpArray::new();
    for adaptation in &class.trait_aliases {
        let Some(alias) = adaptation.alias.as_deref() else {
            continue;
        };
        let source_trait = adaptation
            .trait_name
            .as_ref()
            .and_then(|name| {
                class
                    .uses
                    .iter()
                    .find(|used| used.eq_ignore_ascii_case(name))
            })
            .or_else(|| {
                class.uses.iter().find(|used| {
                    eg.find_class(used).is_some_and(|definition| {
                        definition
                            .methods
                            .iter()
                            .any(|(name, _, _, _, _)| name.eq_ignore_ascii_case(&adaptation.method))
                    })
                })
            });
        if let Some(source_trait) = source_trait {
            result.set_str(
                alias,
                Value::string(format!("{source_trait}::{}", adaptation.method)),
            );
        }
    }
    return_value(rv, Value::array(result))
}

fn enum_case_value(eg: &ExecutorGlobals, class_id: u32, case_index: usize) -> Option<Value> {
    eg.static_property_storage_slot(class_id, case_index)
        .and_then(|slot| eg.static_property_value(slot))
        .cloned()
}

fn enum_is_backed(owner: &str, eg: &ExecutorGlobals) -> bool {
    eg.find_class(owner).is_some_and(|class| {
        class
            .implements
            .iter()
            .any(|interface| interface.eq_ignore_ascii_case("BackedEnum"))
    })
}

fn reflected_enum_case(owner: &str, name: &str, eg: &ExecutorGlobals) -> Option<Value> {
    let class = eg.find_class(owner)?;
    if !class.is_enum {
        return None;
    }
    let index = class
        .static_properties
        .iter()
        .position(|case| case.name == name)?;
    let value = enum_case_value(eg, class.class_id, index)?;
    let reflection_class = if enum_is_backed(owner, eg) {
        "ReflectionEnumBackedCase"
    } else {
        "ReflectionEnumUnitCase"
    };
    Some(object_value(
        reflection_class,
        [
            ("name", Value::string(name)),
            ("class", Value::string(owner)),
            ("__reflection_declaring_class", Value::string(owner)),
            ("__reflection_modifiers", Value::long(1)),
            ("__reflection_value", value),
        ],
    ))
}

fn enum_construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_construct(ed, rv, eg)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return Ok(());
    };
    if !eg.find_class(&owner).is_some_and(|class| class.is_enum) {
        reflection_exception(eg, format!("Class \"{owner}\" is not an enum"));
    }
    Ok(())
}

fn enum_has_case(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    let name = argument_string(ed, 1);
    return_value(
        rv,
        Value::bool(reflected_enum_case(&owner, &name, eg).is_some()),
    )
}

fn enum_get_case(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        reflection_exception(eg, "ReflectionEnum has no resolved enum");
        return Ok(());
    };
    let name = argument_string(ed, 1);
    let Some(case) = reflected_enum_case(&owner, &name, eg) else {
        reflection_exception(eg, format!("Case {owner}::{name} does not exist"));
        return Ok(());
    };
    return_value(rv, case)
}

fn enum_get_cases(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let names = eg.find_class(&owner).map_or_else(Vec::new, |class| {
        class
            .static_properties
            .iter()
            .map(|case| case.name.clone())
            .collect::<Vec<_>>()
    });
    let mut cases = PhpArray::with_packed_capacity(names.len());
    for name in names {
        if let Some(case) = reflected_enum_case(&owner, &name, eg) {
            cases.push(case);
        }
    }
    return_value(rv, Value::array(cases))
}

fn enum_is_backed_reflection(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let backed = generic_target(ed).is_some_and(|(kind, owner)| {
        kind == GenericDeclarationKind::Class && enum_is_backed(&owner, eg)
    });
    return_value(rv, Value::bool(backed))
}

fn enum_get_backing_type(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::null());
    };
    if !enum_is_backed(&owner, eg) {
        return return_value(rv, Value::null());
    }
    let backing_type = eg.find_class(&owner).and_then(|class| {
        class
            .properties
            .iter()
            .find(|property| property.name == "value")
            .map(|property| property.type_hint.display_name())
    });
    return_value(
        rv,
        backing_type.map_or_else(Value::null, |name| named_reflected_type(&name)),
    )
}

fn enum_case_construct_common(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    require_backed: bool,
) -> Result<(), VmError> {
    let owner = with_argument(ed, 1, |value| {
        value
            .as_object()
            .map(|object| object.class_name.to_string())
            .unwrap_or_else(|| argument_string(ed, 1))
    });
    let name = argument_string(ed, 2);
    class_constant_construct(ed, rv, eg)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let is_case = eg.find_class(&owner).is_some_and(|class| {
        class.is_enum && class.static_properties.iter().any(|case| case.name == name)
    });
    if !is_case {
        reflection_exception(eg, format!("Constant {owner}::{name} is not a case"));
    } else if require_backed && !enum_is_backed(&owner, eg) {
        reflection_exception(
            eg,
            format!("Enum case {owner}::{name} is not a backed case"),
        );
    }
    Ok(())
}

fn enum_unit_case_construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    enum_case_construct_common(ed, rv, eg, false)
}

fn enum_backed_case_construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    enum_case_construct_common(ed, rv, eg, true)
}

fn enum_case_get_enum(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let owner = reflected_property(ed, "class")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    return_value(
        rv,
        object_value(
            "ReflectionEnum",
            [
                ("__generic_kind", Value::string("class")),
                ("__generic_owner", Value::string(owner.clone())),
                ("name", Value::string(owner)),
            ],
        ),
    )
}

fn enum_case_get_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__reflection_value").unwrap_or_else(Value::null),
    )
}

fn enum_case_get_backing_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = reflected_property(ed, "__reflection_value").unwrap_or_else(Value::null);
    return_value(
        rv,
        value
            .as_object()
            .and_then(|object| object.get_property("value").cloned())
            .unwrap_or_else(Value::null),
    )
}

fn reflected_class_constant(
    name: &str,
    declaring_class: &str,
    modifiers: i64,
    value: Value,
) -> Value {
    object_value(
        "ReflectionClassConstant",
        [
            ("name", Value::string(name)),
            ("class", Value::string(declaring_class)),
            (
                "__reflection_declaring_class",
                Value::string(declaring_class),
            ),
            ("__reflection_modifiers", Value::long(modifiers)),
            ("__reflection_value", value),
        ],
    )
}

fn class_get_constants(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let Some(class) = eg.find_class(&owner) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let filter = with_argument(ed, 1, Value::as_long);
    let enum_case_count = class
        .is_enum
        .then_some(class.static_properties.len())
        .unwrap_or(0);
    let mut constants = PhpArray::with_hash_capacity(class.constants.len() + enum_case_count);
    if class.is_enum && !filter.is_some_and(|filter| filter & 1 == 0) {
        for (index, case) in class.static_properties.iter().enumerate() {
            if let Some(value) = enum_case_value(eg, class.class_id, index) {
                constants.set_str(&case.name, value);
            }
        }
    }
    for constant in &class.constants {
        let visibility = match constant.visibility {
            Visibility::Public => 1,
            Visibility::Protected => 2,
            Visibility::Private => 4,
        };
        let modifiers = visibility | if constant.is_final { 32 } else { 0 };
        if filter.is_some_and(|filter| modifiers & filter == 0) {
            continue;
        }
        constants.set_str(&constant.name, constant.value.clone());
    }
    return_value(rv, Value::array(constants))
}

fn class_get_constant(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::bool(false));
    }
    let name = argument_string(ed, 1);
    let (definition, enum_value) = eg.find_class(&owner).map_or((None, None), |class| {
        let definition = class
            .constants
            .iter()
            .find(|constant| constant.name == name)
            .cloned();
        let enum_value = (definition.is_none() && class.is_enum)
            .then(|| {
                class
                    .static_properties
                    .iter()
                    .position(|case| case.name == name)
                    .and_then(|index| enum_case_value(eg, class.class_id, index))
            })
            .flatten();
        (definition, enum_value)
    });
    if let Some(value) = enum_value {
        return return_value(rv, value);
    }
    let Some(definition) = definition else {
        return return_value(rv, Value::bool(false));
    };
    if let Some(error) = class_constant_evaluation_error_value(&definition) {
        crate::vm::execute::attach_internal_constant_expression_trace(&error, ed, eg);
        eg.exception = Some(error);
        return return_value(rv, Value::null());
    }
    if definition.value_is_deferred {
        let Some(value) = evaluate_deferred_class_constant_value(&definition, eg)? else {
            if let Some(exception) = eg.exception.as_ref() {
                crate::vm::execute::attach_internal_constant_expression_trace(exception, ed, eg);
            }
            return return_value(rv, Value::null());
        };
        return return_value(rv, value);
    }
    return_value(rv, definition.value)
}

fn class_get_reflection_constants(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let Some(class) = eg.find_class(&owner) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let filter = with_argument(ed, 1, Value::as_long);
    let enum_case_count = class
        .is_enum
        .then_some(class.static_properties.len())
        .unwrap_or(0);
    let mut constants = PhpArray::with_packed_capacity(class.constants.len() + enum_case_count);
    if class.is_enum && !filter.is_some_and(|filter| filter & 1 == 0) {
        for (index, case) in class.static_properties.iter().enumerate() {
            if let Some(value) = enum_case_value(eg, class.class_id, index) {
                constants.push(reflected_class_constant(&case.name, &class.name, 1, value));
            }
        }
    }
    for constant in &class.constants {
        let visibility = match constant.visibility {
            Visibility::Public => 1,
            Visibility::Protected => 2,
            Visibility::Private => 4,
        };
        let modifiers = visibility | if constant.is_final { 32 } else { 0 };
        if filter.is_some_and(|filter| modifiers & filter == 0) {
            continue;
        }
        constants.push(reflected_class_constant(
            &constant.name,
            &constant.declaring_class,
            modifiers,
            constant.value.clone(),
        ));
    }
    return_value(rv, Value::array(constants))
}

fn class_get_reflection_constant(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::bool(false));
    }
    let name = argument_string(ed, 1);
    let reflected = eg.find_class(&owner).and_then(|class| {
        if let Some(constant) = class
            .constants
            .iter()
            .find(|constant| constant.name == name)
        {
            let visibility = match constant.visibility {
                Visibility::Public => 1,
                Visibility::Protected => 2,
                Visibility::Private => 4,
            };
            let modifiers = visibility | if constant.is_final { 32 } else { 0 };
            return Some(reflected_class_constant(
                &constant.name,
                &constant.declaring_class,
                modifiers,
                constant.value.clone(),
            ));
        }
        if !class.is_enum {
            return None;
        }
        class
            .static_properties
            .iter()
            .position(|case| case.name == name)
            .and_then(|index| enum_case_value(eg, class.class_id, index))
            .map(|value| reflected_class_constant(&name, &class.name, 1, value))
    });
    let Some(reflected) = reflected else {
        return return_value(rv, Value::bool(false));
    };
    return_value(rv, reflected)
}

fn class_get_default_properties(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let Some(class) = eg.find_class(&owner) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let mut defaults =
        PhpArray::with_hash_capacity(class.properties.len() + class.static_properties.len());
    let static_properties = class.static_properties.iter().filter(|_| !class.is_enum);
    for property in static_properties.chain(class.properties.iter()) {
        if let Some(default) = &property.default {
            defaults.set_str(&property.name, default.clone());
        }
    }
    return_value(rv, Value::array(defaults))
}

fn method_modifiers(visibility: Visibility, is_static: bool, is_final: bool) -> i64 {
    let visibility = match visibility {
        Visibility::Public => 1,
        Visibility::Protected => 2,
        Visibility::Private => 4,
    };
    visibility | if is_static { 16 } else { 0 } | if is_final { 32 } else { 0 }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn collect_reflected_methods(
    eg: &ExecutorGlobals,
    owner: &str,
    methods: &mut Vec<(
        String,
        Visibility,
        bool,
        bool,
        *const FunctionCommon,
        String,
    )>,
    seen: &mut HashSet<String>,
) {
    let Some(class) = eg.find_class(owner) else {
        return;
    };
    for (name, visibility, is_static, is_final, function) in &class.methods {
        if class.method_is_abstract(name) || !seen.insert(name.to_ascii_lowercase()) {
            continue;
        }
        methods.push((
            name.clone(),
            *visibility,
            *is_static,
            *is_final,
            &function.common,
            class.name.clone(),
        ));
    }
    for method in eg.effective_composed_trait_methods(class) {
        if !seen.insert(method.target.to_ascii_lowercase()) {
            continue;
        }
        let Some(function) = eg.find_function(&format!("{}::{}", class.name, method.target)) else {
            continue;
        };
        methods.push((
            method.target,
            method.visibility,
            method.is_static,
            method.is_final,
            function,
            class.name.clone(),
        ));
    }
    if let Some(parent) = class.parent.clone() {
        collect_reflected_methods(eg, &parent, methods, seen);
    }
}

fn reflected_method_value(
    name: String,
    visibility: Visibility,
    is_static: bool,
    is_final: bool,
    function: *const FunctionCommon,
    declaring_class: String,
) -> Value {
    object_value(
        "ReflectionMethod",
        [
            ("name", Value::string(name)),
            ("class", Value::string(declaring_class.clone())),
            (
                "__reflection_declaring_class",
                Value::string(declaring_class.clone()),
            ),
            ("__reflection_method_class", Value::string(declaring_class)),
            ("__reflection_method_static", Value::bool(is_static)),
            ("__reflection_method_final", Value::bool(is_final)),
            (
                "__reflection_method_visibility",
                Value::long(match visibility {
                    Visibility::Public => 1,
                    Visibility::Protected => 2,
                    Visibility::Private => 4,
                }),
            ),
            (
                "__reflection_function_pointer",
                Value::long(function as usize as i64),
            ),
        ],
    )
}

fn class_get_constructor(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::null());
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::null());
    }
    let Some((name, visibility, is_static, is_final, function, declaring_class)) =
        find_reflected_method(eg, &owner, "__construct")
    else {
        return return_value(rv, Value::null());
    };
    return_value(
        rv,
        reflected_method_value(
            name,
            visibility,
            is_static,
            is_final,
            function,
            declaring_class,
        ),
    )
}

fn find_reflected_method(
    eg: &ExecutorGlobals,
    owner: &str,
    method_name: &str,
) -> Option<(
    String,
    Visibility,
    bool,
    bool,
    *const FunctionCommon,
    String,
)> {
    let mut methods = Vec::new();
    collect_reflected_methods(eg, owner, &mut methods, &mut HashSet::new());
    methods
        .into_iter()
        .find(|(name, ..)| name.eq_ignore_ascii_case(method_name))
        .or_else(|| {
            let function = eg.find_function(&format!("{owner}::{method_name}"))?;
            let declaring_class = eg.declaring_class_of(function).unwrap_or(owner).to_string();
            let is_implicit_enum_static = eg.find_class(owner).is_some_and(|class| {
                class.is_enum
                    && matches!(
                        method_name.to_ascii_lowercase().as_str(),
                        "cases" | "from" | "tryfrom"
                    )
            });
            Some((
                method_name.to_string(),
                Visibility::Public,
                is_implicit_enum_static
                    || eg
                        .registered_function_common(function)
                        .is_some_and(|function| function.sig.this_offset == 0),
                false,
                function,
                declaring_class,
            ))
        })
}

fn class_has_method(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::bool(false));
    }
    let method_name = argument_string(ed, 1);
    return_value(
        rv,
        Value::bool(find_reflected_method(eg, &owner, &method_name).is_some()),
    )
}

fn class_get_method(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        reflection_exception(eg, "ReflectionClass has no resolved class");
        return Ok(());
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        reflection_exception(eg, format!("Class \"{owner}\" does not exist"));
        return Ok(());
    }
    let method_name = argument_string(ed, 1);
    let Some((name, visibility, is_static, is_final, function, declaring_class)) =
        find_reflected_method(eg, &owner, &method_name)
    else {
        reflection_exception(
            eg,
            format!("Method {owner}::{method_name}() does not exist"),
        );
        return Ok(());
    };
    return_value(
        rv,
        reflected_method_value(
            name,
            visibility,
            is_static,
            is_final,
            function,
            declaring_class,
        ),
    )
}

fn class_kind_predicate(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    predicate: impl FnOnce(&crate::compiler::compile::ClassDef) -> bool,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::bool(false));
    }
    return_value(
        rv,
        Value::bool(eg.find_class(&owner).is_some_and(predicate)),
    )
}

fn class_is_interface(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_kind_predicate(ed, rv, eg, |class| class.is_interface)
}

fn class_is_trait(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_kind_predicate(ed, rv, eg, |class| class.is_trait)
}

fn class_is_abstract(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_kind_predicate(ed, rv, eg, |class| class.is_abstract || class.is_interface)
}

fn class_is_final(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_kind_predicate(ed, rv, eg, |class| class.is_final)
}

fn class_is_readonly(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_kind_predicate(ed, rv, eg, |class| class.is_readonly)
}

fn class_is_instantiable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_kind_predicate(ed, rv, eg, |class| {
        !class.is_interface && !class.is_trait && !class.is_abstract
    })
}

fn class_get_methods(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let filter = with_argument(ed, 1, Value::as_long);
    let mut methods = Vec::new();
    collect_reflected_methods(eg, &owner, &mut methods, &mut HashSet::new());
    let mut result = PhpArray::with_packed_capacity(methods.len());
    for (name, visibility, is_static, is_final, function, declaring_class) in methods {
        let modifiers = method_modifiers(visibility, is_static, is_final);
        if filter.is_some_and(|filter| modifiers & filter == 0) {
            continue;
        }
        result.push(reflected_method_value(
            name,
            visibility,
            is_static,
            is_final,
            function,
            declaring_class,
        ));
    }
    return_value(rv, Value::array(result))
}

fn method_get_modifiers(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let visibility = reflected_property(ed, "__reflection_method_visibility")
        .and_then(|value| value.as_long())
        .unwrap_or(1);
    let modifiers = visibility
        | if parameter_property_bool(ed, "__reflection_method_static") {
            16
        } else {
            0
        }
        | if parameter_property_bool(ed, "__reflection_method_final") {
            32
        } else {
            0
        };
    return_value(rv, Value::long(modifiers))
}

fn method_is_constructor(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let constructor = reflected_property(ed, "name")
        .and_then(|value| {
            value
                .as_str()
                .map(|name| name.eq_ignore_ascii_case("__construct"))
        })
        .unwrap_or(false);
    return_value(rv, Value::bool(constructor))
}

fn method_name_is(ed: *mut ExecuteData, expected: &str) -> bool {
    reflected_property(ed, "name")
        .and_then(|value| {
            value
                .as_str()
                .map(|name| name.eq_ignore_ascii_case(expected))
        })
        .unwrap_or(false)
}

fn method_is_destructor(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::bool(method_name_is(ed, "__destruct")))
}

fn method_visibility_is(
    ed: *mut ExecuteData,
    rv: *mut Value,
    expected: i64,
) -> Result<(), VmError> {
    let visibility = reflected_property(ed, "__reflection_method_visibility")
        .and_then(|value| value.as_long())
        .unwrap_or(1);
    return_value(rv, Value::bool(visibility == expected))
}

fn method_is_public(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    method_visibility_is(ed, rv, 1)
}

fn method_is_protected(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    method_visibility_is(ed, rv, 2)
}

fn method_is_private(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    method_visibility_is(ed, rv, 4)
}

fn method_is_static(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_method_static")),
    )
}

fn method_is_final(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_method_final")),
    )
}

fn method_is_abstract(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_method_abstract")),
    )
}

fn method_has_prototype(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::bool(find_method_prototype(ed, eg).is_some()))
}

fn find_method_prototype(
    ed: *mut ExecuteData,
    eg: &ExecutorGlobals,
) -> Option<(
    String,
    Visibility,
    bool,
    bool,
    *const FunctionCommon,
    String,
)> {
    let class_name = reflected_property(ed, "__reflection_method_class")?
        .as_str()?
        .to_string();
    let method_name = reflected_property(ed, "name")?.as_str()?.to_string();
    let class = eg.find_class(&class_name)?;
    if let Some(parent) = &class.parent {
        let mut methods = Vec::new();
        collect_reflected_methods(eg, parent, &mut methods, &mut HashSet::new());
        if let Some(method) = methods
            .into_iter()
            .find(|(name, ..)| name.eq_ignore_ascii_case(&method_name))
        {
            return Some(method);
        }
    }
    for interface in &class.implements {
        let mut methods = Vec::new();
        collect_reflected_methods(eg, interface, &mut methods, &mut HashSet::new());
        if let Some(method) = methods
            .into_iter()
            .find(|(name, ..)| name.eq_ignore_ascii_case(&method_name))
        {
            return Some(method);
        }
    }
    None
}

fn method_get_prototype(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((name, visibility, is_static, is_final, function, declaring_class)) =
        find_method_prototype(ed, eg)
    else {
        reflection_exception(eg, "Method does not have a prototype");
        return Ok(());
    };
    return_value(
        rv,
        reflected_method_value(
            name,
            visibility,
            is_static,
            is_final,
            function,
            declaring_class,
        ),
    )
}

fn method_invoke(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    invoke_reflected_method(ed, rv, eg, "invoke", ReflectedMethodArguments::Packed)
}

fn method_invoke_raw(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    supplied_num_args: u32,
) -> Result<(), VmError> {
    invoke_reflected_method(
        ed,
        rv,
        eg,
        "invoke",
        ReflectedMethodArguments::Raw(supplied_num_args),
    )
}

fn method_invoke_args(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let valid = with_argument(ed, 2, |value| value.value_type() == ValueType::Array);
    if !valid {
        let given = with_argument(ed, 2, reflection_argument_type_name);
        eg.exception = Some(make_error_value(
            "TypeError",
            &format!(
                "ReflectionMethod::invokeArgs(): Argument #2 ($args) must be of type array, {given} given"
            ),
        ));
        return Ok(());
    }
    invoke_reflected_method(ed, rv, eg, "invokeArgs", ReflectedMethodArguments::Packed)
}

#[derive(Clone, Copy)]
enum ReflectedMethodArguments {
    Packed,
    Raw(u32),
}

fn invoke_reflected_method(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    api: &str,
    arguments_source: ReflectedMethodArguments,
) -> Result<(), VmError> {
    let Some(function) = reflected_function(ed) else {
        reflection_exception(eg, "ReflectionMethod has no resolved method");
        return Ok(());
    };
    let receiver = with_argument(ed, 1, Clone::clone);
    if !matches!(receiver.value_type(), ValueType::Null | ValueType::Object) {
        eg.exception = Some(make_error_value(
            "TypeError",
            &format!(
                "ReflectionMethod::{api}(): Argument #1 ($object) must be of type ?object, {} given",
                reflection_argument_type_name(&receiver)
            ),
        ));
        return Ok(());
    }
    let (user, _, receiver_class) = reflected_invocation_metadata(function, Some(&receiver));
    let is_static = if let Some(user) = user {
        user.common.plan.is_static_method()
    } else {
        parameter_property_bool(ed, "__reflection_method_static")
    };
    if !is_static {
        let Some((receiver_class_id, receiver_class_name)) = receiver_class else {
            let reflected_class = reflected_property(ed, "__reflection_method_class")
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default();
            let name = reflected_property(ed, "name")
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default();
            eg.exception = Some(make_error_value(
                "ReflectionException",
                &format!(
                    "Trying to invoke non static method {reflected_class}::{name}() without an object"
                ),
            ));
            return Ok(());
        };
        const REFLECTION_RECEIVER_GUARD: u64 = 1 << 63;
        let receiver_guard = REFLECTION_RECEIVER_GUARD | u64::from(receiver_class_id);
        let cached_receiver = receiver_class_id != 0
            && user.is_some_and(|user| user.compact_class_guard.get() == receiver_guard);
        let cacheable_receiver = !cached_receiver
            && user.is_some()
            && receiver_class_id != 0
            && !function
                .sig
                .param_type_hints
                .iter()
                .any(|hint| matches!(hint, ParamTypeHint::ClassName(_)));
        let valid_receiver = cached_receiver
            || with_reflected_property(ed, "__reflection_declaring_class", |value| {
                value
                    .and_then(Value::as_str)
                    .is_some_and(|declaring_class| {
                        receiver_class_name.eq_ignore_ascii_case(declaring_class)
                            || eg.class_is_a(receiver_class_name, declaring_class)
                    })
            });
        if !valid_receiver {
            eg.exception = Some(make_error_value(
                "ReflectionException",
                "Given object is not an instance of the class this method was declared in",
            ));
            return Ok(());
        }
        if cacheable_receiver && !cached_receiver {
            user.expect("cacheable Reflection receiver belongs to a user method")
                .compact_class_guard
                .set(receiver_guard);
        }
    }

    if let ReflectedMethodArguments::Raw(supplied_num_args) = arguments_source
        && function.sig.ref_args == 0
    {
        let public_arguments = supplied_num_args.saturating_sub(1).min(1) as usize;
        let mut arguments = [Value::undef(), Value::undef()];
        let mut length = 0;
        if function.sig.this_offset == 1 {
            arguments[length] = receiver.clone();
            length += 1;
        }
        if public_arguments != 0 {
            arguments[length] = with_argument(ed, 2, Clone::clone);
            length += 1;
        }
        let result = crate::vm::execute::call_function(eg, function, &arguments[..length])?;
        if eg.exception.is_some() {
            return Ok(());
        }
        return return_value(rv, result);
    }

    let called_scope_class_id = receiver_class
        .map(|(class_id, _)| class_id)
        .or_else(|| {
            with_reflected_property(ed, "__reflection_method_class", |value| {
                value
                    .and_then(Value::as_str)
                    .and_then(|class| eg.find_class(class).map(|class| class.class_id))
            })
        })
        .unwrap_or(0);

    let call_receiver = if is_static {
        Value::null()
    } else {
        receiver.clone()
    };

    if let ReflectedMethodArguments::Raw(supplied_num_args) = arguments_source {
        let public_arguments = supplied_num_args.saturating_sub(1).min(1) as usize;
        let argument = (public_arguments != 0).then(|| {
            with_raw_argument(ed, 2, |value| {
                if function.sig.is_param_by_ref(0) {
                    value.clone_closure_capture()
                } else {
                    value.clone()
                }
            })
        });
        let prepended = (function.sig.this_offset == 1).then_some(call_receiver);
        let num_args = prepended.iter().count() + public_arguments;
        let result = crate::vm::execute::call_function_owned_iter_with_context(
            eg,
            function,
            num_args,
            prepended.into_iter().chain(argument),
            called_scope_class_id,
            (!is_static).then_some(receiver),
            0,
            None,
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        return return_value(rv, result);
    }

    // The ordinary positional, by-value form is the established hot shape for
    // ReflectionMethod::invoke(). Enter it directly from the live variadic
    // bucket: allocating a normalized array/vector and a ResolvedCallback on
    // every call nearly doubles the cost of this existing API. Named and
    // by-reference arguments still take the canonical normalizer below.
    let packed_by_value = function.sig.ref_args == 0
        && with_argument(ed, 2, |value| {
            value
                .as_array()
                .is_some_and(|arguments| !arguments.has_string_keys())
        });
    if packed_by_value {
        let result = with_argument(ed, 2, |value| {
            let arguments = value
                .as_array()
                .expect("ReflectionMethod variadic arguments must be an array");
            let prepended = (function.sig.this_offset == 1).then_some(&call_receiver);
            crate::vm::execute::call_function_iter_with_context(
                eg,
                function,
                prepended.iter().count() + arguments.len(),
                prepended.into_iter().chain(arguments.values()),
                called_scope_class_id,
                (!is_static).then_some(&receiver),
                0,
                None,
            )
        });
        let result = result?;
        if eg.exception.is_some() {
            return Ok(());
        }
        return return_value(rv, result);
    }

    let arguments =
        with_argument(ed, 2, |value| value.as_array().cloned()).unwrap_or_else(PhpArray::new);
    let callback = super::ResolvedCallback {
        func_ptr: function as *const FunctionCommon,
        prepend_args: (function.sig.this_offset == 1)
            .then_some(call_receiver)
            .into_iter()
            .collect(),
        use_vars: Vec::new(),
        called_scope_class_id,
        bound_this: (!is_static).then_some(receiver),
        closure_static_vars: None,
        is_magic_call: false,
    };
    let result = super::call_resolved_with_php_array(eg, callback, &arguments, true)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    return_value(rv, result)
}

fn property_modifiers(property: &PropertyDefinition, is_static: bool) -> i64 {
    let visibility = match property.visibility {
        Visibility::Public => 1,
        Visibility::Protected => 2,
        Visibility::Private => 4,
    };
    visibility
        | if is_static { 16 } else { 0 }
        | if property.is_final() { 32 } else { 0 }
        | if property.abstract_get_hook() || property.abstract_set_hook() {
            64
        } else {
            0
        }
        | if property.is_readonly { 128 } else { 0 }
        | if property.is_virtual_hook_property() {
            512
        } else {
            0
        }
}

fn reflection_property_metadata_for_definition(
    target: Value,
    property: &PropertyDefinition,
    is_static: bool,
) -> ReflectionPropertyMetadata {
    let has_type = !matches!(property.type_hint, ParamTypeHint::None);
    let (type_kind, type_name, allows_null) = hint_metadata(&property.type_hint);
    ReflectionPropertyMetadata {
        target,
        property: property.name.clone(),
        modifiers: property_modifiers(property, is_static),
        has_type,
        type_kind: type_kind.to_string(),
        type_name,
        allows_null,
        has_default: property.has_default(),
        default: property.default.clone().unwrap_or_else(Value::null),
    }
}

fn reflection_property_public_value(
    name: String,
    declaring_class: String,
    eg: &ExecutorGlobals,
) -> Value {
    let class = eg
        .find_class("ReflectionProperty")
        .expect("ReflectionProperty must be registered before use");
    Value::object(PhpObject::with_layout(
        class.class_id,
        Rc::clone(&class.property_layout),
        vec![Value::string(name), Value::string(declaring_class)],
    ))
}

fn reflected_property_value(
    property: &PropertyDefinition,
    is_static: bool,
    eg: &mut ExecutorGlobals,
) -> Value {
    let declaring_class = property.declaring_class.clone();
    let reflected =
        reflection_property_public_value(property.name.clone(), declaring_class.clone(), eg);
    let metadata = reflection_property_metadata_for_definition(
        Value::string(declaring_class),
        property,
        is_static,
    );
    eg.register_reflection_property(&reflected, metadata);
    reflected
}

fn class_get_properties(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let Some(class) = eg.find_class(&owner) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let class_name = class.name.clone();
    let filter = with_argument(ed, 1, |value| value.as_long());
    let mut declarations = class
        .properties
        .iter()
        .cloned()
        .map(|property| (property, false))
        .chain(
            class
                .static_properties
                .iter()
                .filter(|_| !class.is_enum)
                .cloned()
                .map(|property| (property, true)),
        )
        .collect::<Vec<_>>();
    declarations.sort_by_key(|(property, _)| {
        let mut rank = 0usize;
        let mut current = Some(class_name.as_str());
        while let Some(owner) = current {
            if property.declaring_class.eq_ignore_ascii_case(owner) {
                break;
            }
            rank += 1;
            current = eg
                .find_class(owner)
                .and_then(|class| class.parent.as_deref());
        }
        (rank, property.reflection_order)
    });
    let mut properties = PhpArray::with_packed_capacity(declarations.len());
    for (property, is_static) in declarations {
        if property.visibility == Visibility::Private
            && !property.declaring_class.eq_ignore_ascii_case(&class_name)
        {
            continue;
        }
        let modifiers = property_modifiers(&property, is_static);
        if filter.is_some_and(|filter| modifiers & filter == 0) {
            continue;
        }
        properties.push(reflected_property_value(&property, is_static, eg));
    }
    return_value(rv, Value::array(properties))
}

fn class_get_property(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        reflection_exception(eg, "ReflectionClass has no resolved class");
        return Ok(());
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        reflection_exception(eg, format!("Class \"{owner}\" does not exist"));
        return Ok(());
    }
    let property_name = argument_string(ed, 1);
    let property = eg.find_class(&owner).and_then(|class| {
        class
            .properties
            .iter()
            .find(|property| {
                property.name == property_name
                    && (property.visibility != Visibility::Private
                        || property.declaring_class.eq_ignore_ascii_case(&class.name))
            })
            .map(|property| (property.clone(), false))
            .or_else(|| {
                class
                    .static_properties
                    .iter()
                    .find(|property| {
                        property.name == property_name
                            && (property.visibility != Visibility::Private
                                || property.declaring_class.eq_ignore_ascii_case(&class.name))
                    })
                    .map(|property| (property.clone(), true))
            })
    });
    let Some((property, is_static)) = property else {
        reflection_exception(
            eg,
            format!("Property {owner}::${property_name} does not exist"),
        );
        return Ok(());
    };
    return_value(rv, reflected_property_value(&property, is_static, eg))
}

fn class_new_lazy_ghost(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_new_lazy_object(ed, rv, eg, LazyObjectStrategy::Ghost)
}

const LAZY_SKIP_INITIALIZATION_ON_SERIALIZE: i64 = 8;
const LAZY_SKIP_DESTRUCTOR: i64 = 16;

fn lazy_object_options(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    method: &str,
    index: u32,
    allow_skip_destructor: bool,
) -> Option<u8> {
    let options = with_argument(ed, index, |value| {
        if value.is_undef() {
            0
        } else {
            value.as_long().unwrap_or(0)
        }
    });
    let allowed = LAZY_SKIP_INITIALIZATION_ON_SERIALIZE | LAZY_SKIP_DESTRUCTOR;
    if options < 0 || options & !allowed != 0 {
        reflection_exception(
            eg,
            format!(
                "ReflectionClass::{method}(): Argument #{index} ($options) contains invalid flags"
            ),
        );
        return None;
    }
    if !allow_skip_destructor && options & LAZY_SKIP_DESTRUCTOR != 0 {
        reflection_exception(
            eg,
            format!(
                "ReflectionClass::{method}(): Argument #{index} ($options) does not accept ReflectionClass::SKIP_DESTRUCTOR"
            ),
        );
        return None;
    }
    Some(options as u8)
}

fn class_new_lazy_object(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    strategy: LazyObjectStrategy,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return Err(VmError::Fatal(
            "ReflectionClass::newLazyGhost() requires a reflected class".into(),
        ));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return Err(VmError::Fatal(format!("Class {owner} does not exist")));
    }
    let class = eg
        .find_class(&owner)
        .ok_or_else(|| VmError::Fatal(format!("Class {owner} does not exist")))?;
    if class.is_interface || class.is_trait || class.is_abstract || class.is_enum {
        return Err(VmError::Fatal(format!(
            "Class {owner} cannot be instantiated as a lazy object"
        )));
    }
    if eg.class_is_internal(&owner) && !owner.eq_ignore_ascii_case("stdClass") {
        reflection_exception(
            eg,
            format!("Class {owner} is an internal class and cannot be lazy"),
        );
        return Ok(());
    }
    let class_id = class.class_id;
    let property_layout = class.property_layout.clone();
    let property_count = class.property_defaults.len();
    let method = match strategy {
        LazyObjectStrategy::Ghost => "newLazyGhost",
        LazyObjectStrategy::Proxy => "newLazyProxy",
    };
    let Some(options) = lazy_object_options(ed, eg, method, 2, false) else {
        return Ok(());
    };
    let object = PhpObject::with_layout(
        class_id,
        property_layout,
        (0..property_count).map(|_| Value::undef()).collect(),
    );
    let lazy_object = Value::object(object);
    let initializer = with_argument(ed, 1, Clone::clone);
    let resolved = crate::stdlib::resolve_callback_at_callsite(&initializer, eg, ed)
        .ok_or_else(|| VmError::Fatal("Lazy object initializer must be callable".into()))?;
    eg.register_lazy_object(&lazy_object, strategy, initializer, resolved, options, None);
    return_value(rv, lazy_object)
}

fn class_new_lazy_proxy(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_new_lazy_object(ed, rv, eg, LazyObjectStrategy::Proxy)
}

fn restore_lazy_property_defaults(eg: &ExecutorGlobals, object: &Value, lazy_slots: &[usize]) {
    let Some((class_name, property_count)) = object
        .as_object()
        .map(|object| (object.class_name.clone(), object.property_values.len()))
    else {
        return;
    };
    let Some(defaults) = eg
        .find_class(class_name.as_ref())
        .map(|class| class.property_defaults.clone())
    else {
        return;
    };
    debug_assert_eq!(defaults.len(), property_count);
    let Some(mut object) = object.as_object_mut() else {
        return;
    };
    for &slot in lazy_slots {
        if object
            .property_values
            .get(slot)
            .is_some_and(Value::is_undef)
        {
            object.property_values[slot] = defaults[slot].clone();
        }
    }
}

type LazyPropertySnapshot = (Vec<Value>, Option<Box<DynamicPropertyMap>>);

fn snapshot_lazy_property_storage(object: &Value) -> Option<LazyPropertySnapshot> {
    object.as_object().map(|object| {
        let properties = object
            .property_values
            .iter()
            .map(|value| {
                if value.is_owned_reference() {
                    let mut alias = value.clone_owned_reference_alias();
                    alias.mark_internal_reference_alias();
                    alias
                } else {
                    value.clone()
                }
            })
            .collect();
        let dynamic = object
            .dynamic_properties
            .as_ref()
            .map(|properties| Box::new(properties.clone_for_storage_snapshot()));
        (properties, dynamic)
    })
}

fn restore_lazy_ghost_property_storage(
    eg: &ExecutorGlobals,
    object: &Value,
    snapshot: LazyPropertySnapshot,
) {
    let (mut properties, mut dynamic) = snapshot;
    for value in &mut properties {
        value.unmark_internal_reference_alias();
    }
    if let Some(dynamic) = dynamic.as_mut() {
        dynamic.activate_storage_snapshot_aliases();
    }
    let Some(mut object) = object.as_object_mut() else {
        return;
    };
    for (slot, value) in object.property_values.iter().enumerate() {
        value.remove_reference_property_constraint(object.instance_property_reference_owner(slot));
    }
    object.property_values = properties;
    object.dynamic_properties = dynamic;

    for (slot, value) in object.property_values.iter().enumerate() {
        let Some(definition) = eg.instance_property_definition(object.class_id, slot) else {
            continue;
        };
        if !definition.is_typed() || !value.is_owned_reference() {
            continue;
        }
        value.add_reference_property_constraint(ReferencePropertyConstraint {
            owner: object.instance_property_reference_owner(slot),
            declaring_class: definition.declaring_class.clone(),
            property: definition.name.clone(),
            type_scope: definition.type_scope.clone(),
            called_class: object.class_name.to_string(),
            type_hint: definition.type_hint.clone(),
        });
    }
}

fn detach_lazy_proxy_shell_reference_constraints(object: &Value) {
    let Some(object) = object.as_object() else {
        return;
    };
    for (slot, value) in object.property_values.iter().enumerate() {
        value.remove_reference_property_constraint(object.instance_property_reference_owner(slot));
    }
}

/// Initialize one Reflection lazy object at the property-access boundary.
/// Ghosts return their original identity; proxies return their real instance.
pub(crate) fn initialize_lazy_object(
    eg: &mut ExecutorGlobals,
    object: &Value,
) -> Result<Value, VmError> {
    if let Some(state) = eg.lazy_object_state(object) {
        if let Some(instance) = state.proxy_instance.as_ref() {
            return Ok(instance.clone());
        }
        if state.initializing {
            return Ok(object.clone());
        }
    } else {
        return Ok(object.clone());
    }
    let property_snapshot = snapshot_lazy_property_storage(object);
    let Some((strategy, initializer, lazy_slots_before)) =
        eg.lazy_object_state_mut(object).map(|state| {
            state.initializing = true;
            (
                state.strategy,
                state.initializer.clone(),
                state.lazy_slots.clone(),
            )
        })
    else {
        return Ok(object.clone());
    };

    // Ghost storage becomes an ordinary object before user code runs, so
    // declared defaults are observable inside the initializer itself. Proxy
    // shells keep their lazy storage; their factory produces the real object.
    if strategy == LazyObjectStrategy::Ghost {
        restore_lazy_property_defaults(eg, object, &lazy_slots_before);
    }

    let result = match strategy {
        LazyObjectStrategy::Ghost => crate::stdlib::call_resolved_with_values(
            eg,
            &initializer,
            std::slice::from_ref(object),
        )?,
        LazyObjectStrategy::Proxy => crate::stdlib::call_resolved_with_values(
            eg,
            &initializer,
            std::slice::from_ref(object),
        )?,
    };
    if eg.exception.is_some() {
        if strategy == LazyObjectStrategy::Ghost
            && let Some(snapshot) = property_snapshot
        {
            restore_lazy_ghost_property_storage(eg, object, snapshot);
        }
        if let Some(state) = eg.lazy_object_state_mut(object) {
            state.initializing = false;
            state.lazy_slots = lazy_slots_before.clone();
        }
        return Ok(object.clone());
    }

    match strategy {
        LazyObjectStrategy::Ghost => {
            if result.value_type() != ValueType::Null {
                if strategy == LazyObjectStrategy::Ghost
                    && let Some(snapshot) = property_snapshot
                {
                    restore_lazy_ghost_property_storage(eg, object, snapshot);
                }
                if let Some(state) = eg.lazy_object_state_mut(object) {
                    state.initializing = false;
                    state.lazy_slots = lazy_slots_before.clone();
                }
                eg.exception = Some(make_error_value(
                    "TypeError",
                    "Lazy object initializer must return NULL or no value",
                ));
                return Ok(object.clone());
            }
            eg.take_lazy_object_state(object);
            Ok(object.clone())
        }
        LazyObjectStrategy::Proxy => {
            let valid_instance = result.as_object().is_some_and(|instance| {
                object
                    .as_object()
                    .is_some_and(|lazy| eg.class_is_a(&lazy.class_name, &instance.class_name))
            });
            if !valid_instance {
                if strategy == LazyObjectStrategy::Ghost
                    && let Some((properties, dynamic)) = property_snapshot
                {
                    if let Some(mut object) = object.as_object_mut() {
                        object.property_values = properties;
                        object.dynamic_properties = dynamic;
                    }
                }
                if let Some(state) = eg.lazy_object_state_mut(object) {
                    state.initializing = false;
                    state.lazy_slots = lazy_slots_before.clone();
                }
                eg.exception = Some(make_error_value(
                    "TypeError",
                    "Lazy proxy factory must return an instance of the reflected class",
                ));
                return Ok(object.clone());
            }
            if let Some(state) = eg.lazy_object_state_mut(object) {
                detach_lazy_proxy_shell_reference_constraints(object);
                state.initializing = false;
                state.initializer_value = Value::null();
                state.lazy_slots.clear();
                state.proxy_instance = Some(result.clone());
            }
            Ok(result)
        }
    }
}

/// Follow an initialized proxy chain and initialize every lazy endpoint
/// reached along the way. Output and iteration projections require the full
/// terminal object; explicit Reflection initialization keeps its one-object
/// contract above.
pub(crate) fn resolve_lazy_object_chain(
    eg: &mut ExecutorGlobals,
    object: &Value,
) -> Result<Value, VmError> {
    let mut target = object.clone();
    let mut identities = Vec::with_capacity(4);
    for _ in 0..16 {
        let Some(identity) = target.object_identity() else {
            break;
        };
        if identities.contains(&identity) {
            break;
        }
        identities.push(identity);

        if eg.is_uninitialized_lazy_object(&target) {
            target = initialize_lazy_object(eg, &target)?;
            if eg.exception.is_some() {
                break;
            }
            continue;
        }
        let Some(instance) = eg.lazy_proxy_instance(&target) else {
            break;
        };
        target = instance;
    }
    Ok(target)
}

/// Resolve the same chain for one property operation while retaining per-slot
/// skip state. A proxy endpoint is initialized only when this access would
/// trigger that exact property on the endpoint itself.
pub(crate) fn resolve_lazy_property_chain(
    eg: &mut ExecutorGlobals,
    object: &Value,
    key: &str,
    may_initialize: bool,
) -> Result<Value, VmError> {
    let mut target = object.clone();
    let mut identities = Vec::with_capacity(4);
    for _ in 0..16 {
        let Some(identity) = target.object_identity() else {
            break;
        };
        if identities.contains(&identity) {
            break;
        }
        identities.push(identity);

        if eg.is_uninitialized_lazy_object(&target) {
            if !may_initialize || !eg.lazy_property_requires_initialization(&target, key) {
                break;
            }
            target = initialize_lazy_object(eg, &target)?;
            if eg.exception.is_some() {
                break;
            }
            continue;
        }
        let Some(instance) = eg.lazy_proxy_instance(&target) else {
            break;
        };
        target = instance;
    }
    Ok(target)
}

fn reflected_class_object_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    method: &str,
) -> Option<Value> {
    let (_, owner) = generic_target(ed)?;
    let object = with_argument(ed, 1, Clone::clone);
    let given = object
        .as_object()
        .map(|object| object.class_name.to_string())?;
    if !eg.class_is_a(&given, &owner) {
        eg.exception = Some(make_error_value(
            "TypeError",
            &format!(
                "ReflectionClass::{method}(): Argument #1 ($object) must be of type {owner}, {given} given"
            ),
        ));
        return None;
    }
    Some(object)
}

fn class_initialize_lazy_object(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(object) = reflected_class_object_argument(ed, eg, "initializeLazyObject") else {
        return Ok(());
    };
    let initialized = initialize_lazy_object(eg, &object)?;
    return_value(rv, initialized)
}

fn class_is_uninitialized_lazy_object(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(object) = reflected_class_object_argument(ed, eg, "isUninitializedLazyObject") else {
        return Ok(());
    };
    return_value(rv, Value::bool(eg.is_uninitialized_lazy_object(&object)))
}

fn class_mark_lazy_object_as_initialized(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(object) = reflected_class_object_argument(ed, eg, "markLazyObjectAsInitialized")
    else {
        return Ok(());
    };
    if let Some(state) = eg.take_lazy_object_state(&object) {
        restore_lazy_property_defaults(eg, &object, &state.lazy_slots);
    }
    return_value(rv, object)
}

fn class_get_lazy_initializer(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(object) = reflected_class_object_argument(ed, eg, "getLazyInitializer") else {
        return Ok(());
    };
    let initializer = eg
        .lazy_object_state(&object)
        .filter(|state| state.proxy_instance.is_none())
        .map(|state| state.initializer_value.clone())
        .unwrap_or_else(Value::null);
    return_value(rv, initializer)
}

fn reflected_reset_lazy_slots(
    eg: &ExecutorGlobals,
    reflected_class: &str,
    object: &Value,
) -> Vec<usize> {
    let Some(class) = eg.find_class(reflected_class) else {
        return Vec::new();
    };
    let reflected_class_id = class.class_id;
    let reflected_layout = class.property_layout.clone();
    let Some(object) = object.as_object() else {
        return Vec::new();
    };
    let mut slots = Vec::with_capacity(reflected_layout.len());
    for reflected_slot in 0..reflected_layout.len() {
        let Some(key) = reflected_layout.key(reflected_slot) else {
            continue;
        };
        let Some(slot) = object.property_slot(key) else {
            continue;
        };
        let definition = eg.instance_property_definition(object.class_id, slot);
        if definition.is_some_and(PropertyDefinition::is_virtual_hook_property) {
            continue;
        }
        // An initialized readonly property inherited from another class keeps
        // its value. Resetting the declaring class itself starts a new
        // lifecycle for that storage; an uninitialized inherited readonly
        // slot can also participate normally.
        if definition.is_some_and(|definition| {
            definition.is_readonly
                && eg
                    .find_class(&definition.declaring_class)
                    .is_some_and(|declaring| declaring.class_id != reflected_class_id)
                && object
                    .get_property_slot(slot)
                    .is_some_and(|value| !value.is_undef())
        }) {
            continue;
        }
        if !slots.contains(&slot) {
            slots.push(slot);
        }
    }
    slots
}

fn class_reset_as_lazy_object(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    strategy: LazyObjectStrategy,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        reflection_exception(eg, "ReflectionClass has no resolved class");
        return Ok(());
    };
    let method = match strategy {
        LazyObjectStrategy::Ghost => "resetAsLazyGhost",
        LazyObjectStrategy::Proxy => "resetAsLazyProxy",
    };
    let object = with_argument(ed, 1, Clone::clone);
    let Some(object_class) = object
        .as_object()
        .map(|object| object.class_name.to_string())
    else {
        eg.exception = Some(make_error_value(
            "TypeError",
            "Lazy object reset expects an object",
        ));
        return Ok(());
    };
    if !eg.class_is_a(&object_class, &owner) {
        eg.exception = Some(make_error_value(
            "TypeError",
            &format!(
                "ReflectionClass::{method}(): Argument #1 ($object) must be of type {owner}, {object_class} given"
            ),
        ));
        return Ok(());
    }
    if let Some(state) = eg.lazy_object_state(&object) {
        if state.initializing {
            eg.exception = Some(make_error_value(
                "Error",
                "Can not reset an object while it is being initialized",
            ));
            return Ok(());
        }
        if state.proxy_instance.is_none() {
            reflection_exception(eg, "Object is already lazy");
            return Ok(());
        }
    }
    let Some(options) = lazy_object_options(ed, eg, method, 3, true) else {
        return Ok(());
    };
    let initializer = with_argument(ed, 2, Clone::clone);
    let resolved = crate::stdlib::resolve_callback_at_callsite(&initializer, eg, ed)
        .ok_or_else(|| VmError::Fatal("Lazy object initializer must be callable".into()))?;

    let lazy_slots = reflected_reset_lazy_slots(eg, &owner, &object);
    let destructor_target = eg
        .lazy_proxy_instance(&object)
        .unwrap_or_else(|| object.clone());
    let previous_lazy_state = eg.take_lazy_object_state(&object);
    if options as i64 & LAZY_SKIP_DESTRUCTOR == 0 {
        let has_destructor = destructor_target.as_object().is_some_and(|object| {
            eg.find_method_info(&object.class_name, "__destruct")
                .is_some()
        });
        let destructor_result = if has_destructor && destructor_target.mark_object_destructed() {
            crate::stdlib::call_object_public_method(eg, &destructor_target, "__destruct", &[])
        } else {
            Ok(None)
        };
        if let Err(error) = destructor_result {
            if let Some(state) = previous_lazy_state {
                eg.restore_lazy_object_state(&object, state);
            }
            return Err(error);
        }
        if eg.exception.is_some() {
            if let Some(state) = previous_lazy_state {
                eg.restore_lazy_object_state(&object, state);
            }
            return Ok(());
        }
    }

    let (declared_keys, dynamic_keys) = object.as_object().map_or_else(
        || (Vec::new(), Vec::new()),
        |object| {
            let declared = lazy_slots
                .iter()
                .filter_map(|slot| object.property_name_at_slot(*slot).map(str::to_owned))
                .collect();
            let mut dynamic = Vec::new();
            object.for_each_dynamic_property(|key, _| dynamic.push(key.to_owned()));
            (declared, dynamic)
        },
    );
    let mut property_destructors = Vec::new();
    for key in declared_keys.into_iter().chain(dynamic_keys) {
        let destructor = object.as_object().and_then(|object| {
            object
                .get_property(&key)
                .and_then(|value| crate::vm::execute::prepare_replaced_value_destructor(eg, value))
        });
        if let Some(mut object_data) = object.as_object_mut() {
            object_data.unset_property(&key);
        }
        if let Some(destructor) = destructor {
            property_destructors.push(destructor);
        }
    }
    object.clear_object_destructed();
    eg.register_lazy_object(
        &object,
        strategy,
        initializer,
        resolved,
        options,
        Some(lazy_slots),
    );
    for destructor in property_destructors {
        crate::vm::execute::run_prepared_value_destructor(eg, Some(destructor))?;
        if eg.exception.is_some() {
            break;
        }
    }
    Ok(())
}

fn class_reset_as_lazy_ghost(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_reset_as_lazy_object(ed, eg, LazyObjectStrategy::Ghost)
}

fn class_reset_as_lazy_proxy(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_reset_as_lazy_object(ed, eg, LazyObjectStrategy::Proxy)
}

fn reflected_class_instance(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Result<Option<(String, Value)>, VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return Err(VmError::Fatal(
            "ReflectionClass instance creation requires a reflected class".into(),
        ));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return Err(VmError::Fatal(format!("Class {owner} does not exist")));
    }
    let class = eg
        .find_class(&owner)
        .ok_or_else(|| VmError::Fatal(format!("Class {owner} does not exist")))?;
    let instantiation_error = if class.is_trait {
        Some(format!("Cannot instantiate trait {}", class.name))
    } else if class.is_interface {
        Some(format!("Cannot instantiate interface {}", class.name))
    } else if class.is_abstract {
        Some(format!("Cannot instantiate abstract class {}", class.name))
    } else if class.is_enum {
        Some(format!("Cannot instantiate enum {}", class.name))
    } else {
        None
    };
    if let Some(message) = instantiation_error {
        eg.exception = Some(make_error_value("Error", &message));
        return Ok(None);
    }
    let class_id = class.class_id;
    let class_name = class.name.clone();
    let property_layout = class.property_layout.clone();
    let property_defaults = class.property_defaults.clone();
    if class_id != 0
        && eg.deferred_class_constants_require_activation(class_id)
        && !activate_deferred_class_constants(class_id, eg)?
    {
        if let Some(exception) = eg.exception.as_ref() {
            crate::vm::execute::attach_internal_constant_expression_trace(exception, ed, eg);
        }
        return Ok(None);
    }
    let object = if class_id == 0 {
        PhpObject::dynamic(class_name, 0, HashMap::new())
    } else {
        PhpObject::with_layout(
            class_id,
            property_layout,
            property_defaults.as_ref().to_vec(),
        )
    };
    Ok(Some((owner, Value::object(object))))
}

fn class_new_instance_without_constructor(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((_, object)) = reflected_class_instance(ed, eg)? else {
        return return_value(rv, Value::null());
    };
    return_value(rv, object)
}

fn construct_reflected_class_instance(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    arguments: PhpArray,
    preserve_reference_aliases: bool,
) -> Result<(), VmError> {
    let Some((owner, object)) = reflected_class_instance(ed, eg)? else {
        return return_value(rv, Value::null());
    };
    let constructor_info = eg.find_method_info(&owner, "__construct");
    let Some((visibility, _, _)) = constructor_info else {
        if !arguments.is_empty() {
            reflection_exception(
                eg,
                format!(
                    "Class {owner} does not have a constructor, so you cannot pass any constructor arguments"
                ),
            );
            return Ok(());
        }
        return return_value(rv, object);
    };
    if visibility != Visibility::Public {
        reflection_exception(
            eg,
            format!("Access to non-public constructor of class {owner}"),
        );
        return Ok(());
    }
    let Some(constructor) = crate::stdlib::resolve_object_public_method(eg, &object, "__construct")
    else {
        if constructor_info.is_some() {
            reflection_exception(
                eg,
                format!("Access to non-public constructor of class {owner}"),
            );
        }
        return Ok(());
    };

    if !super::report_callback_reference_warnings(
        eg,
        ed,
        &constructor,
        &arguments,
        !preserve_reference_aliases,
        &format!("{owner}::__construct"),
    )? {
        return Ok(());
    }

    let _ = super::call_resolved_with_php_array(
        eg,
        constructor,
        &arguments,
        preserve_reference_aliases,
    )?;
    if eg.exception.is_some() {
        return Ok(());
    }
    return_value(rv, object)
}

fn class_new_instance(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arguments =
        with_argument(ed, 1, |value| value.as_array().cloned()).unwrap_or_else(PhpArray::new);
    construct_reflected_class_instance(ed, rv, eg, arguments, false)
}

fn class_new_instance_args(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let valid = with_argument(ed, 1, |value| value.value_type() == ValueType::Array);
    if !valid {
        let given = with_argument(ed, 1, reflection_argument_type_name);
        eg.exception = Some(make_error_value(
            "TypeError",
            &format!(
                "ReflectionClass::newInstanceArgs(): Argument #1 ($args) must be of type array, {given} given"
            ),
        ));
        return Ok(());
    }
    let arguments = with_argument(ed, 1, |value| {
        value.as_array().cloned().unwrap_or_else(PhpArray::new)
    });
    construct_reflected_class_instance(ed, rv, eg, arguments, true)
}

fn object_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = with_argument(ed, 1, Clone::clone);
    let owner = target
        .as_object()
        .map(|object| object.class_name.to_string())
        .ok_or_else(|| VmError::Fatal("ReflectionObject expects an object".into()))?;
    set_target(ed, "class", owner);
    with_argument(ed, 0, |value| {
        if let Some(mut object) = value.as_object_mut() {
            object.set_property("__generic_object", target);
        }
    });
    Ok(())
}

fn property_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = with_argument(ed, 1, Clone::clone);
    let name = argument_string(ed, 2);
    let class_name = target
        .as_object()
        .map(|object| object.class_name.to_string())
        .or_else(|| target.as_str().map(str::to_string));
    let definition = class_name.as_deref().and_then(|class_name| {
        let class = eg.find_class(class_name)?;
        class
            .properties
            .iter()
            .find(|property| property.name == name)
            .cloned()
            .map(|property| (property, false))
            .or_else(|| {
                class
                    .static_properties
                    .iter()
                    .find(|property| property.name == name)
                    .cloned()
                    .map(|property| (property, true))
            })
    });
    let receiver = with_argument(ed, 0, Clone::clone);
    if let Some(mut object) = receiver.as_object_mut() {
        object.set_property("name", Value::string(name.clone()));
        if let Some(declaring_class) = definition
            .as_ref()
            .map(|(property, _)| property.declaring_class.as_str())
            .or(class_name.as_deref())
        {
            object.set_property("class", Value::string(declaring_class));
        }
    }
    if let Some((property, is_static)) = definition {
        let metadata = reflection_property_metadata_for_definition(target, &property, is_static);
        eg.register_reflection_property(&receiver, metadata);
    } else if target.as_object().is_some_and(|object| {
        object
            .dynamic_properties
            .as_ref()
            .is_some_and(|properties| properties.get(&name).is_some())
    }) {
        eg.register_reflection_property(
            &receiver,
            ReflectionPropertyMetadata {
                target,
                property: name,
                modifiers: 1,
                has_type: false,
                type_kind: "named".to_string(),
                type_name: String::new(),
                allows_null: true,
                has_default: false,
                default: Value::null(),
            },
        );
    }
    Ok(())
}

fn property_get_modifiers(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::long(reflection_property_modifiers(ed, eg)))
}

fn reflection_property_modifiers(ed: *mut ExecuteData, eg: &ExecutorGlobals) -> i64 {
    reflection_property_metadata(ed, eg).map_or_else(
        || {
            reflected_property(ed, "__reflection_modifiers")
                .and_then(|value| value.as_long())
                .unwrap_or(0)
        },
        |metadata| metadata.modifiers,
    )
}

fn property_is_static(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let modifiers = reflection_property_modifiers(ed, eg);
    return_value(rv, Value::bool(modifiers & 16 != 0))
}

fn property_is_readonly(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let modifiers = reflection_property_modifiers(ed, eg);
    return_value(rv, Value::bool(modifiers & 128 != 0))
}

fn property_modifier_is(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &ExecutorGlobals,
    expected: i64,
) -> Result<(), VmError> {
    let modifiers = reflection_property_modifiers(ed, eg);
    return_value(rv, Value::bool(modifiers & expected != 0))
}

fn property_is_final(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_modifier_is(ed, rv, eg, 32)
}

fn property_is_abstract(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_modifier_is(ed, rv, eg, 64)
}

fn property_is_virtual(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_modifier_is(ed, rv, eg, 512)
}

fn property_has_default_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let has_default = reflection_property_metadata(ed, eg).map_or_else(
        || {
            reflected_property(ed, "__reflection_has_default")
                .is_some_and(|value| value.is_truthy())
        },
        |metadata| metadata.has_default,
    );
    return_value(rv, Value::bool(has_default))
}

fn property_get_default_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let (has_default, default) = reflection_property_metadata(ed, eg).map_or_else(
        || {
            (
                reflected_property(ed, "__reflection_has_default")
                    .is_some_and(|value| value.is_truthy()),
                reflected_property(ed, "__reflection_default").unwrap_or_else(Value::null),
            )
        },
        |metadata| (metadata.has_default, metadata.default.clone()),
    );
    if !has_default {
        super::report_internal_deprecation(
            eg,
            ed,
            "ReflectionProperty::getDefaultValue() for a property without a default value is deprecated, use ReflectionProperty::hasDefaultValue() to check if the default value exists",
        )?;
    }
    return_value(rv, default)
}

fn property_is_default(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(reflection_property_definition(ed, eg).is_some()),
    )
}

fn property_visibility_is(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &ExecutorGlobals,
    expected: i64,
) -> Result<(), VmError> {
    let modifiers = reflection_property_metadata(ed, eg).map_or_else(
        || {
            reflected_property(ed, "__reflection_modifiers")
                .and_then(|value| value.as_long())
                .unwrap_or(1)
        },
        |metadata| metadata.modifiers,
    );
    return_value(rv, Value::bool(modifiers & 7 == expected))
}

fn property_is_public(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_visibility_is(ed, rv, eg, 1)
}

fn property_is_protected(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_visibility_is(ed, rv, eg, 2)
}

fn property_is_private(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_visibility_is(ed, rv, eg, 4)
}

fn reflected_property_target(
    ed: *mut ExecuteData,
    eg: &ExecutorGlobals,
) -> Option<(Value, String)> {
    reflection_property_metadata(ed, eg)
        .map(|metadata| (metadata.target.clone(), metadata.property.clone()))
        .or_else(|| {
            with_argument(ed, 0, |value| {
                let object = value.as_object()?;
                let target = object.get_property("__reflection_target")?.clone();
                let property = object
                    .get_property("__reflection_property")?
                    .as_str()?
                    .to_string();
                Some((target, property))
            })
        })
}

fn reflected_property_object(
    ed: *mut ExecuteData,
    eg: &ExecutorGlobals,
    index: u32,
) -> Option<Value> {
    with_argument(ed, index, |value| {
        (value.value_type() == crate::value::ValueType::Object).then(|| value.clone())
    })
    .or_else(|| {
        reflected_property_target(ed, eg).and_then(|(target, _)| {
            (target.value_type() == crate::value::ValueType::Object).then_some(target)
        })
    })
}

fn reflection_property_key(
    eg: &ExecutorGlobals,
    object: &PhpObject,
    reflected_scope: Option<&str>,
    property: &str,
) -> String {
    crate::runtime::resolve_property_key(
        eg,
        object.class_name.as_ref(),
        property,
        Some(reflected_scope.unwrap_or(object.class_name.as_ref())),
    )
}

fn reflected_property_scope(ed: *mut ExecuteData) -> Option<String> {
    reflected_property(ed, "class").and_then(|value| value.as_str().map(str::to_owned))
}

fn reflected_property_access_object(
    eg: &mut ExecutorGlobals,
    mut object: Value,
    key: &str,
) -> Result<Value, VmError> {
    for _ in 0..16 {
        if eg.lazy_property_requires_initialization(&object, key) {
            object = initialize_lazy_object(eg, &object)?;
            if eg.exception.is_some() {
                break;
            }
        } else if let Some(instance) = eg.lazy_proxy_instance(&object) {
            object = instance;
        } else {
            break;
        }
    }
    Ok(object)
}

fn reflected_lazy_property_operation_target(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    method: &str,
) -> Option<(Value, String, usize, Option<PropertyDefinition>)> {
    let (_, property) = reflected_property_target(ed, eg)?;
    let mut target = reflected_property_object(ed, eg, 1)?;
    let reflected_scope = reflected_property_scope(ed);
    let modifiers = reflection_property_modifiers(ed, eg);
    let initial_class = target
        .as_object()
        .map(|object| object.class_name.to_string())?;
    let declaring_class = reflected_scope.as_deref().unwrap_or(initial_class.as_str());

    if modifiers & 16 != 0 {
        reflection_exception(
            eg,
            format!("Can not use {method} on static property {declaring_class}::${property}"),
        );
        return None;
    }
    if modifiers & 512 != 0 {
        reflection_exception(
            eg,
            format!("Can not use {method} on virtual property {declaring_class}::${property}"),
        );
        return None;
    }

    // An initialized proxy may itself point at another initialized proxy.
    // Reflection operates on the endpoint visible at method entry, while an
    // uninitialized endpoint must remain untouched by ordinary initialization.
    if let Some(instance) = eg.lazy_proxy_instance(&target) {
        target = instance;
    }
    let (key, slot, class_id) = {
        let object = target.as_object()?;
        let key = reflection_property_key(eg, &object, reflected_scope.as_deref(), &property);
        let Some(slot) = object.property_slot(&key) else {
            reflection_exception(
                eg,
                format!("Can not use {method} on dynamic property {declaring_class}::${property}"),
            );
            return None;
        };
        (key, slot, object.class_id)
    };
    let definition = eg.instance_property_definition(class_id, slot).cloned();
    Some((target, key, slot, definition))
}

fn property_hint_accepts_string(hint: &ParamTypeHint) -> bool {
    match hint {
        ParamTypeHint::String => true,
        ParamTypeHint::Nullable(inner) => property_hint_accepts_string(inner),
        ParamTypeHint::Union(parts) => parts.iter().any(property_hint_accepts_string),
        _ => false,
    }
}

fn prepare_reflected_property_assignment(
    eg: &mut ExecutorGlobals,
    target: &Value,
    definition: &PropertyDefinition,
    value: Value,
) -> Result<Option<Value>, VmError> {
    let first_error = match crate::vm::execute::prepare_property_assignment(
        value.clone(),
        definition,
        eg,
        false,
        target
            .as_object()
            .as_deref()
            .map_or(definition.declaring_class.as_str(), |object| {
                object.class_name.as_ref()
            }),
    ) {
        Ok(value) => return Ok(Some(value)),
        Err(message) => message,
    };

    if value.value_type() == ValueType::Object
        && property_hint_accepts_string(&definition.type_hint)
        && let Some(rendered) =
            crate::stdlib::call_object_public_method(eg, &value, "__tostring", &[])?
    {
        if eg.exception.is_some() {
            return Ok(None);
        }
        let called_class = target
            .as_object()
            .map(|object| object.class_name.to_string())
            .unwrap_or_else(|| definition.declaring_class.clone());
        match crate::vm::execute::prepare_property_assignment(
            rendered,
            definition,
            eg,
            false,
            &called_class,
        ) {
            Ok(value) => return Ok(Some(value)),
            Err(message) => {
                eg.exception = Some(make_error_value("TypeError", &message));
                return Ok(None);
            }
        }
    }

    eg.exception = Some(make_error_value("TypeError", &first_error));
    Ok(None)
}

fn property_is_initialized(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((_, property)) = reflected_property_target(ed, eg) else {
        return return_value(rv, Value::bool(false));
    };
    let reflected_scope = reflected_property_scope(ed);
    let initialized = if let Some(target) = reflected_property_object(ed, eg, 1) {
        let key = target
            .as_object()
            .map(|object| {
                reflection_property_key(eg, &object, reflected_scope.as_deref(), &property)
            })
            .unwrap_or_else(|| property.clone());
        let target = reflected_property_access_object(eg, target, &key)?;
        target.as_object().is_some_and(|object| {
            object
                .get_property(&key)
                .is_some_and(|value| !value.is_undef())
        })
    } else {
        false
    };
    return_value(rv, Value::bool(initialized))
}

fn property_get_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((_, property)) = reflected_property_target(ed, eg) else {
        return return_value(rv, Value::null());
    };
    let reflected_scope = reflected_property_scope(ed);
    let value = if let Some(target) = reflected_property_object(ed, eg, 1) {
        let key = target
            .as_object()
            .map(|object| {
                reflection_property_key(eg, &object, reflected_scope.as_deref(), &property)
            })
            .unwrap_or_else(|| property.clone());
        let target = reflected_property_access_object(eg, target, &key)?;
        target
            .as_object()
            .and_then(|object| object.get_property(&key).cloned())
            .unwrap_or_else(Value::null)
    } else {
        Value::null()
    };
    return_value(rv, value)
}

fn property_set_value(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((_, property)) = reflected_property_target(ed, eg) else {
        return Ok(());
    };
    let reflected_scope = reflected_property_scope(ed);
    let value = with_argument(ed, 2, Clone::clone);
    if let Some(target) = reflected_property_object(ed, eg, 1) {
        let key = target
            .as_object()
            .map(|object| {
                reflection_property_key(eg, &object, reflected_scope.as_deref(), &property)
            })
            .unwrap_or_else(|| property.clone());
        let target = reflected_property_access_object(eg, target, &key)?;
        if let Some(mut object) = target.as_object_mut() {
            object.set_property(&key, value);
        }
    }
    Ok(())
}

fn property_get_raw_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_get_value(ed, rv, eg)
}

fn property_set_raw_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_set_value(ed, rv, eg)
}

fn property_is_lazy(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((_, property)) = reflected_property_target(ed, eg) else {
        return return_value(rv, Value::bool(false));
    };
    let Some(mut target) = reflected_property_object(ed, eg, 1) else {
        return return_value(rv, Value::bool(false));
    };
    for _ in 0..16 {
        let Some(instance) = eg.lazy_proxy_instance(&target) else {
            break;
        };
        target = instance;
    }
    let reflected_scope = reflected_property_scope(ed);
    let lazy = target.as_object().is_some_and(|object| {
        let key = reflection_property_key(eg, &object, reflected_scope.as_deref(), &property);
        let Some(slot) = object.property_slot(&key) else {
            return false;
        };
        eg.lazy_object_state(&target)
            .is_some_and(|state| state.proxy_instance.is_none() && state.lazy_slots.contains(&slot))
    });
    return_value(rv, Value::bool(lazy))
}

fn property_skip_lazy_initialization(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((target, _key, slot, _definition)) =
        reflected_lazy_property_operation_target(ed, eg, "skipLazyInitialization")
    else {
        return Ok(());
    };
    let became_initialized = eg.lazy_object_state_mut(&target).and_then(|state| {
        if state.proxy_instance.is_some() {
            None
        } else {
            state.lazy_slots.retain(|candidate| *candidate != slot);
            Some(state.lazy_slots.is_empty())
        }
    });
    let Some(became_initialized) = became_initialized else {
        return Ok(());
    };
    restore_lazy_property_defaults(eg, &target, std::slice::from_ref(&slot));
    if became_initialized {
        eg.take_lazy_object_state(&target);
    }
    Ok(())
}

fn property_set_raw_value_without_lazy_initialization(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((target, key, slot, definition)) =
        reflected_lazy_property_operation_target(ed, eg, "setRawValueWithoutLazyInitialization")
    else {
        return Ok(());
    };
    if definition.as_ref().is_some_and(|definition| {
        definition.is_readonly
            && target
                .as_object()
                .and_then(|object| {
                    object
                        .get_property_slot(slot)
                        .map(|value| !value.is_undef())
                })
                .unwrap_or(false)
    }) {
        let definition = definition.as_ref().unwrap();
        eg.exception = Some(make_error_value(
            "Error",
            &format!(
                "Cannot modify readonly property {}::${}",
                definition.declaring_class, definition.name
            ),
        ));
        return Ok(());
    }

    let mut value = with_argument(ed, 2, Clone::clone);
    if let Some(definition) = definition.as_ref() {
        let Some(prepared) = prepare_reflected_property_assignment(eg, &target, definition, value)?
        else {
            return Ok(());
        };
        value = prepared;
        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
        if let Some(declaration) = definition.generic_declaration
            && let Err(message) = eg.check_cached_generic_property_value(
                &target,
                &definition.name,
                &value,
                declaration,
            )
        {
            eg.exception = Some(make_error_value("TypeError", &message));
            return Ok(());
        }
    }

    let destructor = target.as_object().and_then(|object| {
        object
            .get_property_slot(slot)
            .and_then(|current| crate::vm::execute::prepare_replaced_value_destructor(eg, current))
    });
    if let Some(mut object) = target.as_object_mut() {
        object.set_property(&key, value);
    }
    let became_initialized = if let Some(state) = eg.lazy_object_state_mut(&target) {
        if state.proxy_instance.is_some() || !state.lazy_slots.contains(&slot) {
            false
        } else {
            state.lazy_slots.retain(|candidate| *candidate != slot);
            state.lazy_slots.is_empty()
        }
    } else {
        false
    };
    if became_initialized {
        eg.take_lazy_object_state(&target);
    }
    crate::vm::execute::run_prepared_value_destructor(eg, destructor)
}

fn class_file_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::bool(false));
    }
    let value = eg
        .find_class(&owner)
        .and_then(|class| class.source_file.as_ref())
        .map_or_else(|| Value::bool(false), |file| Value::string(file.clone()));
    return_value(rv, value)
}

fn method_file_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let class_name =
        reflected_property(ed, "class").and_then(|value| value.as_str().map(str::to_owned));
    let value = class_name
        .as_deref()
        .and_then(|class_name| eg.find_class(class_name))
        .and_then(|class| class.source_file.as_ref())
        .map_or_else(|| Value::bool(false), |file| Value::string(file.clone()));
    return_value(rv, value)
}

fn method_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = with_argument(ed, 1, Clone::clone);
    let method_name = argument_string(ed, 2);
    if method_name.eq_ignore_ascii_case("__invoke")
        && let Some(closure) = target.as_closure()
        && !closure.func.is_null()
    {
        let function = closure.func;
        let called_class = (closure.called_scope_class_id != 0)
            .then(|| eg.class_by_id(closure.called_scope_class_id))
            .flatten()
            .map(|class| class.name.clone())
            .or_else(|| eg.declaring_class_of(function).map(str::to_owned));
        set_target(ed, "method", "Closure::__invoke".to_string());
        with_argument(ed, 0, |value| {
            if let Some(mut object) = value.as_object_mut() {
                object.set_property(
                    "__reflection_function_pointer",
                    Value::long(function as usize as i64),
                );
                object.set_property("__reflection_method_static", Value::bool(false));
                object.set_property("__reflection_method_final", Value::bool(false));
                object.set_property("__reflection_method_visibility", Value::long(1));
                object.set_property("__reflection_method_class", Value::string("Closure"));
                object.set_property("__reflection_declaring_class", Value::string("Closure"));
                object.set_property("__reflection_closure_method", Value::bool(true));
                object.set_property(
                    "__reflection_closure_called_class",
                    called_class.map_or_else(Value::null, Value::string),
                );
                object.set_property("name", Value::string("__invoke"));
            }
        });
        return Ok(());
    }

    let class_name = target
        .as_object()
        .map(|object| object.class_name.to_string())
        .unwrap_or_else(|| argument_string(ed, 1));
    if eg.find_class(&class_name).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &class_name)?
    {
        reflection_exception(eg, format!("Class \"{class_name}\" does not exist"));
        return Ok(());
    }
    let Some((_, visibility, is_static, is_final, function, declaring_class)) =
        find_reflected_method(eg, &class_name, &method_name)
    else {
        reflection_exception(
            eg,
            format!("Method {class_name}::{method_name}() does not exist"),
        );
        return Ok(());
    };
    let owner = format!("{class_name}::{method_name}");
    set_target(ed, "method", owner);
    with_argument(ed, 0, |value| {
        if let Some(mut object) = value.as_object_mut() {
            object.set_property(
                "__reflection_function_pointer",
                Value::long(function as usize as i64),
            );
            object.set_property("__reflection_method_static", Value::bool(is_static));
            object.set_property("__reflection_method_final", Value::bool(is_final));
            object.set_property(
                "__reflection_method_visibility",
                Value::long(match visibility {
                    Visibility::Public => 1,
                    Visibility::Protected => 2,
                    Visibility::Private => 4,
                }),
            );
            object.set_property(
                "__reflection_method_class",
                Value::string(class_name.clone()),
            );
            object.set_property(
                "__reflection_declaring_class",
                Value::string(declaring_class),
            );
            object.set_property("name", Value::string(method_name));
        }
    });
    Ok(())
}

fn method_create_from_method_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let method = argument_string(ed, 1);
    let Some((class_name, method_name)) = method.split_once("::") else {
        reflection_exception(
            eg,
            "ReflectionMethod::createFromMethodName(): Argument #1 ($method) must be a valid method name",
        );
        return Ok(());
    };
    if eg.find_class(class_name).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, class_name)?
    {
        reflection_exception(eg, format!("Class \"{class_name}\" does not exist"));
        return Ok(());
    }
    let Some((name, visibility, is_static, is_final, function, declaring_class)) =
        find_reflected_method(eg, class_name, method_name)
    else {
        reflection_exception(
            eg,
            format!("Method {class_name}::{method_name}() does not exist"),
        );
        return Ok(());
    };
    let value = reflected_method_value(
        name,
        visibility,
        is_static,
        is_final,
        function,
        declaring_class,
    );
    let reflection_class = crate::vm::execute::called_class_name_for_internal_call(eg, ed)
        .unwrap_or("ReflectionMethod");
    if !reflection_class.eq_ignore_ascii_case("ReflectionMethod")
        && eg.class_is_a(reflection_class, "ReflectionMethod")
        && let Some(mut object) = value.as_object_mut()
    {
        object.class_name = Rc::from(reflection_class);
        object.class_id = eg
            .find_class(reflection_class)
            .map(|class| class.class_id)
            .unwrap_or(0);
    }
    return_value(rv, value)
}

fn method_get_closure(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(function) = reflected_function(ed) else {
        reflection_exception(eg, "ReflectionMethod has no resolved method");
        return Ok(());
    };
    let is_static = parameter_property_bool(ed, "__reflection_method_static");
    let reflected_class = reflected_property(ed, "__reflection_method_class")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    let object = with_argument(ed, 1, |value| {
        (!matches!(value.value_type(), ValueType::Undef | ValueType::Null)).then(|| value.clone())
    });
    if !is_static {
        let Some(instance) = object.as_ref().and_then(Value::as_object) else {
            eg.exception = Some(make_error_value(
                "ValueError",
                "ReflectionMethod::getClosure(): Argument #1 ($object) must be provided for non-static methods",
            ));
            return Ok(());
        };
        if !eg.class_is_a(instance.class_name.as_ref(), &reflected_class) {
            eg.exception = Some(make_error_value(
                "ReflectionException",
                "Given object is not an instance of the class this method was declared in",
            ));
            return Ok(());
        }
    }
    let called_scope_class_id = object
        .as_ref()
        .and_then(Value::as_object)
        .map(|object| object.class_id)
        .or_else(|| eg.find_class(&reflected_class).map(|class| class.class_id))
        .unwrap_or(0);
    return_value(
        rv,
        Value::closure(PhpClosure {
            object_handle: 0,
            func: function as *const FunctionCommon,
            called_scope_class_id,
            trait_scope_class_id: 0,
            is_static,
            bound_this: (!is_static).then_some(object).flatten(),
            captures: vec![],
            static_vars: None,
            has_heap_captures: false,
            scope_is_dummy: false,
        }),
    )
}

fn generic_target(ed: *mut ExecuteData) -> Option<(GenericDeclarationKind, String)> {
    with_argument(ed, 0, |value| {
        let object = value.as_object()?;
        let kind = match object.get_property("__generic_kind")?.as_str()? {
            "function" => GenericDeclarationKind::Function,
            "closure" => GenericDeclarationKind::Closure,
            "class" => GenericDeclarationKind::Class,
            "method" => GenericDeclarationKind::Method,
            _ => return None,
        };
        let owner = object
            .get_property("__generic_owner")?
            .as_str()?
            .to_string();
        Some((kind, owner))
    })
}

fn generic_runtime_modes(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let capabilities = GenericRuntimeCapabilities::CONFIGURED;
    let mut modes = PhpArray::with_packed_capacity(
        capabilities.erased as usize + capabilities.reified as usize,
    );
    if capabilities.erased {
        modes.push(Value::string("bound-erased"));
    }
    if capabilities.reified {
        modes.push(Value::string("reified"));
    }
    return_value(rv, Value::array(modes))
}

fn generic_arguments(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = with_argument(ed, 0, |value| {
        value
            .as_object()
            .and_then(|object| object.get_property("__generic_object").cloned())
    });
    #[cfg(feature = "php-generics-reified")]
    let arguments = if let Some(binding) = target
        .as_ref()
        .and_then(|target| eg.reified_object_binding(target))
    {
        let declaration = eg.generic_metadata.declaration(binding);
        eg.generic_metadata
            .reflection_reified_binding(binding)
            .map_or_else(PhpArray::new, |binding| {
                reflected_arguments(&eg.generic_metadata, declaration, &binding)
            })
    } else {
        PhpArray::new()
    };
    #[cfg(not(feature = "php-generics-reified"))]
    let arguments = {
        let _ = (target, eg);
        PhpArray::new()
    };
    return_value(rv, Value::array(arguments))
}
