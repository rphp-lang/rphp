/// Minimal PHP parser — produces AST from token stream.

use crate::lexer::Token;

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
        args: Vec<Expr>,
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
    Closure {          // function($x) use($y) { ... }
        params: Vec<Param>,
        use_vars: Vec<String>,
        body: Vec<Stmt>,
    },
    New {              // new ClassName(args)
        class_name: String,
        args: Vec<Expr>,
    },
    PropertyAccess {   // $obj->prop
        object: Box<Expr>,
        property: String,
    },
    MethodCall {       // $obj->method(args)
        object: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    StaticCall {       // ClassName::method(args)
        class_name: String,
        method: String,
        args: Vec<Expr>,
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
        args: Vec<Expr>,
    },
    Instanceof {       // $obj instanceof ClassName
        expr: Box<Expr>,
        class_name: String,
    },
    Constant(String),  // FOO, PHP_INT_MAX — named constant reference
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
}

/// Function parameter with optional default value.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub default: Option<Expr>,
    pub is_variadic: bool,
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
        properties: Vec<ClassProperty>,
        methods: Vec<ClassMethod>,
    },
    AssignProp {       // $obj->prop = expr
        object: Expr,
        property: String,
        expr: Expr,
    },
    Const {            // const FOO = expr;
        name: String,
        value: Expr,
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassMethod {
    pub visibility: Visibility,
    pub name: String,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub is_static: bool,
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
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
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
                    // Check for property assignment: $obj->prop = expr
                    if self.peek() == Token::Assign {
                        if let Expr::PropertyAccess { object, property } = expr {
                            self.advance(); // consume '='
                            let rhs = self.parse_expr()?;
                            self.expect(&Token::Semicolon)?;
                            return Ok(Stmt::AssignProp { object: *object, property, expr: rhs });
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
                self.expect(&Token::LBrace)?;
                let mut body = Vec::new();
                while self.peek() != Token::RBrace && !self.at_eof() {
                    body.push(self.parse_stmt()?);
                }
                self.expect(&Token::RBrace)?;
                Ok(Stmt::Function { name, params, body })
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
            Token::Class => {
                self.parse_class()
            }
            Token::Isset | Token::Empty | Token::Match | Token::New => {
                let expr = self.parse_expr()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::ExprStmt(expr))
            }
            Token::Identifier(_) => {
                let expr = self.parse_expr()?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::ExprStmt(expr))
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
            Token::SlashAssign => Some(BinOp::Div),
            Token::PercentAssign => Some(BinOp::Mod),
            Token::DotAssign => Some(BinOp::Concat),
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
        let mut left = self.parse_comparison()?;

        while self.peek() == Token::AmpAmp {
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::BinaryOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Comparison: ==, !=, <, <=, >, >=, instanceof
    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_concat()?;

        loop {
            // instanceof has same precedence as comparison operators
            if self.peek() == Token::Instanceof {
                self.advance();
                let class_name = match self.advance() {
                    Token::Identifier(name) => name,
                    other => return Err(format!("Expected class name after instanceof, got {:?}", other)),
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
        let mut left = self.parse_additive()?;

        while self.peek() == Token::Dot {
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::BinaryOp {
                op: BinOp::Concat,
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
                self.parse_primary()
            }
            _ => self.parse_primary(),
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
            Token::Identifier(_) => {
                let name = match self.advance() {
                    Token::Identifier(n) => n,
                    _ => unreachable!(),
                };
                // Static access: ClassName::method() or ClassName::$prop
                if self.peek() == Token::DoubleColon {
                    self.advance(); // consume ::
                    if let Token::Variable(_) = self.peek() {
                        let prop = match self.advance() {
                            Token::Variable(n) => n,
                            _ => unreachable!(),
                        };
                        return Ok(Expr::StaticProperty { class_name: name, property: prop });
                    }
                    let method = match self.advance() {
                        Token::Identifier(n) => n,
                        other => return Err(format!("Expected method name after ::, got {:?}", other)),
                    };
                    self.expect(&Token::LParen)?;
                    let mut args = Vec::new();
                    if self.peek() != Token::RParen {
                        args.push(self.parse_expr()?);
                        while self.peek() == Token::Comma {
                            self.advance();
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(&Token::RParen)?;
                    return Ok(Expr::StaticCall { class_name: name, method, args });
                }
                // Check if this is a function call (followed by `(`)
                if self.peek() == Token::LParen {
                    self.advance(); // consume (
                    let mut args = Vec::new();
                    if self.peek() != Token::RParen {
                        args.push(self.parse_expr()?);
                        while self.peek() == Token::Comma {
                            self.advance();
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(&Token::RParen)?;
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
                let class_name = match self.advance() {
                    Token::Identifier(n) => n,
                    other => return Err(format!("Expected class name after 'new', got {:?}", other)),
                };
                let mut args = Vec::new();
                if self.peek() == Token::LParen {
                    self.advance(); // consume (
                    if self.peek() != Token::RParen {
                        args.push(self.parse_expr()?);
                        while self.peek() == Token::Comma {
                            self.advance();
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(&Token::RParen)?;
                }
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
                    let mut args = Vec::new();
                    if self.peek() != Token::RParen {
                        args.push(self.parse_expr()?);
                        while self.peek() == Token::Comma {
                            self.advance();
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(&Token::RParen)?;
                    expr = Expr::DynamicCall {
                        callable: Box::new(expr),
                        args,
                    };
                }
                Token::Arrow => {
                    self.advance();
                    let member = match self.advance() {
                        Token::Identifier(n) => n,
                        other => return Err(format!("Expected property/method name after ->, got {:?}", other)),
                    };
                    if self.peek() == Token::LParen {
                        self.advance();
                        let mut args = Vec::new();
                        if self.peek() != Token::RParen {
                            args.push(self.parse_expr()?);
                            while self.peek() == Token::Comma {
                                self.advance();
                                args.push(self.parse_expr()?);
                            }
                        }
                        self.expect(&Token::RParen)?;
                        expr = Expr::MethodCall {
                            object: Box::new(expr),
                            method: member,
                            args,
                        };
                    } else {
                        expr = Expr::PropertyAccess {
                            object: Box::new(expr),
                            property: member,
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
            let type_name = match self.advance() {
                Token::Identifier(n) => n,
                other => return Err(format!("Expected exception type, got {:?}", other)),
            };
            types.push(type_name);
            while self.peek() == Token::Pipe {
                self.advance();
                let t = match self.advance() {
                    Token::Identifier(n) => n,
                    other => return Err(format!("Expected exception type, got {:?}", other)),
                };
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
        self.advance(); // consume 'class'
        let name = match self.advance() {
            Token::Identifier(n) => n,
            other => return Err(format!("Expected class name, got {:?}", other)),
        };
        let parent = if self.peek() == Token::Extends {
            self.advance();
            match self.advance() {
                Token::Identifier(n) => Some(n),
                other => return Err(format!("Expected parent class name, got {:?}", other)),
            }
        } else {
            None
        };
        self.expect(&Token::LBrace)?;

        let mut properties = Vec::new();
        let mut methods = Vec::new();

        while self.peek() != Token::RBrace && !self.at_eof() {
            let (vis, is_static) = self.parse_visibility_and_static();

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
                self.expect(&Token::LBrace)?;
                let mut body = Vec::new();
                while self.peek() != Token::RBrace && !self.at_eof() {
                    body.push(self.parse_stmt()?);
                }
                self.expect(&Token::RBrace)?;
                methods.push(ClassMethod { visibility: vis, name: method_name, params, body, is_static });
            } else if let Token::Variable(_) = self.peek() {
                // Property
                let prop_name = match self.advance() {
                    Token::Variable(n) => n,
                    _ => unreachable!(),
                };
                let default = if self.peek() == Token::Assign {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.expect(&Token::Semicolon)?;
                properties.push(ClassProperty { visibility: vis, name: prop_name, default, is_static });
            } else {
                return Err(format!("Unexpected token in class body: {:?}", self.peek()));
            }
        }
        self.expect(&Token::RBrace)?;

        Ok(Stmt::Class { name, parent, properties, methods })
    }

    fn parse_visibility_and_static(&mut self) -> (Visibility, bool) {
        let mut vis = Visibility::Public;
        let mut is_static = false;

        loop {
            match self.peek() {
                Token::Public => { self.advance(); vis = Visibility::Public; }
                Token::Protected => { self.advance(); vis = Visibility::Protected; }
                Token::Private => { self.advance(); vis = Visibility::Private; }
                Token::Static => { self.advance(); is_static = true; }
                _ => break,
            }
        }
        (vis, is_static)
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
        self.expect(&Token::DoubleArrow)?;
        let expr = self.parse_expr()?;

        // Auto-capture: collect free variables from expr that aren't params
        let param_names: std::collections::HashSet<&str> = params.iter().map(|p| p.name.as_str()).collect();
        let mut free_vars = Vec::new();
        Self::collect_free_vars(&expr, &param_names, &mut free_vars);

        let body = vec![Stmt::Return(Some(expr))];
        Ok(Expr::Closure { params, use_vars: free_vars, body })
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
            Expr::UnaryMinus(inner) | Expr::Not(inner) | Expr::Throw(inner) | Expr::Empty(inner) => {
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
                    Self::collect_free_vars(arg, bound, out);
                }
            }
            Expr::DynamicCall { callable, args } => {
                Self::collect_free_vars(callable, bound, out);
                for arg in args {
                    Self::collect_free_vars(arg, bound, out);
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
                    Self::collect_free_vars(arg, bound, out);
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
                    Self::collect_free_vars(arg, bound, out);
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

        self.expect(&Token::LBrace)?;
        let mut body = Vec::new();
        while self.peek() != Token::RBrace && !self.at_eof() {
            body.push(self.parse_stmt()?);
        }
        self.expect(&Token::RBrace)?;

        Ok(Expr::Closure { params, use_vars, body })
    }

    fn peek(&self) -> Token {
        self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof)
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
        if matches!(self.peek(), Token::Variable(_) | Token::DotDotDot) {
            params.push(self.parse_one_param()?);
            while self.peek() == Token::Comma {
                self.advance();
                params.push(self.parse_one_param()?);
            }
        }
        Ok(params)
    }

    fn parse_one_param(&mut self) -> Result<Param, String> {
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
        Ok(Param { name, default, is_variadic })
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
                args: vec![Expr::Integer(21)],
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
