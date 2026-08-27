use crate::generics::GenericRuntimeCapabilities;
/// Minimal PHP parser — produces AST from token stream.
use crate::lexer::Token;

include!("parser/ast.rs");

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Doc comments keyed by the position of the following syntax token.
    /// Keeping trivia out of `tokens` preserves every existing grammar path.
    doc_comments: Vec<(usize, std::sync::Arc<str>)>,
    /// Human-readable source identity used by parser diagnostics. Embedders
    /// that do not have one keep the historical context-free errors.
    source_name: Option<String>,
    in_class_body: bool,
    /// Whether declarations currently have a lexical PHP class scope.
    /// Bindable closures may accept relative return types without one, while
    /// named functions deliberately clear this state.
    class_scope_active: bool,
    generic_scopes: Vec<Vec<GenericParameter>>,
    /// PHP compile-time semantic errors discovered while parsing must survive
    /// dead-branch elimination. The first one is replayed as a top-level AST
    /// marker after the full source has parsed successfully.
    deferred_compile_error: Option<(String, usize)>,
    /// Syntactic deprecations must survive dead-branch elimination just like
    /// lexer-discovered source-unit diagnostics.
    deferred_compile_deprecations: Vec<(String, usize)>,
    /// `strict_types` is legal only before the first non-declare source
    /// statement. Empty statements and earlier declare directives do not end
    /// eligibility. The state is shared by nested and namespace parsers
    /// because the rule applies to the complete source unit.
    strict_types_allowed: bool,
    /// Empty dimensions use a distinct diagnostic in unset() targets.
    empty_dimension_unset_context: bool,
    /// Some write/reference grammars parse the target prefix first and consume
    /// the trailing [] in their caller.
    preserve_empty_dimension_suffix: bool,
    /// Suffix appended to invalid postfix diagnostics for an unparenthesized
    /// named `new` expression. The surrounding grammar supplies the token it
    /// is currently waiting for (for example echo's comma/semicolon or a call
    /// argument's closing parenthesis).
    new_postfix_error_suffix: Option<&'static str>,
    /// Source line of the primary currently entering its postfix chain.
    last_primary_line: Option<usize>,
    /// Whether the current statement is parsed in the source unit's outermost
    /// scope, the only context where __halt_compiler() is legal.
    outermost_scope: bool,
    /// A recognized halt directive intentionally removes every following
    /// token. Nested grammar may therefore unwind through synthetic closers
    /// while the deferred compile error remains authoritative.
    halted: bool,
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
