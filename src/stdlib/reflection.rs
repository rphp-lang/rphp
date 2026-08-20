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
use crate::runtime::{ExecutorGlobals, LazyObjectStrategy};
use crate::value::{
    ArrayKey, DynamicPropertyMap, PhpArray, PhpClosure, PhpObject, ReferencePropertyConstraint,
    Value, ValueType, make_error_value,
};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{
    ATTRIBUTE_PUBLIC_TARGET_MASK, ATTRIBUTE_TARGET_PROPERTY_HOOK, AttributeDefinition,
    AttributeEvaluationScope, FunctionCommon, FunctionType, ParamTypeHint, UserFunction,
};

pub(super) use registry::register;

fn with_argument<R>(ed: *mut ExecuteData, index: u32, visit: impl FnOnce(&Value) -> R) -> R {
    let value = unsafe { (*ed).cv(index) };
    let value = if value.is_reference() {
        unsafe { &*value.as_ref_ptr() }
    } else {
        value
    };
    visit(value)
}

fn argument_string(ed: *mut ExecuteData, index: u32) -> String {
    with_argument(ed, index, |value| {
        value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.echo_to_string())
    })
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
    set_target(ed, kind, owner.clone());
    with_argument(ed, 0, |value| {
        if let Some(mut object) = value.as_object_mut() {
            object.set_property(
                "name",
                Value::string(
                    owner
                        .rsplit_once("::")
                        .map_or(owner.as_str(), |(_, name)| name),
                ),
            );
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
    (function.fn_type == FunctionType::User).then(|| {
        // SAFETY: FunctionCommon is the first field of every repr(C)
        // UserFunction and the discriminant above proves this allocation kind.
        unsafe { &*(function as *const FunctionCommon as *const UserFunction) }
    })
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
            let mut definitions = reflected_user_function(ed)
                .map(|function| function.attributes.clone())
                .unwrap_or_default();
            let called_class = reflected_property(ed, "__reflection_closure_called_class")
                .and_then(|value| value.as_str().map(str::to_owned));
            rebind_attribute_evaluation_scope(&mut definitions, called_class.as_deref(), eg);
            definitions
        }
        Some("ReflectionMethod") => {
            let mut definitions = reflected_user_function(ed)
                .map(|function| function.attributes.clone())
                .unwrap_or_default();
            let called_class = reflected_function_attribute_scope(ed);
            rebind_attribute_evaluation_scope(&mut definitions, called_class.as_deref(), eg);
            definitions
        }
        Some("ReflectionClass" | "ReflectionObject") => {
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
        Some("ReflectionClassConstant") => {
            let owner =
                reflected_property(ed, "class").and_then(|value| value.as_str().map(str::to_owned));
            let name =
                reflected_property(ed, "name").and_then(|value| value.as_str().map(str::to_owned));
            owner
                .as_deref()
                .zip(name.as_deref())
                .and_then(|(owner, name)| {
                    eg.find_class(owner)?
                        .constants
                        .iter()
                        .find(|constant| constant.name == name)
                })
                .map(|constant| constant.attributes.clone())
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
    Vm(VmError),
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
        Expr::Bool(value) => Ok(Value::bool(*value)),
        Expr::Null => Ok(Value::null()),
        Expr::Constant(name) => {
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
            ..
        } => deferred_class_constant(class_name, constant, scope, eg),
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
                constant_imports: HashMap::new(),
                lexical_class: scope.lexical_class.clone(),
                lexical_parent: scope.lexical_parent.clone(),
                lexical_property: scope.lexical_property.clone(),
                source_directory: scope.source_directory.clone(),
            };
            deferred_class_constant(class, constant, &dynamic_scope, eg)
        }
        Expr::BinaryOp { op, left, right } => {
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
        Expr::BitwiseNot(inner) => {
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
        Expr::ArrayAccess { array, index, .. } => {
            let array = evaluate_deferred_attribute_expression(array, scope, source_file, eg)?;
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
                &definition.source_file,
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
    let (Some(expression), Some(scope)) =
        (&definition.source_expression, &definition.evaluation_scope)
    else {
        return Ok(Some(definition.value.clone()));
    };
    match evaluate_deferred_attribute_expression(expression, scope, &definition.source_file, eg) {
        Ok(value) => Ok(Some(value)),
        Err(DeferredAttributeError::Message(error)) => {
            eg.exception = Some(make_error_value("Error", &error));
            Ok(None)
        }
        Err(DeferredAttributeError::Vm(error)) => Err(error),
    }
}

pub(crate) fn evaluate_deferred_property_default_value(
    definition: &crate::compiler::compile::DeferredPropertyDefault,
    eg: &mut ExecutorGlobals,
) -> Result<Option<Value>, VmError> {
    match evaluate_deferred_attribute_expression(
        &definition.expression,
        &definition.evaluation_scope,
        &definition.source_file,
        eg,
    ) {
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
        Expr::Constant(name) => {
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
        Expr::BinaryOp { op, left, right } => {
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
        | Expr::BitwiseNot(inner)
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
    eg: &mut ExecutorGlobals,
) -> Value {
    let object = object_value(
        "ReflectionAttribute",
        [("name", Value::string(definition.name.clone()))],
    );
    eg.register_reflection_attribute(&object, definition.clone(), repeated);
    object
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
        result.push(reflection_attribute_value(definition, repeated, eg));
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
    let Some((definition, repeated)) = eg
        .reflection_attribute_state(&receiver)
        .map(|state| (state.definition.clone(), state.repeated))
    else {
        eg.exception = Some(make_error_value(
            "Error",
            "Invalid ReflectionAttribute object",
        ));
        return Ok(());
    };
    instantiate_attribute_definition(ed, rv, &definition, repeated, eg)
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
    instantiate_attribute_definition_at_use(ed, rv, definition, repeated, eg, None)
}

fn instantiate_attribute_definition_at_use(
    ed: *mut ExecuteData,
    rv: *mut Value,
    definition: &AttributeDefinition,
    repeated: bool,
    eg: &mut ExecutorGlobals,
    deprecated_use_site: Option<&DeprecatedUseSite>,
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
            crate::vm::execute::CallArgumentPreparation::Coerced(value) => {
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
        }),
    )
}

fn hint_metadata(hint: &ParamTypeHint) -> (&'static str, String, bool) {
    match hint {
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

fn function_get_parameters(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(function) = reflected_function(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
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
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let count = reflected_function(ed).map_or(0, |function| {
        function.sig.public_arity() + u32::from(function.sig.is_variadic)
    });
    return_value(rv, Value::long(i64::from(count)))
}

fn function_get_number_of_required_parameters(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
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
    });
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
    let anonymous = reflected_property(ed, "__generic_kind")
        .and_then(|kind| kind.as_str().map(|kind| kind == "closure"))
        .unwrap_or(false)
        && reflected_property(ed, "__generic_owner")
            .and_then(|owner| owner.as_str().map(|owner| owner.starts_with("__closure_")))
            .unwrap_or(false);
    return_value(rv, Value::bool(anonymous))
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

fn parameter_property_bool(ed: *mut ExecuteData, name: &str) -> bool {
    reflected_property(ed, name)
        .map(|value| value.is_truthy())
        .unwrap_or(false)
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

fn parameter_to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
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
    // Default expressions currently live in bytecode. Zend uses the same
    // `<default>` placeholder for values unavailable through reflection.
    let default = if has_default { " = <default>" } else { "" };
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
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if !parameter_property_bool(ed, "__reflection_has_type") {
        return return_value(rv, Value::null());
    }
    let kind = reflected_property(ed, "__reflection_type_kind")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "named".to_string());
    let name = reflected_property(ed, "__reflection_type_name")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    let class = match kind.as_str() {
        "union" => "ReflectionUnionType",
        "intersection" => "ReflectionIntersectionType",
        _ => "ReflectionNamedType",
    };
    return_value(
        rv,
        object_value(
            class,
            [
                ("__generic_name", Value::string(name.clone())),
                ("__generic_string", Value::string(name)),
                (
                    "__reflection_allows_null",
                    Value::bool(parameter_property_bool(ed, "__reflection_allows_null")),
                ),
            ],
        ),
    )
}

fn parameter_has_type(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_has_type")),
    )
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
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // Default expressions currently live in function bytecode rather than
    // signature metadata; null is the conservative reflected placeholder.
    return_value(rv, Value::null())
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
    let owner = eg.find_class(&owner).map_or(owner, |class| {
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
    let Some((declaring_class, value, modifiers)) = eg.find_class(&class_name).and_then(|class| {
        class
            .constants
            .iter()
            .find(|constant| constant.name == constant_name)
            .map(|constant| {
                let visibility = match constant.visibility {
                    Visibility::Public => 1,
                    Visibility::Protected => 2,
                    Visibility::Private => 4,
                };
                (
                    constant.declaring_class.clone(),
                    constant.value.clone(),
                    visibility | if constant.is_final { 32 } else { 0 },
                )
            })
    }) else {
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
    if let Some(set_visibility) = property.set_visibility {
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
    let class_name = reflected_property(ed, "__reflection_target")
        .and_then(|target| {
            target
                .as_object()
                .map(|object| object.class_name.to_string())
                .or_else(|| target.as_str().map(str::to_owned))
        })
        .or_else(|| {
            reflected_property(ed, "class").and_then(|class| class.as_str().map(str::to_owned))
        })?;
    let name = reflected_property(ed, "__reflection_property")
        .or_else(|| reflected_property(ed, "name"))?
        .as_str()?
        .to_string();
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
    let name = reflected_property(ed, "__reflection_property")
        .or_else(|| reflected_property(ed, "name"))
        .and_then(|name| name.as_str().map(str::to_owned))
        .unwrap_or_default();
    let rendered = reflection_property_definition(ed, eg).map_or_else(
        || format!("Property [ <dynamic> public ${name} ]\n"),
        |(property, is_static)| {
            format!("{}\n", render_reflection_property(property, is_static, eg))
        },
    );
    return_value(rv, Value::string(rendered))
}

fn render_reflection_method(
    name: &str,
    visibility: Visibility,
    is_static: bool,
    is_final: bool,
    is_abstract: bool,
) -> String {
    let mut declaration = String::new();
    if is_final {
        declaration.push_str("final ");
    }
    if is_abstract {
        declaration.push_str("abstract ");
    }
    declaration.push_str(reflection_visibility(visibility));
    declaration.push(' ');
    if is_static {
        declaration.push_str("static ");
    }
    declaration.push_str("method ");
    declaration.push_str(name);
    format!("Method [ <user> {declaration} ] {{\n    }}")
}

fn render_reflection_signature_parameter(
    function: &FunctionCommon,
    index: u32,
    variadic: bool,
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
        " = <default>"
    } else {
        ""
    };
    format!(
        "Parameter #{index} [ <{requirement}> {type_prefix}{reference}{variadic_prefix}${name}{default} ]"
    )
}

fn method_to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
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
    let mut modifiers = String::new();
    if parameter_property_bool(ed, "__reflection_method_final") {
        modifiers.push_str("final ");
    }
    if parameter_property_bool(ed, "__reflection_method_abstract") {
        modifiers.push_str("abstract ");
    }
    modifiers.push_str(match visibility {
        2 => "protected ",
        4 => "private ",
        _ => "public ",
    });
    if parameter_property_bool(ed, "__reflection_method_static") {
        modifiers.push_str("static ");
    }
    let provenance = if function.fn_type == FunctionType::User {
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

    if let Some(user) = reflected_user_function(ed) {
        let start = user.op_array.declaration_line().or_else(|| {
            user.op_array
                .source_lines
                .iter()
                .find_map(|(index, line)| (*index != u32::MAX).then_some(*line as usize))
        });
        let end = user
            .op_array
            .source_lines
            .iter()
            .filter_map(|(index, line)| (*index != u32::MAX).then_some(*line as usize))
            .max()
            .or(start);
        if !user.op_array.source_file.is_empty()
            && let (Some(start), Some(end)) = (start, end)
        {
            rendered.push_str(&format!(
                "  @@ {} {start} - {end}\n\n",
                user.op_array.source_file
            ));
        }
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
                function, index, variadic,
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
        // Property setters have an implicit void reflection contract even
        // though the execution signature does not need a return-type guard.
        rendered.push_str("  - Return [ void ]\n");
    }
    rendered.push_str("}\n");
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
    let is_object = with_argument(ed, 0, |value| {
        value
            .as_object()
            .is_some_and(|object| object.class_name.eq_ignore_ascii_case("ReflectionObject"))
    });
    let mut modifiers = String::new();
    if class.is_final {
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
    let mut rendered = format!(
        "{title} [ <{provenance}> {modifiers}{kind} {} ] {{\n",
        class.name
    );
    if let Some(source_file) = &class.source_file {
        rendered.push_str(&format!(
            "  @@ {source_file} {}-{}\n\n",
            class.declaration_line, class.declaration_line
        ));
    }

    rendered.push_str(&format!("  - Constants [{}] {{\n", class.constants.len()));
    for constant in &class.constants {
        let final_modifier = if constant.is_final { "final " } else { "" };
        let type_name = if matches!(constant.type_hint, ParamTypeHint::None) {
            String::new()
        } else {
            format!("{} ", constant.type_hint.display_name())
        };
        rendered.push_str(&format!(
            "    Constant [ {final_modifier}{} {type_name}{} ] {{ {} }}\n",
            reflection_visibility(constant.visibility),
            constant.name,
            reflection_value_name(&constant.value, eg)
        ));
    }
    rendered.push_str("  }\n\n");

    rendered.push_str(&format!(
        "  - Static properties [{}] {{\n",
        class.static_properties.len()
    ));
    for property in &class.static_properties {
        rendered.push_str("    ");
        rendered.push_str(&render_reflection_property(property, true, eg));
        rendered.push('\n');
    }
    rendered.push_str("  }\n\n");

    let mut methods = Vec::new();
    collect_reflected_methods(eg, &owner, &mut methods, &mut HashSet::new());
    let static_method_count = methods
        .iter()
        .filter(|(_, _, is_static, ..)| *is_static)
        .count();
    rendered.push_str(&format!("  - Static methods [{static_method_count}] {{\n"));
    for (name, visibility, is_static, is_final, _, declaring_class) in &methods {
        if !is_static {
            continue;
        }
        rendered.push_str("    ");
        rendered.push_str(&render_reflection_method(
            name,
            *visibility,
            true,
            *is_final,
            eg.find_class(declaring_class)
                .is_some_and(|class| class.method_is_abstract(name)),
        ));
        rendered.push('\n');
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
    for (name, visibility, is_static, is_final, _, declaring_class) in &methods {
        if *is_static {
            continue;
        }
        rendered.push_str("    ");
        rendered.push_str(&render_reflection_method(
            name,
            *visibility,
            false,
            *is_final,
            eg.find_class(declaring_class)
                .is_some_and(|class| class.method_is_abstract(name)),
        ));
        rendered.push('\n');
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

fn class_get_attributes(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let attributes = reflected_attribute_definitions(ed, eg);
    reflection_attributes(ed, rv, eg, attributes)
}

fn reflection_get_doc_comment(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // The frontend deliberately discards comments today. Returning false is
    // PHP's truthful "no retained doc comment" result; an empty string would
    // incorrectly claim that a doc comment exists.
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

fn collect_reflected_interface_names(
    eg: &ExecutorGlobals,
    owner: &str,
    names: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    let Some(class) = eg.find_class(owner) else {
        return;
    };
    let parent = class.parent.clone();
    let interfaces = class.implements.clone();

    // PHP reports interfaces inherited from the parent class first, then the
    // class's own declarations and each interface's extended ancestors.
    if let Some(parent) = parent {
        collect_reflected_interface_names(eg, &parent, names, seen);
    }
    for interface in interfaces {
        let canonical = eg
            .find_class(&interface)
            .map_or(interface, |class| class.name.clone());
        if seen.insert(canonical.to_ascii_lowercase()) {
            names.push(canonical.clone());
            collect_reflected_interface_names(eg, &canonical, names, seen);
        }
    }
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
    let mut names = Vec::new();
    collect_reflected_interface_names(eg, &owner, &mut names, &mut HashSet::new());
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
    let mut names = Vec::new();
    collect_reflected_interface_names(eg, &owner, &mut names, &mut HashSet::new());
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
    let mut constants = PhpArray::with_hash_capacity(class.constants.len());
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
    let Some(definition) = eg.find_class(&owner).and_then(|class| {
        class
            .constants
            .iter()
            .find(|constant| constant.name == name)
            .cloned()
    }) else {
        return return_value(rv, Value::bool(false));
    };
    if let Some(error) = &definition.evaluation_error {
        eg.exception = Some(make_error_value("Error", error));
        return return_value(rv, Value::null());
    }
    if definition.value_is_deferred {
        let Some(value) = evaluate_deferred_class_constant_value(&definition, eg)? else {
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
    let mut constants = PhpArray::with_packed_capacity(class.constants.len());
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
        constants.push(object_value(
            "ReflectionClassConstant",
            [
                ("name", Value::string(constant.name.clone())),
                ("class", Value::string(constant.declaring_class.clone())),
                (
                    "__reflection_declaring_class",
                    Value::string(constant.declaring_class.clone()),
                ),
                ("__reflection_modifiers", Value::long(modifiers)),
                ("__reflection_value", constant.value.clone()),
            ],
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
    let Some(constant) = eg.find_class(&owner).and_then(|class| {
        class
            .constants
            .iter()
            .find(|constant| constant.name == name)
    }) else {
        return return_value(rv, Value::bool(false));
    };
    let visibility = match constant.visibility {
        Visibility::Public => 1,
        Visibility::Protected => 2,
        Visibility::Private => 4,
    };
    let modifiers = visibility | if constant.is_final { 32 } else { 0 };
    return_value(
        rv,
        object_value(
            "ReflectionClassConstant",
            [
                ("name", Value::string(constant.name.clone())),
                ("class", Value::string(constant.declaring_class.clone())),
                (
                    "__reflection_declaring_class",
                    Value::string(constant.declaring_class.clone()),
                ),
                ("__reflection_modifiers", Value::long(modifiers)),
                ("__reflection_value", constant.value.clone()),
            ],
        ),
    )
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
    for property in class
        .static_properties
        .iter()
        .chain(class.properties.iter())
    {
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
    for trait_name in &class.uses {
        let Some(trait_class) = eg.find_class(trait_name) else {
            continue;
        };
        for (name, visibility, is_static, is_final, function) in &trait_class.methods {
            if trait_class.method_is_abstract(name) || !seen.insert(name.to_ascii_lowercase()) {
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
                is_implicit_enum_static || {
                    // SAFETY: find_function() returns a registered
                    // FunctionCommon owned by ExecutorGlobals for the full
                    // request lifetime.
                    unsafe { (*function).sig.this_offset == 0 }
                },
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
    let Some(function) = reflected_function(ed) else {
        reflection_exception(eg, "ReflectionMethod has no resolved method");
        return Ok(());
    };
    let receiver = with_argument(ed, 1, Clone::clone);
    let arguments = with_argument(ed, 2, |value| {
        if let Some(values) = value.as_array() {
            values.values().cloned().collect::<Vec<_>>()
        } else if matches!(value.value_type(), ValueType::Undef) {
            Vec::new()
        } else {
            vec![value.clone()]
        }
    });
    let mut call_arguments = Vec::with_capacity(arguments.len() + 1);
    if function.sig.this_offset == 1 {
        call_arguments.push(receiver);
    }
    call_arguments.extend(arguments);
    let result = crate::vm::execute::call_function(eg, function, &call_arguments)?;
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

fn reflected_property_value(property: &PropertyDefinition, is_static: bool) -> Value {
    let declaring_class = property.declaring_class.clone();
    let has_type = !matches!(property.type_hint, ParamTypeHint::None);
    let (type_kind, type_name, allows_null) = hint_metadata(&property.type_hint);
    object_value(
        "ReflectionProperty",
        [
            (
                "__reflection_target",
                Value::string(declaring_class.clone()),
            ),
            (
                "__reflection_property",
                Value::string(property.name.clone()),
            ),
            (
                "__reflection_modifiers",
                Value::long(property_modifiers(property, is_static)),
            ),
            ("__reflection_has_type", Value::bool(has_type)),
            ("__reflection_type_kind", Value::string(type_kind)),
            ("__reflection_type_name", Value::string(type_name)),
            ("__reflection_allows_null", Value::bool(allows_null)),
            (
                "__reflection_has_default",
                Value::bool(property.has_default()),
            ),
            (
                "__reflection_default",
                property.default.clone().unwrap_or_else(Value::null),
            ),
            ("name", Value::string(property.name.clone())),
            ("class", Value::string(declaring_class)),
        ],
    )
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
    let filter = with_argument(ed, 1, |value| value.as_long());
    let mut declarations = class
        .properties
        .iter()
        .map(|property| (property, false))
        .chain(
            class
                .static_properties
                .iter()
                .filter(|_| !class.is_enum)
                .map(|property| (property, true)),
        )
        .collect::<Vec<_>>();
    declarations.sort_by_key(|(property, _)| {
        let mut rank = 0usize;
        let mut current = Some(class.name.as_str());
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
            && !property.declaring_class.eq_ignore_ascii_case(&class.name)
        {
            continue;
        }
        let modifiers = property_modifiers(property, is_static);
        if filter.is_some_and(|filter| modifiers & filter == 0) {
            continue;
        }
        properties.push(reflected_property_value(property, is_static));
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
            .map(|property| (property, false))
            .or_else(|| {
                class
                    .static_properties
                    .iter()
                    .find(|property| {
                        property.name == property_name
                            && (property.visibility != Visibility::Private
                                || property.declaring_class.eq_ignore_ascii_case(&class.name))
                    })
                    .map(|property| (property, true))
            })
    });
    let Some((property, is_static)) = property else {
        reflection_exception(
            eg,
            format!("Property {owner}::${property_name} does not exist"),
        );
        return Ok(());
    };
    return_value(rv, reflected_property_value(property, is_static))
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

fn class_new_instance_without_constructor(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return Err(VmError::Fatal(
            "ReflectionClass::newInstanceWithoutConstructor() requires a reflected class".into(),
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
            "Class {owner} cannot be instantiated without invoking its constructor"
        )));
    }
    let object = if class.class_id == 0 {
        PhpObject::dynamic(class.name.clone(), 0, HashMap::new())
    } else {
        PhpObject::with_layout(
            class.class_id,
            class.property_layout.clone(),
            class.property_defaults.as_ref().to_vec(),
        )
    };
    return_value(rv, Value::object(object))
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
    let metadata = class_name.as_deref().and_then(|class_name| {
        let class = eg.find_class(class_name)?;
        class
            .properties
            .iter()
            .find(|property| property.name == name)
            .map(|property| {
                (
                    property.declaring_class.clone(),
                    property_modifiers(property, false),
                    property.type_hint.clone(),
                    property.has_default(),
                    property.default.clone().unwrap_or_else(Value::null),
                )
            })
            .or_else(|| {
                class
                    .static_properties
                    .iter()
                    .find(|property| property.name == name)
                    .map(|property| {
                        (
                            property.declaring_class.clone(),
                            property_modifiers(property, true),
                            property.type_hint.clone(),
                            property.has_default(),
                            property.default.clone().unwrap_or_else(Value::null),
                        )
                    })
            })
    });
    with_argument(ed, 0, |value| {
        if let Some(mut object) = value.as_object_mut() {
            object.set_property("__reflection_target", target);
            object.set_property("__reflection_property", Value::string(name.clone()));
            object.set_property("name", Value::string(name));
            if let Some((declaring_class, modifiers, type_hint, has_default, default)) = metadata {
                let has_type = !matches!(type_hint, ParamTypeHint::None);
                let (type_kind, type_name, allows_null) = hint_metadata(&type_hint);
                object.set_property("class", Value::string(declaring_class));
                object.set_property("__reflection_modifiers", Value::long(modifiers));
                object.set_property("__reflection_has_type", Value::bool(has_type));
                object.set_property("__reflection_type_kind", Value::string(type_kind));
                object.set_property("__reflection_type_name", Value::string(type_name));
                object.set_property("__reflection_allows_null", Value::bool(allows_null));
                object.set_property("__reflection_has_default", Value::bool(has_default));
                object.set_property("__reflection_default", default);
            }
        }
    });
    Ok(())
}

fn property_get_modifiers(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__reflection_modifiers").unwrap_or_else(|| Value::long(0)),
    )
}

fn property_is_static(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let modifiers = reflected_property(ed, "__reflection_modifiers")
        .and_then(|value| value.as_long())
        .unwrap_or(0);
    return_value(rv, Value::bool(modifiers & 16 != 0))
}

fn property_is_readonly(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let modifiers = reflected_property(ed, "__reflection_modifiers")
        .and_then(|value| value.as_long())
        .unwrap_or(0);
    return_value(rv, Value::bool(modifiers & 128 != 0))
}

fn property_modifier_is(
    ed: *mut ExecuteData,
    rv: *mut Value,
    expected: i64,
) -> Result<(), VmError> {
    let modifiers = reflected_property(ed, "__reflection_modifiers")
        .and_then(|value| value.as_long())
        .unwrap_or(0);
    return_value(rv, Value::bool(modifiers & expected != 0))
}

fn property_is_final(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_modifier_is(ed, rv, 32)
}

fn property_is_abstract(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_modifier_is(ed, rv, 64)
}

fn property_is_virtual(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_modifier_is(ed, rv, 512)
}

fn property_has_default_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let has_default =
        reflected_property(ed, "__reflection_has_default").is_some_and(|value| value.is_truthy());
    return_value(rv, Value::bool(has_default))
}

fn property_get_default_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let has_default =
        reflected_property(ed, "__reflection_has_default").is_some_and(|value| value.is_truthy());
    if !has_default {
        super::report_internal_deprecation(
            eg,
            ed,
            "ReflectionProperty::getDefaultValue() for a property without a default value is deprecated, use ReflectionProperty::hasDefaultValue() to check if the default value exists",
        )?;
    }
    return_value(
        rv,
        reflected_property(ed, "__reflection_default").unwrap_or_else(Value::null),
    )
}

fn property_is_default(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::bool(true))
}

fn property_visibility_is(
    ed: *mut ExecuteData,
    rv: *mut Value,
    expected: i64,
) -> Result<(), VmError> {
    let modifiers = reflected_property(ed, "__reflection_modifiers")
        .and_then(|value| value.as_long())
        .unwrap_or(1);
    return_value(rv, Value::bool(modifiers & 7 == expected))
}

fn property_is_public(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_visibility_is(ed, rv, 1)
}

fn property_is_protected(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_visibility_is(ed, rv, 2)
}

fn property_is_private(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_visibility_is(ed, rv, 4)
}

fn reflected_property_target(ed: *mut ExecuteData) -> Option<(Value, String)> {
    with_argument(ed, 0, |value| {
        let object = value.as_object()?;
        let target = object.get_property("__reflection_target")?.clone();
        let property = object
            .get_property("__reflection_property")?
            .as_str()?
            .to_string();
        Some((target, property))
    })
}

fn reflected_property_object(ed: *mut ExecuteData, index: u32) -> Option<Value> {
    with_argument(ed, index, |value| {
        (value.value_type() == crate::value::ValueType::Object).then(|| value.clone())
    })
    .or_else(|| {
        reflected_property_target(ed).and_then(|(target, _)| {
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
    let (_, property) = reflected_property_target(ed)?;
    let mut target = reflected_property_object(ed, 1)?;
    let reflected_scope = reflected_property_scope(ed);
    let modifiers = reflected_property(ed, "__reflection_modifiers")
        .and_then(|value| value.as_long())
        .unwrap_or(0);
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
    let Some((_, property)) = reflected_property_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    let reflected_scope = reflected_property_scope(ed);
    let initialized = if let Some(target) = reflected_property_object(ed, 1) {
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
    let Some((_, property)) = reflected_property_target(ed) else {
        return return_value(rv, Value::null());
    };
    let reflected_scope = reflected_property_scope(ed);
    let value = if let Some(target) = reflected_property_object(ed, 1) {
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
    let Some((_, property)) = reflected_property_target(ed) else {
        return Ok(());
    };
    let reflected_scope = reflected_property_scope(ed);
    let value = with_argument(ed, 2, Clone::clone);
    if let Some(target) = reflected_property_object(ed, 1) {
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
    let Some((_, property)) = reflected_property_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    let Some(mut target) = reflected_property_object(ed, 1) else {
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
