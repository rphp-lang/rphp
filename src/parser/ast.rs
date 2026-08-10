/// Target in a list()/[] destructuring assignment.
#[derive(Debug, Clone, PartialEq)]
pub enum ListTarget {
    Variable(String),
    Skip,                                     // empty slot: list(,$b)
    Nested(Vec<ListTarget>),                  // nested destructuring
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
        generic_args: Vec<TypeHint>,
    },
    PostInc(String),       // $i++
    PostDec(String),       // $i--
    PreInc(String),        // ++$i
    PreDec(String),        // --$i
    Not(Box<Expr>),        // !expr
    UnaryMinus(Box<Expr>), // -$x
    Ternary {
        // cond ? then : else
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
    ArrayLiteral(Vec<ArrayElement>), // [1, 2] or ['a' => 1]
    ArrayAccess {
        // $a[0], $a['key']
        array: Box<Expr>,
        index: Box<Expr>,
    },
    Cast {
        // (int)$x, (string)$x, etc.
        cast_type: CastType,
        expr: Box<Expr>,
    },
    Isset(Vec<Expr>), // isset($a, $b)
    Empty(Box<Expr>), // empty($a)
    NullCoalesce {
        // $a ?? $b
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Elvis {
        // $a ?: $b (evaluates lhs once)
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Match {
        // match($x) { ... }
        expr: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    Closure {
        // function($x) use($y) { ... }: ReturnType
        params: Vec<Param>,
        use_vars: Vec<String>,
        body: Vec<Stmt>,
        return_type: Option<TypeHint>,
        generic_params: Vec<GenericParameter>,
    },
    New {
        // new ClassName(args)
        class_name: String,
        args: Vec<CallArg>,
        generic_args: Vec<TypeHint>,
    },
    PropertyAccess {
        // $obj->prop or $obj?->prop
        object: Box<Expr>,
        property: String,
        nullsafe: bool,
    },
    MethodCall {
        // $obj->method(args) or $obj?->method(args)
        object: Box<Expr>,
        method: String,
        args: Vec<CallArg>,
        generic_args: Vec<TypeHint>,
        nullsafe: bool,
    },
    StaticCall {
        // ClassName::method(args)
        class_name: String,
        method: String,
        args: Vec<CallArg>,
        generic_args: Vec<TypeHint>,
    },
    StaticProperty {
        // ClassName::$prop
        class_name: String,
        property: String,
    },
    Throw(Box<Expr>), // throw expr (PHP 8 expression)
    Assign {
        // $var = expr (used in expressions like $a = $b ?? $c)
        var: String,
        expr: Box<Expr>,
    },
    DynamicCall {
        // $var(args) — variable function call / closure call
        callable: Box<Expr>,
        args: Vec<CallArg>,
        generic_args: Vec<TypeHint>,
    },
    Instanceof {
        // $obj instanceof ClassName
        expr: Box<Expr>,
        class_name: String,
    },
    Constant(String), // FOO, PHP_INT_MAX — named constant reference
    Yield {
        // yield $value or yield $key => $value
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
    And,        // &&
    Or,         // ||
    Spaceship,  // <=>
    Pow,        // **
    BitwiseAnd, // &
    BitwiseOr,  // |
    BitwiseXor, // ^
    ShiftLeft,  // <<
    ShiftRight, // >>
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
    ClassName(std::string::String), // includes "self", "parent", "static"
    Nullable(Box<TypeHint>),        // ?int, ?string, ?ClassName, etc.
    Union(Vec<TypeHint>),           // int|string, Foo|Bar, etc.
    GenericParameter {
        name: std::string::String,
        erased: Box<TypeHint>,
    },
    GenericApplication {
        base: std::string::String,
        arguments: Vec<TypeHint>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericVariance {
    Invariant,
    Covariant,
    Contravariant,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericParameter {
    pub name: std::string::String,
    pub variance: GenericVariance,
    pub bound: Option<TypeHint>,
    pub default: Option<TypeHint>,
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
        generic_params: Vec<GenericParameter>,
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
    ArrayAssign {
        // $a[idx] = expr
        var: String,
        index: Expr,
        expr: Expr,
    },
    ArrayPush {
        // $a[] = expr
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
        uses: Vec<String>, // trait names from `use Foo, Bar;`
        generic_params: Vec<GenericParameter>,
    },
    Interface {
        name: String,
        extends: Vec<String>,
        methods: Vec<ClassMethod>, // all public, abstract (no body)
        generic_params: Vec<GenericParameter>,
    },
    Trait {
        name: String,
        properties: Vec<ClassProperty>,
        methods: Vec<ClassMethod>,
        generic_params: Vec<GenericParameter>,
    },
    AssignProp {
        // $obj->prop = expr
        object: Expr,
        property: String,
        expr: Expr,
    },
    AssignObjArrayDim {
        // $obj->prop[$key] = expr
        object: Expr,
        property: String,
        index: Expr,
        expr: Expr,
    },
    Declare {
        // declare(strict_types=1);
        directive: String,
        value: i64,
    },
    Namespace {
        // namespace App\Models;
        name: String,
        body: Vec<Stmt>, // if braced: namespace App { ... }, else: rest of file
    },
    UseDecl {
        // use App\Models\User; or use App\Models\User as U;
        imports: Vec<(String, String)>, // (fully_qualified, alias)
    },
    Const {
        // const FOO = expr;
        name: String,
        value: Expr,
    },
    ListAssign {
        // list($a, $b) = expr; or [$a, $b] = expr;
        targets: Vec<ListTarget>,
        expr: Expr,
    },
    Global(Vec<String>), // global $a, $b;
    StaticVar {
        // static $a = 0, $b = "";
        vars: Vec<(String, Option<Expr>)>,
    },
    Enum {
        name: String,
        backing_type: Option<TypeHint>,
        cases: Vec<(String, Option<Expr>)>, // (case_name, optional_value)
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
    pub types: Vec<String>, // Exception class names (multi-catch: ExA | ExB)
    pub var: String,        // $e
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
    pub generic_params: Vec<GenericParameter>,
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
