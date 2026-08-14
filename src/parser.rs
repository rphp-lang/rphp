use crate::generics::GenericRuntimeCapabilities;
/// Minimal PHP parser — produces AST from token stream.
use crate::lexer::Token;

include!("parser/ast.rs");

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Human-readable source identity used by parser diagnostics. Embedders
    /// that do not have one keep the historical context-free errors.
    source_name: Option<String>,
    in_class_body: bool,
    /// Whether relative return types such as `static` have an active PHP
    /// class scope. Closures inherit it; named functions do not.
    class_scope_active: bool,
    generic_scopes: Vec<Vec<GenericParameter>>,
    /// PHP compile-time semantic errors discovered while parsing must survive
    /// dead-branch elimination. The first one is replayed as a top-level AST
    /// marker after the full source has parsed successfully.
    deferred_compile_error: Option<(String, usize)>,
    /// Empty dimensions use a distinct diagnostic in unset() targets.
    empty_dimension_unset_context: bool,
    /// Some write/reference grammars parse the target prefix first and consume
    /// the trailing [] in their caller.
    preserve_empty_dimension_suffix: bool,
    /// Source line of the primary currently entering its postfix chain.
    last_primary_line: Option<usize>,
}

include!("parser/generics.rs");
include!("parser/statements.rs");
include!("parser/expressions.rs");
include!("parser/postfix.rs");
include!("parser/declarations.rs");
include!("parser/helpers.rs");

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
