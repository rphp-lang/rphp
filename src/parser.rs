/// Minimal PHP parser — produces AST from token stream.

use crate::lexer::Token;

/// Target in a list()/[] destructuring assignment.
#[derive(Debug, Clone, PartialEq)]
pub enum ListTarget {
    Variable(String),
    Skip,  // empty slot: list(,$b)
    Nested(Vec<ListTarget>), // nested destructuring
    KeyedVariable { key: Expr, var: String }, // explicit key: [0 => $a, 2 => $c]
}

/// A call-site argument: either positional or named (PHP 8).
#[derive(Debug, Clone, PartialEq)]
pub enum CallArg {
    Positional(Expr),
    Named { name: String, value: Expr },
}

impl CallArg {
    /// Return a reference to the underlying expression.
    pub fn expr(&self) -> &Expr {
        match self {
            CallArg::Positional(e) => e,
            CallArg::Named { value, .. } => value,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Integer(i64),
    Float(f64),
    StringLiteral(String),
    Null,
    Bool(bool),
    Variable(String),
    BinaryOp {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    FunctionCall {
        name: String,
        args: Vec<CallArg>,
    },
    PostInc(String),   // $i++
    PostDec(String),   // $i--
    PreInc(String),    // ++$i
    PreDec(String),    // --$i
    Not(Box<Expr>),    // !expr
    UnaryMinus(Box<Expr>), // -$x
    Ternary {          // cond ? then : else
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
    ArrayLiteral(Vec<ArrayElement>),  // [1, 2] or ['a' => 1]
    ArrayAccess {      // $a[0], $a['key']
        array: Box<Expr>,
        index: Box<Expr>,
    },
    Cast {             // (int)$x, (string)$x, etc.
        cast_type: CastType,
        expr: Box<Expr>,
    },
    Isset(Vec<Expr>),  // isset($a, $b)
    Empty(Box<Expr>),  // empty($a)
    NullCoalesce {     // $a ?? $b
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Elvis {            // $a ?: $b (evaluates lhs once)
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Match {            // match($x) { ... }
        expr: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    Closure {          // function($x) use($y) { ... }: ReturnType
        params: Vec<Param>,
        use_vars: Vec<String>,
        body: Vec<Stmt>,
        return_type: Option<TypeHint>,
    },
    New {              // new ClassName(args)
        class_name: String,
        args: Vec<CallArg>,
    },
    PropertyAccess {   // $obj->prop or $obj?->prop
        object: Box<Expr>,
        property: String,
        nullsafe: bool,
    },
    MethodCall {       // $obj->method(args) or $obj?->method(args)
        object: Box<Expr>,
        method: String,
        args: Vec<CallArg>,
        nullsafe: bool,
    },
    StaticCall {       // ClassName::method(args)
        class_name: String,
        method: String,
        args: Vec<CallArg>,
    },
    StaticProperty {   // ClassName::$prop
        class_name: String,
        property: String,
    },
    Throw(Box<Expr>),  // throw expr (PHP 8 expression)
    Assign {           // $var = expr (used in expressions like $a = $b ?? $c)
        var: String,
        expr: Box<Expr>,
    },
    DynamicCall {      // $var(args) — variable function call / closure call
        callable: Box<Expr>,
        args: Vec<CallArg>,
    },
    Instanceof {       // $obj instanceof ClassName
        expr: Box<Expr>,
        class_name: String,
    },
    Constant(String),  // FOO, PHP_INT_MAX — named constant reference
    Yield {            // yield $value or yield $key => $value
        value: Option<Box<Expr>>,
        key: Option<Box<Expr>>,
    },
    YieldFrom(Box<Expr>),  // yield from $expr
    Print(Box<Expr>),      // print expr (returns 1)
    BitwiseNot(Box<Expr>), // ~expr
    Clone(Box<Expr>),      // clone $expr
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CastType {
    Int = 0,
    Float = 1,
    String = 2,
    Bool = 3,
    Array = 4,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayElement {
    pub key: Option<Expr>,
    pub value: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Concat,
    Equal,
    NotEqual,
    Identical,    // ===
    NotIdentical, // !==
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,    // &&
    Or,     // ||
    Spaceship,    // <=>
    Pow,          // **
    BitwiseAnd,   // &
    BitwiseOr,    // |
    BitwiseXor,   // ^
    ShiftLeft,    // <<
    ShiftRight,   // >>
}

/// PHP type hint for function parameters.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeHint {
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
    ClassName(std::string::String),  // includes "self", "parent", "static"
    Nullable(Box<TypeHint>),         // ?int, ?string, ?ClassName, etc.
    Union(Vec<TypeHint>),            // int|string, Foo|Bar, etc.
}

/// Function parameter with optional default value.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: std::string::String,
    pub default: Option<Expr>,
    pub is_variadic: bool,
    pub is_ref: bool,
    pub type_hint: Option<TypeHint>,
    /// Constructor property promotion: Some((visibility, is_readonly))
    pub promotion: Option<(Visibility, bool)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Echo(Expr),
    Assign {
        var: String,
        expr: Expr,
    },
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    For {
        init: Vec<Stmt>,
        condition: Option<Expr>,
        update: Option<Expr>,
        body: Vec<Stmt>,
    },
    Function {
        name: String,
        params: Vec<Param>,
        body: Vec<Stmt>,
        return_type: Option<TypeHint>,
    },
    DoWhile {
        condition: Expr,
        body: Vec<Stmt>,
    },
    Break(Option<u32>),
    Continue(Option<u32>),
    Switch {
        expr: Expr,
        cases: Vec<SwitchCase>,
    },
    Return(Option<Expr>),
    ExprStmt(Expr),
    ArrayAssign {      // $a[idx] = expr
        var: String,
        index: Expr,
        expr: Expr,
    },
    ArrayPush {        // $a[] = expr
        var: String,
        expr: Expr,
    },
    Foreach {
        array: Expr,
        value_var: String,
        key_var: Option<String>,
        body: Vec<Stmt>,
    },
    Unset(Vec<Expr>),
    TryCatch {
        try_body: Vec<Stmt>,
        catches: Vec<CatchClause>,
        finally_body: Option<Vec<Stmt>>,
    },
    Throw(Expr),
    Class {
        name: String,
        parent: Option<String>,
        implements: Vec<String>,
        is_abstract: bool,
        is_final: bool,
        properties: Vec<ClassProperty>,
        methods: Vec<ClassMethod>,
        uses: Vec<String>,          // trait names from `use Foo, Bar;`
    },
    Interface {
        name: String,
        extends: Vec<String>,
        methods: Vec<ClassMethod>,  // all public, abstract (no body)
    },
    Trait {
        name: String,
        properties: Vec<ClassProperty>,
        methods: Vec<ClassMethod>,
    },
    AssignProp {       // $obj->prop = expr
        object: Expr,
        property: String,
        expr: Expr,
    },
    AssignObjArrayDim {  // $obj->prop[$key] = expr
        object: Expr,
        property: String,
        index: Expr,
        expr: Expr,
    },
    Declare {           // declare(strict_types=1);
        directive: String,
        value: i64,
    },
    Namespace {         // namespace App\Models;
        name: String,
        body: Vec<Stmt>, // if braced: namespace App { ... }, else: rest of file
    },
    UseDecl {           // use App\Models\User; or use App\Models\User as U;
        imports: Vec<(String, String)>, // (fully_qualified, alias)
    },
    Const {            // const FOO = expr;
        name: String,
        value: Expr,
    },
    ListAssign {       // list($a, $b) = expr; or [$a, $b] = expr;
        targets: Vec<ListTarget>,
        expr: Expr,
    },
    Global(Vec<String>),  // global $a, $b;
    StaticVar {           // static $a = 0, $b = "";
        vars: Vec<(String, Option<Expr>)>,
    },
    Enum {
        name: String,
        backing_type: Option<TypeHint>,
        cases: Vec<(String, Option<Expr>)>,  // (case_name, optional_value)
        methods: Vec<ClassMethod>,
    },
    Include {
        path: Expr,
        is_require: bool,
        is_once: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatchClause {
    pub types: Vec<String>,     // Exception class names (multi-catch: ExA | ExB)
    pub var: String,            // $e
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Visibility {
    Public,
    Protected,
    Private,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassProperty {
    pub visibility: Visibility,
    pub name: String,
    pub default: Option<Expr>,
    pub is_static: bool,
    pub is_readonly: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassMethod {
    pub visibility: Visibility,
    pub name: String,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub is_static: bool,
    pub is_final: bool,
    pub return_type: Option<TypeHint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    /// None = default arm
    pub conditions: Option<Vec<Expr>>,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase {
    /// None = default case
    pub value: Option<Expr>,
    pub body: Vec<Stmt>,
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    in_class_body: bool,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0, in_class_body: false }
    }

    pub fn parse(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect(&Token::OpenTag)?;
        let mut stmts = Vec::new();

        while !self.at_eof() {
            stmts.push(self.parse_stmt()?);
        }

        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        match self.peek() {
            Token::Declare => {
                self.advance(); // consume 'declare'
                self.expect(&Token::LParen)?;
                let directive = match self.advance() {
                    Token::Identifier(n) => n,
                    other => return Err(format!("Expected directive name in declare(), got {:?}", other)),
                };
                self.expect(&Token::Assign)?;
                let value = match self.advance() {
                    Token::Integer(n) => n,
                    Token::True => 1,
                    Token::False => 0,
                    other => return Err(format!("Expected integer value in declare(), got {:?}", other)),
                };
                self.expect(&Token::RParen)?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Declare { directive, value })
            }
            Token::Namespace => {
                self.advance(); // consume 'namespace'
                let name = self.parse_qualified_name()?;
                if self.peek() == Token::LBrace {
                    // Braced namespace: namespace App\Models { ... }
                    self.advance(); // consume '{'
                    let mut body = Vec::new();
                    while self.peek() != Token::RBrace && self.peek() != Token::Eof {
                        body.push(self.parse_stmt()?);
                    }
                    self.expect(&Token::RBrace)?;
                    Ok(Stmt::Namespace { name, body })
                } else {
                    // Unbraced namespace: namespace App\Models; (rest of file belongs to this namespace)
                    self.expect(&Token::Semicolon)?;
                    let mut body = Vec::new();
                    while self.peek() != Token::Eof && self.peek() != Token::Namespace {
                        body.push(self.parse_stmt()?);
                    }
                    Ok(Stmt::Namespace { name, body })
                }
            }
            Token::Use if !self.in_class_body => {
                // Top-level use declaration: use App\Models\User; or use App\Models\User as Alias;
                self.advance(); // consume 'use'
                let mut imports = Vec::new();
                loop {
                    let fqn = self.parse_qualified_name()?;
                    let alias = if self.peek() == Token::As {
                        self.advance(); // consume 'as'
                        match self.advance() {
                            Token::Identifier(n) => n,
                            other => return Err(format!("Expected alias name after 'as', got {:?}", other)),
                        }
                    } else {
                        // Default alias = last segment
                        fqn.rsplit('\\').next().unwrap_or(&fqn).to_string()
                    };
                    imports.push((fqn, alias));
                    if self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::UseDecl { imports })
            }
            Token::Const => {
                self.advance(); // consume 'const'
                let name = match self.advance() {
                    Token::Identifier(n) => n,
                    other => return Err(format!("Expected constant name after 'const', got {:?}", other)),
                };
                self.expect(&Token::Assign)?;
                let value = self.parse_expr()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Const { name, value })
            }
            Token::Echo => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Echo(expr))
            }
            Token::Include | Token::IncludeOnce | Token::Require | Token::RequireOnce => {
                let tok = self.advance();
                let (is_require, is_once) = match tok {
                    Token::Include => (false, false),
                    Token::IncludeOnce => (false, true),
                    Token::Require => (true, false),
                    Token::RequireOnce => (true, true),
                    _ => unreachable!(),
                };
                let path = self.parse_expr()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Include { path, is_require, is_once })
            }
            Token::Variable(_) => {
                // Peek ahead to determine statement type
                let next = self.tokens.get(self.pos + 1).cloned().unwrap_or(Token::Eof);
                if next == Token::LBracket {
                    // Could be $a[] = ..., $a[idx] = ..., or expression
                    // Check for $a[] = (array push)
                    let is_push = self.tokens.get(self.pos + 2) == Some(&Token::RBracket)
                        && self.tokens.get(self.pos + 3) == Some(&Token::Assign);
                    if is_push {
                        let var_name = match self.advance() {
                            Token::Variable(name) => name,
                            _ => unreachable!(),
                        };
                        self.advance(); // consume '['
                        self.advance(); // consume ']'
                        self.advance(); // consume '='
                        let expr = self.parse_expr()?;
                        self.expect(&Token::Semicolon)?;
                        return Ok(Stmt::ArrayPush { var: var_name, expr });
                    }
                    // Check for $a[idx] = by scanning ahead for ] =
                    // Simple heuristic: find matching ] then check for =
                    if self.is_array_assign() {
                        let var_name = match self.advance() {
                            Token::Variable(name) => name,
                            _ => unreachable!(),
                        };
                        self.advance(); // consume '['
                        let index = self.parse_expr()?;
                        self.expect(&Token::RBracket)?;
                        self.expect(&Token::Assign)?;
                        let expr = self.parse_expr()?;
                        self.expect(&Token::Semicolon)?;
                        return Ok(Stmt::ArrayAssign { var: var_name, index, expr });
                    }
                    // Otherwise fall through to expression parsing
                    let expr = self.parse_expr()?;
                    self.expect(&Token::Semicolon)?;
                    Ok(Stmt::ExprStmt(expr))
                } else if next == Token::Assign {
                    let var_name = match self.advance() {
                        Token::Variable(name) => name,
                        _ => unreachable!(),
                    };
                    self.expect(&Token::Assign)?;
                    let expr = self.parse_expr()?;
                    self.expect(&Token::Semicolon)?;
                    Ok(Stmt::Assign {
                        var: var_name,
                        expr,
                    })
                } else if let Some(bin_op) = Self::compound_assign_op(&next) {
                    // Compound assignment: $x += expr  →  $x = $x + expr
                    let var_name = match self.advance() {
                        Token::Variable(name) => name,
                        _ => unreachable!(),
                    };
                    self.advance(); // consume the compound operator
                    let rhs = self.parse_expr()?;
                    self.expect(&Token::Semicolon)?;
                    Ok(Stmt::Assign {
                        var: var_name.clone(),
                        expr: Expr::BinaryOp {
                            op: bin_op,
                            left: Box::new(Expr::Variable(var_name)),
                            right: Box::new(rhs),
                        },
                    })
                } else {
                    let expr = self.parse_expr()?;
                    // Check for property/array-dim assignment: $obj->prop = expr or $obj->prop[$key] = expr
                    if self.peek() == Token::Assign {
                        // Check structure without consuming
                        let is_prop_assign = matches!(&expr, Expr::PropertyAccess { .. });
                        let is_obj_dim_assign = matches!(&expr, Expr::ArrayAccess { array, .. } if matches!(array.as_ref(), Expr::PropertyAccess { .. }));

                        if is_prop_assign {
                            if let Expr::PropertyAccess { object, property, .. } = expr {
                                self.advance(); // consume '='
                                let rhs = self.parse_expr()?;
                                self.expect(&Token::Semicolon)?;
                                return Ok(Stmt::AssignProp { object: *object, property, expr: rhs });
                            }
                        } else if is_obj_dim_assign {
                            if let Expr::ArrayAccess { array, index } = expr {
                                if let Expr::PropertyAccess { object, property, .. } = *array {
                                    self.advance(); // consume '='
                                    let rhs = self.parse_expr()?;
                                    self.expect(&Token::Semicolon)?;
                                    return Ok(Stmt::AssignObjArrayDim {
                                        object: *object,
                                        property,
                                        index: *index,
                                        expr: rhs,
                                    });
                                }
                            }
                            unreachable!();
                        }
                    }
                    self.expect(&Token::Semicolon)?;
                    Ok(Stmt::ExprStmt(expr))
                }
            }
            Token::If => {
                self.parse_if()
            }
            Token::ElseIf => {
                // elseif at statement level (shouldn't happen normally, but handle gracefully)
                self.parse_if()
            }
            Token::While => {
                self.advance(); // consume 'while'
                self.expect(&Token::LParen)?;
                let condition = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                let body = self.parse_block_or_stmt()?;
                Ok(Stmt::While { condition, body })
            }
            Token::Do => {
                self.advance(); // consume 'do'
                let body = self.parse_block_or_stmt()?;
                self.expect(&Token::While)?;
                self.expect(&Token::LParen)?;
                let condition = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::DoWhile { condition, body })
            }
            Token::Break => {
                self.advance();
                let level = self.parse_break_continue_level()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Break(level))
            }
            Token::Continue => {
                self.advance();
                let level = self.parse_break_continue_level()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Continue(level))
            }
            Token::Switch => {
                self.advance(); // consume 'switch'
                self.expect(&Token::LParen)?;
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                self.expect(&Token::LBrace)?;
                let mut cases = Vec::new();
                let mut has_default = false;
                while self.peek() != Token::RBrace && !self.at_eof() {
                    match self.peek() {
                        Token::Case => {
                            self.advance();
                            let value = self.parse_expr()?;
                            self.expect(&Token::Colon)?;
                            let mut body = Vec::new();
                            while !matches!(self.peek(), Token::Case | Token::Default | Token::RBrace) && !self.at_eof() {
                                body.push(self.parse_stmt()?);
                            }
                            cases.push(SwitchCase { value: Some(value), body });
                        }
                        Token::Default => {
                            if has_default {
                                return Err("Switch statements may only contain one default clause".into());
                            }
                            has_default = true;
                            self.advance();
                            self.expect(&Token::Colon)?;
                            let mut body = Vec::new();
                            while !matches!(self.peek(), Token::Case | Token::Default | Token::RBrace) && !self.at_eof() {
                                body.push(self.parse_stmt()?);
                            }
                            cases.push(SwitchCase { value: None, body });
                        }
                        other => return Err(format!("Expected 'case' or 'default' in switch, got {:?}", other)),
                    }
                }
                self.expect(&Token::RBrace)?;
                Ok(Stmt::Switch { expr, cases })
            }
            Token::For => {
                self.advance(); // consume 'for'
                self.expect(&Token::LParen)?;

                // Init: optional assignment or expression before first ;
                let init = if self.peek() == Token::Semicolon {
                    vec![]
                } else {
                    let stmt = self.parse_for_init()?;
                    vec![stmt]
                };
                self.expect(&Token::Semicolon)?;

                // Condition: optional expression before second ;
                let condition = if self.peek() == Token::Semicolon {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.expect(&Token::Semicolon)?;

                // Update: optional expression before )
                let update = if self.peek() == Token::RParen {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.expect(&Token::RParen)?;

                let body = self.parse_block_or_stmt()?;
                Ok(Stmt::For { init, condition, update, body })
            }
            Token::Foreach => {
                self.advance(); // consume 'foreach'
                self.expect(&Token::LParen)?;
                let array = self.parse_expr()?;
                self.expect(&Token::As)?;
                // foreach ($arr as $key => $val) or foreach ($arr as $val)
                let first_var = match self.advance() {
                    Token::Variable(name) => name,
                    other => return Err(format!("Expected variable after 'as', got {:?}", other)),
                };
                let (key_var, value_var) = if self.peek() == Token::DoubleArrow {
                    self.advance(); // consume '=>'
                    let val = match self.advance() {
                        Token::Variable(name) => name,
                        other => return Err(format!("Expected variable after '=>', got {:?}", other)),
                    };
                    (Some(first_var), val)
                } else {
                    (None, first_var)
                };
                self.expect(&Token::RParen)?;
                let body = self.parse_block_or_stmt()?;
                Ok(Stmt::Foreach { array, value_var, key_var, body })
            }
            Token::Function => {
                self.advance(); // consume 'function'
                let name = match self.advance() {
                    Token::Identifier(n) => n,
                    other => return Err(format!("Expected function name, got {:?}", other)),
                };
                self.expect(&Token::LParen)?;
                let params = self.parse_param_list()?;
                self.expect(&Token::RParen)?;
                let return_type = self.parse_return_type()?;
                self.expect(&Token::LBrace)?;
                let mut body = Vec::new();
                while self.peek() != Token::RBrace && !self.at_eof() {
                    body.push(self.parse_stmt()?);
                }
                self.expect(&Token::RBrace)?;
                Ok(Stmt::Function { name, params, body, return_type })
            }
            Token::Return => {
                self.advance(); // consume 'return'
                if self.peek() == Token::Semicolon {
                    self.advance();
                    Ok(Stmt::Return(None))
                } else {
                    let expr = self.parse_expr()?;
                    self.expect(&Token::Semicolon)?;
                    Ok(Stmt::Return(Some(expr)))
                }
            }
            Token::Unset => {
                self.advance();
                self.expect(&Token::LParen)?;
                let mut targets = Vec::new();
                let expr = self.parse_expr()?;
                if !Self::is_variable_like(&expr) {
                    return Err("Cannot use unset() on the result of an expression".into());
                }
                targets.push(expr);
                while self.peek() == Token::Comma {
                    self.advance();
                    let expr = self.parse_expr()?;
                    if !Self::is_variable_like(&expr) {
                        return Err("Cannot use unset() on the result of an expression".into());
                    }
                    targets.push(expr);
                }
                self.expect(&Token::RParen)?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Unset(targets))
            }
            Token::Try => {
                self.parse_try_catch()
            }
            Token::Throw => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Throw(expr))
            }
            Token::Class | Token::Abstract | Token::Final => {
                self.parse_class()
            }
            Token::Enum => {
                self.parse_enum()
            }
            Token::Interface => {
                self.parse_interface()
            }
            Token::Trait => {
                self.parse_trait()
            }
            Token::Isset | Token::Empty | Token::Match | Token::New | Token::Yield | Token::Clone => {
                let expr = self.parse_expr()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::ExprStmt(expr))
            }
            Token::Identifier(_) | Token::Backslash => {
                // Check for list() destructuring: list($a, $b) = expr;
                if let Token::Identifier(ref name) = self.peek() {
                    if name == "list" && self.peek_at(1) == Token::LParen {
                        return self.parse_list_assign();
                    }
                }
                let expr = self.parse_expr()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::ExprStmt(expr))
            }
            Token::LBracket => {
                // Try short destructuring: [$a, $b] = expr;
                if self.is_short_list_assign() {
                    return self.parse_short_list_assign();
                }
                // Otherwise treat as expression statement (array literal)
                let expr = self.parse_expr()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::ExprStmt(expr))
            }
            Token::Global => {
                self.advance(); // consume 'global'
                let mut vars = Vec::new();
                loop {
                    match self.advance() {
                        Token::Variable(name) => vars.push(name),
                        other => return Err(format!("Expected variable after 'global', got {:?}", other)),
                    }
                    if self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Global(vars))
            }
            Token::Static if !self.in_class_body && matches!(self.peek_at(1), Token::Variable(_)) => {
                // static $var = expr; (function-level static variable)
                self.advance(); // consume 'static'
                let mut vars = Vec::new();
                loop {
                    let var_name = match self.advance() {
                        Token::Variable(name) => name,
                        other => return Err(format!("Expected variable after 'static', got {:?}", other)),
                    };
                    let default = if self.peek() == Token::Assign {
                        self.advance();
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    vars.push((var_name, default));
                    if self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::StaticVar { vars })
            }
            other => Err(format!("Unexpected token: {:?}", other)),
        }
    }

    /// Map compound assignment token to BinOp, or None.
    fn compound_assign_op(tok: &Token) -> Option<BinOp> {
        match tok {
            Token::PlusAssign => Some(BinOp::Add),
            Token::MinusAssign => Some(BinOp::Sub),
            Token::StarAssign => Some(BinOp::Mul),
            Token::StarStarAssign => Some(BinOp::Pow),
            Token::SlashAssign => Some(BinOp::Div),
            Token::PercentAssign => Some(BinOp::Mod),
            Token::DotAssign => Some(BinOp::Concat),
            Token::AmpAssign => Some(BinOp::BitwiseAnd),
            Token::PipeAssign => Some(BinOp::BitwiseOr),
            Token::CaretAssign => Some(BinOp::BitwiseXor),
            Token::ShiftLeftAssign => Some(BinOp::ShiftLeft),
            Token::ShiftRightAssign => Some(BinOp::ShiftRight),
            _ => None,
        }
    }

    /// Parse if / elseif / else chain.
    fn parse_if(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'if' or 'elseif'
        self.expect(&Token::LParen)?;
        let condition = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        let then_body = self.parse_block_or_stmt()?;
        let else_body = if self.peek() == Token::ElseIf {
            // elseif desugars to else { if (...) { ... } }
            vec![self.parse_if()?]
        } else if self.peek() == Token::Else {
            self.advance();
            // Check for "else if" (two tokens) which is equivalent to "elseif"
            if self.peek() == Token::If {
                vec![self.parse_if()?]
            } else {
                self.parse_block_or_stmt()?
            }
        } else {
            vec![]
        };
        Ok(Stmt::If {
            condition,
            then_body,
            else_body,
        })
    }

    /// Parse for-loop init: either `$var = expr`, `$var op= expr`, or an expression.
    fn parse_for_init(&mut self) -> Result<Stmt, String> {
        if let Token::Variable(_) = self.peek() {
            let next = self.tokens.get(self.pos + 1).cloned().unwrap_or(Token::Eof);
            if next == Token::Assign {
                let var_name = match self.advance() {
                    Token::Variable(name) => name,
                    _ => unreachable!(),
                };
                self.expect(&Token::Assign)?;
                let expr = self.parse_expr()?;
                return Ok(Stmt::Assign { var: var_name, expr });
            } else if let Some(bin_op) = Self::compound_assign_op(&next) {
                let var_name = match self.advance() {
                    Token::Variable(name) => name,
                    _ => unreachable!(),
                };
                self.advance(); // consume compound operator
                let rhs = self.parse_expr()?;
                return Ok(Stmt::Assign {
                    var: var_name.clone(),
                    expr: Expr::BinaryOp {
                        op: bin_op,
                        left: Box::new(Expr::Variable(var_name)),
                        right: Box::new(rhs),
                    },
                });
            }
        }
        let expr = self.parse_expr()?;
        Ok(Stmt::ExprStmt(expr))
    }

    /// Parse either { stmts } or a single stmt
    fn parse_block_or_stmt(&mut self) -> Result<Vec<Stmt>, String> {
        if self.peek() == Token::LBrace {
            self.advance(); // consume {
            let mut stmts = Vec::new();
            while self.peek() != Token::RBrace && !self.at_eof() {
                stmts.push(self.parse_stmt()?);
            }
            self.expect(&Token::RBrace)?;
            Ok(stmts)
        } else {
            // Single statement (no braces)
            let stmt = self.parse_stmt()?;
            Ok(vec![stmt])
        }
    }

    /// Parse expression: ternary ? : (lowest precedence, non-associative in PHP 8+)
    fn parse_expr(&mut self) -> Result<Expr, String> {
        // yield has the lowest precedence
        if self.peek() == Token::Yield {
            return self.parse_yield_expr();
        }

        let expr = self.parse_ternary()?;

        // Handle assignment as expression: $var = expr
        if self.peek() == Token::Assign {
            if let Expr::Variable(var) = expr {
                self.advance(); // consume '='
                let rhs = self.parse_expr()?;
                return Ok(Expr::Assign { var, expr: Box::new(rhs) });
            }
        }

        Ok(expr)
    }

    fn parse_yield_expr(&mut self) -> Result<Expr, String> {
        self.advance(); // consume 'yield'

        // yield from <expr>
        if self.peek() == Token::From {
            self.advance(); // consume 'from'
            let expr = self.parse_expr()?;
            return Ok(Expr::YieldFrom(Box::new(expr)));
        }

        // yield; or yield at end of expression context (no value)
        if matches!(self.peek(), Token::Semicolon | Token::RParen | Token::RBracket | Token::RBrace | Token::Comma | Token::Eof) {
            return Ok(Expr::Yield { value: None, key: None });
        }

        // yield <expr> or yield <key> => <value>
        let first = self.parse_ternary()?;
        if self.peek() == Token::DoubleArrow {
            self.advance(); // consume '=>'
            let value = self.parse_ternary()?;
            Ok(Expr::Yield { key: Some(Box::new(first)), value: Some(Box::new(value)) })
        } else {
            Ok(Expr::Yield { value: Some(Box::new(first)), key: None })
        }
    }

    fn parse_ternary(&mut self) -> Result<Expr, String> {
        let expr = self.parse_null_coalesce()?;

        if self.peek() == Token::Question {
            self.advance(); // consume ?

            // Elvis operator: $x ?: $y  (evaluates lhs once)
            if self.peek() == Token::Colon {
                self.advance(); // consume :
                let right = self.parse_null_coalesce()?;
                return Ok(Expr::Elvis {
                    left: Box::new(expr),
                    right: Box::new(right),
                });
            }

            let then_expr = self.parse_ternary()?;
            self.expect(&Token::Colon)?;
            let else_expr = self.parse_null_coalesce()?;

            if self.peek() == Token::Question {
                return Err("Unparenthesized `a ? b : c ? d : e` is not supported. Use explicit parentheses.".into());
            }

            Ok(Expr::Ternary {
                condition: Box::new(expr),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            })
        } else {
            Ok(expr)
        }
    }

    /// Null coalesce: ?? (right-associative)
    fn parse_null_coalesce(&mut self) -> Result<Expr, String> {
        let left = self.parse_logical_or()?;

        if self.peek() == Token::QuestionQuestion {
            self.advance();
            let right = self.parse_null_coalesce()?; // right-associative
            Ok(Expr::NullCoalesce {
                left: Box::new(left),
                right: Box::new(right),
            })
        } else {
            Ok(left)
        }
    }

    /// Logical OR: || (left-associative)
    fn parse_logical_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_logical_and()?;

        while self.peek() == Token::PipePipe {
            self.advance();
            let right = self.parse_logical_and()?;
            left = Expr::BinaryOp {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Logical AND: && (left-associative)
    fn parse_logical_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_bitwise_or()?;

        while self.peek() == Token::AmpAmp {
            self.advance();
            let right = self.parse_bitwise_or()?;
            left = Expr::BinaryOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Bitwise OR: | (left-associative)
    fn parse_bitwise_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_bitwise_xor()?;

        while self.peek() == Token::Pipe {
            self.advance();
            let right = self.parse_bitwise_xor()?;
            left = Expr::BinaryOp {
                op: BinOp::BitwiseOr,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Bitwise XOR: ^ (left-associative)
    fn parse_bitwise_xor(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_bitwise_and()?;

        while self.peek() == Token::Caret {
            self.advance();
            let right = self.parse_bitwise_and()?;
            left = Expr::BinaryOp {
                op: BinOp::BitwiseXor,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Bitwise AND: & (left-associative)
    fn parse_bitwise_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_comparison()?;

        while self.peek() == Token::Ampersand {
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::BinaryOp {
                op: BinOp::BitwiseAnd,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Comparison: ==, !=, <, <=, >, >=, <=>, instanceof
    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_concat()?;

        loop {
            // instanceof has same precedence as comparison operators
            if self.peek() == Token::Instanceof {
                self.advance();
                let class_name = if self.peek() == Token::Backslash || matches!(self.peek(), Token::Identifier(_)) {
                    self.parse_qualified_name()?
                } else {
                    return Err(format!("Expected class name after instanceof, got {:?}", self.peek()));
                };
                left = Expr::Instanceof {
                    expr: Box::new(left),
                    class_name,
                };
                continue;
            }
            let op = match self.peek() {
                Token::EqualEqual => BinOp::Equal,
                Token::NotEqual => BinOp::NotEqual,
                Token::IdenticalEqual => BinOp::Identical,
                Token::NotIdentical => BinOp::NotIdentical,
                Token::Less => BinOp::Less,
                Token::LessEqual => BinOp::LessEqual,
                Token::Greater => BinOp::Greater,
                Token::GreaterEqual => BinOp::GreaterEqual,
                Token::Spaceship => BinOp::Spaceship,
                _ => break,
            };
            self.advance();
            let right = self.parse_concat()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Concat: . (left-associative, lower than additive in PHP 8)
    fn parse_concat(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_shift()?;

        while self.peek() == Token::Dot {
            self.advance();
            let right = self.parse_shift()?;
            left = Expr::BinaryOp {
                op: BinOp::Concat,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Shift: <<, >> (left-associative)
    fn parse_shift(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_additive()?;

        loop {
            let op = match self.peek() {
                Token::ShiftLeft => BinOp::ShiftLeft,
                Token::ShiftRight => BinOp::ShiftRight,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Additive: + and - (left-associative)
    fn parse_additive(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_multiplicative()?;

        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Multiplicative: *, /, % (left-associative)
    fn parse_multiplicative(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_unary()?;

        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Unary: -expr, (int)expr, (string)expr, etc.
    fn parse_unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Token::Minus => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::UnaryMinus(Box::new(expr)))
            }
            Token::Tilde => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::BitwiseNot(Box::new(expr)))
            }
            Token::LParen => {
                // Check for type cast: (int), (string), (float), (bool), (array)
                let next = self.tokens.get(self.pos + 1).cloned().unwrap_or(Token::Eof);
                let cast_type = match &next {
                    Token::Identifier(name) => match name.as_str() {
                        "int" | "integer" => Some(CastType::Int),
                        "float" | "double" | "real" => Some(CastType::Float),
                        "string" => Some(CastType::String),
                        "bool" | "boolean" => Some(CastType::Bool),
                        _ => None,
                    },
                    Token::ArrayKw => Some(CastType::Array),
                    _ => None,
                };
                if let Some(ct) = cast_type {
                    if self.tokens.get(self.pos + 2) == Some(&Token::RParen) {
                        self.advance(); // (
                        self.advance(); // type keyword
                        self.advance(); // )
                        let expr = self.parse_unary()?;
                        return Ok(Expr::Cast { cast_type: ct, expr: Box::new(expr) });
                    }
                }
                self.parse_power()
            }
            _ => self.parse_power(),
        }
    }

    /// Power: ** (right-associative, higher precedence than unary)
    fn parse_power(&mut self) -> Result<Expr, String> {
        let base = self.parse_primary()?;

        if self.peek() == Token::StarStar {
            self.advance();
            let exp = self.parse_unary()?; // right-associative: recurse through unary
            Ok(Expr::BinaryOp {
                op: BinOp::Pow,
                left: Box::new(base),
                right: Box::new(exp),
            })
        } else {
            Ok(base)
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Token::Integer(_) => {
                let val = match self.advance() {
                    Token::Integer(n) => n,
                    _ => unreachable!(),
                };
                Ok(Expr::Integer(val))
            }
            Token::Float(_) => {
                let val = match self.advance() {
                    Token::Float(f) => f,
                    _ => unreachable!(),
                };
                Ok(Expr::Float(val))
            }
            Token::StringLiteral(_) => {
                let val = match self.advance() {
                    Token::StringLiteral(s) => s,
                    _ => unreachable!(),
                };
                Ok(Expr::StringLiteral(val))
            }
            Token::Null => {
                self.advance();
                Ok(Expr::Null)
            }
            Token::True => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Token::False => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Token::Variable(_) => {
                let name = match self.advance() {
                    Token::Variable(n) => n,
                    _ => unreachable!(),
                };
                // Check for postfix ++ / --
                if self.peek() == Token::PlusPlus {
                    self.advance();
                    return Ok(Expr::PostInc(name));
                } else if self.peek() == Token::MinusMinus {
                    self.advance();
                    return Ok(Expr::PostDec(name));
                }
                let expr = Expr::Variable(name);
                let expr = self.parse_postfix_chain(expr)?;
                Ok(expr)
            }
            Token::PlusPlus => {
                self.advance();
                let name = match self.advance() {
                    Token::Variable(n) => n,
                    other => return Err(format!("Expected variable after ++, got {:?}", other)),
                };
                Ok(Expr::PreInc(name))
            }
            Token::MinusMinus => {
                self.advance();
                let name = match self.advance() {
                    Token::Variable(n) => n,
                    other => return Err(format!("Expected variable after --, got {:?}", other)),
                };
                Ok(Expr::PreDec(name))
            }
            Token::Bang => {
                self.advance();
                let expr = self.parse_primary()?;
                Ok(Expr::Not(Box::new(expr)))
            }
            Token::Print => {
                self.advance();
                let expr = self.parse_expr()?;
                Ok(Expr::Print(Box::new(expr)))
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::Isset => {
                self.advance();
                self.expect(&Token::LParen)?;
                let mut args = Vec::new();
                let arg = self.parse_expr()?;
                if !Self::is_variable_like(&arg) {
                    return Err("Cannot use isset() on the result of an expression".into());
                }
                args.push(arg);
                while self.peek() == Token::Comma {
                    self.advance();
                    let arg = self.parse_expr()?;
                    if !Self::is_variable_like(&arg) {
                        return Err("Cannot use isset() on the result of an expression".into());
                    }
                    args.push(arg);
                }
                self.expect(&Token::RParen)?;
                Ok(Expr::Isset(args))
            }
            Token::Empty => {
                self.advance();
                self.expect(&Token::LParen)?;
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(Expr::Empty(Box::new(expr)))
            }
            Token::Backslash => {
                // Fully qualified name: \App\Models\User() or \App\Models\User::method()
                let name = self.parse_qualified_name()?;
                if self.peek() == Token::DoubleColon {
                    self.advance();
                    if let Token::Variable(_) = self.peek() {
                        let prop = match self.advance() {
                            Token::Variable(n) => n,
                            _ => unreachable!(),
                        };
                        return Ok(Expr::StaticProperty { class_name: name, property: prop });
                    }
                    let member = match self.advance() {
                        Token::Identifier(n) => n,
                        other => return Err(format!("Expected member name after ::, got {:?}", other)),
                    };
                    if self.peek() == Token::LParen {
                        self.advance();
                        let args = self.parse_call_args()?;
                        return Ok(Expr::StaticCall { class_name: name, method: member, args });
                    } else {
                        return Ok(Expr::StaticProperty { class_name: name, property: member });
                    }
                }
                if self.peek() == Token::LParen {
                    self.advance();
                    let args = self.parse_call_args()?;
                    Ok(Expr::FunctionCall { name, args })
                } else {
                    Ok(Expr::Constant(name))
                }
            }
            Token::Identifier(_) => {
                let name = if self.peek_at(1) == Token::Backslash {
                    // Qualified name: App\Models\User
                    self.parse_qualified_name()?
                } else {
                    match self.advance() {
                        Token::Identifier(n) => n,
                        _ => unreachable!(),
                    }
                };
                // Static access: ClassName::method() or ClassName::$prop
                if self.peek() == Token::DoubleColon {
                    self.advance(); // consume ::
                    if let Token::Variable(_) = self.peek() {
                        let prop = match self.advance() {
                            Token::Variable(n) => n,
                            _ => unreachable!(),
                        };
                        let expr = Expr::StaticProperty { class_name: name, property: prop };
                        return Ok(self.parse_postfix_chain(expr)?);
                    }
                    let member = match self.advance() {
                        Token::Identifier(n) => n,
                        Token::Class => "class".to_string(),
                        other => return Err(format!("Expected member name after ::, got {:?}", other)),
                    };
                    if self.peek() == Token::LParen {
                        self.advance();
                        let args = self.parse_call_args()?;
                        let expr = Expr::StaticCall { class_name: name, method: member, args };
                        return Ok(self.parse_postfix_chain(expr)?);
                    } else {
                        // Static constant/enum case access: ClassName::CONSTANT
                        let expr = Expr::StaticProperty { class_name: name, property: member };
                        return Ok(self.parse_postfix_chain(expr)?);
                    }
                }
                // Check if this is a function call (followed by `(`)
                if self.peek() == Token::LParen {
                    self.advance(); // consume (
                    let args = self.parse_call_args()?;
                    Ok(Expr::FunctionCall { name, args })
                } else {
                    // Bare identifier — constant reference (e.g., PHP_INT_MAX, FOO)
                    Ok(Expr::Constant(name))
                }
            }
            Token::Match => {
                return self.parse_match_expr();
            }
            Token::Function => {
                // Closure (anonymous function)
                return self.parse_closure();
            }
            Token::Fn => {
                // Arrow function: fn($x) => expr
                return self.parse_arrow_function();
            }
            Token::New => {
                self.advance(); // consume 'new'
                let class_name = if self.peek() == Token::Backslash || matches!(self.peek(), Token::Identifier(_)) {
                    self.parse_qualified_name()?
                } else {
                    return Err(format!("Expected class name after 'new', got {:?}", self.peek()));
                };
                let args = if self.peek() == Token::LParen {
                    self.advance(); // consume (
                    self.parse_call_args()?
                } else {
                    Vec::new()
                };
                let mut expr = Expr::New { class_name, args };
                // Handle ->method() / ->prop chains on new
                expr = self.parse_postfix_chain(expr)?;
                return Ok(expr);
            }
            Token::Throw => {
                self.advance();
                let expr = self.parse_expr()?;
                return Ok(Expr::Throw(Box::new(expr)));
            }
            Token::Clone => {
                self.advance(); // consume 'clone'
                let expr = self.parse_unary()?;
                return Ok(Expr::Clone(Box::new(expr)));
            }
            Token::LBracket => {
                // Short array syntax: [1, 2, 'a' => 3]
                self.advance(); // consume '['
                let elements = self.parse_array_elements(Token::RBracket)?;
                self.expect(&Token::RBracket)?;
                Ok(Expr::ArrayLiteral(elements))
            }
            Token::ArrayKw => {
                // Long array syntax: array(1, 2, 'a' => 3)
                self.advance(); // consume 'array'
                self.expect(&Token::LParen)?;
                let elements = self.parse_array_elements(Token::RParen)?;
                self.expect(&Token::RParen)?;
                Ok(Expr::ArrayLiteral(elements))
            }
            other => Err(format!("Expected expression, got {:?}", other)),
        }
    }

    /// Parse comma-separated array elements until `end_token`.
    fn parse_array_elements(&mut self, end_token: Token) -> Result<Vec<ArrayElement>, String> {
        let mut elements = Vec::new();
        if std::mem::discriminant(&self.peek()) == std::mem::discriminant(&end_token) {
            return Ok(elements);
        }
        loop {
            let value = self.parse_expr()?;
            if self.peek() == Token::DoubleArrow {
                // key => value
                self.advance();
                let actual_value = self.parse_expr()?;
                elements.push(ArrayElement { key: Some(value), value: actual_value });
            } else {
                elements.push(ArrayElement { key: None, value });
            }
            if self.peek() == Token::Comma {
                self.advance();
                // Allow trailing comma
                if std::mem::discriminant(&self.peek()) == std::mem::discriminant(&end_token) {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(elements)
    }

    /// Parse postfix chains: [idx], ->prop, ->method()
    fn parse_postfix_chain(&mut self, mut expr: Expr) -> Result<Expr, String> {
        loop {
            match self.peek() {
                Token::LBracket => {
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(&Token::RBracket)?;
                    expr = Expr::ArrayAccess {
                        array: Box::new(expr),
                        index: Box::new(index),
                    };
                }
                Token::LParen => {
                    // Dynamic call: $var(...), $arr[0](...), etc.
                    self.advance(); // consume '('
                    let args = self.parse_call_args()?;
                    expr = Expr::DynamicCall {
                        callable: Box::new(expr),
                        args,
                    };
                }
                Token::Arrow | Token::NullSafe => {
                    let nullsafe = matches!(self.peek(), Token::NullSafe);
                    self.advance();
                    let member = match self.advance() {
                        Token::Identifier(n) => n,
                        other => return Err(format!("Expected property/method name after {}, got {:?}", if nullsafe { "?->" } else { "->" }, other)),
                    };
                    if self.peek() == Token::LParen {
                        self.advance();
                        let args = self.parse_call_args()?;
                        expr = Expr::MethodCall {
                            object: Box::new(expr),
                            method: member,
                            args,
                            nullsafe,
                        };
                    } else {
                        expr = Expr::PropertyAccess {
                            object: Box::new(expr),
                            property: member,
                            nullsafe,
                        };
                    }
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    /// Parse try { } catch (Type $e) { } finally { }
    fn parse_try_catch(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'try'
        self.expect(&Token::LBrace)?;
        let mut try_body = Vec::new();
        while self.peek() != Token::RBrace && !self.at_eof() {
            try_body.push(self.parse_stmt()?);
        }
        self.expect(&Token::RBrace)?;

        let mut catches = Vec::new();
        while self.peek() == Token::Catch {
            self.advance(); // consume 'catch'
            self.expect(&Token::LParen)?;
            // Parse exception type(s): ExA | ExB
            let mut types = Vec::new();
            let type_name = self.parse_qualified_name()?;
            types.push(type_name);
            while self.peek() == Token::Pipe {
                self.advance();
                let t = self.parse_qualified_name()?;
                types.push(t);
            }
            let var = match self.advance() {
                Token::Variable(n) => n,
                other => return Err(format!("Expected variable in catch, got {:?}", other)),
            };
            self.expect(&Token::RParen)?;
            self.expect(&Token::LBrace)?;
            let mut body = Vec::new();
            while self.peek() != Token::RBrace && !self.at_eof() {
                body.push(self.parse_stmt()?);
            }
            self.expect(&Token::RBrace)?;
            catches.push(CatchClause { types, var, body });
        }

        let finally_body = if self.peek() == Token::Finally {
            self.advance();
            self.expect(&Token::LBrace)?;
            let mut body = Vec::new();
            while self.peek() != Token::RBrace && !self.at_eof() {
                body.push(self.parse_stmt()?);
            }
            self.expect(&Token::RBrace)?;
            Some(body)
        } else {
            None
        };

        if catches.is_empty() && finally_body.is_none() {
            return Err("Cannot use try without catch or finally".into());
        }

        Ok(Stmt::TryCatch { try_body, catches, finally_body })
    }

    /// Parse class declaration
    fn parse_class(&mut self) -> Result<Stmt, String> {
        let mut is_abstract = false;
        let mut is_final = false;
        // Consume leading modifiers (abstract, final) in any order before 'class'
        loop {
            match self.peek() {
                Token::Abstract => { self.advance(); is_abstract = true; }
                Token::Final => { self.advance(); is_final = true; }
                _ => break,
            }
        }
        if is_abstract && is_final {
            return Err("Cannot use the final modifier on an abstract class".into());
        }
        self.advance(); // consume 'class'
        let name = match self.advance() {
            Token::Identifier(n) => n,
            other => return Err(format!("Expected class name, got {:?}", other)),
        };
        let parent = if self.peek() == Token::Extends {
            self.advance();
            Some(self.parse_qualified_name()?)
        } else {
            None
        };
        let implements = if self.peek() == Token::Implements {
            self.advance();
            let mut ifaces = Vec::new();
            loop {
                ifaces.push(self.parse_qualified_name()?);
                if self.peek() == Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
            ifaces
        } else {
            Vec::new()
        };
        self.expect(&Token::LBrace)?;

        let mut properties = Vec::new();
        let mut methods = Vec::new();
        let mut uses = Vec::new();

        let prev_in_class = self.in_class_body;
        self.in_class_body = true;

        while self.peek() != Token::RBrace && !self.at_eof() {
            // Trait `use` statements: use Foo, Bar;
            if self.peek() == Token::Use {
                self.advance(); // consume 'use'
                loop {
                    uses.push(self.parse_qualified_name()?);
                    if self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&Token::Semicolon)?;
                continue;
            }

            let (vis, is_static, is_final, is_readonly) = self.parse_visibility_and_static();

            if self.peek() == Token::Function {
                // Method
                self.advance(); // consume 'function'
                let method_name = match self.advance() {
                    Token::Identifier(n) => n,
                    other => return Err(format!("Expected method name, got {:?}", other)),
                };
                self.expect(&Token::LParen)?;
                let params = self.parse_param_list()?;
                self.expect(&Token::RParen)?;
                let return_type = self.parse_return_type()?;
                self.expect(&Token::LBrace)?;
                let mut body = Vec::new();
                while self.peek() != Token::RBrace && !self.at_eof() {
                    body.push(self.parse_stmt()?);
                }
                self.expect(&Token::RBrace)?;
                methods.push(ClassMethod { visibility: vis, name: method_name, params, body, is_static, is_final, return_type });
            } else if matches!(self.peek(), Token::Variable(_)) || self.is_type_hint_start() {
                // Property — possibly with type hint: `private int $x = 0;`
                // Skip type hint if present (we don't enforce property types at runtime yet)
                let _type_hint = self.try_parse_type_hint()?;
                let prop_name = match self.advance() {
                    Token::Variable(n) => n,
                    other => return Err(format!("Expected property variable, got {:?}", other)),
                };
                let default = if self.peek() == Token::Assign {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.expect(&Token::Semicolon)?;
                properties.push(ClassProperty { visibility: vis, name: prop_name, default, is_static, is_readonly });
            } else if matches!(self.peek(), Token::Const) {
                // Class constants — not yet implemented
                return Err(format!("Unexpected token in class body: {:?}", self.peek()));
            } else {
                return Err(format!("Unexpected token in class body: {:?}", self.peek()));
            }
        }
        self.in_class_body = prev_in_class;
        self.expect(&Token::RBrace)?;

        Ok(Stmt::Class { name, parent, implements, is_abstract, is_final, properties, methods, uses })
    }

    /// Parse trait declaration
    fn parse_trait(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'trait'
        let name = match self.advance() {
            Token::Identifier(n) => n,
            other => return Err(format!("Expected trait name, got {:?}", other)),
        };
        self.expect(&Token::LBrace)?;

        let mut properties = Vec::new();
        let mut methods = Vec::new();

        while self.peek() != Token::RBrace && !self.at_eof() {
            let (vis, is_static, is_final, is_readonly) = self.parse_visibility_and_static();

            if self.peek() == Token::Function {
                self.advance();
                let method_name = match self.advance() {
                    Token::Identifier(n) => n,
                    other => return Err(format!("Expected method name, got {:?}", other)),
                };
                self.expect(&Token::LParen)?;
                let params = self.parse_param_list()?;
                self.expect(&Token::RParen)?;
                let return_type = self.parse_return_type()?;
                self.expect(&Token::LBrace)?;
                let mut body = Vec::new();
                while self.peek() != Token::RBrace && !self.at_eof() {
                    body.push(self.parse_stmt()?);
                }
                self.expect(&Token::RBrace)?;
                methods.push(ClassMethod { visibility: vis, name: method_name, params, body, is_static, is_final, return_type });
            } else if matches!(self.peek(), Token::Variable(_)) || self.is_type_hint_start() {
                // Property — possibly with type hint
                let _type_hint = self.try_parse_type_hint()?;
                let prop_name = match self.advance() {
                    Token::Variable(n) => n,
                    other => return Err(format!("Expected property variable, got {:?}", other)),
                };
                let default = if self.peek() == Token::Assign {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.expect(&Token::Semicolon)?;
                properties.push(ClassProperty { visibility: vis, name: prop_name, default, is_static, is_readonly });
            } else {
                return Err(format!("Unexpected token in trait body: {:?}", self.peek()));
            }
        }
        self.expect(&Token::RBrace)?;

        Ok(Stmt::Trait { name, properties, methods })
    }

    /// Parse interface declaration
    fn parse_interface(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'interface'
        let name = match self.advance() {
            Token::Identifier(n) => n,
            other => return Err(format!("Expected interface name, got {:?}", other)),
        };
        // interface Foo extends Bar, Baz { ... }
        let extends = if self.peek() == Token::Extends {
            self.advance();
            let mut parents = Vec::new();
            loop {
                parents.push(self.parse_qualified_name()?);
                if self.peek() == Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
            parents
        } else {
            Vec::new()
        };
        self.expect(&Token::LBrace)?;

        let mut methods = Vec::new();
        while self.peek() != Token::RBrace && !self.at_eof() {
            let (vis, is_static, _is_final, _is_readonly) = self.parse_visibility_and_static();
            if self.peek() == Token::Function {
                self.advance(); // consume 'function'
                let method_name = match self.advance() {
                    Token::Identifier(n) => n,
                    other => return Err(format!("Expected method name, got {:?}", other)),
                };
                // Interface methods must be public (PHP rule)
                if vis != Visibility::Public {
                    let vis_str = match vis { Visibility::Protected => "protected", Visibility::Private => "private", _ => "public" };
                    return Err(format!(
                        "Access type for interface method {}::{}() must be public (got {})",
                        name, method_name, vis_str
                    ));
                }
                self.expect(&Token::LParen)?;
                let params = self.parse_param_list()?;
                self.expect(&Token::RParen)?;
                let return_type = self.parse_return_type()?;
                self.expect(&Token::Semicolon)?; // interface methods end with ;
                methods.push(ClassMethod { visibility: vis, name: method_name, params, body: vec![], is_static, is_final: false, return_type });
            } else {
                return Err(format!("Unexpected token in interface body: {:?}", self.peek()));
            }
        }
        self.expect(&Token::RBrace)?;

        Ok(Stmt::Interface { name, extends, methods })
    }

    /// Parse enum declaration
    fn parse_enum(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'enum'
        let name = match self.advance() {
            Token::Identifier(n) => n,
            other => return Err(format!("Expected enum name, got {:?}", other)),
        };
        // Optional backing type: enum Foo: string { ... }
        let backing_type = if self.peek() == Token::Colon {
            self.advance(); // consume ':'
            Some(self.parse_base_type_hint()?)
        } else {
            None
        };
        self.expect(&Token::LBrace)?;

        let mut cases = Vec::new();
        let mut methods = Vec::new();

        let prev_in_class = self.in_class_body;
        self.in_class_body = true;

        while self.peek() != Token::RBrace && !self.at_eof() {
            if self.peek() == Token::Case {
                self.advance(); // consume 'case'
                let case_name = match self.advance() {
                    Token::Identifier(n) => n,
                    other => return Err(format!("Expected enum case name, got {:?}", other)),
                };
                let value = if self.peek() == Token::Assign {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.expect(&Token::Semicolon)?;
                cases.push((case_name, value));
            } else {
                // Method in enum
                let (vis, is_static, is_final, _is_readonly) = self.parse_visibility_and_static();
                if self.peek() == Token::Function {
                    self.advance();
                    let method_name = match self.advance() {
                        Token::Identifier(n) => n,
                        other => return Err(format!("Expected method name, got {:?}", other)),
                    };
                    self.expect(&Token::LParen)?;
                    let params = self.parse_param_list()?;
                    self.expect(&Token::RParen)?;
                    let return_type = self.parse_return_type()?;
                    self.expect(&Token::LBrace)?;
                    let mut body = Vec::new();
                    while self.peek() != Token::RBrace && !self.at_eof() {
                        body.push(self.parse_stmt()?);
                    }
                    self.expect(&Token::RBrace)?;
                    methods.push(ClassMethod { visibility: vis, name: method_name, params, body, is_static, is_final, return_type });
                } else {
                    return Err(format!("Unexpected token in enum body: {:?}", self.peek()));
                }
            }
        }
        self.in_class_body = prev_in_class;
        self.expect(&Token::RBrace)?;

        Ok(Stmt::Enum { name, backing_type, cases, methods })
    }

    fn parse_visibility_and_static(&mut self) -> (Visibility, bool, bool, bool) {
        let mut vis = Visibility::Public;
        let mut is_static = false;
        let mut is_final = false;
        let mut is_readonly = false;

        loop {
            match self.peek() {
                Token::Public => { self.advance(); vis = Visibility::Public; }
                Token::Protected => { self.advance(); vis = Visibility::Protected; }
                Token::Private => { self.advance(); vis = Visibility::Private; }
                Token::Static => { self.advance(); is_static = true; }
                Token::Final => { self.advance(); is_final = true; }
                Token::Abstract => { self.advance(); /* absorbed for abstract methods */ }
                Token::Identifier(ref s) if s == "readonly" => { self.advance(); is_readonly = true; }
                _ => break,
            }
        }
        (vis, is_static, is_final, is_readonly)
    }

    /// Parse match expression
    fn parse_match_expr(&mut self) -> Result<Expr, String> {
        self.advance(); // consume 'match'
        self.expect(&Token::LParen)?;
        let expr = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        self.expect(&Token::LBrace)?;

        let mut arms = Vec::new();
        while self.peek() != Token::RBrace && !self.at_eof() {
            if self.peek() == Token::Default {
                self.advance();
                self.expect(&Token::DoubleArrow)?;
                let body = self.parse_expr()?;
                arms.push(MatchArm { conditions: None, body });
            } else {
                // One or more comma-separated conditions
                let mut conditions = Vec::new();
                conditions.push(self.parse_expr()?);
                while self.peek() == Token::Comma {
                    // Peek ahead: if next is => or }, this comma terminates the arm
                    let next = self.tokens.get(self.pos + 1).cloned().unwrap_or(Token::Eof);
                    if next == Token::DoubleArrow || next == Token::RBrace {
                        break;
                    }
                    self.advance(); // consume comma
                    conditions.push(self.parse_expr()?);
                }
                self.expect(&Token::DoubleArrow)?;
                let body = self.parse_expr()?;
                arms.push(MatchArm { conditions: Some(conditions), body });
            }
            // Optional trailing comma between arms
            if self.peek() == Token::Comma {
                self.advance();
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(Expr::Match { expr: Box::new(expr), arms })
    }

    /// Parse arrow function: fn($x, $y) => expr
    /// Desugars to Closure with auto-captured use vars and body = [Return(expr)]
    fn parse_arrow_function(&mut self) -> Result<Expr, String> {
        self.advance(); // consume 'fn'
        self.expect(&Token::LParen)?;
        let params = self.parse_param_list()?;
        self.expect(&Token::RParen)?;
        let return_type = self.parse_return_type()?;
        self.expect(&Token::DoubleArrow)?;
        let expr = self.parse_expr()?;

        // Auto-capture: collect free variables from expr that aren't params
        let param_names: std::collections::HashSet<&str> = params.iter().map(|p| p.name.as_str()).collect();
        let mut free_vars = Vec::new();
        Self::collect_free_vars(&expr, &param_names, &mut free_vars);

        let body = vec![Stmt::Return(Some(expr))];
        Ok(Expr::Closure { params, use_vars: free_vars, body, return_type })
    }

    /// Collect variable names referenced in an expression that are not in `bound`.
    fn collect_free_vars(expr: &Expr, bound: &std::collections::HashSet<&str>, out: &mut Vec<String>) {
        match expr {
            Expr::Variable(name) => {
                if !bound.contains(name.as_str()) && !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Expr::BinaryOp { left, right, .. } => {
                Self::collect_free_vars(left, bound, out);
                Self::collect_free_vars(right, bound, out);
            }
            Expr::UnaryMinus(inner) | Expr::Not(inner) | Expr::Throw(inner) | Expr::Empty(inner) | Expr::Print(inner) | Expr::BitwiseNot(inner) => {
                Self::collect_free_vars(inner, bound, out);
            }
            Expr::Assign { var, expr: inner } => {
                if !bound.contains(var.as_str()) && !out.contains(var) {
                    out.push(var.clone());
                }
                Self::collect_free_vars(inner, bound, out);
            }
            Expr::FunctionCall { args, .. } | Expr::StaticCall { args, .. } => {
                for arg in args {
                    Self::collect_free_vars(arg.expr(), bound, out);
                }
            }
            Expr::DynamicCall { callable, args } => {
                Self::collect_free_vars(callable, bound, out);
                for arg in args {
                    Self::collect_free_vars(arg.expr(), bound, out);
                }
            }
            Expr::Isset(exprs) => {
                for e in exprs {
                    Self::collect_free_vars(e, bound, out);
                }
            }
            Expr::PostInc(name) | Expr::PostDec(name) | Expr::PreInc(name) | Expr::PreDec(name) => {
                if !bound.contains(name.as_str()) && !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Expr::Ternary { condition, then_expr, else_expr } => {
                Self::collect_free_vars(condition, bound, out);
                Self::collect_free_vars(then_expr, bound, out);
                Self::collect_free_vars(else_expr, bound, out);
            }
            Expr::NullCoalesce { left, right } | Expr::Elvis { left, right } => {
                Self::collect_free_vars(left, bound, out);
                Self::collect_free_vars(right, bound, out);
            }
            Expr::ArrayLiteral(elements) => {
                for elem in elements {
                    if let Some(k) = &elem.key {
                        Self::collect_free_vars(k, bound, out);
                    }
                    Self::collect_free_vars(&elem.value, bound, out);
                }
            }
            Expr::ArrayAccess { array, index } => {
                Self::collect_free_vars(array, bound, out);
                Self::collect_free_vars(index, bound, out);
            }
            Expr::PropertyAccess { object, .. } => {
                Self::collect_free_vars(object, bound, out);
            }
            Expr::MethodCall { object, args, .. } => {
                Self::collect_free_vars(object, bound, out);
                for arg in args {
                    Self::collect_free_vars(arg.expr(), bound, out);
                }
            }
            Expr::Closure { use_vars, .. } => {
                // Nested closure — only capture its explicit use vars
                for v in use_vars {
                    if !bound.contains(v.as_str()) && !out.contains(v) {
                        out.push(v.clone());
                    }
                }
            }
            Expr::Cast { expr: inner, .. } => {
                Self::collect_free_vars(inner, bound, out);
            }
            Expr::Instanceof { expr: inner, .. } => {
                Self::collect_free_vars(inner, bound, out);
            }
            Expr::New { args, .. } => {
                for arg in args {
                    Self::collect_free_vars(arg.expr(), bound, out);
                }
            }
            Expr::Match { expr: inner, arms } => {
                Self::collect_free_vars(inner, bound, out);
                for arm in arms {
                    if let Some(conds) = &arm.conditions {
                        for cond in conds {
                            Self::collect_free_vars(cond, bound, out);
                        }
                    }
                    Self::collect_free_vars(&arm.body, bound, out);
                }
            }
            Expr::StaticProperty { .. } => {}
            // Literals and constants — no variables
            Expr::Integer(_) | Expr::Float(_) | Expr::StringLiteral(_)
            | Expr::Bool(_) | Expr::Null | Expr::Constant(_) => {}
            // Yield — collect vars from value/key expressions
            Expr::Yield { value, key } => {
                if let Some(v) = value { Self::collect_free_vars(v, bound, out); }
                if let Some(k) = key { Self::collect_free_vars(k, bound, out); }
            }
            Expr::YieldFrom(sub) => {
                Self::collect_free_vars(sub, bound, out);
            }
            Expr::Clone(inner) => {
                Self::collect_free_vars(inner, bound, out);
            }
        }
    }

    /// Parse closure: function($a, $b) use($c) { ... }
    fn parse_closure(&mut self) -> Result<Expr, String> {
        self.advance(); // consume 'function'
        self.expect(&Token::LParen)?;
        let params = self.parse_param_list()?;
        self.expect(&Token::RParen)?;

        let mut use_vars = Vec::new();
        if self.peek() == Token::Use {
            self.advance();
            self.expect(&Token::LParen)?;
            let v = match self.advance() {
                Token::Variable(n) => n,
                other => return Err(format!("Expected variable in use, got {:?}", other)),
            };
            use_vars.push(v);
            while self.peek() == Token::Comma {
                self.advance();
                let v = match self.advance() {
                    Token::Variable(n) => n,
                    other => return Err(format!("Expected variable in use, got {:?}", other)),
                };
                use_vars.push(v);
            }
            self.expect(&Token::RParen)?;
        }

        let return_type = self.parse_return_type()?;

        self.expect(&Token::LBrace)?;
        let mut body = Vec::new();
        while self.peek() != Token::RBrace && !self.at_eof() {
            body.push(self.parse_stmt()?);
        }
        self.expect(&Token::RBrace)?;

        Ok(Expr::Closure { params, use_vars, body, return_type })
    }

    fn peek(&self) -> Token {
        self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof)
    }

    fn peek_at(&self, offset: usize) -> Token {
        self.tokens.get(self.pos + offset).cloned().unwrap_or(Token::Eof)
    }

    /// Parse comma-separated call arguments supporting both positional and
    /// named (PHP 8 `name: expr`) arguments.  The opening `(` must already
    /// be consumed; this method consumes everything up to and including the
    /// closing `)`.
    /// Try to extract a string name from the current token if it can serve as
    /// a named argument label. Returns Some(name) for Identifier and keyword
    /// tokens that PHP accepts as named arg labels (array, string, int, etc.).
    /// Any token that can serve as a named argument label.
    /// PHP allows all reserved words as named arg labels.
    fn token_as_named_arg_label(tok: &Token) -> Option<String> {
        match tok {
            Token::Identifier(n) => Some(n.clone()),
            // All keyword tokens — PHP accepts any reserved word as a named arg label
            Token::ArrayKw => Some("array".to_string()),
            Token::Null => Some("null".to_string()),
            Token::True => Some("true".to_string()),
            Token::False => Some("false".to_string()),
            Token::Match => Some("match".to_string()),
            Token::Static => Some("static".to_string()),
            Token::Function => Some("function".to_string()),
            Token::Class => Some("class".to_string()),
            Token::New => Some("new".to_string()),
            Token::Return => Some("return".to_string()),
            Token::Echo => Some("echo".to_string()),
            Token::If => Some("if".to_string()),
            Token::Else => Some("else".to_string()),
            Token::ElseIf => Some("elseif".to_string()),
            Token::While => Some("while".to_string()),
            Token::Do => Some("do".to_string()),
            Token::For => Some("for".to_string()),
            Token::Foreach => Some("foreach".to_string()),
            Token::As => Some("as".to_string()),
            Token::Switch => Some("switch".to_string()),
            Token::Case => Some("case".to_string()),
            Token::Default => Some("default".to_string()),
            Token::Break => Some("break".to_string()),
            Token::Continue => Some("continue".to_string()),
            Token::Try => Some("try".to_string()),
            Token::Catch => Some("catch".to_string()),
            Token::Finally => Some("finally".to_string()),
            Token::Throw => Some("throw".to_string()),
            Token::Instanceof => Some("instanceof".to_string()),
            Token::Abstract => Some("abstract".to_string()),
            Token::Interface => Some("interface".to_string()),
            Token::Implements => Some("implements".to_string()),
            Token::Extends => Some("extends".to_string()),
            Token::Public => Some("public".to_string()),
            Token::Protected => Some("protected".to_string()),
            Token::Private => Some("private".to_string()),
            Token::Const => Some("const".to_string()),
            Token::Isset => Some("isset".to_string()),
            Token::Empty => Some("empty".to_string()),
            Token::Unset => Some("unset".to_string()),
            Token::Fn => Some("fn".to_string()),
            Token::Use => Some("use".to_string()),
            Token::Declare => Some("declare".to_string()),
            Token::Trait => Some("trait".to_string()),
            Token::Final => Some("final".to_string()),
            Token::Enum => Some("enum".to_string()),
            Token::Namespace => Some("namespace".to_string()),
            Token::Yield => Some("yield".to_string()),
            Token::From => Some("from".to_string()),
            Token::Global => Some("global".to_string()),
            Token::Print => Some("print".to_string()),
            Token::Clone => Some("clone".to_string()),
            _ => None,
        }
    }

    fn parse_call_args(&mut self) -> Result<Vec<CallArg>, String> {
        let mut args: Vec<CallArg> = Vec::new();
        let mut seen_named = false;
        if self.peek() != Token::RParen {
            loop {
                // Check for named argument: identifier-like token followed by Colon
                if let Some(_label) = Self::token_as_named_arg_label(&self.peek()) {
                    if self.peek_at(1) == Token::Colon {
                        let name = Self::token_as_named_arg_label(&self.advance()).unwrap();
                        self.advance(); // consume ':'
                        let value = self.parse_expr()?;
                        args.push(CallArg::Named { name, value });
                        seen_named = true;
                        if self.peek() == Token::Comma {
                            self.advance();
                            continue;
                        } else {
                            break;
                        }
                    }
                }
                // Positional argument
                if seen_named {
                    return Err("Cannot use positional argument after named argument".to_string());
                }
                let expr = self.parse_expr()?;
                args.push(CallArg::Positional(expr));
                if self.peek() == Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(&Token::RParen)?;
        Ok(args)
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek();
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<(), String> {
        let tok = self.advance();
        if std::mem::discriminant(&tok) == std::mem::discriminant(expected) {
            Ok(())
        } else {
            Err(format!("Expected {:?}, got {:?}", expected, tok))
        }
    }

    fn at_eof(&self) -> bool {
        self.peek() == Token::Eof
    }

    /// Parse a comma-separated list of function parameters with optional defaults.
    /// Expects the opening '(' to already be consumed; stops before ')'.
    fn parse_param_list(&mut self) -> Result<Vec<Param>, String> {
        let mut params = Vec::new();
        if self.is_param_start() {
            params.push(self.parse_one_param()?);
            while self.peek() == Token::Comma {
                self.advance();
                // Allow trailing comma before closing paren
                if !self.is_param_start() {
                    break;
                }
                params.push(self.parse_one_param()?);
            }
        }
        Ok(params)
    }

    /// Check if the current token can start a parameter declaration.
    /// Matches: type hints (identifiers, ?, array, null), &, ..., $var
    fn is_param_start(&self) -> bool {
        matches!(self.peek(),
            Token::Variable(_) | Token::DotDotDot | Token::Ampersand
            | Token::Question | Token::ArrayKw | Token::Null
            | Token::Identifier(_)
            | Token::Public | Token::Protected | Token::Private
        )
    }

    /// Parse an optional return type hint after `)`.
    /// If the next token is `:`, consume it and parse a type hint.
    /// Parse a qualified name like `App\Models\User` or `\App\Models\User`.
    /// Consumes Identifier (Backslash Identifier)* tokens.
    /// May start with a leading backslash for fully qualified names.
    fn parse_qualified_name(&mut self) -> Result<String, String> {
        let mut parts = Vec::new();
        // Optional leading backslash (fully qualified)
        let leading_bs = if self.peek() == Token::Backslash {
            self.advance();
            true
        } else {
            false
        };
        match self.advance() {
            Token::Identifier(n) => parts.push(n),
            other => return Err(format!("Expected identifier in qualified name, got {:?}", other)),
        }
        while self.peek() == Token::Backslash {
            self.advance(); // consume '\'
            match self.advance() {
                Token::Identifier(n) => parts.push(n),
                other => return Err(format!("Expected identifier after '\\' in qualified name, got {:?}", other)),
            }
        }
        let name = parts.join("\\");
        if leading_bs {
            Ok(format!("\\{}", name))
        } else {
            Ok(name)
        }
    }

    fn parse_return_type(&mut self) -> Result<Option<TypeHint>, String> {
        if self.peek() == Token::Colon {
            self.advance(); // consume ':'
            // Handle nullable return types: ?: type
            if self.peek() == Token::Question {
                self.advance(); // consume '?'
                let inner = self.parse_base_type_hint()?;
                return Ok(Some(TypeHint::Nullable(Box::new(inner))));
            }
            let hint = self.parse_base_type_hint()?;
            let hint = self.maybe_parse_union_type(hint)?;
            Ok(Some(hint))
        } else {
            Ok(None)
        }
    }

    /// If the next token is `|`, parse remaining types to form a union type.
    fn maybe_parse_union_type(&mut self, first: TypeHint) -> Result<TypeHint, String> {
        if self.peek() != Token::Pipe {
            return Ok(first);
        }
        let mut types = vec![first];
        while self.peek() == Token::Pipe {
            self.advance(); // consume '|'
            let t = self.parse_base_type_hint()?;
            types.push(t);
        }
        Ok(TypeHint::Union(types))
    }

    /// Check if the current token could be the start of a type hint (without consuming tokens).
    fn is_type_hint_start(&self) -> bool {
        match self.peek() {
            Token::Question => {
                matches!(self.tokens.get(self.pos + 1),
                    Some(Token::Identifier(_)) | Some(Token::ArrayKw) | Some(Token::Null))
            }
            Token::Identifier(_) | Token::ArrayKw | Token::Null => {
                matches!(self.tokens.get(self.pos + 1),
                    Some(Token::Variable(_)) | Some(Token::Pipe))
            }
            _ => false,
        }
    }

    /// Try to parse a type hint at the start of a parameter.
    /// Returns None if no type hint is present (next token is $var, &, or ...).
    fn try_parse_type_hint(&mut self) -> Result<Option<TypeHint>, String> {
        // Nullable: ?type
        if self.peek() == Token::Question {
            // Peek ahead: ?$var or ?... means ternary/other, not type hint
            // In param context, ?Identifier or ?ArrayKw means nullable type
            let next = self.tokens.get(self.pos + 1);
            let is_type = matches!(next,
                Some(Token::Identifier(_)) | Some(Token::ArrayKw) | Some(Token::Null)
            );
            if is_type {
                self.advance(); // consume '?'
                let inner = self.parse_base_type_hint()?;
                return Ok(Some(TypeHint::Nullable(Box::new(inner))));
            }
            return Ok(None);
        }
        // Check if current token looks like a type hint
        // Disambiguate: Identifier followed by $var, &, or ... means it's a type hint
        // Identifier NOT followed by those means it's not a type hint (shouldn't happen in param context)
        match self.peek() {
            Token::Identifier(_) => {
                let next = self.tokens.get(self.pos + 1);
                let is_type_context = matches!(next,
                    Some(Token::Variable(_)) | Some(Token::Ampersand) | Some(Token::DotDotDot)
                    | Some(Token::Pipe)
                );
                if is_type_context {
                    let hint = self.parse_base_type_hint()?;
                    let hint = self.maybe_parse_union_type(hint)?;
                    return Ok(Some(hint));
                }
                Ok(None)
            }
            Token::ArrayKw => {
                let next = self.tokens.get(self.pos + 1);
                let is_type_context = matches!(next,
                    Some(Token::Variable(_)) | Some(Token::Ampersand) | Some(Token::DotDotDot)
                    | Some(Token::Pipe)
                );
                if is_type_context {
                    self.advance(); // consume 'array'
                    let hint = self.maybe_parse_union_type(TypeHint::Array)?;
                    return Ok(Some(hint));
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// Parse a non-nullable type hint (int, string, float, bool, array, ClassName).
    fn parse_base_type_hint(&mut self) -> Result<TypeHint, String> {
        match self.advance() {
            Token::Identifier(name) => {
                match name.as_str() {
                    "int" | "integer" => Ok(TypeHint::Int),
                    "float" | "double" => Ok(TypeHint::Float),
                    "string" => Ok(TypeHint::String),
                    "bool" | "boolean" => Ok(TypeHint::Bool),
                    "callable" => Ok(TypeHint::Callable),
                    "null" => Ok(TypeHint::Null),
                    "void" => Ok(TypeHint::Void),
                    "mixed" => Ok(TypeHint::Mixed),
                    "never" => Ok(TypeHint::Never),
                    _ => Ok(TypeHint::ClassName(name)),
                }
            }
            Token::ArrayKw => Ok(TypeHint::Array),
            Token::Null => Ok(TypeHint::Null),
            other => Err(format!("Expected type hint, got {:?}", other)),
        }
    }

    fn parse_one_param(&mut self) -> Result<Param, String> {
        // Check for constructor property promotion: visibility keyword before type hint
        let mut promotion: Option<(Visibility, bool)> = None;
        let mut promo_readonly = false;
        match self.peek() {
            Token::Public | Token::Protected | Token::Private => {
                let vis = match self.advance() {
                    Token::Public => Visibility::Public,
                    Token::Protected => Visibility::Protected,
                    Token::Private => Visibility::Private,
                    _ => unreachable!(),
                };
                // Check for 'readonly' after visibility
                if matches!(self.peek(), Token::Identifier(ref s) if s == "readonly") {
                    self.advance();
                    promo_readonly = true;
                }
                promotion = Some((vis, promo_readonly));
            }
            _ => {}
        }
        // Optional type hint before &, ..., $var
        let type_hint = self.try_parse_type_hint()?;
        // Optional & prefix for pass-by-reference
        let is_ref = if self.peek() == Token::Ampersand {
            self.advance(); // consume '&'
            true
        } else {
            false
        };
        let is_variadic = if self.peek() == Token::DotDotDot {
            self.advance(); // consume '...'
            true
        } else {
            false
        };
        let name = match self.advance() {
            Token::Variable(n) => n,
            other => return Err(format!("Expected parameter variable, got {:?}", other)),
        };
        let default = if self.peek() == Token::Assign {
            if is_variadic {
                return Err(format!("Variadic parameter ${} cannot have a default value", name));
            }
            self.advance(); // consume '='
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Param { name, default, is_variadic, is_ref, type_hint, promotion })
    }

    /// Check if an expression is a variable-like target (valid for isset/empty/unset).
    fn is_variable_like(expr: &Expr) -> bool {
        matches!(expr, Expr::Variable(_) | Expr::ArrayAccess { .. })
    }

    /// Check if current $var[...] is an array assignment ($var[idx] =).
    /// Scans ahead from pos+2 (inside brackets) to find matching ] then =.
    fn is_array_assign(&self) -> bool {
        let mut i = self.pos + 2; // skip $var and [
        let mut depth = 1;
        while i < self.tokens.len() && depth > 0 {
            match &self.tokens[i] {
                Token::LBracket | Token::LParen => depth += 1,
                Token::RBracket => {
                    depth -= 1;
                    if depth == 0 {
                        return self.tokens.get(i + 1) == Some(&Token::Assign);
                    }
                }
                Token::RParen => {
                    if depth > 1 { depth -= 1; }
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// Check if `[` at current position starts a short list destructuring pattern.
    /// Scans ahead for pattern: `[` (vars/commas) `]` `=`
    fn is_short_list_assign(&self) -> bool {
        let mut i = self.pos + 1; // skip '['
        let mut depth = 1;
        while i < self.tokens.len() && depth > 0 {
            match &self.tokens[i] {
                Token::LBracket => depth += 1,
                Token::RBracket => {
                    depth -= 1;
                    if depth == 0 {
                        return self.tokens.get(i + 1) == Some(&Token::Assign);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// Parse `list($a, $b, ...) = expr;`
    fn parse_list_assign(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume 'list' identifier
        self.expect(&Token::LParen)?;
        let targets = self.parse_list_targets(&Token::RParen)?;
        self.expect(&Token::RParen)?;
        self.expect(&Token::Assign)?;
        let expr = self.parse_expr()?;
        self.expect(&Token::Semicolon)?;
        Ok(Stmt::ListAssign { targets, expr })
    }

    /// Parse `[$a, $b, ...] = expr;`
    fn parse_short_list_assign(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume '['
        let targets = self.parse_list_targets(&Token::RBracket)?;
        self.expect(&Token::RBracket)?;
        self.expect(&Token::Assign)?;
        let expr = self.parse_expr()?;
        self.expect(&Token::Semicolon)?;
        Ok(Stmt::ListAssign { targets, expr })
    }

    /// Parse comma-separated list targets (variables, skips, nested brackets).
    /// `end_token` is `)` for list() or `]` for short syntax.
    fn parse_list_targets(&mut self, end_token: &Token) -> Result<Vec<ListTarget>, String> {
        let mut targets = Vec::new();
        while self.peek() != *end_token && !self.at_eof() {
            if self.peek() == Token::Comma {
                // Skip element (empty slot before comma or between commas)
                targets.push(ListTarget::Skip);
                self.advance(); // consume ','
                continue;
            }
            // Check for nested: list(...) or [...]
            if self.peek() == Token::LBracket {
                self.advance(); // consume '['
                let nested = self.parse_list_targets(&Token::RBracket)?;
                self.expect(&Token::RBracket)?;
                targets.push(ListTarget::Nested(nested));
            } else if let Token::Identifier(ref name) = self.peek() {
                if name == "list" && self.peek_at(1) == Token::LParen {
                    self.advance(); // consume 'list'
                    self.expect(&Token::LParen)?;
                    let nested = self.parse_list_targets(&Token::RParen)?;
                    self.expect(&Token::RParen)?;
                    targets.push(ListTarget::Nested(nested));
                } else {
                    return Err(format!("Expected variable in list/destructuring, got identifier '{}'", name));
                }
            } else if let Token::Variable(_) = self.peek() {
                // Could be plain $var or key => $var
                let var_name = match self.advance() {
                    Token::Variable(n) => n,
                    _ => unreachable!(),
                };
                targets.push(ListTarget::Variable(var_name));
            } else if matches!(self.peek(), Token::Integer(_) | Token::StringLiteral(_)) {
                // Explicit key: 0 => $var, 'key' => $var
                let key_expr = self.parse_expr()?;
                self.expect(&Token::DoubleArrow)?;
                let var_name = match self.advance() {
                    Token::Variable(n) => n,
                    other => return Err(format!("Expected variable after '=>' in list, got {:?}", other)),
                };
                targets.push(ListTarget::KeyedVariable { key: key_expr, var: var_name });
            } else {
                return Err(format!("Unexpected token in list/destructuring: {:?}", self.peek()));
            }
            // Consume comma if present
            if self.peek() == Token::Comma {
                self.advance();
            } else {
                break;
            }
        }
        Ok(targets)
    }

    /// Parse optional integer level after break/continue (e.g. `break 2;`)
    fn parse_break_continue_level(&mut self) -> Result<Option<u32>, String> {
        if let Token::Integer(n) = self.peek() {
            self.advance();
            if n < 1 {
                return Err(format!("break/continue level must be at least 1, got {}", n));
            }
            Ok(Some(n as u32))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    #[test]
    fn test_parse_echo_42() {
        let tokens = Lexer::new("<?php echo 42;").tokenize().unwrap();
        let stmts = Parser::new(tokens).parse().unwrap();
        assert_eq!(stmts, vec![Stmt::Echo(Expr::Integer(42))]);
    }

    #[test]
    fn test_parse_assign_echo() {
        let tokens = Lexer::new("<?php $a = 42; echo $a;").tokenize().unwrap();
        let stmts = Parser::new(tokens).parse().unwrap();
        assert_eq!(
            stmts,
            vec![
                Stmt::Assign {
                    var: "a".into(),
                    expr: Expr::Integer(42),
                },
                Stmt::Echo(Expr::Variable("a".into())),
            ]
        );
    }

    #[test]
    fn test_parse_add() {
        let tokens = Lexer::new("<?php echo 20 + 22;").tokenize().unwrap();
        let stmts = Parser::new(tokens).parse().unwrap();
        assert_eq!(
            stmts,
            vec![Stmt::Echo(Expr::BinaryOp {
                op: BinOp::Add,
                left: Box::new(Expr::Integer(20)),
                right: Box::new(Expr::Integer(22)),
            })]
        );
    }

    #[test]
    fn test_parse_function_call() {
        let tokens = Lexer::new("<?php echo my_double(21);")
            .tokenize()
            .unwrap();
        let stmts = Parser::new(tokens).parse().unwrap();
        assert_eq!(
            stmts,
            vec![Stmt::Echo(Expr::FunctionCall {
                name: "my_double".into(),
                args: vec![CallArg::Positional(Expr::Integer(21))],
            })]
        );
    }

    #[test]
    fn test_parse_if() {
        let tokens = Lexer::new("<?php if (1) echo 42;").tokenize().unwrap();
        let stmts = Parser::new(tokens).parse().unwrap();
        assert_eq!(
            stmts,
            vec![Stmt::If {
                condition: Expr::Integer(1),
                then_body: vec![Stmt::Echo(Expr::Integer(42))],
                else_body: vec![],
            }]
        );
    }

    #[test]
    fn test_parse_if_else() {
        let tokens = Lexer::new("<?php if (0) { echo 1; } else { echo 2; }")
            .tokenize()
            .unwrap();
        let stmts = Parser::new(tokens).parse().unwrap();
        assert_eq!(
            stmts,
            vec![Stmt::If {
                condition: Expr::Integer(0),
                then_body: vec![Stmt::Echo(Expr::Integer(1))],
                else_body: vec![Stmt::Echo(Expr::Integer(2))],
            }]
        );
    }

    #[test]
    fn test_parse_while() {
        let tokens = Lexer::new("<?php while ($x < 3) { echo $x; }")
            .tokenize()
            .unwrap();
        let stmts = Parser::new(tokens).parse().unwrap();
        assert_eq!(
            stmts,
            vec![Stmt::While {
                condition: Expr::BinaryOp {
                    op: BinOp::Less,
                    left: Box::new(Expr::Variable("x".into())),
                    right: Box::new(Expr::Integer(3)),
                },
                body: vec![Stmt::Echo(Expr::Variable("x".into()))],
            }]
        );
    }
}
