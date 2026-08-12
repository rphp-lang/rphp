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
    Unpack(Expr),
    Named { name: String, value: Expr },
}

impl CallArg {
    /// Return a reference to the underlying expression.
    pub fn expr(&self) -> &Expr {
        match self {
            CallArg::Positional(e) | CallArg::Unpack(e) => e,
            CallArg::Named { value, .. } => value,
        }
    }

    pub(crate) fn contains_yield(&self) -> bool {
        self.expr().contains_yield()
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
        // [static] function($x) use($y) { ... }: ReturnType
        is_static: bool,
        params: Vec<Param>,
        use_vars: Vec<(String, bool)>, // (name, captured by reference)
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
    ClassConstant {
        // ClassName::CONSTANT, self::CONSTANT, parent::CONSTANT, static::CONSTANT
        class_name: String,
        constant: String,
    },
    DynamicClassConstant {
        // $class::CONSTANT, $object::CONSTANT, or $class::{$constant}
        class: Box<Expr>,
        constant: Box<Expr>,
        dynamic_name: bool,
    },
    DynamicNamedClassConstant {
        // ClassName::{$constant}
        class_name: String,
        constant: Box<Expr>,
    },
    Throw(Box<Expr>), // throw expr (PHP 8 expression)
    Include {
        path: Box<Expr>,
        is_require: bool,
        is_once: bool,
    },
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
    FirstClassCallable(Box<Expr>),
    Instanceof {
        // $obj instanceof ClassName
        expr: Box<Expr>,
        class_name: String,
    },
    Constant(String), // FOO, PHP_INT_MAX — named constant reference
    MagicConstant {
        name: String,
        line: usize,
    },
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

impl Expr {
    pub(crate) fn contains_yield(&self) -> bool {
        match self {
            Expr::Yield { .. } | Expr::YieldFrom(_) => true,
            Expr::BinaryOp { left, right, .. }
            | Expr::NullCoalesce { left, right }
            | Expr::Elvis { left, right } => left.contains_yield() || right.contains_yield(),
            Expr::Not(inner)
            | Expr::UnaryMinus(inner)
            | Expr::Empty(inner)
            | Expr::Throw(inner)
            | Expr::Include { path: inner, .. }
            | Expr::BitwiseNot(inner)
            | Expr::Clone(inner) => inner.contains_yield(),
            Expr::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                condition.contains_yield()
                    || then_expr.contains_yield()
                    || else_expr.contains_yield()
            }
            Expr::ArrayLiteral(elements) => elements.iter().any(|element| {
                element
                    .key
                    .as_ref()
                    .is_some_and(Expr::contains_yield)
                    || element.value.contains_yield()
            }),
            Expr::ArrayAccess { array, index } => {
                array.contains_yield() || index.contains_yield()
            }
            Expr::Cast { expr, .. }
            | Expr::PropertyAccess { object: expr, .. }
            | Expr::Instanceof { expr, .. }
            | Expr::Assign { expr, .. }
            | Expr::Print(expr) => expr.contains_yield(),
            Expr::Isset(expressions) => expressions.iter().any(Expr::contains_yield),
            Expr::Match { expr, arms } => {
                expr.contains_yield()
                    || arms.iter().any(|arm| {
                        arm.conditions
                            .as_ref()
                            .is_some_and(|conditions| {
                                conditions.iter().any(Expr::contains_yield)
                            })
                            || arm.body.contains_yield()
                    })
            }
            Expr::FunctionCall { args, .. }
            | Expr::New { args, .. }
            | Expr::StaticCall { args, .. } => args.iter().any(CallArg::contains_yield),
            Expr::MethodCall { object, args, .. } => {
                object.contains_yield() || args.iter().any(CallArg::contains_yield)
            }
            Expr::DynamicCall { callable, args, .. } => {
                callable.contains_yield() || args.iter().any(CallArg::contains_yield)
            }
            Expr::FirstClassCallable(callable) => callable.contains_yield(),
            // A closure has its own suspension context; declaring it does not
            // suspend the surrounding expression.
            Expr::Closure { .. }
            | Expr::Integer(_)
            | Expr::Float(_)
            | Expr::StringLiteral(_)
            | Expr::Null
            | Expr::Bool(_)
            | Expr::Variable(_)
            | Expr::PostInc(_)
            | Expr::PostDec(_)
            | Expr::PreInc(_)
            | Expr::PreDec(_)
            | Expr::StaticProperty { .. }
            | Expr::ClassConstant { .. }
            | Expr::Constant(_)
            | Expr::MagicConstant { .. } => false,
            Expr::DynamicClassConstant {
                class, constant, ..
            } => {
                class.contains_yield() || constant.contains_yield()
            }
            Expr::DynamicNamedClassConstant { constant, .. } => constant.contains_yield(),
        }
    }
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
    Intersection(Vec<TypeHint>),    // Foo&Bar, etc.
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

/// A class-like name in an inheritance clause together with its pre-erasure
/// generic arguments. Runtime class lookup continues to use only `name`.
#[derive(Debug, Clone, PartialEq)]
pub struct GenericAncestor {
    pub name: std::string::String,
    pub arguments: Vec<TypeHint>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind {
    Class,
    Function,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Noop,
    Echo(Vec<Expr>),
    Assign {
        var: String,
        expr: Expr,
    },
    CoalesceAssign {
        target: Expr,
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
    NestedArrayAssign {
        // $a[first][second]..., $obj->prop[...]..., or Class::$prop[...]... = expr
        root: Expr,
        indices: Vec<Expr>,
        expr: Expr,
    },
    ArrayPush {
        // $a[] = expr
        var: String,
        expr: Expr,
    },
    ArrayAppend {
        // $obj->items[$key][] = expr or Class::$items[] = expr
        target: Expr,
        expr: Expr,
    },
    BindArrayAppendReference {
        // $ref = &$obj->items[$key][]
        var: String,
        target: Expr,
    },
    Foreach {
        array: Expr,
        value_var: String,
        key_var: Option<String>,
        by_ref: bool,
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
        parent: Option<GenericAncestor>,
        implements: Vec<GenericAncestor>,
        is_abstract: bool,
        is_final: bool,
        properties: Vec<ClassProperty>,
        constants: Vec<ClassConstant>,
        methods: Vec<ClassMethod>,
        uses: Vec<GenericAncestor>, // trait names from `use Foo<T>, Bar<U>;`
        generic_params: Vec<GenericParameter>,
    },
    Interface {
        name: String,
        extends: Vec<GenericAncestor>,
        constants: Vec<ClassConstant>,
        methods: Vec<ClassMethod>, // all public, abstract (no body)
        generic_params: Vec<GenericParameter>,
    },
    Trait {
        name: String,
        properties: Vec<ClassProperty>,
        constants: Vec<ClassConstant>,
        methods: Vec<ClassMethod>,
        generic_params: Vec<GenericParameter>,
    },
    AssignProp {
        // $obj->prop = expr
        object: Expr,
        property: String,
        expr: Expr,
    },
    AssignStaticProp {
        // ClassName::$prop = expr
        class_name: String,
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
        // use App\Models\User; or use function App\Helpers\slug as make_slug;
        kind: UseKind,
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
        constants: Vec<ClassConstant>,
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
    /// Source-level property contract. The ordinary runtime currently uses
    /// the erased storage model; generics metadata preserves this form for
    /// reified substitution and Reflection.
    pub type_hint: Option<TypeHint>,
    pub default: Option<Expr>,
    pub is_static: bool,
    pub is_readonly: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassConstant {
    pub visibility: Visibility,
    pub name: String,
    pub value: Expr,
    pub type_hint: Option<TypeHint>,
    pub is_final: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassMethod {
    pub visibility: Visibility,
    pub name: String,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub is_static: bool,
    pub is_final: bool,
    pub is_abstract: bool,
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
