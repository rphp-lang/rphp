/// Minimal PHP parser — produces AST from token stream.
use crate::lexer::Token;

include!("parser/ast.rs");

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    in_class_body: bool,
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
