use crate::generics::GenericRuntimeCapabilities;
/// Minimal PHP parser — produces AST from token stream.
use crate::lexer::Token;

include!("parser/ast.rs");

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    in_class_body: bool,
    /// Whether relative return types such as `static` have an active PHP
    /// class scope. Closures inherit it; named functions do not.
    class_scope_active: bool,
    generic_scopes: Vec<Vec<GenericParameter>>,
}

include!("parser/generics.rs");
include!("parser/statements.rs");
include!("parser/expressions.rs");
include!("parser/declarations.rs");
include!("parser/helpers.rs");

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
