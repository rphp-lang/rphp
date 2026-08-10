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

pub type GenericSymbol = u32;

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
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericParameterMetadata {
    pub name: GenericSymbol,
    pub variance: GenericVariance,
    pub bound: Option<GenericType>,
    pub default: Option<GenericType>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericDeclaration {
    pub kind: GenericDeclarationKind,
    pub owner: GenericSymbol,
    pub parameters: Box<[GenericParameterMetadata]>,
    pub value_parameters: Box<[Option<GenericType>]>,
    pub return_type: Option<GenericType>,
}

#[derive(Debug, Clone)]
pub struct PendingGenericDeclaration {
    pub kind: GenericDeclarationKind,
    pub owner: String,
    pub parameters: Vec<GenericParameter>,
    pub value_parameters: Vec<Option<TypeHint>>,
    pub return_type: Option<TypeHint>,
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
    use_sites: Box<[GenericUseSite]>,
}

impl GenericMetadata {
    pub fn compile(
        pending: Vec<PendingGenericDeclaration>,
        pending_use_sites: Vec<PendingGenericUseSite>,
    ) -> Self {
        let mut builder = GenericMetadataBuilder::default();
        for declaration in pending {
            builder.push(declaration);
        }
        for use_site in pending_use_sites {
            builder.push_use_site(use_site);
        }
        builder.finish()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty()
    }

    #[inline]
    pub fn declarations(&self) -> &[GenericDeclaration] {
        &self.declarations
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
        }
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
            (actual, GenericType::Union(parts)) => parts
                .iter()
                .any(|part| self.type_satisfies(actual, part, arguments, class_is_a)),
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
        if declaration.parameters.is_empty() {
            return;
        }
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
        self.declarations.push(GenericDeclaration {
            kind: declaration.kind,
            owner,
            parameters,
            value_parameters,
            return_type,
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
            TypeHint::ClassName(name) => parameters
                .iter()
                .position(|candidate| *candidate == name)
                .map(|index| GenericType::Parameter(index as u8))
                .unwrap_or_else(|| GenericType::Named {
                    name: self.intern(name),
                    arguments: Box::new([]),
                }),
            TypeHint::GenericParameter { name, erased } => {
                // A method-level parameter may use a class parameter in its
                // bound. Bound erasure intentionally records the class
                // parameter's erased runtime contract here.
                parameters
                    .iter()
                    .position(|candidate| *candidate == name)
                    .map(|index| GenericType::Parameter(index as u8))
                    .unwrap_or_else(|| self.compile_type(erased, parameters))
            }
            TypeHint::GenericApplication { base, arguments } => GenericType::Named {
                name: self.intern(base),
                arguments: arguments
                    .iter()
                    .map(|argument| self.compile_type(argument, parameters))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            },
            TypeHint::Nullable(inner) => {
                GenericType::Nullable(Box::new(self.compile_type(inner, parameters)))
            }
            TypeHint::Union(parts) => GenericType::Union(
                parts
                    .iter()
                    .map(|part| self.compile_type(part, parameters))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
        }
    }

    fn finish(self) -> GenericMetadata {
        GenericMetadata {
            symbols: self.symbols.into_boxed_slice(),
            declarations: self.declarations.into_boxed_slice(),
            use_sites: self.use_sites.into_boxed_slice(),
        }
    }
}
