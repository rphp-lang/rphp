//! RFC generic-parameter and pre-erasure type Reflection objects.
//!
//! Method declarations have two parameter scopes: class-like parameters use
//! ordinary indices while method-local parameters use the compact high-bit
//! encoding. Keeping that translation here prevents Reflection concerns from
//! leaking into runtime contract checks or the main stdlib registration unit.

use super::{
    generic_target, named_reflected_type, object_value, reflected_property, reflection_exception,
    return_value,
};
use crate::generics::{
    GenericDeclaration, GenericDeclarationKind, GenericMetadata, GenericMethodMetadata,
    GenericParameterMetadata, GenericType, GenericVariance, method_parameter_index,
};
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

thread_local! {
    static GENERIC_VARIANCE_CASES: [Value; 3] = [
        generic_variance_case("Invariant"),
        generic_variance_case("Covariant"),
        generic_variance_case("Contravariant"),
    ];
}

fn generic_variance_case(name: &str) -> Value {
    object_value("ReflectionGenericVariance", [("name", Value::string(name))])
}

fn generic_variance_value(variance: GenericVariance) -> Value {
    let index = match variance {
        GenericVariance::Invariant => 0,
        GenericVariance::Covariant => 1,
        GenericVariance::Contravariant => 2,
    };
    GENERIC_VARIANCE_CASES.with(|cases| cases[index].clone())
}

pub(super) fn generic_variance_cases() -> [Value; 3] {
    GENERIC_VARIANCE_CASES.with(Clone::clone)
}

fn reflection_kind_name(kind: GenericDeclarationKind) -> &'static str {
    match kind {
        GenericDeclarationKind::Class
        | GenericDeclarationKind::Interface
        | GenericDeclarationKind::Trait => "class",
        GenericDeclarationKind::Function => "function",
        GenericDeclarationKind::Closure => "closure",
        GenericDeclarationKind::Method => "method",
    }
}

#[derive(Clone, Copy)]
enum GenericReflectionScope<'a> {
    Declaration(&'a GenericDeclaration),
    Method {
        declaration: &'a GenericDeclaration,
        method: &'a GenericMethodMetadata,
    },
}

impl<'a> GenericReflectionScope<'a> {
    fn parameter(self, position: usize) -> Option<&'a GenericParameterMetadata> {
        match self {
            Self::Declaration(declaration) => declaration.parameters.get(position),
            Self::Method { method, .. } => method.parameters.get(position),
        }
    }

    fn parameter_count(self) -> usize {
        match self {
            Self::Declaration(declaration) => declaration.parameters.len(),
            Self::Method { method, .. } => method.parameters.len(),
        }
    }

    fn kind(self) -> GenericDeclarationKind {
        match self {
            Self::Declaration(declaration) => declaration.kind,
            Self::Method { .. } => GenericDeclarationKind::Method,
        }
    }

    fn owner(self, metadata: &GenericMetadata) -> String {
        match self {
            Self::Declaration(declaration) => metadata
                .symbol(declaration.owner)
                .unwrap_or("?")
                .to_string(),
            Self::Method {
                declaration,
                method,
            } => format!(
                "{}::{}",
                metadata.symbol(declaration.owner).unwrap_or("?"),
                metadata.symbol(method.name).unwrap_or("?")
            ),
        }
    }

    fn type_parameter(self, index: u8) -> Option<(Self, usize)> {
        match self {
            Self::Declaration(declaration) => declaration
                .parameters
                .get(index as usize)
                .map(|_| (self, index as usize)),
            Self::Method {
                declaration,
                method,
            } => {
                if let Some(position) = method_parameter_index(index) {
                    method.parameters.get(position).map(|_| (self, position))
                } else {
                    declaration
                        .parameters
                        .get(index as usize)
                        .map(|_| (Self::Declaration(declaration), index as usize))
                }
            }
        }
    }
}

fn generic_parameter_value(
    metadata: &GenericMetadata,
    scope: GenericReflectionScope<'_>,
    position: usize,
) -> Value {
    let parameter = scope
        .parameter(position)
        .expect("reflected generic parameter position");
    let name = metadata.symbol(parameter.name).unwrap_or("?");
    let owner = scope.owner(metadata);
    object_value(
        "ReflectionGenericTypeParameter",
        [
            ("name", Value::string(name)),
            (
                "__generic_kind",
                Value::string(reflection_kind_name(scope.kind())),
            ),
            ("__generic_owner", Value::string(owner)),
            ("__generic_position", Value::long(position as i64)),
            (
                "__generic_variance",
                generic_variance_value(parameter.variance),
            ),
            ("__generic_string", Value::string(name)),
        ],
    )
}

pub(super) fn reflected_type(
    metadata: &GenericMetadata,
    declaration: &GenericDeclaration,
    value: &GenericType,
) -> Value {
    reflected_type_in_scope(
        metadata,
        GenericReflectionScope::Declaration(declaration),
        value,
    )
}

fn reflected_type_in_scope(
    metadata: &GenericMetadata,
    scope: GenericReflectionScope<'_>,
    value: &GenericType,
) -> Value {
    let rendered = format_reflected_type(metadata, scope, value);
    match value {
        GenericType::Parameter(index) => {
            let Some((parameter_scope, position)) = scope.type_parameter(*index) else {
                return named_reflected_type("?");
            };
            let name = parameter_scope
                .parameter(position)
                .and_then(|parameter| metadata.symbol(parameter.name))
                .unwrap_or("?");
            object_value(
                "ReflectionTypeParameterReference",
                [
                    ("name", Value::string(name)),
                    ("__generic_name", Value::string(name)),
                    (
                        "__generic_parameter",
                        generic_parameter_value(metadata, parameter_scope, position),
                    ),
                    ("__generic_string", Value::string(rendered)),
                ],
            )
        }
        GenericType::Union(parts) | GenericType::Intersection(parts) => {
            let mut types = PhpArray::with_packed_capacity(parts.len());
            for part in parts.iter() {
                types.push(reflected_type_in_scope(metadata, scope, part));
            }
            let class_name = if matches!(value, GenericType::Union(_)) {
                "ReflectionUnionType"
            } else {
                "ReflectionIntersectionType"
            };
            object_value(
                class_name,
                [
                    ("__generic_types", Value::array(types)),
                    ("__generic_string", Value::string(rendered)),
                ],
            )
        }
        GenericType::Named { name, arguments } => {
            let mut reflected_arguments = PhpArray::with_packed_capacity(arguments.len());
            for argument in arguments.iter() {
                reflected_arguments.push(reflected_type_in_scope(metadata, scope, argument));
            }
            object_value(
                "ReflectionNamedType",
                [
                    (
                        "__generic_name",
                        Value::string(metadata.symbol(*name).unwrap_or("?")),
                    ),
                    ("__generic_arguments", Value::array(reflected_arguments)),
                    ("__generic_string", Value::string(rendered)),
                ],
            )
        }
        GenericType::Nullable(inner) => {
            let mut types = PhpArray::with_packed_capacity(2);
            types.push(reflected_type_in_scope(metadata, scope, inner));
            types.push(named_reflected_type("null"));
            object_value(
                "ReflectionUnionType",
                [
                    ("__generic_types", Value::array(types)),
                    ("__generic_string", Value::string(rendered)),
                ],
            )
        }
        _ => named_reflected_type(&rendered),
    }
}

fn format_reflected_type(
    metadata: &GenericMetadata,
    scope: GenericReflectionScope<'_>,
    value: &GenericType,
) -> String {
    match value {
        GenericType::Int => "int".into(),
        GenericType::Float => "float".into(),
        GenericType::String => "string".into(),
        GenericType::Bool => "bool".into(),
        GenericType::Array => "array".into(),
        GenericType::Callable => "callable".into(),
        GenericType::Null => "null".into(),
        GenericType::Void => "void".into(),
        GenericType::Mixed => "mixed".into(),
        GenericType::Never => "never".into(),
        GenericType::Parameter(index) => scope
            .type_parameter(*index)
            .and_then(|(scope, position)| scope.parameter(position))
            .and_then(|parameter| metadata.symbol(parameter.name))
            .unwrap_or("?")
            .to_string(),
        GenericType::Named { name, arguments } => {
            let mut rendered = metadata.symbol(*name).unwrap_or("?").to_string();
            if !arguments.is_empty() {
                rendered.push('<');
                for (index, argument) in arguments.iter().enumerate() {
                    if index != 0 {
                        rendered.push_str(", ");
                    }
                    rendered.push_str(&format_reflected_type(metadata, scope, argument));
                }
                rendered.push('>');
            }
            rendered
        }
        GenericType::Nullable(inner) => {
            format!("?{}", format_reflected_type(metadata, scope, inner))
        }
        GenericType::Union(parts) => parts
            .iter()
            .map(|part| format_reflected_type(metadata, scope, part))
            .collect::<Vec<_>>()
            .join("|"),
        GenericType::Intersection(parts) => parts
            .iter()
            .map(|part| format_reflected_type(metadata, scope, part))
            .collect::<Vec<_>>()
            .join("&"),
    }
}

fn reflection_declaration<'a>(
    metadata: &'a GenericMetadata,
    kind: GenericDeclarationKind,
    owner: &str,
) -> Option<&'a GenericDeclaration> {
    if kind == GenericDeclarationKind::Class {
        return metadata
            .find_class_like_index(owner)
            .and_then(|index| metadata.declarations().get(index as usize));
    }
    metadata.find(kind, owner)
}

fn reflection_scope<'a>(
    metadata: &'a GenericMetadata,
    kind: GenericDeclarationKind,
    owner: &str,
) -> Option<GenericReflectionScope<'a>> {
    if kind == GenericDeclarationKind::Method
        && let Some((class_name, method_name)) = owner.rsplit_once("::")
        && let Some(declaration) = metadata
            .find_class_like_index(class_name)
            .and_then(|index| metadata.declarations().get(index as usize))
        && let Some(method) = declaration.methods.iter().find(|method| {
            metadata
                .symbol(method.name)
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(method_name))
        })
    {
        return Some(GenericReflectionScope::Method {
            declaration,
            method,
        });
    }
    reflection_declaration(metadata, kind, owner).map(GenericReflectionScope::Declaration)
}

fn generic_parameter_context<'a>(
    ed: *mut ExecuteData,
    metadata: &'a GenericMetadata,
) -> Option<(GenericReflectionScope<'a>, usize)> {
    let (kind, owner) = generic_target(ed)?;
    let position = reflected_property(ed, "__generic_position")?.as_long()? as usize;
    let scope = reflection_scope(metadata, kind, &owner)?;
    scope.parameter(position)?;
    Some((scope, position))
}

pub(super) fn generic_parameter_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "name").unwrap_or_else(|| Value::string("")),
    )
}

pub(super) fn generic_parameter_position(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__generic_position").unwrap_or_else(|| Value::long(0)),
    )
}

pub(super) fn generic_parameter_variance(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__generic_variance")
            .unwrap_or_else(|| generic_variance_value(GenericVariance::Invariant)),
    )
}

pub(super) fn generic_parameter_has_bound(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let found =
        generic_parameter_context(ed, &eg.generic_metadata).is_some_and(|(scope, position)| {
            scope.parameter(position).is_some_and(|p| p.bound.is_some())
        });
    return_value(rv, Value::bool(found))
}

pub(super) fn generic_parameter_bound(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((scope, position)) = generic_parameter_context(ed, &eg.generic_metadata) else {
        reflection_exception(eg, "Invalid generic type parameter Reflection target");
        return Ok(());
    };
    let Some(parameter) = scope.parameter(position) else {
        reflection_exception(eg, "Invalid generic type parameter Reflection target");
        return Ok(());
    };
    let Some(bound) = parameter.bound.as_ref() else {
        let name = eg.generic_metadata.symbol(parameter.name).unwrap_or("?");
        reflection_exception(eg, format!("Generic type parameter {name} has no bound"));
        return Ok(());
    };
    let bound = reflected_type_in_scope(&eg.generic_metadata, scope, bound);
    return_value(rv, bound)
}

pub(super) fn generic_parameter_has_default(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let found =
        generic_parameter_context(ed, &eg.generic_metadata).is_some_and(|(scope, position)| {
            scope
                .parameter(position)
                .is_some_and(|p| p.default.is_some())
        });
    return_value(rv, Value::bool(found))
}

pub(super) fn generic_parameter_default(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((scope, position)) = generic_parameter_context(ed, &eg.generic_metadata) else {
        reflection_exception(eg, "Invalid generic type parameter Reflection target");
        return Ok(());
    };
    let Some(parameter) = scope.parameter(position) else {
        reflection_exception(eg, "Invalid generic type parameter Reflection target");
        return Ok(());
    };
    let Some(default) = parameter.default.as_ref() else {
        let name = eg.generic_metadata.symbol(parameter.name).unwrap_or("?");
        reflection_exception(eg, format!("Generic type parameter {name} has no default"));
        return Ok(());
    };
    let default = reflected_type_in_scope(&eg.generic_metadata, scope, default);
    return_value(rv, default)
}

pub(super) fn generic_parameter_declaring_entity(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((scope, _)) = generic_parameter_context(ed, &eg.generic_metadata) else {
        reflection_exception(eg, "Invalid generic type parameter Reflection target");
        return Ok(());
    };
    let owner = scope.owner(&eg.generic_metadata);
    let (class_name, kind) = match scope.kind() {
        GenericDeclarationKind::Class
        | GenericDeclarationKind::Interface
        | GenericDeclarationKind::Trait => ("ReflectionClass", "class"),
        GenericDeclarationKind::Function => ("ReflectionFunction", "function"),
        GenericDeclarationKind::Closure => ("ReflectionFunction", "closure"),
        GenericDeclarationKind::Method => ("ReflectionMethod", "method"),
    };
    return_value(
        rv,
        object_value(
            class_name,
            [
                ("__generic_kind", Value::string(kind)),
                ("__generic_owner", Value::string(owner)),
            ],
        ),
    )
}

pub(super) fn type_parameter_reference_parameter(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__generic_parameter").unwrap_or_else(Value::null),
    )
}

pub(super) fn is_generic(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let found = generic_target(ed).is_some_and(|(kind, owner)| {
        reflection_scope(&eg.generic_metadata, kind, &owner)
            .is_some_and(|scope| scope.parameter_count() != 0)
    });
    return_value(rv, Value::bool(found))
}

pub(super) fn generic_parameters(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((kind, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let Some(scope) = reflection_scope(&eg.generic_metadata, kind, &owner) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let mut result = PhpArray::with_packed_capacity(scope.parameter_count());
    for position in 0..scope.parameter_count() {
        result.push(generic_parameter_value(
            &eg.generic_metadata,
            scope,
            position,
        ));
    }
    return_value(rv, Value::array(result))
}
