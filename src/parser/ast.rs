/// Target in a list()/[] destructuring assignment.
#[derive(Debug, Clone, PartialEq)]
pub enum ListTarget {
    Variable(String),
    Reference(Expr),
    Target(Expr),
    AppendTarget(Expr),
    Skip,                                     // empty slot: list(,$b)
    Nested(Vec<ListTarget>),                  // nested destructuring
    KeyedVariable { key: Expr, var: String }, // explicit key: [0 => $a, 2 => $c]
    KeyedReference { key: Expr, target: Expr },
    KeyedTarget { key: Expr, target: Expr },
    KeyedAppendTarget { key: Expr, target: Expr },
    KeyedNested { key: Expr, targets: Vec<ListTarget> },
}

impl ListTarget {
    pub(crate) fn source_line(&self) -> usize {
        fn expression_line(expression: &Expr) -> usize {
            match expression {
                Expr::Variable { line, .. }
                | Expr::DynamicVariable { line, .. }
                | Expr::Globals { line }
                | Expr::CompileError { line, .. }
                | Expr::CompileWarning { line, .. }
                | Expr::CompileDeprecation { line, .. }
                | Expr::ArrayAccess { line, .. }
                | Expr::PropertyAccess { line, .. }
                | Expr::DynamicPropertyAccess { line, .. } => *line,
                _ => 0,
            }
        }

        match self {
            ListTarget::Variable(_) | ListTarget::Skip => 0,
            ListTarget::Reference(target)
            | ListTarget::Target(target)
            | ListTarget::AppendTarget(target) => expression_line(target),
            ListTarget::Nested(targets) | ListTarget::KeyedNested { targets, .. } => targets
                .iter()
                .map(ListTarget::source_line)
                .find(|line| *line != 0)
                .unwrap_or(0),
            ListTarget::KeyedVariable { key, .. } => expression_line(key),
            ListTarget::KeyedReference { key, target }
            | ListTarget::KeyedTarget { key, target }
            | ListTarget::KeyedAppendTarget { key, target } => {
                let target_line = expression_line(target);
                if target_line != 0 {
                    target_line
                } else {
                    expression_line(key)
                }
            }
        }
    }

    pub(crate) fn contains_yield(&self) -> bool {
        match self {
            ListTarget::Variable(_) | ListTarget::Skip => false,
            ListTarget::Reference(target)
            | ListTarget::Target(target)
            | ListTarget::AppendTarget(target) => {
                target.contains_yield()
            }
            ListTarget::Nested(targets) => targets.iter().any(ListTarget::contains_yield),
            ListTarget::KeyedVariable { key, .. } => key.contains_yield(),
            ListTarget::KeyedReference { key, target }
            | ListTarget::KeyedTarget { key, target }
            | ListTarget::KeyedAppendTarget { key, target } => {
                key.contains_yield() || target.contains_yield()
            }
            ListTarget::KeyedNested { key, targets } => {
                key.contains_yield() || targets.iter().any(ListTarget::contains_yield)
            }
        }
    }

    pub(crate) fn contains_reference(&self) -> bool {
        match self {
            ListTarget::Reference(_) | ListTarget::KeyedReference { .. } => true,
            ListTarget::Nested(targets) | ListTarget::KeyedNested { targets, .. } => {
                targets.iter().any(ListTarget::contains_reference)
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ForeachTarget {
    Variable(String),
    Target(Expr),
    Destructure(Vec<ListTarget>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum GlobalTarget {
    Variable(String),
    Dynamic(Expr),
}

impl ForeachTarget {
    pub(crate) fn contains_yield(&self) -> bool {
        match self {
            ForeachTarget::Variable(_) => false,
            ForeachTarget::Target(target) => target.contains_yield(),
            ForeachTarget::Destructure(targets) => {
                targets.iter().any(ListTarget::contains_yield)
            }
        }
    }
}

/// A call-site argument: either positional or named (PHP 8).
#[derive(Debug, Clone, PartialEq)]
pub enum CallArg {
    Positional(Expr),
    Unpack(Expr),
    Named { name: String, value: Expr },
}

/// One source-level PHP attribute. Names are resolved in the declaration's
/// lexical namespace by the compiler; arguments remain constant-expression
/// AST until that same context is available.
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<CallArg>,
    pub line: usize,
}

impl Attribute {
    const NON_ENUM_CASE_MARKER: &'static str = "\0rphp_non_enum_case";

    pub(crate) fn non_enum_case_marker(line: usize) -> Self {
        Self {
            name: Self::NON_ENUM_CASE_MARKER.to_string(),
            args: Vec::new(),
            line,
        }
    }

    pub(crate) fn is_non_enum_case_marker(&self) -> bool {
        self.name == Self::NON_ENUM_CASE_MARKER
    }

    pub(crate) fn non_enum_case_line(attributes: &[Self]) -> Option<usize> {
        attributes
            .iter()
            .find(|attribute| attribute.is_non_enum_case_marker())
            .map(|attribute| attribute.line)
    }
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
    /// Lossless Latin-1 storage for PHP bytes introduced by byte escapes or
    /// preserved document-string content.
    BinaryStringLiteral(String),
    Null,
    Bool(bool),
    Variable {
        name: String,
        /// Source line of the variable token. Synthetic compiler expressions
        /// use zero so they never acquire user-visible diagnostics.
        line: usize,
    },
    /// PHP variable-variable lookup. The name expression is evaluated exactly
    /// once before the surrounding read or l-value operation starts.
    DynamicVariable {
        name: Box<Expr>,
        line: usize,
    },
    /// The PHP 8.2 global symbol-table pseudo-variable. Keeping its source
    /// line distinguishes whole-table restrictions from ordinary CVs while
    /// direct dimensions retain their dedicated compiler lowering.
    Globals { line: usize },
    /// A parsed PHP construct whose syntax is valid but whose target is
    /// rejected during compilation. This preserves PHP's compile-error stage
    /// and source location without making the parser report a syntax error.
    CompileError { message: String, line: usize },
    /// A declaration-time diagnostic whose expression value is unused. The
    /// lexer appends these markers at source-unit scope so dead code cannot
    /// suppress PHP's compile-time deprecation.
    CompileDeprecation { message: String, line: usize },
    /// A source-unit warning discovered while lexing. Like deprecations, this
    /// remains visible even when the containing expression is unreachable.
    CompileWarning { message: String, line: usize },
    BinaryOp {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
        line: usize,
    },
    Pipe {
        input: Box<Expr>,
        callable: Box<Expr>,
        line: usize,
    },
    FunctionCall {
        name: String,
        args: Vec<CallArg>,
        generic_args: Vec<TypeHint>,
        line: usize,
    },
    /// PHP's eval language construct. Runtime compilation shares the caller's
    /// variable and class scope but remains a distinct source unit.
    Eval {
        source: Box<Expr>,
        line: usize,
    },
    PostInc { name: String, line: usize }, // $i++
    PostDec { name: String, line: usize }, // $i--
    PostIncTarget(Box<Expr>), // self::$value++, $object->property++, $array[$key]++
    PostDecTarget(Box<Expr>), // self::$value--, $object->property--, $array[$key]--
    PreInc { name: String, line: usize }, // ++$i
    PreDec { name: String, line: usize }, // --$i
    PreIncTarget(Box<Expr>), // ++self::$value, ++$object->property
    PreDecTarget(Box<Expr>), // --self::$value, --$object->property
    Not(Box<Expr>),        // !expr
    UnaryPlus(Box<Expr>),  // +$x
    UnaryMinus(Box<Expr>), // -$x
    ErrorSuppress(Box<Expr>), // @expr
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
        line: usize,
    },
    /// A positional call argument ending in `[]`. The compiler may bind the
    /// appended slot only when the statically resolved parameter is by
    /// reference; every ordinary value context retains PHP's read error.
    ArrayAppendArgument {
        target: Box<Expr>,
        line: usize,
    },
    Cast {
        // (int)$x, (string)$x, etc.
        cast_type: CastType,
        expr: Box<Expr>,
        line: usize,
    },
    Isset(Vec<Expr>), // isset($a, $b)
    Empty(Box<Expr>), // empty($a)
    NullCoalesce {
        // $a ?? $b
        left: Box<Expr>,
        right: Box<Expr>,
    },
    CoalesceAssign {
        // $a ??= $b (also valid as a value-producing expression)
        target: Box<Expr>,
        expr: Box<Expr>,
    },
    CompoundAssignExpression {
        target: Box<Expr>,
        op: BinOp,
        expr: Box<Expr>,
    },
    Elvis {
        // $a ?: $b (evaluates lhs once)
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Match {
        // match($x) { ... }
        line: usize,
        expr: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    Closure {
        // [static] function($x) use($y) { ... }: ReturnType
        line: usize,
        attributes: Vec<Attribute>,
        is_static: bool,
        returns_by_ref: bool,
        params: Vec<Param>,
        use_vars: Vec<(String, bool, usize)>, // (name, captured by reference, source line)
        body: Vec<Stmt>,
        return_type: Option<TypeHint>,
        generic_params: Vec<GenericParameter>,
    },
    New {
        // new ClassName(args)
        class_name: String,
        args: Vec<CallArg>,
        generic_args: Vec<TypeHint>,
        line: usize,
        call_line: usize,
    },
    DynamicNew {
        // new $class(args)
        class: Box<Expr>,
        args: Vec<CallArg>,
        line: usize,
        call_line: usize,
    },
    AnonymousNew {
        attributes: Vec<Attribute>,
        args: Vec<CallArg>,
        is_readonly: bool,
        allow_dynamic_properties: bool,
        parent: Option<GenericAncestor>,
        implements: Vec<GenericAncestor>,
        properties: Vec<ClassProperty>,
        constants: Vec<ClassConstant>,
        methods: Vec<ClassMethod>,
        uses: Vec<GenericAncestor>,
        trait_aliases: Vec<TraitAlias>,
        line: usize,
        call_line: usize,
    },
    PropertyAccess {
        // $obj->prop or $obj?->prop
        object: Box<Expr>,
        property: String,
        nullsafe: bool,
        line: usize,
    },
    DynamicPropertyAccess {
        // $obj->{$property}
        object: Box<Expr>,
        property: Box<Expr>,
        nullsafe: bool,
        line: usize,
    },
    MethodCall {
        // $obj->method(args) or $obj?->method(args)
        object: Box<Expr>,
        method: String,
        args: Vec<CallArg>,
        generic_args: Vec<TypeHint>,
        nullsafe: bool,
        line: usize,
    },
    StaticCall {
        // ClassName::method(args)
        class_name: String,
        method: String,
        args: Vec<CallArg>,
        generic_args: Vec<TypeHint>,
        line: usize,
    },
    StaticProperty {
        // ClassName::$prop
        class_name: String,
        property: String,
        /// Parentheses make a following `::` operate on the fetched value,
        /// not on the source-level static-property node.
        parenthesized: bool,
        line: usize,
    },
    DynamicNamedStaticProperty {
        // ClassName::$$name or ClassName::${expression}
        class_name: String,
        property: Box<Expr>,
        line: usize,
    },
    DynamicStaticProperty {
        // $class::$prop, $class::$$name, or expression::${expression}
        class: Box<Expr>,
        property: Box<Expr>,
        line: usize,
    },
    ClassConstant {
        // ClassName::CONSTANT, self::CONSTANT, parent::CONSTANT, static::CONSTANT
        class_name: String,
        constant: String,
        line: usize,
    },
    DynamicClassConstant {
        // $class::CONSTANT, $object::CONSTANT, or $class::{$constant}
        class: Box<Expr>,
        constant: Box<Expr>,
        dynamic_name: bool,
        line: usize,
    },
    DynamicNamedClassConstant {
        // ClassName::{$constant}
        class_name: String,
        constant: Box<Expr>,
    },
    Throw {
        expr: Box<Expr>,
        line: usize,
    }, // throw expr (PHP 8 expression)
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
    AssignReference {
        // $var = &$object->property (reference identity must be preserved)
        var: String,
        target: Box<Expr>,
    },
    AssignTargetReference {
        // $array[$key] = &$var or $object->property = &$var
        target: Box<Expr>,
        source: Box<Expr>,
    },
    AssignTarget {
        // Mutable non-variable assignment used as a value-producing expression.
        target: Box<Expr>,
        expr: Box<Expr>,
    },
    ArrayAppendAssign {
        // $target[] = expression / $target[] =& variable.
        target: Box<Expr>,
        expr: Box<Expr>,
        by_ref: bool,
    },
    ListAssign {
        // [$first, , $third] = expression used inside a larger expression.
        targets: Vec<ListTarget>,
        expr: Box<Expr>,
        line: usize,
    },
    DynamicCall {
        // $var(args) — variable function call / closure call
        callable: Box<Expr>,
        args: Vec<CallArg>,
        generic_args: Vec<TypeHint>,
        /// The parser also lowers `$object->$method()` and `Class::$method()`
        /// through a callable pair. Retain their method-call spelling for
        /// compile-time diagnostics without changing dynamic dispatch.
        method_syntax: bool,
        line: usize,
    },
    DynamicStaticCall {
        // $classExpr::$method(args) / $classExpr::method(args)
        class: Box<Expr>,
        method: Box<Expr>,
        args: Vec<CallArg>,
        generic_args: Vec<TypeHint>,
        line: usize,
    },
    FirstClassFunctionCallable {
        name: String,
        line: usize,
    },
    FirstClassCallable {
        callable: Box<Expr>,
        line: usize,
    },
    /// A source member-call FCC whose owner and dynamic member have distinct
    /// evaluation boundaries. Static syntax may autoload/validate the class
    /// after the owner but before evaluating the member expression.
    FirstClassMemberCallable {
        owner: Box<Expr>,
        member: Box<Expr>,
        static_syntax: bool,
        line: usize,
    },
    Instanceof {
        // $obj instanceof ClassName
        expr: Box<Expr>,
        class_name: String,
    },
    DynamicInstanceof {
        // $obj instanceof $className
        expr: Box<Expr>,
        class: Box<Expr>,
    },
    Constant {
        name: String,
        line: usize,
    }, // FOO, PHP_INT_MAX — named constant reference
    CompilerHaltOffsetConstant {
        name: String,
        line: usize,
    },
    MagicConstant {
        name: String,
        line: usize,
    },
    Yield {
        // yield $value or yield $key => $value
        value: Option<Box<Expr>>,
        key: Option<Box<Expr>>,
    },
    YieldFrom {
        expr: Box<Expr>,
        line: usize,
    }, // yield from $expr
    Print(Box<Expr>),      // print expr (returns 1)
    BitwiseNot {
        expr: Box<Expr>,
        line: usize,
    }, // ~expr
    Clone {
        expr: Box<Expr>,
        with_properties: Option<Box<Expr>>,
        line: usize,
    }, // clone $expr / clone($expr, $withProperties)
}

impl Expr {
    pub(crate) fn contains_yield(&self) -> bool {
        match self {
            Expr::Yield { .. } | Expr::YieldFrom { .. } => true,
            Expr::BinaryOp { left, right, .. }
            | Expr::NullCoalesce { left, right }
            | Expr::CoalesceAssign {
                target: left,
                expr: right,
            }
            | Expr::Elvis { left, right } => left.contains_yield() || right.contains_yield(),
            Expr::Not(inner)
            | Expr::UnaryPlus(inner)
            | Expr::UnaryMinus(inner)
            | Expr::ErrorSuppress(inner)
            | Expr::Empty(inner)
            | Expr::Throw { expr: inner, .. }
            | Expr::Include { path: inner, .. }
            | Expr::Eval { source: inner, .. }
            | Expr::BitwiseNot { expr: inner, .. }
            | Expr::DynamicVariable { name: inner, .. } => inner.contains_yield(),
            Expr::Clone {
                expr,
                with_properties,
                ..
            } => {
                expr.contains_yield()
                    || with_properties
                        .as_deref()
                        .is_some_and(Expr::contains_yield)
            }
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
            Expr::ArrayAccess { array, index, .. } => {
                array.contains_yield() || index.contains_yield()
            }
            Expr::ArrayAppendArgument { target, .. } => target.contains_yield(),
            Expr::DynamicPropertyAccess {
                object, property, ..
            } => object.contains_yield() || property.contains_yield(),
            Expr::DynamicNamedStaticProperty { property, .. } => property.contains_yield(),
            Expr::DynamicStaticProperty {
                class, property, ..
            } => {
                class.contains_yield() || property.contains_yield()
            }
            Expr::Cast { expr, .. }
            | Expr::PropertyAccess { object: expr, .. }
            | Expr::Instanceof { expr, .. }
            | Expr::Assign { expr, .. }
            | Expr::AssignReference { target: expr, .. }
            | Expr::Print(expr) => expr.contains_yield(),
            Expr::DynamicInstanceof { expr, class } => {
                expr.contains_yield() || class.contains_yield()
            }
            Expr::AssignTarget { target, expr }
            | Expr::AssignTargetReference {
                target,
                source: expr,
            }
            | Expr::ArrayAppendAssign { target, expr, .. } => {
                target.contains_yield() || expr.contains_yield()
            }
            Expr::Pipe {
                input, callable, ..
            } => input.contains_yield() || callable.contains_yield(),
            Expr::ListAssign { targets, expr, .. } => {
                targets.iter().any(ListTarget::contains_yield) || expr.contains_yield()
            }
            Expr::CompoundAssignExpression { target, expr, .. } => {
                target.contains_yield() || expr.contains_yield()
            }
            Expr::PostIncTarget(target)
            | Expr::PostDecTarget(target)
            | Expr::PreIncTarget(target)
            | Expr::PreDecTarget(target) => target.contains_yield(),
            Expr::Isset(expressions) => expressions.iter().any(Expr::contains_yield),
            Expr::Match { expr, arms, .. } => {
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
            Expr::DynamicNew { class, args, .. } => {
                class.contains_yield() || args.iter().any(CallArg::contains_yield)
            }
            Expr::AnonymousNew { args, .. } => {
                args.iter().any(CallArg::contains_yield)
            }
            Expr::MethodCall { object, args, .. } => {
                object.contains_yield() || args.iter().any(CallArg::contains_yield)
            }
            Expr::DynamicCall { callable, args, .. } => {
                callable.contains_yield() || args.iter().any(CallArg::contains_yield)
            }
            Expr::DynamicStaticCall {
                class,
                method,
                args,
                ..
            } => {
                class.contains_yield()
                    || method.contains_yield()
                    || args.iter().any(CallArg::contains_yield)
            }
            Expr::FirstClassCallable { callable, .. } => callable.contains_yield(),
            Expr::FirstClassMemberCallable { owner, member, .. } => {
                owner.contains_yield() || member.contains_yield()
            }
            // A closure has its own suspension context; declaring it does not
            // suspend the surrounding expression.
            Expr::Closure { .. }
            | Expr::Integer(_)
            | Expr::Float(_)
            | Expr::StringLiteral(_)
            | Expr::BinaryStringLiteral(_)
            | Expr::Null
            | Expr::Bool(_)
            | Expr::Variable { .. }
            | Expr::Globals { .. }
            | Expr::CompileError { .. }
            | Expr::CompileWarning { .. }
            | Expr::CompileDeprecation { .. }
            | Expr::PostInc { .. }
            | Expr::PostDec { .. }
            | Expr::PreInc { .. }
            | Expr::PreDec { .. }
            | Expr::StaticProperty { .. }
            | Expr::ClassConstant { .. }
            | Expr::FirstClassFunctionCallable { .. }
            | Expr::Constant { .. }
            | Expr::CompilerHaltOffsetConstant { .. }
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
    Object = 5,
    Void = 6,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayElement {
    pub key: Option<Expr>,
    pub value: Expr,
    pub unpack: bool,
    /// Source line of `...` for compile-stage operand validation.
    pub unpack_line: Option<usize>,
    pub by_reference: bool,
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
    LogicalXor, // xor
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
    pub attributes: Vec<Attribute>,
    pub name: std::string::String,
    /// Declaration line used by compile-time parameter diagnostics.
    pub line: usize,
    pub default: Option<Expr>,
    pub is_variadic: bool,
    pub is_ref: bool,
    pub type_hint: Option<TypeHint>,
    /// Constructor property promotion: read visibility, optional narrower
    /// write visibility, and readonly state.
    pub promotion: Option<(Visibility, Option<Visibility>, bool)>,
    /// Full promoted-property declaration, including PHP 8.5 final/hook flags.
    pub promoted_property: Option<ClassProperty>,
    /// Hidden methods lowered from a promoted property's hook list.
    pub promotion_hooks: Vec<ClassMethod>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind {
    Class,
    Function,
    Const,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitAlias {
    pub trait_name: Option<String>,
    pub method: String,
    pub alias: Option<String>,
    pub visibility: Option<Visibility>,
    /// PHP 8.3+ permits `final` as the sole non-visibility modifier in a
    /// trait alias. Keep it distinct from an alias literally named `final`.
    pub is_final: bool,
}

/// One `TraitA::method insteadof TraitB, TraitC` precedence rule.
#[derive(Debug, Clone, PartialEq)]
pub struct TraitPrecedence {
    pub trait_name: String,
    pub method: String,
    pub instead_of: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Noop,
    HaltCompiler {
        offset: usize,
        line: usize,
    },
    Block(Vec<Stmt>),
    Label(String),
    Goto {
        name: String,
        line: usize,
    },
    Echo {
        expressions: Vec<Expr>,
        line: usize,
    },
    Assign {
        var: String,
        expr: Expr,
    },
    CoalesceAssign {
        target: Expr,
        expr: Expr,
    },
    CompoundAssign {
        target: Expr,
        op: BinOp,
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
        condition: Vec<Expr>,
        update: Vec<Expr>,
        body: Vec<Stmt>,
    },
    Function {
        line: usize,
        attributes: Vec<Attribute>,
        name: String,
        returns_by_ref: bool,
        params: Vec<Param>,
        body: Vec<Stmt>,
        return_type: Option<TypeHint>,
        generic_params: Vec<GenericParameter>,
    },
    DoWhile {
        condition: Expr,
        body: Vec<Stmt>,
    },
    Break {
        level: Option<u32>,
        line: usize,
    },
    Continue {
        level: Option<u32>,
        line: usize,
    },
    Switch {
        expr: Expr,
        cases: Vec<SwitchCase>,
    },
    Return {
        expr: Option<Expr>,
        line: usize,
    },
    ExprStmt(Expr),
    ArrayAssign {
        // $a[idx] = expr
        var: String,
        index: Expr,
        expr: Expr,
        line: usize,
    },
    NestedArrayAssign {
        // $a[first][second]..., $obj->prop[...]..., or Class::$prop[...]... = expr
        root: Expr,
        indices: Vec<Expr>,
        expr: Expr,
        line: usize,
    },
    ArrayPush {
        // $a[] = expr
        var: String,
        expr: Expr,
        line: usize,
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
        line: usize,
        array: Expr,
        value: ForeachTarget,
        key: Option<ForeachTarget>,
        by_ref: bool,
        body: Vec<Stmt>,
    },
    Unset(Vec<Expr>),
    TryCatch {
        try_body: Vec<Stmt>,
        catches: Vec<CatchClause>,
        finally_body: Option<Vec<Stmt>>,
    },
    Throw {
        expr: Expr,
        line: usize,
    },
    Class {
        line: usize,
        attributes: Vec<Attribute>,
        name: String,
        parent: Option<GenericAncestor>,
        implements: Vec<GenericAncestor>,
        is_abstract: bool,
        is_final: bool,
        is_readonly: bool,
        allow_dynamic_properties: bool,
        properties: Vec<ClassProperty>,
        constants: Vec<ClassConstant>,
        methods: Vec<ClassMethod>,
        uses: Vec<GenericAncestor>, // trait names from `use Foo<T>, Bar<U>;`
        trait_aliases: Vec<TraitAlias>,
        trait_precedences: Vec<TraitPrecedence>,
        generic_params: Vec<GenericParameter>,
    },
    Interface {
        line: usize,
        attributes: Vec<Attribute>,
        name: String,
        extends: Vec<GenericAncestor>,
        properties: Vec<ClassProperty>,
        constants: Vec<ClassConstant>,
        methods: Vec<ClassMethod>, // all public, abstract (no body)
        generic_params: Vec<GenericParameter>,
    },
    Trait {
        line: usize,
        attributes: Vec<Attribute>,
        name: String,
        properties: Vec<ClassProperty>,
        constants: Vec<ClassConstant>,
        methods: Vec<ClassMethod>,
        uses: Vec<GenericAncestor>,
        trait_aliases: Vec<TraitAlias>,
        trait_precedences: Vec<TraitPrecedence>,
        generic_params: Vec<GenericParameter>,
    },
    AssignProp {
        // $obj->prop = expr
        object: Expr,
        property: String,
        expr: Expr,
        line: usize,
    },
    AssignStaticProp {
        // ClassName::$prop = expr
        class_name: String,
        property: String,
        expr: Expr,
        line: usize,
    },
    AssignObjArrayDim {
        // $obj->prop[$key] = expr
        object: Expr,
        property: String,
        index: Expr,
        expr: Expr,
        line: usize,
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
        // Each entry carries its own kind so mixed group-use declarations can
        // import classes, functions and constants in one statement. Preserve
        // explicit-alias syntax because a same-spelling alias suppresses PHP's
        // otherwise mandatory global non-compound-name warning.
        line: usize,
        name_line: usize,
        imports: Vec<(UseKind, String, String, bool)>, // (kind, fqn, alias, explicit alias)
    },
    Const {
        line: usize,
        attributes: Vec<Attribute>,
        // const FOO = expr, BAR = expr;
        declarations: Vec<(String, Expr)>,
    },
    ListAssign {
        // list($a, $b) = expr; or [$a, $b] = expr;
        targets: Vec<ListTarget>,
        expr: Expr,
        line: usize,
    },
    Global(Vec<GlobalTarget>), // global $a, $$name, ${expr};
    StaticVar {
        // static $a = 0, $b = "";
        vars: Vec<(String, Option<Expr>)>,
        line: usize,
    },
    Enum {
        line: usize,
        attributes: Vec<Attribute>,
        name: String,
        backing_type: Option<TypeHint>,
        implements: Vec<GenericAncestor>,
        uses: Vec<GenericAncestor>,
        trait_aliases: Vec<TraitAlias>,
        cases: Vec<EnumCase>,
        /// Parsed only so the compiler can issue PHP's declaration-stage enum
        /// diagnostic instead of rejecting otherwise valid property syntax as
        /// a parser error.
        properties: Vec<ClassProperty>,
        constants: Vec<ClassConstant>,
        methods: Vec<ClassMethod>,
    },
    Include {
        path: Expr,
        is_require: bool,
        is_once: bool,
        line: usize,
    },
}

impl Stmt {
    /// Whether this statement syntactically contains a yield belonging to the
    /// current function, including one in an unreachable branch.
    /// Nested function, method, closure and class bodies own independent
    /// suspension contexts and are therefore deliberately not traversed.
    pub(crate) fn contains_yield(&self) -> bool {
        match self {
            Stmt::Echo { expressions, .. } | Stmt::Unset(expressions) => {
                expressions.iter().any(Expr::contains_yield)
            }
            Stmt::Assign { expr, .. }
            | Stmt::ArrayPush { expr, .. }
            | Stmt::Throw { expr, .. }
            | Stmt::ExprStmt(expr)
            | Stmt::Include { path: expr, .. } => expr.contains_yield(),
            Stmt::Const { declarations, .. } => declarations
                .iter()
                .any(|(_, expression)| expression.contains_yield()),
            Stmt::CoalesceAssign { target, expr }
            | Stmt::CompoundAssign { target, expr, .. }
            | Stmt::ArrayAppend { target, expr }
            | Stmt::AssignProp {
                object: target,
                expr,
                ..
            } => target.contains_yield() || expr.contains_yield(),
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                condition.contains_yield()
                    || then_body.iter().any(Stmt::contains_yield)
                    || else_body.iter().any(Stmt::contains_yield)
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                condition.contains_yield() || body.iter().any(Stmt::contains_yield)
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.iter().any(Stmt::contains_yield)
                    || condition.iter().any(Expr::contains_yield)
                    || update.iter().any(Expr::contains_yield)
                    || body.iter().any(Stmt::contains_yield)
            }
            Stmt::Switch { expr, cases } => {
                expr.contains_yield()
                    || cases.iter().any(|case| {
                        case.value.as_ref().is_some_and(Expr::contains_yield)
                            || case.body.iter().any(Stmt::contains_yield)
                    })
            }
            Stmt::Return { expr, .. } => expr.as_ref().is_some_and(Expr::contains_yield),
            Stmt::ArrayAssign { index, expr, .. } => {
                index.contains_yield() || expr.contains_yield()
            }
            Stmt::NestedArrayAssign {
                root,
                indices,
                expr,
                ..
            } => {
                root.contains_yield()
                    || indices.iter().any(Expr::contains_yield)
                    || expr.contains_yield()
            }
            Stmt::BindArrayAppendReference { target, .. } => target.contains_yield(),
            Stmt::Foreach {
                array,
                value,
                key,
                body,
                ..
            } => {
                array.contains_yield()
                    || value.contains_yield()
                    || key.as_ref().is_some_and(ForeachTarget::contains_yield)
                    || body.iter().any(Stmt::contains_yield)
            }
            Stmt::TryCatch {
                try_body,
                catches,
                finally_body,
            } => {
                try_body.iter().any(Stmt::contains_yield)
                    || catches
                        .iter()
                        .any(|catch| catch.body.iter().any(Stmt::contains_yield))
                    || finally_body
                        .as_ref()
                        .is_some_and(|body| body.iter().any(Stmt::contains_yield))
            }
            Stmt::AssignStaticProp { expr, .. } => expr.contains_yield(),
            Stmt::AssignObjArrayDim {
                object,
                index,
                expr,
                ..
            } => {
                object.contains_yield() || index.contains_yield() || expr.contains_yield()
            }
            Stmt::Namespace { body, .. } => body.iter().any(Stmt::contains_yield),
            Stmt::ListAssign { targets, expr, .. } => {
                targets.iter().any(ListTarget::contains_yield) || expr.contains_yield()
            }
            Stmt::StaticVar { vars, .. } => vars
                .iter()
                .any(|(_, value)| value.as_ref().is_some_and(Expr::contains_yield)),
            Stmt::Global(targets) => targets.iter().any(|target| match target {
                GlobalTarget::Variable(_) => false,
                GlobalTarget::Dynamic(expr) => expr.contains_yield(),
            }),
            Stmt::Block(body) => body.iter().any(Stmt::contains_yield),
            Stmt::Noop
            | Stmt::HaltCompiler { .. }
            | Stmt::Label(_)
            | Stmt::Goto { .. }
            | Stmt::Break { .. }
            | Stmt::Continue { .. }
            | Stmt::Function { .. }
            | Stmt::Class { .. }
            | Stmt::Interface { .. }
            | Stmt::Trait { .. }
            | Stmt::Declare { .. }
            | Stmt::UseDecl { .. }
            | Stmt::Enum { .. } => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatchClause {
    pub types: Vec<String>, // Exception class names (multi-catch: ExA | ExB)
    pub var: Option<String>, // Optional exception variable (PHP 8 permits catch (Exception))
    pub body: Vec<Stmt>,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Visibility {
    Public,
    Protected,
    Private,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassProperty {
    pub attributes: Vec<Attribute>,
    /// Declaration line used by compile/link diagnostics.
    pub line: usize,
    pub visibility: Visibility,
    pub set_visibility: Option<Visibility>,
    pub name: String,
    /// Source-level property contract. The ordinary runtime currently uses
    /// the erased storage model; generics metadata preserves this form for
    /// reified substitution and Reflection.
    pub type_hint: Option<TypeHint>,
    pub default: Option<Expr>,
    pub is_static: bool,
    pub is_readonly: bool,
    /// A final property cannot be redeclared by a child class.
    pub is_final: bool,
    /// The declaration used the property-level `abstract` modifier.
    pub is_abstract: bool,
    /// An explicit PHP 8.4+ `get` hook is compiled as an engine-owned method.
    pub has_get_hook: bool,
    /// The getter was declared without a body.
    pub has_abstract_get_hook: bool,
    /// An explicit PHP 8.4+ `set` hook is compiled as an engine-owned method.
    pub has_set_hook: bool,
    /// The setter was declared without a body.
    pub has_abstract_set_hook: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassConstant {
    pub attributes: Vec<Attribute>,
    pub doc_comment: Option<std::sync::Arc<str>>,
    pub line: usize,
    pub visibility: Visibility,
    pub name: String,
    pub value: Expr,
    pub type_hint: Option<TypeHint>,
    pub is_final: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumCase {
    pub attributes: Vec<Attribute>,
    pub line: usize,
    pub name: String,
    pub value: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassMethod {
    pub line: usize,
    pub attributes: Vec<Attribute>,
    pub visibility: Visibility,
    pub name: String,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub is_static: bool,
    pub is_final: bool,
    pub is_abstract: bool,
    pub returns_by_ref: bool,
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
