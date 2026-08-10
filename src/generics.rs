//! Compact generic declaration metadata.
//!
//! This module is intentionally always compiled. `php-generics-erased` and
//! `php-generics-reified` gate source syntax and select runtime capabilities;
//! erasure, metadata and validation remain normal RPHP machinery.
//! Nothing here is embedded in `FunctionCommon`, a call frame, an object or a
//! `Value`, so ordinary dispatch and instance layout stay unchanged.

use std::collections::HashMap;

use crate::parser::{GenericParameter, GenericVariance as AstVariance, TypeHint};

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
}

#[derive(Debug, Clone)]
pub struct PendingGenericDeclaration {
    pub kind: GenericDeclarationKind,
    pub owner: String,
    pub parameters: Vec<GenericParameter>,
}

/// Immutable declaration table with one copy of each referenced string.
/// Owner lookup is deliberately cold and linear; explicit turbofish sites will
/// cache the resolved declaration after their first execution.
#[derive(Debug, Default)]
pub struct GenericMetadata {
    symbols: Box<[Box<str>]>,
    declarations: Box<[GenericDeclaration]>,
}

impl GenericMetadata {
    pub fn compile(pending: Vec<PendingGenericDeclaration>) -> Self {
        let mut builder = GenericMetadataBuilder::default();
        for declaration in pending {
            builder.push(declaration);
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
    pub fn symbol(&self, symbol: GenericSymbol) -> Option<&str> {
        self.symbols.get(symbol as usize).map(Box::as_ref)
    }

    pub fn find(&self, kind: GenericDeclarationKind, owner: &str) -> Option<&GenericDeclaration> {
        self.declarations.iter().find(|declaration| {
            declaration.kind == kind
                && self
                    .symbol(declaration.owner)
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(owner))
        })
    }
}

#[derive(Default)]
struct GenericMetadataBuilder {
    symbols: Vec<Box<str>>,
    symbol_ids: HashMap<String, GenericSymbol>,
    declarations: Vec<GenericDeclaration>,
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
        self.declarations.push(GenericDeclaration {
            kind: declaration.kind,
            owner,
            parameters,
        });
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
            TypeHint::GenericParameter { erased, .. } => {
                // A method-level parameter may use a class parameter in its
                // bound. Bound erasure intentionally records the class
                // parameter's erased runtime contract here.
                self.compile_type(erased, parameters)
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
        }
    }
}
