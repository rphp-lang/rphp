//! Compact generic declaration metadata.
//!
//! This module is intentionally always compiled. `php-generics-erased` and
//! `php-generics-reified` gate source syntax and select runtime capabilities;
//! erasure, metadata and validation remain normal RPHP machinery.
//! Nothing here is embedded in `FunctionCommon`, a call frame, an object or a
//! `Value`, so ordinary dispatch and instance layout stay unchanged.

use std::collections::HashMap;

use crate::parser::{GenericParameter, GenericVariance as AstVariance, TypeHint};
use crate::value::{Value, ValueType};

#[path = "generics/link.rs"]
mod link;
#[path = "generics/lsp.rs"]
mod lsp;
#[path = "generics/methods.rs"]
mod methods;
#[path = "generics/properties.rs"]
mod properties;
#[path = "generics/reflection.rs"]
mod reflection;
#[path = "generics/variance.rs"]
mod variance;

pub type GenericSymbol = u32;
pub(super) const METHOD_PARAMETER_FLAG: u8 = 1 << 7;

#[inline]
pub(super) const fn method_parameter(index: u8) -> u8 {
    METHOD_PARAMETER_FLAG | index
}

#[inline]
pub(crate) const fn method_parameter_index(index: u8) -> Option<usize> {
    if index & METHOD_PARAMETER_FLAG == 0 {
        None
    } else {
        Some((index & !METHOD_PARAMETER_FLAG) as usize)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenericRuntimeCapabilities {
    pub erased: bool,
    pub reified: bool,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericRuntimeMode {
    BoundErased,
    Reified,
}

impl GenericRuntimeCapabilities {
    pub const CONFIGURED: Self = Self {
        erased: cfg!(feature = "php-generics-erased"),
        reified: cfg!(feature = "php-generics-reified"),
    };

    #[inline(always)]
    pub const fn syntax_enabled(self) -> bool {
        self.erased || self.reified
    }

    #[inline(always)]
    pub const fn supports(self, mode: GenericRuntimeMode) -> bool {
        match mode {
            GenericRuntimeMode::BoundErased => self.erased,
            GenericRuntimeMode::Reified => self.reified,
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericDeclarationKind {
    Function,
    Class,
    Interface,
    Trait,
    Method,
    Closure,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericVariance {
    Invariant,
    Covariant,
    Contravariant,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericInheritanceKind {
    Extends,
    Implements,
    Uses,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericTypePosition {
    Covariant,
    Contravariant,
    Invariant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GenericType {
    Int,
    Float,
    String,
    Bool,
    Array,
    Callable,
    Null,
    Void,
    Mixed,
    Never,
    Named {
        name: GenericSymbol,
        arguments: Box<[GenericType]>,
    },
    Parameter(u8),
    Nullable(Box<GenericType>),
    Union(Box<[GenericType]>),
    Intersection(Box<[GenericType]>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericParameterMetadata {
    pub name: GenericSymbol,
    pub variance: GenericVariance,
    pub bound: Option<GenericType>,
    pub default: Option<GenericType>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericPropertyMetadata {
    pub name: GenericSymbol,
    pub value_type: GenericType,
    pub is_static: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericVarianceUse {
    pub value_type: GenericType,
    pub position: GenericTypePosition,
    pub in_static_context: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericMethodMetadata {
    pub name: GenericSymbol,
    pub parameters: Box<[GenericParameterMetadata]>,
    pub value_parameters: Box<[Option<GenericType>]>,
    pub return_type: Option<GenericType>,
    pub required_parameters: u16,
    pub is_variadic: bool,
    pub is_static: bool,
}

/// Cold runtime view of the generic boundaries that are stricter than the
/// executable method ABI. Reified receivers carry a full substitution;
/// bound-erased descendants carry only link-time-narrowed inherited slots.
/// The view lives in an executor sidecar, so no function or frame grows.
#[derive(Debug, Clone, PartialEq)]
pub struct GenericMethodContract {
    pub owner: Box<str>,
    pub method: Box<str>,
    pub value_parameters: Box<[Option<GenericType>]>,
    pub return_type: Option<GenericType>,
    pub is_variadic: bool,
    pub runtime_mode: GenericRuntimeMode,
}

impl GenericMethodContract {
    /// A direct Long method plan validates every argument representation and
    /// produces a Long or side-exits. It can therefore discharge this
    /// substituted contract without allocating a frame or sidecar entry when
    /// every occupied boundary admits Long.
    #[inline]
    pub fn admits_exact_long_call(&self, arguments: u32) -> bool {
        !self.is_variadic
            && self.value_parameters.len() == arguments as usize
            && self
                .value_parameters
                .iter()
                .all(|value| value.as_ref().is_none_or(generic_type_admits_long))
            && self
                .return_type
                .as_ref()
                .is_none_or(generic_type_admits_long)
    }
}

fn generic_type_admits_long(value: &GenericType) -> bool {
    match value {
        GenericType::Int | GenericType::Mixed => true,
        GenericType::Nullable(inner) => generic_type_admits_long(inner),
        GenericType::Union(parts) => parts.iter().any(generic_type_admits_long),
        GenericType::Intersection(parts) => parts.iter().all(generic_type_admits_long),
        GenericType::Float
        | GenericType::String
        | GenericType::Bool
        | GenericType::Array
        | GenericType::Callable
        | GenericType::Null
        | GenericType::Void
        | GenericType::Never
        | GenericType::Named { .. }
        | GenericType::Parameter(_) => false,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericDeclaration {
    pub kind: GenericDeclarationKind,
    pub owner: GenericSymbol,
    pub parameters: Box<[GenericParameterMetadata]>,
    pub value_parameters: Box<[Option<GenericType>]>,
    pub return_type: Option<GenericType>,
    pub properties: Box<[GenericPropertyMetadata]>,
    pub variance_uses: Box<[GenericVarianceUse]>,
    pub methods: Box<[GenericMethodMetadata]>,
}

#[derive(Debug, Clone)]
pub struct PendingGenericMethodMetadata {
    pub name: String,
    pub parameters: Vec<GenericParameter>,
    pub value_parameters: Vec<Option<TypeHint>>,
    pub return_type: Option<TypeHint>,
    pub required_parameters: u16,
    pub is_variadic: bool,
    pub is_static: bool,
}

#[derive(Debug, Clone)]
pub struct PendingGenericDeclaration {
    pub kind: GenericDeclarationKind,
    pub owner: String,
    pub parameters: Vec<GenericParameter>,
    pub value_parameters: Vec<Option<TypeHint>>,
    pub return_type: Option<TypeHint>,
    pub properties: Vec<(String, TypeHint, bool)>,
    pub variance_uses: Vec<(TypeHint, GenericTypePosition, bool)>,
    pub methods: Vec<PendingGenericMethodMetadata>,
}

#[derive(Debug, Clone)]
pub struct PendingGenericInheritance {
    pub kind: GenericInheritanceKind,
    pub owner: String,
    pub ancestor: String,
    pub owner_parameters: Vec<String>,
    pub arguments: Vec<TypeHint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericInheritance {
    pub kind: GenericInheritanceKind,
    pub owner: GenericSymbol,
    pub ancestor: GenericSymbol,
    pub arguments: Box<[GenericType]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericReflectionBinding {
    pub ancestor: Box<str>,
    pub arguments: Box<[GenericType]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReifiedBinding {
    pub declaration: u32,
    pub use_site: u32,
}

#[derive(Debug, Clone)]
pub struct PendingGenericUseSite {
    pub arguments: Vec<TypeHint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericUseSite {
    pub arguments: Box<[GenericType]>,
}

/// Immutable declaration table with one copy of each referenced string.
/// Owner lookup is deliberately cold and linear; explicit turbofish sites will
/// cache the resolved declaration after their first execution.
#[derive(Debug, Default)]
pub struct GenericMetadata {
    symbols: Box<[Box<str>]>,
    declarations: Box<[GenericDeclaration]>,
    inheritances: Box<[GenericInheritance]>,
    use_sites: Box<[GenericUseSite]>,
}

impl GenericMetadata {
    pub fn compile(
        pending: Vec<PendingGenericDeclaration>,
        pending_use_sites: Vec<PendingGenericUseSite>,
    ) -> Self {
        Self::compile_with_inheritance(pending, Vec::new(), pending_use_sites)
    }

    pub fn compile_with_inheritance(
        pending: Vec<PendingGenericDeclaration>,
        pending_inheritances: Vec<PendingGenericInheritance>,
        pending_use_sites: Vec<PendingGenericUseSite>,
    ) -> Self {
        let mut builder = GenericMetadataBuilder::default();
        for declaration in pending {
            builder.push(declaration);
        }
        for inheritance in pending_inheritances {
            builder.push_inheritance(inheritance);
        }
        for use_site in pending_use_sites {
            builder.push_use_site(use_site);
        }
        builder.finish()
    }

    /// Merge metadata produced by a separately compiled unit into the one
    /// executor-wide intern pool. The returned base relocates only that
    /// unit's `CheckGenericArgs` use-site operands; declaration bindings are
    /// resolved against this combined table and therefore need no opcode
    /// relocation.
    pub fn merge(&mut self, incoming: Self) -> u32 {
        let current = std::mem::take(self);
        let use_site_base = current.use_sites.len() as u32;
        if incoming.symbols.is_empty()
            && incoming.declarations.is_empty()
            && incoming.inheritances.is_empty()
            && incoming.use_sites.is_empty()
        {
            *self = current;
            return use_site_base;
        }

        let mut symbols = current.symbols.into_vec();
        let mut symbol_ids = symbols
            .iter()
            .enumerate()
            .map(|(index, symbol)| (symbol.to_string(), index as GenericSymbol))
            .collect::<HashMap<_, _>>();
        let mut symbol_relocation = Vec::with_capacity(incoming.symbols.len());
        for symbol in incoming.symbols {
            let relocated = if let Some(existing) = symbol_ids.get(symbol.as_ref()) {
                *existing
            } else {
                let index = symbols.len() as GenericSymbol;
                symbol_ids.insert(symbol.to_string(), index);
                symbols.push(symbol);
                index
            };
            symbol_relocation.push(relocated);
        }

        let mut declarations = current.declarations.into_vec();
        for mut declaration in incoming.declarations {
            declaration.owner = symbol_relocation[declaration.owner as usize];
            for parameter in &mut declaration.parameters {
                parameter.name = symbol_relocation[parameter.name as usize];
                if let Some(bound) = &mut parameter.bound {
                    remap_type_symbols(bound, &symbol_relocation);
                }
                if let Some(default) = &mut parameter.default {
                    remap_type_symbols(default, &symbol_relocation);
                }
            }
            for value_parameter in declaration.value_parameters.iter_mut().flatten() {
                remap_type_symbols(value_parameter, &symbol_relocation);
            }
            if let Some(return_type) = &mut declaration.return_type {
                remap_type_symbols(return_type, &symbol_relocation);
            }
            for property in &mut declaration.properties {
                property.name = symbol_relocation[property.name as usize];
                remap_type_symbols(&mut property.value_type, &symbol_relocation);
            }
            for variance_use in &mut declaration.variance_uses {
                remap_type_symbols(&mut variance_use.value_type, &symbol_relocation);
            }
            for method in &mut declaration.methods {
                method.name = symbol_relocation[method.name as usize];
                for parameter in &mut method.parameters {
                    parameter.name = symbol_relocation[parameter.name as usize];
                    if let Some(bound) = &mut parameter.bound {
                        remap_type_symbols(bound, &symbol_relocation);
                    }
                    if let Some(default) = &mut parameter.default {
                        remap_type_symbols(default, &symbol_relocation);
                    }
                }
                for parameter in method.value_parameters.iter_mut().flatten() {
                    remap_type_symbols(parameter, &symbol_relocation);
                }
                if let Some(return_type) = &mut method.return_type {
                    remap_type_symbols(return_type, &symbol_relocation);
                }
            }
            declarations.push(declaration);
        }

        let mut inheritances = current.inheritances.into_vec();
        for mut inheritance in incoming.inheritances {
            inheritance.owner = symbol_relocation[inheritance.owner as usize];
            inheritance.ancestor = symbol_relocation[inheritance.ancestor as usize];
            for argument in &mut inheritance.arguments {
                remap_type_symbols(argument, &symbol_relocation);
            }
            inheritances.push(inheritance);
        }

        let mut use_sites = current.use_sites.into_vec();
        for mut use_site in incoming.use_sites {
            for argument in &mut use_site.arguments {
                remap_type_symbols(argument, &symbol_relocation);
            }
            use_sites.push(use_site);
        }
        *self = Self {
            symbols: symbols.into_boxed_slice(),
            declarations: declarations.into_boxed_slice(),
            inheritances: inheritances.into_boxed_slice(),
            use_sites: use_sites.into_boxed_slice(),
        };
        use_site_base
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty() && self.inheritances.is_empty()
    }

    #[inline]
    pub fn declarations(&self) -> &[GenericDeclaration] {
        &self.declarations
    }

    #[inline]
    pub fn inheritances(&self) -> &[GenericInheritance] {
        &self.inheritances
    }

    #[inline]
    pub fn use_site(&self, index: u32) -> Option<&GenericUseSite> {
        self.use_sites.get(index as usize)
    }

    #[inline]
    pub fn symbol(&self, symbol: GenericSymbol) -> Option<&str> {
        self.symbols.get(symbol as usize).map(Box::as_ref)
    }

    pub fn find(&self, kind: GenericDeclarationKind, owner: &str) -> Option<&GenericDeclaration> {
        self.find_index(kind, owner)
            .and_then(|index| self.declarations.get(index as usize))
    }

    pub fn find_index(&self, kind: GenericDeclarationKind, owner: &str) -> Option<u32> {
        self.declarations
            .iter()
            .position(|declaration| {
                declaration.kind == kind
                    && !declaration.parameters.is_empty()
                    && self
                        .symbol(declaration.owner)
                        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(owner))
            })
            .map(|index| index as u32)
    }

    /// Validate one explicit `::<...>` use without constructing runtime values.
    /// This is the complete bound-erased runtime operation and the shared first
    /// stage of reified dispatch. Call sites cache success in their ordinary
    /// per-opline inline-cache slot.
    pub fn validate_use_site<F>(
        &self,
        kind: GenericDeclarationKind,
        owner: &str,
        use_site: u32,
        class_is_a: F,
    ) -> Result<(), String>
    where
        F: Fn(&str, &str) -> bool,
    {
        self.resolve_binding(kind, owner, use_site, class_is_a)
            .map(|_| ())
    }

    pub fn resolve_binding<F>(
        &self,
        kind: GenericDeclarationKind,
        owner: &str,
        use_site: u32,
        class_is_a: F,
    ) -> Result<ReifiedBinding, String>
    where
        F: Fn(&str, &str) -> bool,
    {
        let declaration_index = self.find_index(kind, owner).ok_or_else(|| {
            format!(
                "Cannot use generic arguments with non-generic {} {}",
                kind.label(),
                owner
            )
        })?;
        let declaration = &self.declarations[declaration_index as usize];
        let site = self
            .use_site(use_site)
            .ok_or_else(|| format!("Invalid generic use-site metadata index {}", use_site))?;
        let required = declaration
            .parameters
            .iter()
            .take_while(|parameter| parameter.default.is_none())
            .count();
        if site.arguments.len() < required || site.arguments.len() > declaration.parameters.len() {
            return Err(format!(
                "Generic {} {} expects {} to {} type arguments, {} given",
                kind.label(),
                owner,
                required,
                declaration.parameters.len(),
                site.arguments.len()
            ));
        }

        let mut effective = site.arguments.to_vec();
        for parameter in declaration.parameters.iter().skip(effective.len()) {
            effective.push(
                parameter
                    .default
                    .clone()
                    .expect("optional generic parameter must have a default"),
            );
        }
        for (index, parameter) in declaration.parameters.iter().enumerate() {
            let Some(bound) = parameter.bound.as_ref() else {
                continue;
            };
            if !self.type_satisfies(&effective[index], bound, &effective, &class_is_a) {
                let parameter_name = self.symbol(parameter.name).unwrap_or("?");
                return Err(format!(
                    "Type argument {} for {} {} does not satisfy bound of {}",
                    index + 1,
                    kind.label(),
                    owner,
                    parameter_name
                ));
            }
        }
        Ok(ReifiedBinding {
            declaration: declaration_index,
            use_site,
        })
    }

    #[inline]
    pub fn declaration(&self, binding: ReifiedBinding) -> Option<&GenericDeclaration> {
        self.declarations.get(binding.declaration as usize)
    }

    pub fn value_matches_binding<F>(
        &self,
        value: &Value,
        expected: &GenericType,
        binding: ReifiedBinding,
        class_is_a: F,
    ) -> bool
    where
        F: Fn(&str, &str) -> bool,
    {
        let Some(declaration) = self.declaration(binding) else {
            return false;
        };
        let Some(site) = self.use_site(binding.use_site) else {
            return false;
        };
        let value = if value.is_reference() {
            unsafe { &*value.as_ref_ptr() }
        } else {
            value
        };
        self.value_matches_type(value, expected, declaration, site, &class_is_a, 0)
    }

    /// Match a type after every declaration parameter has been substituted.
    /// Remaining `Parameter` nodes are malformed metadata and fail closed.
    pub fn value_matches_resolved_type<F>(
        &self,
        value: &Value,
        expected: &GenericType,
        class_is_a: F,
    ) -> bool
    where
        F: Fn(&str, &str) -> bool,
    {
        let value = if value.is_reference() {
            unsafe { &*value.as_ref_ptr() }
        } else {
            value
        };
        self.value_matches_resolved_type_inner(value, expected, &class_is_a)
    }

    fn value_matches_resolved_type_inner<F>(
        &self,
        value: &Value,
        expected: &GenericType,
        class_is_a: &F,
    ) -> bool
    where
        F: Fn(&str, &str) -> bool,
    {
        match expected {
            GenericType::Parameter(_) => false,
            GenericType::Int => value.value_type() == ValueType::Long,
            GenericType::Float => value.value_type() == ValueType::Double,
            GenericType::String => value.value_type() == ValueType::String,
            GenericType::Bool => matches!(value.value_type(), ValueType::True | ValueType::False),
            GenericType::Array => value.value_type() == ValueType::Array,
            GenericType::Callable => matches!(
                value.value_type(),
                ValueType::String | ValueType::Array | ValueType::Closure
            ),
            GenericType::Null => value.value_type() == ValueType::Null,
            GenericType::Void | GenericType::Never => false,
            GenericType::Mixed => true,
            GenericType::Named { name, .. } => {
                let Some(expected_name) = self.symbol(*name) else {
                    return false;
                };
                value.as_object().is_some_and(|object| {
                    expected_name.eq_ignore_ascii_case("object")
                        || object.class_name.eq_ignore_ascii_case(expected_name)
                        || class_is_a(&object.class_name, expected_name)
                })
            }
            GenericType::Nullable(inner) => {
                value.value_type() == ValueType::Null
                    || self.value_matches_resolved_type_inner(value, inner, class_is_a)
            }
            GenericType::Union(parts) => parts
                .iter()
                .any(|part| self.value_matches_resolved_type_inner(value, part, class_is_a)),
            GenericType::Intersection(parts) => parts
                .iter()
                .all(|part| self.value_matches_resolved_type_inner(value, part, class_is_a)),
        }
    }

    pub fn format_type(&self, declaration: &GenericDeclaration, value: &GenericType) -> String {
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
            GenericType::Parameter(index) => declaration
                .parameters
                .get(*index as usize)
                .and_then(|parameter| self.symbol(parameter.name))
                .unwrap_or("?")
                .to_string(),
            GenericType::Named { name, arguments } => {
                let mut rendered = self.symbol(*name).unwrap_or("?").to_string();
                if !arguments.is_empty() {
                    rendered.push('<');
                    for (index, argument) in arguments.iter().enumerate() {
                        if index != 0 {
                            rendered.push_str(", ");
                        }
                        rendered.push_str(&self.format_type(declaration, argument));
                    }
                    rendered.push('>');
                }
                rendered
            }
            GenericType::Nullable(inner) => {
                format!("?{}", self.format_type(declaration, inner))
            }
            GenericType::Union(parts) => parts
                .iter()
                .map(|part| self.format_type(declaration, part))
                .collect::<Vec<_>>()
                .join("|"),
            GenericType::Intersection(parts) => parts
                .iter()
                .map(|part| self.format_type(declaration, part))
                .collect::<Vec<_>>()
                .join("&"),
        }
    }

    pub fn format_binding_arguments(&self, binding: ReifiedBinding) -> Option<Vec<String>> {
        let declaration = self.declaration(binding)?;
        let use_site = self.use_site(binding.use_site)?;
        let mut rendered = Vec::with_capacity(declaration.parameters.len());
        for (index, parameter) in declaration.parameters.iter().enumerate() {
            let argument = use_site
                .arguments
                .get(index)
                .or(parameter.default.as_ref())?;
            rendered.push(self.format_type(declaration, argument));
        }
        Some(rendered)
    }

    fn value_matches_type<F>(
        &self,
        value: &Value,
        expected: &GenericType,
        declaration: &GenericDeclaration,
        site: &GenericUseSite,
        class_is_a: &F,
        depth: usize,
    ) -> bool
    where
        F: Fn(&str, &str) -> bool,
    {
        if depth > declaration.parameters.len() + 1 {
            return false;
        }
        match expected {
            GenericType::Parameter(index) => {
                let resolved = site.arguments.get(*index as usize).or_else(|| {
                    declaration
                        .parameters
                        .get(*index as usize)?
                        .default
                        .as_ref()
                });
                resolved.is_some_and(|resolved| {
                    self.value_matches_type(
                        value,
                        resolved,
                        declaration,
                        site,
                        class_is_a,
                        depth + 1,
                    )
                })
            }
            GenericType::Int => value.value_type() == ValueType::Long,
            GenericType::Float => value.value_type() == ValueType::Double,
            GenericType::String => value.value_type() == ValueType::String,
            GenericType::Bool => matches!(value.value_type(), ValueType::True | ValueType::False),
            GenericType::Array => value.value_type() == ValueType::Array,
            GenericType::Callable => matches!(
                value.value_type(),
                ValueType::String | ValueType::Array | ValueType::Closure
            ),
            GenericType::Null => value.value_type() == ValueType::Null,
            GenericType::Void | GenericType::Never => false,
            GenericType::Mixed => true,
            GenericType::Named { name, .. } => {
                let Some(expected_name) = self.symbol(*name) else {
                    return false;
                };
                value.as_object().is_some_and(|object| {
                    expected_name.eq_ignore_ascii_case("object")
                        || object.class_name.eq_ignore_ascii_case(expected_name)
                        || class_is_a(&object.class_name, expected_name)
                })
            }
            GenericType::Nullable(inner) => {
                value.value_type() == ValueType::Null
                    || self.value_matches_type(value, inner, declaration, site, class_is_a, depth)
            }
            GenericType::Union(parts) => parts.iter().any(|part| {
                self.value_matches_type(value, part, declaration, site, class_is_a, depth)
            }),
            GenericType::Intersection(parts) => parts.iter().all(|part| {
                self.value_matches_type(value, part, declaration, site, class_is_a, depth)
            }),
        }
    }

    fn value_matches_erased_type<F>(
        &self,
        value: &Value,
        expected: &GenericType,
        declaration: &GenericDeclaration,
        class_is_a: &F,
        depth: usize,
    ) -> bool
    where
        F: Fn(&str, &str) -> bool,
    {
        if depth > declaration.parameters.len() + 1 {
            return false;
        }
        match expected {
            GenericType::Parameter(index) => declaration
                .parameters
                .get(*index as usize)
                .and_then(|parameter| parameter.bound.as_ref())
                .is_none_or(|bound| {
                    self.value_matches_erased_type(value, bound, declaration, class_is_a, depth + 1)
                }),
            GenericType::Int => value.value_type() == ValueType::Long,
            GenericType::Float => value.value_type() == ValueType::Double,
            GenericType::String => value.value_type() == ValueType::String,
            GenericType::Bool => matches!(value.value_type(), ValueType::True | ValueType::False),
            GenericType::Array => value.value_type() == ValueType::Array,
            GenericType::Callable => matches!(
                value.value_type(),
                ValueType::String | ValueType::Array | ValueType::Closure
            ),
            GenericType::Null => value.value_type() == ValueType::Null,
            GenericType::Void | GenericType::Never => false,
            GenericType::Mixed => true,
            GenericType::Named { name, .. } => {
                let Some(expected_name) = self.symbol(*name) else {
                    return false;
                };
                value.as_object().is_some_and(|object| {
                    expected_name.eq_ignore_ascii_case("object")
                        || object.class_name.eq_ignore_ascii_case(expected_name)
                        || class_is_a(&object.class_name, expected_name)
                })
            }
            GenericType::Nullable(inner) => {
                value.value_type() == ValueType::Null
                    || self.value_matches_erased_type(value, inner, declaration, class_is_a, depth)
            }
            GenericType::Union(parts) => parts.iter().any(|part| {
                self.value_matches_erased_type(value, part, declaration, class_is_a, depth)
            }),
            GenericType::Intersection(parts) => parts.iter().all(|part| {
                self.value_matches_erased_type(value, part, declaration, class_is_a, depth)
            }),
        }
    }

    fn type_erases_to_mixed(
        value: &GenericType,
        declaration: &GenericDeclaration,
        depth: usize,
    ) -> bool {
        if depth > declaration.parameters.len() + 1 {
            return false;
        }
        match value {
            GenericType::Mixed => true,
            GenericType::Parameter(index) => declaration
                .parameters
                .get(*index as usize)
                .and_then(|parameter| parameter.bound.as_ref())
                .is_none_or(|bound| Self::type_erases_to_mixed(bound, declaration, depth + 1)),
            GenericType::Nullable(inner) => Self::type_erases_to_mixed(inner, declaration, depth),
            GenericType::Union(parts) => parts
                .iter()
                .any(|part| Self::type_erases_to_mixed(part, declaration, depth)),
            GenericType::Intersection(parts) => parts
                .iter()
                .all(|part| Self::type_erases_to_mixed(part, declaration, depth)),
            _ => false,
        }
    }

    fn type_satisfies<F>(
        &self,
        actual: &GenericType,
        bound: &GenericType,
        arguments: &[GenericType],
        class_is_a: &F,
    ) -> bool
    where
        F: Fn(&str, &str) -> bool,
    {
        let actual = self.resolve_parameter(actual, arguments);
        let bound = self.resolve_parameter(bound, arguments);
        if matches!(bound, GenericType::Mixed) || matches!(actual, GenericType::Never) {
            return true;
        }
        match (actual, bound) {
            (GenericType::Union(parts), bound) => parts
                .iter()
                .all(|part| self.type_satisfies(part, bound, arguments, class_is_a)),
            (actual, GenericType::Intersection(parts)) => parts
                .iter()
                .all(|part| self.type_satisfies(actual, part, arguments, class_is_a)),
            (actual, GenericType::Union(parts)) => parts
                .iter()
                .any(|part| self.type_satisfies(actual, part, arguments, class_is_a)),
            (GenericType::Intersection(parts), bound) => parts
                .iter()
                .any(|part| self.type_satisfies(part, bound, arguments, class_is_a)),
            (GenericType::Null, GenericType::Nullable(_)) => true,
            (GenericType::Nullable(actual), GenericType::Nullable(bound)) => {
                self.type_satisfies(actual, bound, arguments, class_is_a)
            }
            (actual, GenericType::Nullable(bound)) => {
                self.type_satisfies(actual, bound, arguments, class_is_a)
            }
            (
                GenericType::Named {
                    name: actual_name, ..
                },
                GenericType::Named {
                    name: bound_name, ..
                },
            ) => {
                let Some(actual_name) = self.symbol(*actual_name) else {
                    return false;
                };
                let Some(bound_name) = self.symbol(*bound_name) else {
                    return false;
                };
                bound_name.eq_ignore_ascii_case("object")
                    || actual_name.eq_ignore_ascii_case(bound_name)
                    || class_is_a(actual_name, bound_name)
            }
            (actual, bound) => std::mem::discriminant(actual) == std::mem::discriminant(bound),
        }
    }

    fn resolve_parameter<'a>(
        &self,
        value: &'a GenericType,
        arguments: &'a [GenericType],
    ) -> &'a GenericType {
        let mut current = value;
        let mut remaining = arguments.len() + 1;
        while let GenericType::Parameter(index) = current {
            let Some(next) = arguments.get(*index as usize) else {
                break;
            };
            current = next;
            remaining -= 1;
            if remaining == 0 {
                break;
            }
        }
        current
    }
}

fn remap_type_symbols(value: &mut GenericType, relocation: &[GenericSymbol]) {
    match value {
        GenericType::Named { name, arguments } => {
            *name = relocation[*name as usize];
            for argument in arguments {
                remap_type_symbols(argument, relocation);
            }
        }
        GenericType::Nullable(inner) => remap_type_symbols(inner, relocation),
        GenericType::Union(parts) | GenericType::Intersection(parts) => {
            for part in parts {
                remap_type_symbols(part, relocation);
            }
        }
        GenericType::Int
        | GenericType::Float
        | GenericType::String
        | GenericType::Bool
        | GenericType::Array
        | GenericType::Callable
        | GenericType::Null
        | GenericType::Void
        | GenericType::Mixed
        | GenericType::Never
        | GenericType::Parameter(_) => {}
    }
}

impl GenericDeclarationKind {
    #[inline]
    pub fn from_tag(tag: u16) -> Option<Self> {
        Some(match tag {
            0 => Self::Function,
            1 => Self::Class,
            2 => Self::Interface,
            3 => Self::Trait,
            4 => Self::Method,
            5 => Self::Closure,
            _ => return None,
        })
    }

    fn label(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Trait => "trait",
            Self::Method => "method",
            Self::Closure => "closure",
        }
    }
}

#[derive(Default)]
struct GenericMetadataBuilder {
    symbols: Vec<Box<str>>,
    symbol_ids: HashMap<String, GenericSymbol>,
    declarations: Vec<GenericDeclaration>,
    inheritances: Vec<GenericInheritance>,
    use_sites: Vec<GenericUseSite>,
}

impl GenericMetadataBuilder {
    fn intern(&mut self, value: &str) -> GenericSymbol {
        if let Some(symbol) = self.symbol_ids.get(value) {
            return *symbol;
        }
        let symbol = self.symbols.len() as GenericSymbol;
        let owned = value.to_string();
        self.symbols.push(owned.clone().into_boxed_str());
        self.symbol_ids.insert(owned, symbol);
        symbol
    }

    fn push(&mut self, declaration: PendingGenericDeclaration) {
        let owner = self.intern(&declaration.owner);
        let parameter_names: Vec<&str> = declaration
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        let parameters = declaration
            .parameters
            .iter()
            .map(|parameter| GenericParameterMetadata {
                name: self.intern(&parameter.name),
                variance: match parameter.variance {
                    AstVariance::Invariant => GenericVariance::Invariant,
                    AstVariance::Covariant => GenericVariance::Covariant,
                    AstVariance::Contravariant => GenericVariance::Contravariant,
                },
                bound: parameter
                    .bound
                    .as_ref()
                    .map(|hint| self.compile_type(hint, &parameter_names)),
                default: parameter
                    .default
                    .as_ref()
                    .map(|hint| self.compile_type(hint, &parameter_names)),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let value_parameters = declaration
            .value_parameters
            .iter()
            .map(|hint| {
                hint.as_ref()
                    .map(|hint| self.compile_type(hint, &parameter_names))
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let return_type = declaration
            .return_type
            .as_ref()
            .map(|hint| self.compile_type(hint, &parameter_names));
        let properties = declaration
            .properties
            .iter()
            .map(|(name, hint, is_static)| GenericPropertyMetadata {
                name: self.intern(name),
                value_type: self.compile_type(hint, &parameter_names),
                is_static: *is_static,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let variance_uses = declaration
            .variance_uses
            .iter()
            .map(|(hint, position, in_static_context)| GenericVarianceUse {
                value_type: self.compile_type(hint, &parameter_names),
                position: *position,
                in_static_context: *in_static_context,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let methods = declaration
            .methods
            .iter()
            .map(|method| {
                let method_parameter_names = method
                    .parameters
                    .iter()
                    .map(|parameter| parameter.name.as_str())
                    .collect::<Vec<_>>();
                GenericMethodMetadata {
                    name: self.intern(&method.name),
                    parameters: method
                        .parameters
                        .iter()
                        .map(|parameter| GenericParameterMetadata {
                            name: self.intern(&parameter.name),
                            variance: match parameter.variance {
                                AstVariance::Invariant => GenericVariance::Invariant,
                                AstVariance::Covariant => GenericVariance::Covariant,
                                AstVariance::Contravariant => GenericVariance::Contravariant,
                            },
                            bound: parameter.bound.as_ref().map(|hint| {
                                self.compile_method_type(
                                    hint,
                                    &parameter_names,
                                    &method_parameter_names,
                                )
                            }),
                            default: parameter.default.as_ref().map(|hint| {
                                self.compile_method_type(
                                    hint,
                                    &parameter_names,
                                    &method_parameter_names,
                                )
                            }),
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                    value_parameters: method
                        .value_parameters
                        .iter()
                        .map(|hint| {
                            hint.as_ref().map(|hint| {
                                self.compile_method_type(
                                    hint,
                                    &parameter_names,
                                    &method_parameter_names,
                                )
                            })
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                    return_type: method.return_type.as_ref().map(|hint| {
                        self.compile_method_type(hint, &parameter_names, &method_parameter_names)
                    }),
                    required_parameters: method.required_parameters,
                    is_variadic: method.is_variadic,
                    is_static: method.is_static,
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        self.declarations.push(GenericDeclaration {
            kind: declaration.kind,
            owner,
            parameters,
            value_parameters,
            return_type,
            properties,
            variance_uses,
            methods,
        });
    }

    fn push_inheritance(&mut self, inheritance: PendingGenericInheritance) {
        let owner = self.intern(&inheritance.owner);
        let ancestor = self.intern(&inheritance.ancestor);
        let parameter_names = inheritance
            .owner_parameters
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let arguments = inheritance
            .arguments
            .iter()
            .map(|argument| self.compile_type(argument, &parameter_names))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        self.inheritances.push(GenericInheritance {
            kind: inheritance.kind,
            owner,
            ancestor,
            arguments,
        });
    }

    fn push_use_site(&mut self, use_site: PendingGenericUseSite) {
        let arguments = use_site
            .arguments
            .iter()
            .map(|argument| self.compile_type(argument, &[]))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        self.use_sites.push(GenericUseSite { arguments });
    }

    fn compile_type(&mut self, hint: &TypeHint, parameters: &[&str]) -> GenericType {
        self.compile_type_in_scopes(hint, parameters, &[])
    }

    fn compile_method_type(
        &mut self,
        hint: &TypeHint,
        class_parameters: &[&str],
        method_parameters: &[&str],
    ) -> GenericType {
        self.compile_type_in_scopes(hint, class_parameters, method_parameters)
    }

    fn compile_type_in_scopes(
        &mut self,
        hint: &TypeHint,
        class_parameters: &[&str],
        method_parameters: &[&str],
    ) -> GenericType {
        match hint {
            TypeHint::Int => GenericType::Int,
            TypeHint::Float => GenericType::Float,
            TypeHint::String => GenericType::String,
            TypeHint::Bool => GenericType::Bool,
            TypeHint::Array => GenericType::Array,
            TypeHint::Callable => GenericType::Callable,
            TypeHint::Null => GenericType::Null,
            TypeHint::Void => GenericType::Void,
            TypeHint::Mixed => GenericType::Mixed,
            TypeHint::Never => GenericType::Never,
            TypeHint::ClassName(name) => method_parameters
                .iter()
                .position(|candidate| *candidate == name)
                .map(|index| GenericType::Parameter(method_parameter(index as u8)))
                .or_else(|| {
                    class_parameters
                        .iter()
                        .position(|candidate| *candidate == name)
                        .map(|index| GenericType::Parameter(index as u8))
                })
                .unwrap_or_else(|| GenericType::Named {
                    name: self.intern(name),
                    arguments: Box::new([]),
                }),
            TypeHint::GenericParameter { name, erased } => {
                // Preserve the pre-erasure identity when the parameter belongs
                // to either active scope. The parser's erased hint remains the
                // fallback for an already-lowered or external type.
                method_parameters
                    .iter()
                    .position(|candidate| *candidate == name)
                    .map(|index| GenericType::Parameter(method_parameter(index as u8)))
                    .or_else(|| {
                        class_parameters
                            .iter()
                            .position(|candidate| *candidate == name)
                            .map(|index| GenericType::Parameter(index as u8))
                    })
                    .unwrap_or_else(|| {
                        self.compile_type_in_scopes(erased, class_parameters, method_parameters)
                    })
            }
            TypeHint::GenericApplication { base, arguments } => GenericType::Named {
                name: self.intern(base),
                arguments: arguments
                    .iter()
                    .map(|argument| {
                        self.compile_type_in_scopes(argument, class_parameters, method_parameters)
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            },
            TypeHint::Nullable(inner) => GenericType::Nullable(Box::new(
                self.compile_type_in_scopes(inner, class_parameters, method_parameters),
            )),
            TypeHint::Union(parts) => GenericType::Union(
                parts
                    .iter()
                    .map(|part| {
                        self.compile_type_in_scopes(part, class_parameters, method_parameters)
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            TypeHint::Intersection(parts) => GenericType::Intersection(
                parts
                    .iter()
                    .map(|part| {
                        self.compile_type_in_scopes(part, class_parameters, method_parameters)
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
        }
    }

    fn finish(self) -> GenericMetadata {
        GenericMetadata {
            symbols: self.symbols.into_boxed_slice(),
            declarations: self.declarations.into_boxed_slice(),
            inheritances: self.inheritances.into_boxed_slice(),
            use_sites: self.use_sites.into_boxed_slice(),
        }
    }
}
