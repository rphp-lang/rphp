/// AST → OpArray compiler.
/// Converts parsed statements into VM instructions.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Global closure counter — ensures unique names across nested compilers.
static CLOSURE_COUNTER: AtomicU32 = AtomicU32::new(0);
/// Global anonymous-class counter — nested compilers must not reuse names.
static ANONYMOUS_CLASS_COUNTER: AtomicU32 = AtomicU32::new(0);
/// Runtime declaration markers must remain unique across separately compiled
/// includes and evals that share one executor.
static CLASS_DECLARATION_COUNTER: AtomicU32 = AtomicU32::new(0);

use super::OpArray;
use crate::generics::{
    GenericDeclarationKind, GenericInheritanceKind, GenericMetadata, GenericTypePosition,
    PendingGenericDeclaration, PendingGenericInheritance, PendingGenericMethodMetadata,
    PendingGenericUseSite,
};
use crate::parser::{
    Attribute, BinOp, CallArg, CastType, ClassConstant, ClassProperty, Expr, ForeachTarget,
    GenericAncestor, GlobalTarget, ListTarget, Param, Stmt, TypeHint, UseKind, Visibility,
};
use crate::value::{
    ObjectLayout, Value, ValueType,
    canonical_decimal_array_key as canonical_string_literal_array_key,
};
use crate::vm::instruction::{
    ARRAY_ELEMENT_REFERENCE, ARRAY_INIT_DYNAMIC_CALL_CLASS, ARRAY_INIT_HASH_HINT,
    ARRAY_UNPACK_CONSTANT_EXPRESSION, ASSIGN_CV_MOVE_SOURCE, ASSIGN_CV_REBIND,
    ASSIGN_DIM_KEY_ALREADY_NORMALIZED, ASSIGN_DIM_REFERENCE, ASSIGN_DIM_RESULT_VALUE,
    ASSIGN_DIM_UNSET_REBUILD, ASSIGN_OBJ_CLONE_WITH, ASSIGN_OBJ_MODIFY, ASSIGN_PROP_MOVE_SOURCE,
    CALL_FLAG_DEFERRED_SCALAR_CANDIDATE, CALL_FLAG_DYNAMIC_STATIC_SCOPE, CALL_FLAG_ERROR_SUPPRESS,
    CALL_FLAG_EXACT_SCALAR_ARGS, CALL_FLAG_RETURN_EXPLICITLY_IGNORED,
    CALL_USER_FUNC_ARRAY_SOURCE_UNPACK, CLASS_CONST_COMPILE_TIME_NAME,
    CLASS_CONST_CONSTANT_EXPRESSION, CLASS_CONST_DYNAMIC_NAME, CLASS_CONST_DYNAMIC_OWNER,
    CLONE_OBJ_WITH_PROPERTIES, EVAL_FLAG_ERROR_SUPPRESS, FETCH_DIM_DESTRUCTURE, FETCH_DIM_EMPTY,
    FETCH_DIM_ERROR_SUPPRESS, FETCH_DIM_ISSET, FETCH_DIM_MUTABLE, FETCH_DIM_SILENT,
    FETCH_DYNAMIC_ERROR_SUPPRESS, FETCH_DYNAMIC_RETAIN_NAME, FETCH_DYNAMIC_SILENT,
    FETCH_OBJ_COMPOUND, FETCH_OBJ_ERROR_SUPPRESS, FETCH_OBJ_INCDEC, FETCH_OBJ_MODIFY,
    FETCH_OBJ_REFERENCE_SOURCE, FETCH_OBJ_SILENT, INSTANCEOF_DYNAMIC_STATIC_SCOPE, InlineCache,
    Instruction, KnownScalarType, NEW_FLAG_DYNAMIC_CLASS_NAME, NEW_FLAG_DYNAMIC_STATIC_SCOPE,
    NEW_FLAG_UNPACKED_ARGUMENTS, OBJ_PROP_HOOK_BYPASS, OBJ_PROP_REFERENCE_BIND, OpType,
    PROPERTY_INCDEC_DECREMENT, PROPERTY_INCDEC_INCREMENT, REFERENCE_RESULT_INTERNAL,
    REFERENCE_SOURCE_MAY_BE_NONREFERENCEABLE, SEND_FLAG_GLOBALS, SEND_FLAG_NONREFERENCEABLE,
    SEND_FLAG_YIELD_SNAPSHOT, STATIC_PROP_DYNAMIC_NAME, STATIC_PROP_DYNAMIC_OWNER,
    STATIC_PROP_INDIRECT_MODIFY, STATIC_PROP_REFERENCE_BIND, STATIC_PROP_REFERENCE_FETCH,
    STATIC_PROP_SILENT, THROW_FLAG_UNHANDLED_MATCH, UNSET_DIM_NESTED,
};
use crate::vm::opcode::OpCode;

/// Stable operand, optional named-argument literal, and optional original CV
/// retained for runtime by-reference selection across a generator suspension.
type CompiledCallArg = (u16, OpType, Option<u16>, Option<u16>);

fn incdec_target_source_line(target: &Expr) -> usize {
    match target {
        Expr::Variable { line, .. }
        | Expr::DynamicVariable { line, .. }
        | Expr::Globals { line }
        | Expr::ArrayAccess { line, .. }
        | Expr::PropertyAccess { line, .. }
        | Expr::DynamicPropertyAccess { line, .. }
        | Expr::StaticProperty { line, .. }
        | Expr::DynamicNamedStaticProperty { line, .. }
        | Expr::DynamicStaticProperty { line, .. } => *line,
        _ => 0,
    }
}

fn assertion_expression_source(expr: &Expr) -> Option<String> {
    fn quote_string(value: &str) -> String {
        format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
    }

    fn render_float(value: f64) -> String {
        let rendered = value.to_string();
        if value.is_finite()
            && !rendered.contains('.')
            && !rendered.contains('e')
            && !rendered.contains('E')
        {
            format!("{rendered}.0")
        } else {
            rendered
        }
    }

    fn render_type_hint(hint: &TypeHint) -> Option<String> {
        Some(match hint {
            TypeHint::Int => "int".to_string(),
            TypeHint::Float => "float".to_string(),
            TypeHint::String => "string".to_string(),
            TypeHint::Bool => "bool".to_string(),
            TypeHint::Array => "array".to_string(),
            TypeHint::Callable => "callable".to_string(),
            TypeHint::Null => "null".to_string(),
            TypeHint::Void => "void".to_string(),
            TypeHint::Mixed => "mixed".to_string(),
            TypeHint::Never => "never".to_string(),
            TypeHint::ClassName(name) => name.clone(),
            TypeHint::Nullable(inner) => format!("?{}", render_type_hint(inner)?),
            TypeHint::Union(members) => members
                .iter()
                .map(render_type_hint)
                .collect::<Option<Vec<_>>>()?
                .join("|"),
            TypeHint::Intersection(members) => members
                .iter()
                .map(render_type_hint)
                .collect::<Option<Vec<_>>>()?
                .join("&"),
            TypeHint::GenericParameter { .. } | TypeHint::GenericApplication { .. } => return None,
        })
    }

    fn render_arguments(arguments: &[CallArg]) -> Option<String> {
        arguments
            .iter()
            .map(|argument| match argument {
                CallArg::Positional(value) => render(value, 0, false),
                CallArg::Named { name, value } => {
                    render(value, 0, false).map(|value| format!("{name}: {value}"))
                }
                CallArg::Unpack(value) => {
                    render(value, 0, false).map(|value| format!("...{value}"))
                }
            })
            .collect::<Option<Vec<_>>>()
            .map(|arguments| arguments.join(", "))
    }

    fn render_parameter(parameter: &Param) -> Option<String> {
        let mut output = String::new();
        if let Some(hint) = &parameter.type_hint {
            output.push_str(&render_type_hint(hint)?);
            output.push(' ');
        }
        if parameter.is_ref {
            output.push('&');
        }
        if parameter.is_variadic {
            output.push_str("...");
        }
        output.push('$');
        output.push_str(&parameter.name);
        if let Some(default) = &parameter.default {
            output.push_str(" = ");
            output.push_str(&render(default, 0, false)?);
        }
        Some(output)
    }

    fn visibility_source(visibility: Visibility) -> &'static str {
        match visibility {
            Visibility::Public => "public",
            Visibility::Protected => "protected",
            Visibility::Private => "private",
        }
    }

    fn render_property(property: &ClassProperty) -> Option<String> {
        if property.is_abstract
            || property.has_get_hook
            || property.has_abstract_get_hook
            || property.has_set_hook
            || property.has_abstract_set_hook
        {
            return None;
        }
        let mut parts = Vec::new();
        if property.is_final {
            parts.push("final".to_string());
        }
        parts.push(visibility_source(property.visibility).to_string());
        if let Some(set_visibility) = property.set_visibility {
            parts.push(format!("{}(set)", visibility_source(set_visibility)));
        }
        if property.is_static {
            parts.push("static".to_string());
        }
        if property.is_readonly {
            parts.push("readonly".to_string());
        }
        if let Some(hint) = &property.type_hint {
            parts.push(render_type_hint(hint)?);
        }
        let mut declaration = format!("{} ${}", parts.join(" "), property.name);
        if let Some(default) = &property.default {
            declaration.push_str(" = ");
            declaration.push_str(&render(default, 0, false)?);
        }
        declaration.push(';');
        Some(declaration)
    }

    fn indent_source(source: &str, spaces: usize) -> String {
        let indent = " ".repeat(spaces);
        source
            .split_inclusive('\n')
            .map(|line| {
                if line == "\n" {
                    line.to_string()
                } else {
                    format!("{indent}{line}")
                }
            })
            .collect()
    }

    fn render_statement(statement: &Stmt) -> Option<String> {
        match statement {
            Stmt::Noop => Some(String::new()),
            Stmt::Block(body) => {
                let body = body
                    .iter()
                    .map(render_statement)
                    .collect::<Option<Vec<_>>>()?
                    .join("\n");
                Some(format!("{{\n{}\n}}", indent_source(&body, 4)))
            }
            Stmt::Return { expr, .. } => Some(match expr {
                Some(expr) => format!("return {};", render(expr, 0, false)?),
                None => "return;".to_string(),
            }),
            Stmt::ExprStmt(expr) => Some(format!("{};", render(expr, 0, false)?)),
            Stmt::Assign { var, expr } => Some(format!("${var} = {};", render(expr, 0, false)?)),
            Stmt::Class {
                name,
                parent,
                implements,
                is_abstract,
                is_final,
                is_readonly,
                allow_dynamic_properties,
                properties,
                constants,
                methods,
                uses,
                trait_aliases,
                generic_params,
                ..
            } if parent.is_none()
                && implements.is_empty()
                && !*is_abstract
                && !*is_final
                && !*is_readonly
                && !*allow_dynamic_properties
                && constants.is_empty()
                && methods.is_empty()
                && uses.is_empty()
                && trait_aliases.is_empty()
                && generic_params.is_empty() =>
            {
                let properties = properties
                    .iter()
                    .map(render_property)
                    .collect::<Option<Vec<_>>>()?;
                let body = properties
                    .iter()
                    .map(|property| indent_source(property, 4))
                    .collect::<Vec<_>>()
                    .join("\n");
                let body = if body.is_empty() {
                    "{\n}".to_string()
                } else {
                    format!("{{\n{body}\n}}")
                };
                Some(format!("class {name} {body}"))
            }
            _ => None,
        }
    }

    fn render_block(statements: &[Stmt]) -> Option<String> {
        let mut output = String::new();
        for statement in statements {
            let rendered = render_statement(statement)?;
            if rendered.is_empty() {
                continue;
            }
            output.push_str(&indent_source(&rendered, 4));
            output.push('\n');
            if matches!(statement, Stmt::Class { .. }) {
                output.push('\n');
            }
        }
        Some(output)
    }

    fn render(expr: &Expr, parent_precedence: u8, right_child: bool) -> Option<String> {
        let (text, precedence) = match expr {
            Expr::Integer(value) => (value.to_string(), 100),
            Expr::Float(value) => (render_float(*value), 100),
            Expr::StringLiteral(value) => (quote_string(value), 100),
            Expr::Bool(value) => (value.to_string(), 100),
            Expr::Null => ("null".to_string(), 100),
            Expr::Variable { name, .. } => (format!("${name}"), 100),
            Expr::Constant(name)
                if name.eq_ignore_ascii_case("exit") || name.eq_ignore_ascii_case("die") =>
            {
                ("\\exit()".to_string(), 100)
            }
            Expr::Constant(name) => (name.clone(), 100),
            Expr::Not(value) => (format!("!{}", render(value, 80, false)?), 80),
            Expr::Cast {
                cast_type: CastType::Void,
                expr,
                ..
            } => (format!("(void){}", render(expr, 80, false)?), 80),
            Expr::FirstClassCallable(callable) => {
                let callable = render(callable, 100, false)?;
                (format!("{callable}(...)"), 100)
            }
            Expr::FirstClassFunctionCallable(name) => (format!("{name}(...)"), 100),
            Expr::FunctionCall { name, args, .. } => {
                let name = if name.eq_ignore_ascii_case("exit") || name.eq_ignore_ascii_case("die")
                {
                    "\\exit"
                } else {
                    name
                };
                (format!("{name}({})", render_arguments(args)?), 100)
            }
            Expr::New {
                class_name, args, ..
            } => (
                format!("new {class_name}({})", render_arguments(args)?),
                100,
            ),
            Expr::AnonymousNew {
                args,
                is_readonly,
                allow_dynamic_properties,
                parent,
                implements,
                properties,
                constants,
                methods,
                uses,
                trait_aliases,
                ..
            } if !*allow_dynamic_properties
                && constants.is_empty()
                && methods.is_empty()
                && uses.is_empty()
                && trait_aliases.is_empty()
                && parent
                    .as_ref()
                    .is_none_or(|ancestor| ancestor.arguments.is_empty())
                && implements
                    .iter()
                    .all(|ancestor| ancestor.arguments.is_empty()) =>
            {
                let arguments = render_arguments(args)?;
                let arguments = (!args.is_empty()).then(|| format!("({arguments})"));
                let parent = parent
                    .as_ref()
                    .map(|parent| format!(" extends {}", parent.name))
                    .unwrap_or_default();
                let implements = if implements.is_empty() {
                    String::new()
                } else {
                    format!(
                        " implements {}",
                        implements
                            .iter()
                            .map(|ancestor| ancestor.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                let properties = properties
                    .iter()
                    .map(render_property)
                    .collect::<Option<Vec<_>>>()?;
                let body = properties
                    .iter()
                    .map(|property| indent_source(property, 4))
                    .collect::<Vec<_>>()
                    .join("\n");
                let readonly = if *is_readonly { "readonly " } else { "" };
                let body = if body.is_empty() {
                    "{\n}".to_string()
                } else {
                    format!("{{\n{body}\n}}")
                };
                (
                    format!(
                        "new {readonly}class{}{}{} {body}",
                        arguments.as_deref().unwrap_or_default(),
                        parent,
                        implements,
                    ),
                    100,
                )
            }
            Expr::Instanceof { expr, class_name } => {
                let expr = render(expr, 30, false)?;
                (format!("{expr} instanceof {class_name}"), 30)
            }
            Expr::DynamicInstanceof { expr, class } => {
                let expr = render(expr, 30, false)?;
                let class = render(class, 30, true)?;
                (format!("{expr} instanceof {class}"), 30)
            }
            Expr::ArrayAccess { array, index, .. } => {
                let array = render(array, 100, false)?;
                let index = render(index, 0, false)?;
                (format!("{array}[{index}]"), 100)
            }
            Expr::DynamicCall { callable, args, .. } => {
                let callable = if matches!(callable.as_ref(), Expr::Closure { .. }) {
                    format!("({})", render(callable, 0, false)?)
                } else {
                    render(callable, 100, false)?
                };
                (format!("{callable}({})", render_arguments(args)?), 100)
            }
            Expr::Closure {
                is_static,
                returns_by_ref,
                params,
                use_vars,
                body,
                return_type,
                generic_params,
                ..
            } if generic_params.is_empty() => {
                let params = params
                    .iter()
                    .map(render_parameter)
                    .collect::<Option<Vec<_>>>()?
                    .join(", ");
                let return_type = match return_type {
                    Some(hint) => format!(": {}", render_type_hint(hint)?),
                    None => String::new(),
                };
                if body.len() == 1
                    && let Stmt::Return {
                        expr: Some(value),
                        line: 0,
                    } = &body[0]
                {
                    let static_prefix = if *is_static { "static " } else { "" };
                    let reference = if *returns_by_ref { "&" } else { "" };
                    (
                        format!(
                            "{static_prefix}fn{reference}({params}){return_type} => {}",
                            render(value, 0, false)?
                        ),
                        100,
                    )
                } else {
                    let static_prefix = if *is_static { "static " } else { "" };
                    let reference = if *returns_by_ref { " &" } else { "" };
                    let captures = if use_vars.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " use ({})",
                            use_vars
                                .iter()
                                .map(|(name, by_reference, _)| format!(
                                    "{}${name}",
                                    if *by_reference { "&" } else { "" }
                                ))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    };
                    (
                        format!(
                            "{static_prefix}function{reference} ({params}){captures}{return_type} {{\n{}}}",
                            render_block(body)?
                        ),
                        100,
                    )
                }
            }
            Expr::Assign { var, expr } => (format!("${var} = {}", render(expr, 5, true)?), 5),
            Expr::CompoundAssignExpression { target, op, expr } => {
                let operator = match op {
                    BinOp::Add => "+=",
                    BinOp::Sub => "-=",
                    BinOp::Mul => "*=",
                    BinOp::Div => "/=",
                    BinOp::Mod => "%=",
                    BinOp::Concat => ".=",
                    BinOp::Pow => "**=",
                    BinOp::BitwiseAnd => "&=",
                    BinOp::BitwiseOr => "|=",
                    BinOp::BitwiseXor => "^=",
                    BinOp::ShiftLeft => "<<=",
                    BinOp::ShiftRight => ">>=",
                    _ => return None,
                };
                let target = render(target, 5, false)?;
                let expr = render(expr, 5, true)?;
                (format!("{target} {operator} {expr}"), 5)
            }
            Expr::Match { expr, arms, .. } => {
                let mut output = format!("match ({}) {{\n", render(expr, 0, false)?);
                for arm in arms {
                    let conditions = match &arm.conditions {
                        Some(conditions) => conditions
                            .iter()
                            .map(|condition| render(condition, 0, false))
                            .collect::<Option<Vec<_>>>()?
                            .join(", "),
                        None => "default".to_string(),
                    };
                    output.push_str(&format!(
                        "    {conditions} => {},\n",
                        render(&arm.body, 0, false)?
                    ));
                }
                output.push('}');
                (output, 100)
            }
            Expr::BinaryOp { op, left, right } => {
                let (operator, precedence) = match op {
                    BinOp::Or => ("||", 10),
                    BinOp::And => ("&&", 20),
                    BinOp::BitwiseOr => ("|", 25),
                    BinOp::BitwiseXor => ("^", 26),
                    BinOp::BitwiseAnd => ("&", 27),
                    BinOp::Equal => ("==", 30),
                    BinOp::NotEqual => ("!=", 30),
                    BinOp::Identical => ("===", 30),
                    BinOp::NotIdentical => ("!==", 30),
                    BinOp::Less => ("<", 30),
                    BinOp::LessEqual => ("<=", 30),
                    BinOp::Greater => (">", 30),
                    BinOp::GreaterEqual => (">=", 30),
                    BinOp::Concat => (".", 50),
                    BinOp::ShiftLeft => ("<<", 55),
                    BinOp::ShiftRight => (">>", 55),
                    BinOp::Add => ("+", 60),
                    BinOp::Sub => ("-", 60),
                    BinOp::Mul => ("*", 70),
                    BinOp::Div => ("/", 70),
                    BinOp::Mod => ("%", 70),
                    BinOp::Pow => ("**", 80),
                    _ => return None,
                };
                let right_associative = matches!(op, BinOp::Pow);
                let left = render(left, precedence, right_associative)?;
                let right = render(right, precedence, !right_associative)?;
                (format!("{left} {operator} {right}"), precedence)
            }
            Expr::Pipe {
                input, callable, ..
            } => {
                let precedence = 40;
                let input = render(input, precedence, false)?;
                let callable = if matches!(callable.as_ref(), Expr::Closure { .. }) {
                    format!("({})", render(callable, 0, false)?)
                } else {
                    render(callable, precedence, true)?
                };
                (format!("{input} |> {callable}"), precedence)
            }
            _ => return None,
        };
        if precedence < parent_precedence || (right_child && precedence == parent_precedence) {
            Some(format!("({text})"))
        } else {
            Some(text)
        }
    }
    render(expr, 0, false).map(|expression| format!("assert({expression})"))
}

use super::{
    finalize_user_method, make_user_function_full,
    make_user_function_typed_with_return_mode as make_user_function_typed,
    make_user_function_with_args,
};
use crate::vm::function::{
    ATTRIBUTE_TARGET_PROPERTY_HOOK, AttributeArgument, AttributeDefinition,
    AttributeEvaluationScope, CallStrategy, ParamTypeHint, UserFunction,
};

#[inline]
fn attribute_method_target(name: &str) -> i64 {
    4 | if name.starts_with('$') {
        ATTRIBUTE_TARGET_PROPERTY_HOOK
    } else {
        0
    }
}

/// One declaration-time warning or deprecation emitted before the compiled
/// unit runs. The existing collection name is retained for API stability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileDeprecation {
    pub message: String,
    pub file: String,
    pub line: usize,
    pub warning: bool,
}

/// A fatal compilation error together with diagnostics emitted before it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileFailure {
    pub message: String,
    pub deprecations: Vec<CompileDeprecation>,
}

impl std::fmt::Display for CompileFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::ops::Deref for CompileFailure {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.message
    }
}

/// Result of compiling a script — main OpArray + declared functions + class defs.
pub struct CompileResult {
    pub main: OpArray,
    pub functions: Vec<(String, UserFunction)>,
    pub class_defs: Vec<ClassDef>,
    /// Named classes whose trait composition must happen at their executable
    /// declaration marker rather than during source-unit setup.
    pub runtime_class_defs: Vec<(String, ClassDef)>,
    pub constant_attributes: HashMap<String, Vec<AttributeDefinition>>,
    pub constant_expressions: HashMap<String, ConstantExpressionMetadata>,
    pub generic_metadata: GenericMetadata,
    pub deprecations: Vec<CompileDeprecation>,
}

/// Cold source expression retained only for constants whose value references
/// another symbol. PHP diagnoses deprecated dependencies when the containing
/// constant is read, even when its scalar value was folded during compilation.
#[derive(Clone, Debug)]
pub struct ConstantExpressionMetadata {
    pub expression: Expr,
    pub evaluation_scope: Rc<AttributeEvaluationScope>,
    pub source_file: String,
}

impl CompileResult {
    /// Relocate generic use-site operands after this separately compiled unit
    /// is linked into an executor-wide metadata table.
    pub fn relocate_generic_use_sites(&mut self, base: u32) -> Result<(), String> {
        if base == 0 {
            return Ok(());
        }
        relocate_op_array_generic_use_sites(&mut self.main, base)?;
        for (_, function) in &mut self.functions {
            relocate_op_array_generic_use_sites(&mut function.op_array, base)?;
        }
        for class in &mut self.class_defs {
            for (_, _, _, _, method) in &mut class.methods {
                relocate_op_array_generic_use_sites(&mut method.op_array, base)?;
            }
        }
        Ok(())
    }
}

fn relocate_op_array_generic_use_sites(op_array: &mut OpArray, base: u32) -> Result<(), String> {
    for instruction in &mut op_array.instructions {
        if instruction.opcode == OpCode::CheckGenericArgs {
            instruction.extended_value = instruction
                .extended_value
                .checked_add(base)
                .ok_or_else(|| "Generic use-site metadata index overflow".to_string())?;
        }
    }
    Ok(())
}

enum ArrayLiteralStorageHint {
    Packed,
    Hash,
    Unknown,
}

/// Prove the initial representation without speculating about dynamic keys.
/// Unknown literals keep zero-capacity packed storage and let canonical runtime
/// insertion choose, avoiding an allocation that an immediate transition
/// would discard.
fn array_literal_storage_hint(elements: &[crate::parser::ArrayElement]) -> ArrayLiteralStorageHint {
    if elements.iter().any(|element| element.unpack) {
        return ArrayLiteralStorageHint::Unknown;
    }
    if elements.iter().any(|element| {
        matches!(
            element.key.as_ref(),
            Some(Expr::StringLiteral(value))
                if canonical_string_literal_array_key(value).is_none()
        )
    }) {
        return ArrayLiteralStorageHint::Hash;
    }

    let mut next_key = 0i64;
    for element in elements {
        let key = match element.key.as_ref() {
            None => {
                next_key += 1;
                continue;
            }
            Some(Expr::Integer(key)) => *key,
            Some(Expr::StringLiteral(key)) => canonical_string_literal_array_key(key).unwrap(),
            _ => return ArrayLiteralStorageHint::Unknown,
        };

        if key == next_key {
            next_key += 1;
        } else if key < 0 || key > next_key {
            // Sparse integer literals also require hash storage, but their
            // capacity belongs to the integer index rather than the string
            // index. Keep this hint allocation-neutral for now.
            return ArrayLiteralStorageHint::Unknown;
        }
    }
    ArrayLiteralStorageHint::Packed
}

#[cfg(test)]
mod array_literal_hint_tests {
    use super::{
        ArrayLiteralStorageHint, array_literal_storage_hint, canonical_string_literal_array_key,
    };
    use crate::parser::{ArrayElement, Expr};

    fn element(key: Option<Expr>) -> ArrayElement {
        ArrayElement {
            key,
            value: Expr::Integer(1),
            unpack: false,
            unpack_line: None,
            by_reference: false,
        }
    }

    #[test]
    fn distinguishes_canonical_numeric_string_keys() {
        assert_eq!(canonical_string_literal_array_key("0"), Some(0));
        assert_eq!(canonical_string_literal_array_key("-3"), Some(-3));
        assert_eq!(canonical_string_literal_array_key("01"), None);
        assert_eq!(canonical_string_literal_array_key("-0"), None);
        assert_eq!(canonical_string_literal_array_key("name"), None);
    }

    #[test]
    fn proves_packed_hash_and_unknown_literal_storage() {
        assert!(matches!(
            array_literal_storage_hint(&[element(None), element(None)]),
            ArrayLiteralStorageHint::Packed
        ));
        assert!(matches!(
            array_literal_storage_hint(&[
                element(Some(Expr::Integer(0))),
                element(Some(Expr::StringLiteral("1".into()))),
            ]),
            ArrayLiteralStorageHint::Packed
        ));
        assert!(matches!(
            array_literal_storage_hint(&[element(Some(Expr::Integer(4)))]),
            ArrayLiteralStorageHint::Unknown
        ));
        assert!(matches!(
            array_literal_storage_hint(&[element(Some(Expr::StringLiteral("name".into())))]),
            ArrayLiteralStorageHint::Hash
        ));
        assert!(matches!(
            array_literal_storage_hint(&[element(Some(Expr::Variable {
                name: "key".into(),
                line: 0,
            }))]),
            ArrayLiteralStorageHint::Unknown
        ));
    }
}

/// Refine the conservative per-function global-access flag once every declared
/// function in the compilation unit is known.
///
/// During body compilation an `InitFcall` has to be treated as potentially
/// reaching `global`, because its target may not have been compiled yet. Here
/// direct calls can be resolved into a small call graph. Only dynamic/unknown
/// calls and chains that actually reach a `global` binding remain conservative.
fn instructions_may_access_globals(instructions: &[Instruction]) -> bool {
    instructions.iter().any(|instruction| {
        matches!(
            instruction.opcode,
            OpCode::InitFcall
                | OpCode::InitDynamicCall
                | OpCode::InitUserCall
                | OpCode::CallUserFuncArray
                | OpCode::InitMethodCall
                | OpCode::InitStaticCall
                | OpCode::InitLateStaticCall
                | OpCode::Include
                | OpCode::FetchGlobals
                | OpCode::FetchGlobal
                | OpCode::AssignGlobal
                | OpCode::UnsetGlobal
                | OpCode::BindGlobalRef
                | OpCode::AssignGlobalRef
                | OpCode::BindDynamicGlobal
        )
    })
}

fn refine_function_global_access(functions: &mut [(String, UserFunction)]) {
    let function_indices: HashMap<String, usize> = functions
        .iter()
        .enumerate()
        .map(|(index, (name, _))| (name.to_ascii_lowercase(), index))
        .collect();

    let mut direct_global_access = vec![false; functions.len()];
    let mut callees = vec![Vec::<usize>::new(); functions.len()];

    for (index, (_, function)) in functions.iter().enumerate() {
        let op_array = &function.op_array;
        direct_global_access[index] = !op_array.global_vars.is_empty()
            || op_array.instructions.iter().any(|instruction| {
                matches!(
                    instruction.opcode,
                    OpCode::FetchGlobals
                        | OpCode::FetchGlobal
                        | OpCode::AssignGlobal
                        | OpCode::UnsetGlobal
                        | OpCode::BindGlobalRef
                        | OpCode::AssignGlobalRef
                        | OpCode::BindDynamicGlobal
                )
            });

        for instruction in &op_array.instructions {
            match instruction.opcode {
                OpCode::InitFcall => {
                    let primary = op_array
                        .literals
                        .get(instruction.op2 as usize)
                        .and_then(Value::as_str)
                        .and_then(|name| function_indices.get(&name.to_ascii_lowercase()))
                        .copied();

                    // Namespaced unqualified calls fall back to the global
                    // function only when the primary target is not declared.
                    let resolved = primary.or_else(|| {
                        if instruction.extended_value == 0 {
                            return None;
                        }
                        op_array
                            .literals
                            .get(instruction.extended_value as usize)
                            .and_then(Value::as_str)
                            .and_then(|name| function_indices.get(&name.to_ascii_lowercase()))
                            .copied()
                    });

                    if let Some(callee) = resolved {
                        callees[index].push(callee);
                    } else {
                        // Unknown targets include builtins and functions loaded
                        // later via include. Keep the conservative behavior.
                        direct_global_access[index] = true;
                    }
                }
                OpCode::InitDynamicCall
                | OpCode::InitUserCall
                | OpCode::CallUserFuncArray
                | OpCode::InitMethodCall
                | OpCode::InitStaticCall
                | OpCode::InitLateStaticCall
                | OpCode::Include => {
                    direct_global_access[index] = true;
                }
                _ => {}
            }
        }
    }

    let mut may_access_globals = direct_global_access;
    loop {
        let mut changed = false;
        for index in 0..functions.len() {
            if !may_access_globals[index]
                && callees[index]
                    .iter()
                    .any(|&callee| may_access_globals[callee])
            {
                may_access_globals[index] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    for (index, (_, function)) in functions.iter_mut().enumerate() {
        function.op_array.may_access_globals = may_access_globals[index];

        // A direct, fixed-arity scalar call chain proven not to reach globals
        // can use the untyped or exact-Long scalar protocol and hot executor.
        let common = &mut function.common;
        let scalar_strategy = common.sig.declared_scalar_call_strategy();
        let can_use_fast_scalar = scalar_strategy.is_some()
            && !may_access_globals[index]
            && !common.sig.is_variadic
            && common.sig.ref_args == 0
            && common.sig.public_arity() == common.sig.required_num_args
            && function.op_array.global_vars.is_empty()
            && function.op_array.static_vars.is_empty()
            && function.op_array.try_entries.is_empty()
            && !function.op_array.is_generator;

        if can_use_fast_scalar {
            common.plan.call = scalar_strategy.unwrap();
        }
        function.scalar_long_plan = super::build_scalar_long_function_plan(function);
        function.scalar_double_plan = super::build_scalar_double_function_plan(function);
        function.composed_scalar_double_plan =
            super::build_composed_scalar_double_function_plan(function);
        function.scalar_string_plan = super::build_scalar_string_function_plan(function);
        function.composed_scalar_long_plan =
            super::build_composed_scalar_long_function_plan(function);
        function.composed_typed_long_plan =
            super::build_composed_typed_long_function_plan(function);
    }
}

fn exact_declared_scalar_type(hint: &ParamTypeHint) -> KnownScalarType {
    match hint {
        ParamTypeHint::Int => KnownScalarType::Long,
        ParamTypeHint::String => KnownScalarType::String,
        ParamTypeHint::Bool => KnownScalarType::Bool,
        // A weak `float` declaration also accepts Long in the current PHP
        // boundary semantics, so it does not prove one exact representation.
        _ => KnownScalarType::Unknown,
    }
}

fn literal_scalar_type(value: &Value) -> KnownScalarType {
    match value.value_type() {
        ValueType::Long => KnownScalarType::Long,
        ValueType::Double => KnownScalarType::Double,
        ValueType::String => KnownScalarType::String,
        ValueType::True | ValueType::False => KnownScalarType::Bool,
        _ => KnownScalarType::Unknown,
    }
}

fn declared_function_return_types(
    functions: &[(String, UserFunction)],
) -> HashMap<String, KnownScalarType> {
    functions
        .iter()
        .filter_map(|(name, function)| {
            let known = exact_declared_scalar_type(&function.common.sig.return_type_hint);
            (known != KnownScalarType::Unknown).then_some((name.to_ascii_lowercase(), known))
        })
        .collect()
}

fn declared_function_parameter_types(
    functions: &[(String, UserFunction)],
) -> HashMap<String, Vec<ParamTypeHint>> {
    functions
        .iter()
        .map(|(name, function)| {
            (
                name.to_ascii_lowercase(),
                function.common.sig.param_type_hints.clone(),
            )
        })
        .collect()
}

fn declared_function_ref_args(functions: &[(String, UserFunction)]) -> HashMap<String, u64> {
    functions
        .iter()
        .map(|(name, function)| (name.to_ascii_lowercase(), function.common.sig.ref_args))
        .collect()
}

#[derive(Clone)]
struct DeclaredMethodFacts {
    return_type: KnownScalarType,
    parameter_types: Vec<ParamTypeHint>,
    ref_args: u64,
}

/// Declaration contracts indexed by receiver class + method. Direct methods,
/// including untyped overrides, win. Missing methods inherit to a fixed point
/// because source class definitions are not guaranteed to be parent-first.
fn declared_method_facts(
    class_defs: &[ClassDef],
) -> HashMap<(String, String), DeclaredMethodFacts> {
    let mut result = HashMap::new();
    for class in class_defs {
        let class_name = class.name.to_ascii_lowercase();
        for (method_name, _, _, _, method) in &class.methods {
            result.insert(
                (class_name.clone(), method_name.to_ascii_lowercase()),
                DeclaredMethodFacts {
                    return_type: exact_declared_scalar_type(&method.common.sig.return_type_hint),
                    parameter_types: method.common.sig.param_type_hints.clone(),
                    ref_args: method.common.sig.ref_args,
                },
            );
        }
    }

    loop {
        let snapshot = result.clone();
        let mut changed = false;
        for class in class_defs {
            let Some(parent) = class.parent.as_ref() else {
                continue;
            };
            let class_name = class.name.to_ascii_lowercase();
            let parent_name = parent.to_ascii_lowercase();
            for ((owner, method_name), facts) in &snapshot {
                if owner == &parent_name
                    && !result.contains_key(&(class_name.clone(), method_name.clone()))
                {
                    result.insert((class_name.clone(), method_name.clone()), facts.clone());
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    result
}

/// PHP cannot early-link a class that consumes a trait. Its declaration may
/// emit a composition error after earlier top-level statements have already
/// run. Descendants share that runtime dependency even when they do not use a
/// trait directly.
fn runtime_class_declaration_names(class_defs: &[ClassDef]) -> HashSet<String> {
    let mut runtime = class_defs
        .iter()
        .filter(|class| {
            !class.is_anonymous()
                && !class.is_interface
                && !class.is_trait
                && !class.uses.is_empty()
        })
        .map(|class| class.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    loop {
        let mut changed = false;
        for class in class_defs {
            if class.is_anonymous() || class.is_interface || class.is_trait {
                continue;
            }
            if class
                .parent
                .as_ref()
                .is_some_and(|parent| runtime.contains(&parent.to_ascii_lowercase()))
            {
                changed |= runtime.insert(class.name.to_ascii_lowercase());
            }
        }
        if !changed {
            return runtime;
        }
    }
}

fn declared_receiver_class(
    hint: &ParamTypeHint,
    current_class: Option<&str>,
    parent_class: Option<&str>,
) -> Option<String> {
    match hint {
        ParamTypeHint::ClassName(name) => match name.to_ascii_lowercase().as_str() {
            "self" | "static" => current_class.map(str::to_ascii_lowercase),
            "parent" => parent_class.map(str::to_ascii_lowercase),
            _ => Some(name.to_ascii_lowercase()),
        },
        // A non-null method call proves the nullable value is an object on the
        // continuing path. Nullsafe calls are excluded separately.
        ParamTypeHint::Nullable(inner) => {
            declared_receiver_class(inner, current_class, parent_class)
        }
        _ => None,
    }
}

fn resolved_init_function_return_type(
    op_array: &OpArray,
    instruction: &Instruction,
    return_types: &HashMap<String, KnownScalarType>,
) -> KnownScalarType {
    let primary = op_array
        .literals
        .get(instruction.op2 as usize)
        .and_then(Value::as_str)
        .and_then(|name| return_types.get(&name.to_ascii_lowercase()))
        .copied();
    primary
        .or_else(|| {
            if instruction.extended_value == 0 {
                return None;
            }
            op_array
                .literals
                .get(instruction.extended_value as usize)
                .and_then(Value::as_str)
                .and_then(|name| return_types.get(&name.to_ascii_lowercase()))
                .copied()
        })
        .unwrap_or(KnownScalarType::Unknown)
}

fn resolved_init_function_parameter_types(
    op_array: &OpArray,
    instruction: &Instruction,
    parameter_types: &HashMap<String, Vec<ParamTypeHint>>,
) -> Option<Vec<ParamTypeHint>> {
    let primary = op_array
        .literals
        .get(instruction.op2 as usize)
        .and_then(Value::as_str)
        .and_then(|name| parameter_types.get(&name.to_ascii_lowercase()))
        .cloned();
    primary.or_else(|| {
        if instruction.extended_value == 0 {
            return None;
        }
        op_array
            .literals
            .get(instruction.extended_value as usize)
            .and_then(Value::as_str)
            .and_then(|name| parameter_types.get(&name.to_ascii_lowercase()))
            .cloned()
    })
}

fn resolved_init_function_ref_args(
    op_array: &OpArray,
    instruction: &Instruction,
    ref_args: &HashMap<String, u64>,
) -> Option<u64> {
    let primary = op_array
        .literals
        .get(instruction.op2 as usize)
        .and_then(Value::as_str)
        .and_then(|name| ref_args.get(&name.to_ascii_lowercase()))
        .copied();
    primary.or_else(|| {
        if instruction.extended_value == 0 {
            return None;
        }
        op_array
            .literals
            .get(instruction.extended_value as usize)
            .and_then(Value::as_str)
            .and_then(|name| ref_args.get(&name.to_ascii_lowercase()))
            .copied()
    })
}

fn known_argument_satisfies_hint(
    known: KnownScalarType,
    receiver_class: Option<&str>,
    hint: &ParamTypeHint,
    strict: bool,
) -> bool {
    match hint {
        ParamTypeHint::None | ParamTypeHint::Mixed => true,
        ParamTypeHint::Int => known == KnownScalarType::Long,
        ParamTypeHint::Float => {
            known == KnownScalarType::Double || (!strict && known == KnownScalarType::Long)
        }
        ParamTypeHint::String => known == KnownScalarType::String,
        ParamTypeHint::Bool => known == KnownScalarType::Bool,
        ParamTypeHint::ClassName(expected) => {
            receiver_class.is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
        }
        ParamTypeHint::Nullable(inner) => {
            known_argument_satisfies_hint(known, receiver_class, inner, strict)
        }
        ParamTypeHint::Union(types) => types
            .iter()
            .any(|member| known_argument_satisfies_hint(known, receiver_class, member, strict)),
        ParamTypeHint::Intersection(types) => types
            .iter()
            .all(|member| known_argument_satisfies_hint(known, receiver_class, member, strict)),
        _ => false,
    }
}

struct PendingScalarCallFacts {
    return_type: KnownScalarType,
    parameter_types: Option<Vec<ParamTypeHint>>,
    parameter_offset: usize,
    ref_args: Option<u64>,
    allow_exact_argument_skip: bool,
    arguments_proven: bool,
}

fn operand_scalar_type(
    op_array: &OpArray,
    slots: &[KnownScalarType],
    op_type: OpType,
    operand: u16,
) -> KnownScalarType {
    match op_type {
        OpType::Cv | OpType::Tmp | OpType::Var => slots
            .get(operand as usize)
            .copied()
            .unwrap_or(KnownScalarType::Unknown),
        OpType::Const => op_array
            .literals
            .get(operand as usize)
            .map(literal_scalar_type)
            .unwrap_or(KnownScalarType::Unknown),
        OpType::Unused => KnownScalarType::Unknown,
    }
}

/// Propagate exact scalar facts through one already-planned function body.
///
/// Function plans and quick regions are selected before this pass. Rewriting
/// only their canonical bytecode fallback therefore cannot change selection;
/// it makes ordinary execution consume the same type contract that a later
/// native-code tier will receive.
fn propagate_declared_scalar_types(
    op_array: &mut OpArray,
    reference_cvs: &[u32],
    this_offset: u32,
    param_type_hints: &[ParamTypeHint],
    ref_args: u64,
    return_types: &HashMap<String, KnownScalarType>,
    parameter_types: &HashMap<String, Vec<ParamTypeHint>>,
    function_ref_args: &HashMap<String, u64>,
    current_class: Option<&str>,
    parent_class: Option<&str>,
    method_facts: &HashMap<(String, String), DeclaredMethodFacts>,
) {
    let slot_count = (op_array.num_cvs + op_array.num_temps) as usize;
    let mut slots = vec![KnownScalarType::Unknown; slot_count];
    let mut receiver_classes = vec![None::<String>; slot_count];
    let mut directly_mutated_params = vec![false; param_type_hints.len()];
    let mut maybe_aliased_params = vec![false; param_type_hints.len()];
    let mut aliased_cvs = vec![false; op_array.num_cvs as usize];
    let mut reference_wrapped_cvs = vec![false; op_array.num_cvs as usize];
    for &cv in reference_cvs {
        if let Some(aliased) = aliased_cvs.get_mut(cv as usize) {
            *aliased = true;
        }
        if let Some(reference) = reference_wrapped_cvs.get_mut(cv as usize) {
            *reference = true;
        }
    }
    let straight_line = !op_array.instructions.iter().any(|instruction| {
        matches!(
            instruction.opcode,
            OpCode::Jmp
                | OpCode::JmpZ
                | OpCode::JmpNZ
                | OpCode::AssertCheck
                | OpCode::QuickLongLoopJmp
                | OpCode::ForeachInit
                | OpCode::ForeachNext
                | OpCode::ForeachNextPlain
                | OpCode::BindDefaultParam
                | OpCode::CheckStatic
        )
    });

    for instruction in &op_array.instructions {
        let mark_param = |params: &mut [bool], slot: u16| {
            let slot = slot as u32;
            if slot >= this_offset && slot < this_offset + param_type_hints.len() as u32 {
                params[(slot - this_offset) as usize] = true;
            }
        };
        match instruction.opcode {
            OpCode::AssignCv
            | OpCode::AssignConcat
            | OpCode::PreInc
            | OpCode::PreDec
            | OpCode::PostInc
            | OpCode::PostDec
            | OpCode::BindDefaultParam => mark_param(&mut directly_mutated_params, instruction.op1),
            OpCode::BindGlobal | OpCode::CheckStatic | OpCode::BindStatic => {
                mark_param(&mut directly_mutated_params, instruction.op1);
                mark_param(&mut maybe_aliased_params, instruction.op1);
                if let Some(aliased) = aliased_cvs.get_mut(instruction.op1 as usize) {
                    *aliased = true;
                }
                if let Some(reference) = reference_wrapped_cvs.get_mut(instruction.op1 as usize) {
                    *reference = true;
                }
            }
            OpCode::BindCvRef => {
                for cv in [instruction.op1, instruction.result] {
                    mark_param(&mut directly_mutated_params, cv);
                    mark_param(&mut maybe_aliased_params, cv);
                    if let Some(aliased) = aliased_cvs.get_mut(cv as usize) {
                        *aliased = true;
                    }
                    if let Some(reference) = reference_wrapped_cvs.get_mut(cv as usize) {
                        *reference = true;
                    }
                }
            }
            OpCode::SendRef if instruction.op1_type == OpType::Cv => {
                mark_param(&mut maybe_aliased_params, instruction.op1);
                if let Some(aliased) = aliased_cvs.get_mut(instruction.op1 as usize) {
                    *aliased = true;
                }
                if let Some(reference) = reference_wrapped_cvs.get_mut(instruction.op1 as usize) {
                    *reference = true;
                }
            }
            OpCode::SendVarEx if instruction.op1_type == OpType::Cv => {
                mark_param(&mut maybe_aliased_params, instruction.op1);
                if let Some(aliased) = aliased_cvs.get_mut(instruction.op1 as usize) {
                    *aliased = true;
                }
            }
            OpCode::ForeachNext | OpCode::ForeachNextPlain => directly_mutated_params.fill(true),
            _ => {}
        }
    }

    for (index, hint) in param_type_hints.iter().enumerate() {
        if index < 64 && ref_args & (1u64 << index) != 0 {
            let cv = this_offset as usize + index;
            if let Some(aliased) = aliased_cvs.get_mut(cv) {
                *aliased = true;
            }
            if let Some(reference) = reference_wrapped_cvs.get_mut(cv) {
                *reference = true;
            }
        }
        if !directly_mutated_params[index]
            && (straight_line || !maybe_aliased_params[index])
            && (index >= 64 || ref_args & (1u64 << index) == 0)
        {
            let cv = this_offset as usize + index;
            if cv < slots.len() {
                slots[cv] = exact_declared_scalar_type(hint);
                receiver_classes[cv] = declared_receiver_class(hint, current_class, parent_class);
            }
        }
    }
    if this_offset == 1 && !aliased_cvs.first().copied().unwrap_or(false) {
        receiver_classes[0] = current_class.map(str::to_ascii_lowercase);
    }

    let mut pending_calls = Vec::new();

    for ip in 0..op_array.instructions.len() {
        let instruction = op_array.instructions[ip];

        // A CV exposed by reference can be changed by code outside this body.
        // Forget any straight-line fact before later instructions consume it.
        match instruction.opcode {
            OpCode::SendRef if instruction.op1_type == OpType::Cv => {
                if let Some(slot) = slots.get_mut(instruction.op1 as usize) {
                    *slot = KnownScalarType::Unknown;
                }
                if let Some(slot) = receiver_classes.get_mut(instruction.op1 as usize) {
                    *slot = None;
                }
            }
            OpCode::BindGlobal | OpCode::CheckStatic | OpCode::BindStatic => {
                if let Some(slot) = slots.get_mut(instruction.op1 as usize) {
                    *slot = KnownScalarType::Unknown;
                }
                if let Some(slot) = receiver_classes.get_mut(instruction.op1 as usize) {
                    *slot = None;
                }
            }
            OpCode::BindCvRef => {
                for cv in [instruction.op1, instruction.result] {
                    if let Some(slot) = slots.get_mut(cv as usize) {
                        *slot = KnownScalarType::Unknown;
                    }
                    if let Some(slot) = receiver_classes.get_mut(cv as usize) {
                        *slot = None;
                    }
                }
            }
            OpCode::ForeachNext | OpCode::ForeachNextPlain => {
                slots.fill(KnownScalarType::Unknown);
                receiver_classes.fill(None);
            }
            // Included code and direct `$GLOBALS[...]` mutation execute
            // against the current symbol table and may replace any local.
            OpCode::Include
            | OpCode::AssignGlobal
            | OpCode::UnsetGlobal
            | OpCode::BindGlobalRef
            | OpCode::AssignGlobalRef
            | OpCode::FetchDynamicVar
            | OpCode::AssignDynamicVar
            | OpCode::UnsetDynamicVar
            | OpCode::BindDynamicVarRef
            | OpCode::AssignDynamicVarRef
            | OpCode::BindDynamicGlobal => {
                slots.fill(KnownScalarType::Unknown);
                receiver_classes.fill(None);
            }
            _ => {}
        }

        match instruction.opcode {
            OpCode::InitFcall => pending_calls.push(PendingScalarCallFacts {
                return_type: resolved_init_function_return_type(
                    op_array,
                    &instruction,
                    return_types,
                ),
                parameter_types: resolved_init_function_parameter_types(
                    op_array,
                    &instruction,
                    parameter_types,
                ),
                parameter_offset: 0,
                ref_args: resolved_init_function_ref_args(
                    op_array,
                    &instruction,
                    function_ref_args,
                ),
                allow_exact_argument_skip: true,
                arguments_proven: true,
            }),
            OpCode::InitMethodCall => {
                let nullsafe =
                    ip > 0 && op_array.instructions[ip - 1].opcode == OpCode::NullSafeCheck;
                let receiver_stable = match instruction.op1_type {
                    OpType::Cv => aliased_cvs
                        .get(instruction.op1 as usize)
                        .is_some_and(|aliased| !aliased),
                    OpType::Tmp | OpType::Var => true,
                    _ => false,
                };
                let receiver_class =
                    if matches!(instruction.op1_type, OpType::Cv | OpType::Tmp | OpType::Var) {
                        receiver_classes
                            .get(instruction.op1 as usize)
                            .and_then(|class| class.as_deref())
                    } else {
                        None
                    };
                let method_name = op_array
                    .literals
                    .get(instruction.op2 as usize)
                    .and_then(Value::as_str)
                    .map(str::to_ascii_lowercase);
                let declaration =
                    receiver_class
                        .zip(method_name.as_deref())
                        .and_then(|(class, method)| {
                            method_facts
                                .get(&(class.to_string(), method.to_string()))
                                .cloned()
                        });
                let exact_return = declaration
                    .as_ref()
                    .map(|facts| facts.return_type)
                    .filter(|known| *known != KnownScalarType::Unknown);
                let exact_parameters = declaration
                    .as_ref()
                    .map(|facts| facts.parameter_types.clone());
                let exact_ref_args = declaration.as_ref().map(|facts| facts.ref_args);
                let guarded_return = (!nullsafe && receiver_stable)
                    .then_some(exact_return)
                    .flatten();
                if let Some(return_type) = guarded_return {
                    op_array.instructions[ip].set_method_return_guard_type(return_type);
                }
                let exact_long_arguments = exact_parameters.as_ref().is_some_and(|hints| {
                    !hints.is_empty()
                        && hints.iter().all(|hint| matches!(hint, ParamTypeHint::Int))
                        && exact_ref_args == Some(0)
                });
                if exact_long_arguments {
                    op_array.instructions[ip].set_method_long_args_guard();
                }
                let allow_exact_argument_skip =
                    exact_ref_args == Some(0) && exact_parameters.is_some();
                pending_calls.push(PendingScalarCallFacts {
                    return_type: guarded_return.unwrap_or(KnownScalarType::Unknown),
                    parameter_types: exact_parameters,
                    parameter_offset: 1,
                    ref_args: exact_ref_args,
                    allow_exact_argument_skip,
                    arguments_proven: true,
                });
            }
            OpCode::InitStaticCall
            | OpCode::InitLateStaticCall
            | OpCode::InitDynamicCall
            | OpCode::InitUserCall
            | OpCode::NewObj => pending_calls.push(PendingScalarCallFacts {
                return_type: KnownScalarType::Unknown,
                parameter_types: None,
                parameter_offset: 0,
                ref_args: None,
                allow_exact_argument_skip: false,
                arguments_proven: false,
            }),
            _ => {}
        }

        let exact_operand_type = |op_type: OpType, operand: u16| {
            if op_type == OpType::Cv
                && reference_wrapped_cvs
                    .get(operand as usize)
                    .copied()
                    .unwrap_or(false)
            {
                KnownScalarType::Unknown
            } else {
                operand_scalar_type(op_array, &slots, op_type, operand)
            }
        };
        let left = exact_operand_type(instruction.op1_type, instruction.op1);
        let right = exact_operand_type(instruction.op2_type, instruction.op2);
        let argument_receiver_class =
            if matches!(instruction.op1_type, OpType::Cv | OpType::Tmp | OpType::Var) {
                receiver_classes
                    .get(instruction.op1 as usize)
                    .and_then(|class| class.as_deref())
            } else {
                None
            };
        let mut send_var_may_alias = false;
        if matches!(instruction.opcode, OpCode::SendVal) {
            if let Some(call) = pending_calls.last_mut() {
                call.arguments_proven &= call
                    .parameter_types
                    .as_ref()
                    .and_then(|hints| {
                        (instruction.op2 as usize)
                            .checked_sub(call.parameter_offset)
                            .and_then(|index| hints.get(index))
                    })
                    .is_some_and(|hint| {
                        known_argument_satisfies_hint(
                            left,
                            argument_receiver_class,
                            hint,
                            op_array.strict_types,
                        )
                    });
            }
        } else if instruction.opcode == OpCode::SendVarEx {
            if let Some(call) = pending_calls.last_mut() {
                let parameter_index = (instruction.op2 as usize).checked_sub(call.parameter_offset);
                let passed_by_reference = parameter_index
                    .and_then(|index| {
                        call.ref_args
                            .map(|mask| (index < 64) && mask & (1u64 << index) != 0)
                    })
                    .unwrap_or(true);
                if passed_by_reference {
                    call.arguments_proven = false;
                    send_var_may_alias = true;
                } else {
                    call.arguments_proven &= call
                        .parameter_types
                        .as_ref()
                        .and_then(|hints| parameter_index.and_then(|index| hints.get(index)))
                        .is_some_and(|hint| {
                            known_argument_satisfies_hint(
                                left,
                                argument_receiver_class,
                                hint,
                                op_array.strict_types,
                            )
                        });
                }
            } else {
                send_var_may_alias = true;
            }
        } else if matches!(
            instruction.opcode,
            OpCode::SendRef | OpCode::SendNamed | OpCode::SendUser
        ) {
            if let Some(call) = pending_calls.last_mut() {
                call.arguments_proven = false;
            }
        }
        let aliased_send_cv = if instruction._pad & SEND_FLAG_YIELD_SNAPSHOT != 0
            && matches!(instruction.opcode, OpCode::SendVarEx | OpCode::SendNamed)
            && (send_var_may_alias || instruction.opcode == OpCode::SendNamed)
        {
            Some(instruction.result)
        } else if send_var_may_alias && instruction.op1_type == OpType::Cv {
            Some(instruction.op1)
        } else {
            None
        };
        if let Some(aliased_send_cv) = aliased_send_cv {
            if let Some(slot) = slots.get_mut(aliased_send_cv as usize) {
                *slot = KnownScalarType::Unknown;
            }
            if let Some(slot) = receiver_classes.get_mut(aliased_send_cv as usize) {
                *slot = None;
            }
            if let Some(reference) = reference_wrapped_cvs.get_mut(aliased_send_cv as usize) {
                *reference = true;
            }
        }
        if matches!(instruction.opcode, OpCode::SendNamed | OpCode::SendUser)
            && instruction.op1_type == OpType::Cv
        {
            if let Some(reference) = reference_wrapped_cvs.get_mut(instruction.op1 as usize) {
                *reference = true;
            }
        }
        let mut result = KnownScalarType::Unknown;
        let mut result_receiver_class = (instruction.opcode == OpCode::NewObj)
            .then(|| {
                op_array
                    .literals
                    .get(instruction.op1 as usize)
                    .and_then(Value::as_str)
                    .map(str::to_ascii_lowercase)
            })
            .flatten();
        let mut exact_call_arguments = false;
        let rewritten = match instruction.opcode {
            OpCode::Add if left == KnownScalarType::Long && right == KnownScalarType::Long => {
                OpCode::Add_LongLong
            }
            OpCode::Sub if left == KnownScalarType::Long && right == KnownScalarType::Long => {
                OpCode::Sub_LongLong
            }
            OpCode::Mul if left == KnownScalarType::Long && right == KnownScalarType::Long => {
                OpCode::Mul_LongLong
            }
            OpCode::Mod if left == KnownScalarType::Long && right == KnownScalarType::Long => {
                result = KnownScalarType::Long;
                OpCode::Mod_LongLong
            }
            OpCode::BitwiseXor
                if left == KnownScalarType::Long && right == KnownScalarType::Long =>
            {
                result = KnownScalarType::Long;
                OpCode::BitwiseXor_LongLong
            }
            OpCode::BitwiseAnd
                if left == KnownScalarType::Long && right == KnownScalarType::Long =>
            {
                result = KnownScalarType::Long;
                OpCode::BitwiseAnd_LongLong
            }
            OpCode::BitwiseOr
                if left == KnownScalarType::Long && right == KnownScalarType::Long =>
            {
                result = KnownScalarType::Long;
                OpCode::BitwiseOr_LongLong
            }
            OpCode::Concat
                if left == KnownScalarType::String && right == KnownScalarType::String =>
            {
                result = KnownScalarType::String;
                OpCode::Concat_StringString
            }
            OpCode::Strlen | OpCode::Strlen_Cv if left == KnownScalarType::String => {
                result = KnownScalarType::Long;
                OpCode::Strlen_String
            }
            OpCode::Echo if left == KnownScalarType::String => OpCode::Echo_String,
            OpCode::Echo if left == KnownScalarType::Long => OpCode::Echo_Long,
            _ => instruction.opcode,
        };

        match instruction.opcode {
            OpCode::DoFcall => {
                if let Some(call) = pending_calls.pop() {
                    result = call.return_type;
                    exact_call_arguments = call.allow_exact_argument_skip
                        && call.arguments_proven
                        && call.parameter_types.is_some();
                }
            }
            OpCode::Strlen | OpCode::Strlen_Cv | OpCode::Strlen_String => {
                result = KnownScalarType::Long;
            }
            OpCode::Concat | OpCode::Concat_StringString => {
                result = KnownScalarType::String;
            }
            OpCode::Mod_LongLong
            | OpCode::BitwiseXor_LongLong
            | OpCode::BitwiseAnd_LongLong
            | OpCode::BitwiseOr_LongLong => {
                result = KnownScalarType::Long;
            }
            OpCode::BitwiseAnd | OpCode::BitwiseOr | OpCode::BitwiseXor => {
                result = if left == KnownScalarType::String && right == KnownScalarType::String {
                    KnownScalarType::String
                } else {
                    KnownScalarType::Long
                };
            }
            OpCode::BitwiseNot => {
                result = if left == KnownScalarType::String {
                    KnownScalarType::String
                } else {
                    KnownScalarType::Long
                };
            }
            OpCode::ShiftLeft | OpCode::ShiftRight => result = KnownScalarType::Long,
            OpCode::IsEqual
            | OpCode::IsNotEqual
            | OpCode::IsSmaller
            | OpCode::IsSmallerOrEqual
            | OpCode::IsIdentical
            | OpCode::IsNotIdentical
            | OpCode::Isset
            | OpCode::BoolNot
            | OpCode::Instanceof => result = KnownScalarType::Bool,
            OpCode::AssignCv if straight_line => {
                if instruction.op1_type == OpType::Cv {
                    let assigned_receiver_class =
                        if matches!(instruction.op2_type, OpType::Cv | OpType::Tmp | OpType::Var) {
                            receiver_classes
                                .get(instruction.op2 as usize)
                                .cloned()
                                .flatten()
                        } else {
                            None
                        };
                    if let Some(destination) = slots.get_mut(instruction.op1 as usize) {
                        *destination = right;
                    }
                    if let Some(destination) = receiver_classes.get_mut(instruction.op1 as usize) {
                        *destination = assigned_receiver_class;
                    }
                }
                result = right;
                result_receiver_class =
                    if matches!(instruction.op2_type, OpType::Cv | OpType::Tmp | OpType::Var) {
                        receiver_classes
                            .get(instruction.op2 as usize)
                            .cloned()
                            .flatten()
                    } else {
                        None
                    };
            }
            OpCode::Return => result = left,
            _ => {}
        }

        let rewritten_instruction = &mut op_array.instructions[ip];
        rewritten_instruction.opcode = rewritten;
        if exact_call_arguments {
            rewritten_instruction._pad |= CALL_FLAG_EXACT_SCALAR_ARGS;
        }
        rewritten_instruction.set_known_result_type(result);
        if result != KnownScalarType::Unknown
            && matches!(
                instruction.result_type,
                OpType::Cv | OpType::Tmp | OpType::Var
            )
        {
            if let Some(destination) = slots.get_mut(instruction.result as usize) {
                *destination = result;
            }
        }
        if matches!(
            instruction.result_type,
            OpType::Cv | OpType::Tmp | OpType::Var
        ) {
            if let Some(destination) = receiver_classes.get_mut(instruction.result as usize) {
                *destination = result_receiver_class;
            }
        }
    }
}

/// A single catch clause within a try entry
#[derive(Debug, Clone)]
pub struct CatchEntry {
    pub types: Vec<String>, // catch type names (e.g., ["Exception"], ["Foo", "Bar"] for multi-catch)
    pub catch_start: u32,   // instruction offset of catch body
    pub catch_cv: Option<u32>, // CV index when the catch declares an exception variable
}

/// Exception handler entry for try/catch
#[derive(Debug, Clone)]
pub struct TryEntry {
    pub try_start: u32,
    pub try_end: u32,
    pub catches: Vec<CatchEntry>, // ordered list of catch clauses
    pub finally_start: u32,       // 0xFFFFFFFF if no finally
    pub finally_end: u32,         // end marker instruction after finally body
}

/// Compiled parameter metadata from compile_params.
pub(crate) struct CompiledParams {
    pub num_args: u32,
    pub required_num_args: u32,
    pub is_variadic: bool,
    pub variadic_cv_index: u32,
    pub ref_args: u64,
    pub type_hints: Vec<crate::vm::function::ParamTypeHint>,
    pub param_names: Vec<String>,
    pub return_type_hint: crate::vm::function::ParamTypeHint,
}

/// One compiled property declaration. Instance and static tables share this
/// metadata shape while keeping their storage domains separate.
#[derive(Debug, Clone)]
pub struct PropertyDefinition {
    pub name: String,
    pub default: Option<Value>,
    pub visibility: Visibility,
    /// Write visibility when it is narrower than the read visibility.
    pub set_visibility: Option<Visibility>,
    pub declaring_class: String,
    /// Lexical class scope used by `self`/`parent` property contracts. This is
    /// normally the declaring class; trait composition rewrites it to the
    /// consuming class while preserving `declaring_class` as provenance.
    pub type_scope: String,
    /// Erased PHP runtime contract. Generic metadata retains the richer
    /// parameterized form outside this storage-oriented declaration table.
    pub type_hint: ParamTypeHint,
    pub is_readonly: bool,
    /// The erased contract is insufficient for this declaration in reified
    /// mode (for example `Box<int>`). Its full type stays interned in the
    /// executor-wide GenericMetadata graph.
    pub requires_reified_check: bool,
    /// Explicit getter hook. Hook execution stays on the cold property path.
    pub has_get_hook: bool,
    /// The getter directly accesses `$this` backing storage for this property.
    pub get_hook_is_backed: bool,
    pub has_set_hook: bool,
    pub set_hook_is_backed: bool,
    /// Runtime-local class declaration used by warmed instance-property
    /// writes. Registration rewrites inherited definitions to the concrete
    /// receiver declaration, so one cached definition pointer carries both
    /// the erased PHP contract and the bound/reified generic lookup key.
    pub(crate) generic_declaration: Option<u32>,
    /// Source declaration location used only by cold link-time diagnostics.
    /// Keep it after execution metadata so established hot-field offsets stay
    /// stable.
    pub source_file: Option<String>,
    pub source_line: usize,
    /// Source property line used only to restore declaration order after
    /// instance and static definitions have been stored separately. Link-time
    /// diagnostics intentionally keep `source_line` at the owning class line.
    pub reflection_order: usize,
    /// Reflection-only declaration metadata stays after all established
    /// property execution fields so their offsets remain stable.
    pub attributes: Vec<AttributeDefinition>,
}

impl PropertyDefinition {
    /// Internal declarations without a source-level type or readonly marker.
    pub fn new(
        name: String,
        default: Option<Value>,
        visibility: Visibility,
        declaring_class: String,
    ) -> Self {
        let has_default = default.is_some();
        let type_scope = declaring_class.clone();
        Self {
            attributes: Vec::new(),
            name,
            default,
            visibility,
            set_visibility: None,
            declaring_class,
            type_scope,
            type_hint: ParamTypeHint::None,
            is_readonly: false,
            requires_reified_check: false,
            generic_declaration: None,
            source_file: None,
            source_line: 0,
            reflection_order: 0,
            has_get_hook: false,
            get_hook_is_backed: false,
            has_set_hook: false,
            set_hook_is_backed: false,
        }
        .with_default_presence(has_default)
    }

    pub fn declared(
        name: String,
        default: Option<Value>,
        visibility: Visibility,
        declaring_class: String,
        type_hint: ParamTypeHint,
        is_readonly: bool,
        requires_reified_check: bool,
    ) -> Self {
        let has_default = default.is_some() || matches!(type_hint, ParamTypeHint::None);
        let type_scope = declaring_class.clone();
        Self {
            attributes: Vec::new(),
            name,
            default,
            visibility,
            set_visibility: None,
            declaring_class,
            type_scope,
            type_hint,
            is_readonly,
            requires_reified_check,
            generic_declaration: None,
            source_file: None,
            source_line: 0,
            reflection_order: 0,
            has_get_hook: false,
            get_hook_is_backed: false,
            has_set_hook: false,
            set_hook_is_backed: false,
        }
        .with_default_presence(has_default)
    }

    pub fn declared_with_set_visibility(
        name: String,
        default: Option<Value>,
        visibility: Visibility,
        set_visibility: Option<Visibility>,
        declaring_class: String,
        type_hint: ParamTypeHint,
        is_readonly: bool,
        requires_reified_check: bool,
    ) -> Self {
        let mut property = Self::declared(
            name,
            default,
            visibility,
            declaring_class,
            type_hint,
            is_readonly,
            requires_reified_check,
        );
        property.set_visibility = set_visibility;
        property
    }

    pub fn with_source_location(mut self, source_file: &str, source_line: usize) -> Self {
        if !source_file.is_empty() {
            self.source_file = Some(source_file.to_string());
        }
        self.source_line =
            (source_line & !Self::DECLARATION_FLAGS) | (self.source_line & Self::DECLARATION_FLAGS);
        self
    }

    pub fn with_reflection_order(mut self, source_line: usize) -> Self {
        self.reflection_order = source_line;
        self
    }

    const FINAL_FLAG: usize = 1usize << (usize::BITS - 1);
    const ABSTRACT_GET_FLAG: usize = 1usize << (usize::BITS - 2);
    const ABSTRACT_SET_FLAG: usize = 1usize << (usize::BITS - 3);
    const HAS_DEFAULT_FLAG: usize = 1usize << (usize::BITS - 4);
    const DECLARATION_FLAGS: usize = Self::FINAL_FLAG
        | Self::ABSTRACT_GET_FLAG
        | Self::ABSTRACT_SET_FLAG
        | Self::HAS_DEFAULT_FLAG;

    #[inline]
    fn with_default_presence(mut self, has_default: bool) -> Self {
        self.set_has_default(has_default);
        self
    }

    #[inline]
    pub fn has_default(&self) -> bool {
        self.source_line & Self::HAS_DEFAULT_FLAG != 0
    }

    #[inline]
    pub fn set_has_default(&mut self, has_default: bool) {
        if has_default {
            self.source_line |= Self::HAS_DEFAULT_FLAG;
        } else {
            self.source_line &= !Self::HAS_DEFAULT_FLAG;
        }
    }

    /// Finality is cold declaration metadata. Store it in the otherwise
    /// unused high bit of the source-line word so ordinary property metadata
    /// keeps its established size and hot-field layout.
    #[inline]
    pub fn is_final(&self) -> bool {
        self.source_line & Self::FINAL_FLAG != 0
    }

    #[inline]
    pub fn set_final(&mut self, is_final: bool) {
        if is_final {
            self.source_line |= Self::FINAL_FLAG;
        } else {
            self.source_line &= !Self::FINAL_FLAG;
        }
    }

    #[inline]
    pub fn abstract_get_hook(&self) -> bool {
        self.source_line & Self::ABSTRACT_GET_FLAG != 0
    }

    #[inline]
    pub fn abstract_set_hook(&self) -> bool {
        self.source_line & Self::ABSTRACT_SET_FLAG != 0
    }

    #[inline]
    pub fn set_abstract_hooks(&mut self, get: bool, set: bool) {
        self.source_line &= !(Self::ABSTRACT_GET_FLAG | Self::ABSTRACT_SET_FLAG);
        if get {
            self.source_line |= Self::ABSTRACT_GET_FLAG;
        }
        if set {
            self.source_line |= Self::ABSTRACT_SET_FLAG;
        }
    }

    #[inline]
    pub fn declaration_line(&self) -> usize {
        self.source_line & !Self::DECLARATION_FLAGS
    }

    #[inline]
    pub fn is_virtual_hook_property(&self) -> bool {
        (self.has_get_hook || self.has_set_hook)
            && !self.get_hook_is_backed
            && !self.set_hook_is_backed
    }

    #[inline]
    pub fn is_typed(&self) -> bool {
        !matches!(self.type_hint, ParamTypeHint::None)
    }
}

fn type_hint_requires_reified_check(hint: &Option<TypeHint>) -> bool {
    fn contains_application(hint: &TypeHint) -> bool {
        match hint {
            TypeHint::GenericApplication { .. } => true,
            TypeHint::GenericParameter { erased, .. } | TypeHint::Nullable(erased) => {
                contains_application(erased)
            }
            TypeHint::Union(parts) | TypeHint::Intersection(parts) => {
                parts.iter().any(contains_application)
            }
            _ => false,
        }
    }

    hint.as_ref().is_some_and(contains_application)
}

fn property_default_matches_exact(value: &Value, hint: &ParamTypeHint) -> bool {
    match hint {
        ParamTypeHint::None | ParamTypeHint::Mixed => true,
        ParamTypeHint::Int => value.value_type() == ValueType::Long,
        ParamTypeHint::Float => value.value_type() == ValueType::Double,
        ParamTypeHint::String => value.value_type() == ValueType::String,
        ParamTypeHint::Bool => matches!(value.value_type(), ValueType::True | ValueType::False),
        ParamTypeHint::Array => value.value_type() == ValueType::Array,
        ParamTypeHint::Nullable(inner) => {
            value.value_type() == ValueType::Null
                || (!matches!(inner.as_ref(), ParamTypeHint::None)
                    && property_default_matches_exact(value, inner))
        }
        ParamTypeHint::Union(parts) => parts
            .iter()
            .any(|part| property_default_matches_exact(value, part)),
        ParamTypeHint::Intersection(parts) => parts
            .iter()
            .all(|part| property_default_matches_exact(value, part)),
        ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("iterable") => {
            value.value_type() == ValueType::Array
        }
        ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("object") => {
            value.value_type() == ValueType::Object
        }
        ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("false") => {
            value.value_type() == ValueType::False
        }
        ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("true") => {
            value.value_type() == ValueType::True
        }
        ParamTypeHint::ClassName(name) => value
            .as_object()
            .is_some_and(|object| object.class_name.eq_ignore_ascii_case(name)),
        ParamTypeHint::Callable | ParamTypeHint::Void | ParamTypeHint::Never => false,
    }
}

/// Property defaults are compile-time constants and never use weak scalar
/// coercion. PHP's one widening exception is an integer default for `float`.
fn normalize_typed_declaration_default(value: Value, hint: &ParamTypeHint) -> Result<Value, Value> {
    if property_default_matches_exact(&value, hint) {
        return Ok(value);
    }
    match hint {
        ParamTypeHint::Float if value.value_type() == ValueType::Long => Ok(Value::double(
            value
                .as_long()
                .expect("checked typed property integer default") as f64,
        )),
        ParamTypeHint::Nullable(inner) if value.value_type() != ValueType::Null => {
            normalize_typed_declaration_default(value, inner)
        }
        ParamTypeHint::Union(parts) => {
            let mut rejected = value;
            for part in parts {
                match normalize_typed_declaration_default(rejected, part) {
                    Ok(value) => return Ok(value),
                    Err(value) => rejected = value,
                }
            }
            Err(rejected)
        }
        _ => Err(value),
    }
}

pub(crate) fn normalize_rebound_property_default(
    value: Value,
    definition: &PropertyDefinition,
    class: &str,
) -> Result<Value, String> {
    normalize_typed_declaration_default(value, &definition.type_hint).map_err(|value| {
        invalid_typed_declaration_default_message(
            &value,
            &definition.type_hint,
            class,
            &definition.name,
        )
    })
}

fn invalid_typed_declaration_default_message(
    value: &Value,
    hint: &ParamTypeHint,
    class: &str,
    property: &str,
) -> String {
    let declared_type = hint.property_declaration_display_name();
    if value.value_type() == ValueType::Null && !matches!(hint, ParamTypeHint::Intersection(_)) {
        let nullable_type = if matches!(hint, ParamTypeHint::Union(_)) {
            format!("{declared_type}|null")
        } else {
            format!("?{declared_type}")
        };
        return format!(
            "Default value for property of type {declared_type} may not be null. Use the nullable type {nullable_type} to allow null default value"
        );
    }
    let class = class
        .strip_prefix("class@anonymous#")
        .map_or(class, |_| "class@anonymous");
    format!(
        "Cannot use {} as default value for property {class}::${property} of type {declared_type}",
        value.diagnostic_type_name()
    )
}

fn constant_expression_references_symbol(expression: &Expr) -> bool {
    match expression {
        Expr::Constant(_)
        | Expr::ClassConstant { .. }
        | Expr::DynamicClassConstant { .. }
        | Expr::DynamicNamedClassConstant { .. } => true,
        Expr::BinaryOp { left, right, .. }
        | Expr::NullCoalesce { left, right }
        | Expr::Elvis { left, right } => {
            constant_expression_references_symbol(left)
                || constant_expression_references_symbol(right)
        }
        Expr::Not(inner)
        | Expr::UnaryPlus(inner)
        | Expr::UnaryMinus(inner)
        | Expr::BitwiseNot(inner)
        | Expr::ErrorSuppress(inner)
        | Expr::Cast { expr: inner, .. } => constant_expression_references_symbol(inner),
        Expr::Ternary {
            condition,
            then_expr,
            else_expr,
        } => {
            constant_expression_references_symbol(condition)
                || constant_expression_references_symbol(then_expr)
                || constant_expression_references_symbol(else_expr)
        }
        Expr::ArrayLiteral(elements) => elements.iter().any(|element| {
            element
                .key
                .as_ref()
                .is_some_and(constant_expression_references_symbol)
                || constant_expression_references_symbol(&element.value)
        }),
        Expr::ArrayAccess { array, index, .. } => {
            constant_expression_references_symbol(array)
                || constant_expression_references_symbol(index)
        }
        _ => false,
    }
}

/// Runtime materialization is reserved for PHP constant-expression forms that
/// this evaluator can reproduce after a define() or external class becomes
/// available. An unavailable symbol must not postpone an otherwise invalid
/// operation such as a function call from compile time to first use.
fn deferred_constant_expression_is_supported(expression: &Expr) -> bool {
    match expression {
        Expr::Integer(_)
        | Expr::Float(_)
        | Expr::StringLiteral(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Constant(_)
        | Expr::ClassConstant { .. } => true,
        Expr::MagicConstant { name, .. } => [
            "__LINE__",
            "__FILE__",
            "__DIR__",
            "__CLASS__",
            "__PROPERTY__",
        ]
        .iter()
        .any(|supported| name.eq_ignore_ascii_case(supported)),
        Expr::DynamicNamedClassConstant { constant, .. } => {
            deferred_constant_expression_is_supported(constant)
        }
        Expr::DynamicClassConstant {
            class, constant, ..
        } => {
            deferred_constant_expression_is_supported(class)
                && deferred_constant_expression_is_supported(constant)
        }
        Expr::BinaryOp { left, right, .. }
        | Expr::NullCoalesce { left, right }
        | Expr::Elvis { left, right } => {
            deferred_constant_expression_is_supported(left)
                && deferred_constant_expression_is_supported(right)
        }
        Expr::Not(inner)
        | Expr::UnaryPlus(inner)
        | Expr::UnaryMinus(inner)
        | Expr::BitwiseNot(inner) => deferred_constant_expression_is_supported(inner),
        Expr::Ternary {
            condition,
            then_expr,
            else_expr,
        } => {
            deferred_constant_expression_is_supported(condition)
                && deferred_constant_expression_is_supported(then_expr)
                && deferred_constant_expression_is_supported(else_expr)
        }
        Expr::ArrayLiteral(elements) => elements.iter().all(|element| {
            element
                .key
                .as_ref()
                .is_none_or(deferred_constant_expression_is_supported)
                && deferred_constant_expression_is_supported(&element.value)
        }),
        Expr::ArrayAccess { array, index, .. } => {
            deferred_constant_expression_is_supported(array)
                && deferred_constant_expression_is_supported(index)
        }
        _ => false,
    }
}

/// Trait property defaults using the consuming class must be re-evaluated at
/// each trait-composition boundary. `self::class` has the same rebinding
/// contract as `__CLASS__`; other declaration magic constants retain their
/// source scope and remain ordinary eager constants.
fn trait_property_default_rebinds_class(expression: &Expr) -> bool {
    match expression {
        Expr::MagicConstant { name, .. } => name.eq_ignore_ascii_case("__CLASS__"),
        Expr::ClassConstant {
            class_name,
            constant,
            ..
        } => class_name.eq_ignore_ascii_case("self") && constant.eq_ignore_ascii_case("class"),
        Expr::DynamicNamedClassConstant { constant, .. } => {
            trait_property_default_rebinds_class(constant)
        }
        Expr::DynamicClassConstant {
            class, constant, ..
        } => {
            trait_property_default_rebinds_class(class)
                || trait_property_default_rebinds_class(constant)
        }
        Expr::BinaryOp { left, right, .. }
        | Expr::NullCoalesce { left, right }
        | Expr::Elvis { left, right } => {
            trait_property_default_rebinds_class(left)
                || trait_property_default_rebinds_class(right)
        }
        Expr::Not(inner)
        | Expr::UnaryPlus(inner)
        | Expr::UnaryMinus(inner)
        | Expr::BitwiseNot(inner)
        | Expr::ErrorSuppress(inner)
        | Expr::Cast { expr: inner, .. } => trait_property_default_rebinds_class(inner),
        Expr::Ternary {
            condition,
            then_expr,
            else_expr,
        } => {
            trait_property_default_rebinds_class(condition)
                || trait_property_default_rebinds_class(then_expr)
                || trait_property_default_rebinds_class(else_expr)
        }
        Expr::ArrayLiteral(elements) => elements.iter().any(|element| {
            element
                .key
                .as_ref()
                .is_some_and(trait_property_default_rebinds_class)
                || trait_property_default_rebinds_class(&element.value)
        }),
        Expr::ArrayAccess { array, index, .. } => {
            trait_property_default_rebinds_class(array)
                || trait_property_default_rebinds_class(index)
        }
        _ => false,
    }
}

/// Only a missing global/class symbol may postpone a property default to
/// first object construction. Other constant-expression failures remain
/// compile-time diagnostics even when the expression happens to mention a
/// symbol elsewhere.
fn constant_expression_dependency_is_unavailable(reason: &str) -> bool {
    reason
        .strip_prefix("class constant ")
        .is_some_and(|reason| reason.ends_with(" is not available in this constant expression"))
        || matches!(
            reason,
            "STDIN is not available in constant expressions"
                | "STDOUT is not available in constant expressions"
                | "STDERR is not available in constant expressions"
        )
        || (reason.starts_with("expression Constant(\"")
            && reason.ends_with("\") is not a compile-time constant"))
}

#[derive(Debug, Clone)]
pub struct DeferredPropertyDefault {
    pub property_name: String,
    pub declaring_class: String,
    pub property_index: usize,
    pub expression: Box<Expr>,
    pub evaluation_scope: Rc<AttributeEvaluationScope>,
    pub source_file: String,
    pub source_line: usize,
}

/// Source expression retained only for a trait property whose default binds
/// to the current consumer. The enclosing sidecar keeps ordinary ClassDef and
/// PropertyDefinition layouts unchanged.
#[derive(Debug, Clone)]
pub struct ReboundTraitPropertyDefault {
    pub definition: DeferredPropertyDefault,
    pub is_static: bool,
}

#[derive(Debug)]
pub struct DeferredInstancePropertyDefaults {
    entries: Rc<Vec<DeferredPropertyDefault>>,
    rebound_trait_entries: Rc<Vec<ReboundTraitPropertyDefault>>,
    resolved: RefCell<Option<Rc<[Value]>>>,
}

impl DeferredInstancePropertyDefaults {
    pub(crate) fn new(entries: Vec<DeferredPropertyDefault>) -> Self {
        Self {
            entries: Rc::new(entries),
            rebound_trait_entries: Rc::new(Vec::new()),
            resolved: RefCell::new(None),
        }
    }

    pub(crate) fn with_rebound_trait_entries(
        entries: Vec<DeferredPropertyDefault>,
        rebound_trait_entries: Vec<ReboundTraitPropertyDefault>,
    ) -> Self {
        Self {
            entries: Rc::new(entries),
            rebound_trait_entries: Rc::new(rebound_trait_entries),
            resolved: RefCell::new(None),
        }
    }

    #[inline]
    pub(crate) fn entries(&self) -> Rc<Vec<DeferredPropertyDefault>> {
        Rc::clone(&self.entries)
    }

    #[inline]
    pub(crate) fn rebound_trait_entries(&self) -> Rc<Vec<ReboundTraitPropertyDefault>> {
        Rc::clone(&self.rebound_trait_entries)
    }

    #[inline]
    pub(crate) fn has_runtime_entries(&self) -> bool {
        !self.entries.is_empty()
    }

    #[inline]
    pub(crate) fn resolved(&self) -> Option<Rc<[Value]>> {
        self.resolved.borrow().clone()
    }

    #[inline]
    pub(crate) fn cache_resolved(&self, defaults: Rc<[Value]>) {
        *self.resolved.borrow_mut() = Some(defaults);
    }
}

#[derive(Debug, Clone)]
pub struct ClassConstantDefinition {
    pub name: String,
    pub value: Value,
    /// Source unit retained only for deferred magic-constant evaluation and
    /// use-site dependency diagnostics.
    pub source_file: String,
    /// Source expression retained only when dependency diagnostics or a
    /// runtime-defined constant prevent complete eager materialization.
    pub source_expression: Option<Box<Expr>>,
    pub evaluation_scope: Option<Rc<AttributeEvaluationScope>>,
    pub value_is_deferred: bool,
    /// PHP resolves class-constant dependency graphs lazily. A declaration
    /// cycle therefore links successfully and raises Error only when the
    /// affected constant is read.
    pub evaluation_error: Option<String>,
    pub visibility: Visibility,
    pub declaring_class: String,
    pub type_hint: ParamTypeHint,
    pub is_final: bool,
    pub attributes: Vec<AttributeDefinition>,
}

impl ClassConstantDefinition {
    /// Ordinary class-constant reads stay on the established cache fast path.
    /// Only declarations with a direct Deprecated marker, a dependency
    /// expression, or a deferred value need the cold use-site reporter.
    #[inline]
    pub(crate) fn requires_deprecated_use_check(&self) -> bool {
        self.value_is_deferred
            || self.source_expression.is_some()
            || self
                .attributes
                .iter()
                .any(|attribute| attribute.name.eq_ignore_ascii_case("Deprecated"))
    }
}

#[derive(Debug, Clone)]
pub struct TraitMethodAlias {
    pub trait_name: Option<String>,
    pub method: String,
    pub alias: Option<String>,
    pub visibility: Option<Visibility>,
}

#[derive(Debug, Clone)]
pub struct TraitMethodPrecedence {
    pub trait_name: String,
    pub method: String,
    pub instead_of: Vec<String>,
}

/// Deferred PHP diagnostic produced when a backed enum's case table is first
/// used through a declared constant or `from()`/`tryFrom()`. `cases()`
/// deliberately does not build that table and therefore must not surface this
/// metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumBackingValidationError {
    exception_class: &'static str,
    message: String,
}

impl EnumBackingValidationError {
    pub(crate) fn type_mismatch(actual: &str, expected: &str) -> Self {
        Self {
            exception_class: "TypeError",
            message: format!("Enum case type {actual} does not match enum backing type {expected}"),
        }
    }

    pub(crate) fn duplicate(enum_name: &str, first: &str, second: &str) -> Self {
        Self {
            exception_class: "Error",
            message: format!("Duplicate value in enum {enum_name} for cases {first} and {second}"),
        }
    }

    #[inline]
    pub(crate) fn exception_class(&self) -> &'static str {
        self.exception_class
    }

    #[inline]
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

pub struct ClassDef {
    pub name: String,
    /// Canonical source unit that declared this class-like symbol. Built-ins
    /// have no source file and expose `false` through Reflection.
    pub source_file: Option<String>,
    /// Source line of the class-like declaration for cold link diagnostics.
    /// Built-ins use zero.
    pub declaration_line: usize,
    pub parent: Option<String>,
    pub implements: Vec<String>,
    pub is_interface: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_readonly: bool,
    pub allow_dynamic_properties: bool,
    pub is_trait: bool,
    pub is_enum: bool,
    pub uses: Vec<String>, // trait names from `use Foo, Bar;`
    pub trait_aliases: Vec<TraitMethodAlias>,
    pub trait_precedences: Vec<TraitMethodPrecedence>,
    /// Instance-property declarations in deterministic layout order.
    pub properties: Vec<PropertyDefinition>,
    /// Static properties remain outside every object layout. Mutable static
    /// storage can build on this separate declaration table without widening
    /// ordinary objects.
    pub static_properties: Vec<PropertyDefinition>,
    /// Immutable class/interface/trait constants declared by this owner.
    pub constants: Vec<ClassConstantDefinition>,
    /// Shared declared-property storage-key → numeric slot layout.
    /// Rebuilt after inheritance and trait properties are merged.
    pub property_layout: std::rc::Rc<ObjectLayout>,
    /// Fully resolved initial values in property-layout order. Runtime object
    /// construction clones this immutable template instead of rebuilding it.
    pub property_defaults: std::rc::Rc<[Value]>,
    pub readonly_props: Vec<String>, // names of readonly properties
    pub methods: Vec<(String, Visibility, bool, bool, UserFunction)>, // (name, vis, is_static, is_final, func)
    /// Body-less method contracts declared by abstract classes or traits.
    /// Kept outside the callable tuple so abstract stubs cannot accidentally
    /// enter the function table as executable implementations.
    pub abstract_methods: Vec<String>,
    /// Stable numeric ID assigned at registration time. Used as inline cache key.
    /// 0 = not yet assigned (set by ExecutorGlobals::register_class).
    pub class_id: u32,
    /// Reflection-only declaration metadata stays after the existing runtime
    /// class fields so frequently read field offsets remain stable.
    pub attributes: Vec<AttributeDefinition>,
    /// Cold, compiler-proven invalid backed-enum table. The boxed sidecar keeps
    /// valid and non-enum class metadata to one nullable word.
    pub enum_backing_error: Option<Box<EnumBackingValidationError>>,
    /// Cold first-use metadata for instance defaults that depend on a global
    /// or class symbol unavailable while this source unit was compiled. The
    /// boxed sidecar leaves ordinary class metadata at one nullable word and
    /// keeps PropertyDefinition plus the immutable fast allocation template
    /// unchanged.
    pub deferred_instance_defaults: Option<Box<DeferredInstancePropertyDefaults>>,
}

impl ClassDef {
    #[inline]
    pub fn is_anonymous(&self) -> bool {
        self.name.starts_with("class@anonymous#")
    }

    /// Project the compact request-unique runtime key to PHP's public
    /// parent/interface-derived anonymous name. The bytes after NUL remain an
    /// opaque identity suffix to userland.
    pub fn anonymous_public_name(&self) -> Option<String> {
        self.is_anonymous().then(|| {
            let base = self
                .parent
                .as_deref()
                .or_else(|| self.implements.first().map(String::as_str))
                .unwrap_or("class");
            format!("{base}@anonymous\0{}", self.name)
        })
    }

    #[inline]
    pub fn method_is_abstract(&self, method_name: &str) -> bool {
        self.abstract_methods
            .iter()
            .any(|name| name.eq_ignore_ascii_case(method_name))
    }
}

/// Tracks loop context for break/continue patching
struct LoopContext {
    /// Instruction index to Jmp back to (loop start / update section).
    /// None if not yet known (do..while, for — set after body).
    continue_target: Option<usize>,
    /// Indices of Jmp instructions that need patching to after-loop
    break_patches: Vec<usize>,
    /// Indices of Jmp instructions that need patching to continue target
    continue_patches: Vec<usize>,
    /// True if this is a switch context (continue acts as break)
    is_switch: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GotoRegionKind {
    LoopOrSwitch,
    Finally,
    TryFinally,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GotoRegion {
    id: u32,
    kind: GotoRegionKind,
}

struct GotoLabel {
    instruction: u16,
    regions: Vec<GotoRegion>,
}

struct GotoPatch {
    instruction: usize,
    label: String,
    regions: Vec<GotoRegion>,
    line: usize,
}

pub struct Compiler {
    instructions: Vec<Instruction>,
    instruction_source_lines: Vec<(u32, u32)>,
    literals: Vec<Value>,
    /// Variable name → CV index
    cv_table: HashMap<String, u32>,
    /// CVs proven initialized on every path reaching the current source
    /// position. This keeps ordinary defined reads on their existing compact
    /// CV operands while maybe-undefined reads acquire a diagnostic snapshot.
    definitely_defined_cvs: HashSet<u16>,
    next_cv: u32,
    next_tmp: u32,
    /// One function-local TMP shared by every trait-bound `__CLASS__` read.
    /// The method/closure call boundary initializes it before execution.
    trait_class_scope_tmp: Option<u16>,
    /// Collected function declarations
    functions: Vec<(String, UserFunction)>,
    /// Loop context stack for break/continue
    loop_stack: Vec<LoopContext>,
    labels: HashMap<String, GotoLabel>,
    goto_patches: Vec<GotoPatch>,
    goto_regions: Vec<GotoRegion>,
    next_goto_region_id: u32,
    /// One function-local hidden continuation slot shared by all finally
    /// blocks. Functions without finally keep their existing frame shape.
    finally_jump_cv: Option<u32>,
    /// Try/catch entries
    try_entries: Vec<TryEntry>,
    /// Class definitions
    class_defs: Vec<ClassDef>,
    /// Unique runtime marker aligned one-for-one with `class_defs`, plus
    /// whether the declaration belongs to a child op-array and therefore may
    /// only be published when that function, method or closure executes. Only
    /// named class/enum declarations need a marker; traits, interfaces and
    /// anonymous classes retain their existing publication mechanisms.
    class_declaration_keys: Vec<Option<(String, bool)>>,
    /// Source-unit constant attributes are shared by nested compilers and
    /// published into one cold executor side table after compilation.
    constant_attributes: Rc<RefCell<HashMap<String, Vec<AttributeDefinition>>>>,
    constant_expressions: Rc<RefCell<HashMap<String, ConstantExpressionMetadata>>>,
    /// Cold declaration metadata. Finalized into one interned side table after
    /// compilation; never embedded in a function, frame, object or Value.
    generic_declarations: Vec<PendingGenericDeclaration>,
    /// Direct `extends`/`implements`/trait `use` bindings. Kept cold and
    /// validated when each class-like is linked.
    generic_inheritances: Vec<PendingGenericInheritance>,
    /// Globally numbered explicit type-argument sites shared by every nested
    /// compiler in this compilation unit. This is cold compiler state only.
    generic_use_sites: Rc<RefCell<Vec<PendingGenericUseSite>>>,
    /// Declaration-time deprecations shared by nested function compilers.
    compile_deprecations: Rc<RefCell<Vec<CompileDeprecation>>>,
    /// Deferred error from compile_expr (which can't return Result)
    deferred_error: Option<String>,
    /// ref_args for functions known from parent scope (inherited by child compilers)
    known_ref_args: HashMap<String, u64>,
    /// Per-file strict_types flag from `declare(strict_types=1);`
    strict_types: bool,
    /// Request startup mode for PHP's assertion construct. A negative value
    /// removes the expression; zero and one retain a runtime guard so
    /// `ini_set("zend.assertions", ...)` can toggle already-compiled code.
    zend_assertions: i8,
    /// Current namespace (None = global namespace)
    current_namespace: Option<String>,
    /// Use aliases: alias → fully qualified name
    use_map: HashMap<String, String>,
    /// Class-like aliases are case-insensitive. Keep a normalized cold index
    /// so collision validation does not turn import-heavy files quadratic.
    class_import_map: HashMap<String, String>,
    /// Function imports have a distinct, case-insensitive alias namespace.
    function_use_map: HashMap<String, String>,
    /// Constant imports have a distinct, case-sensitive alias namespace.
    constant_use_map: HashMap<String, String>,
    /// Declaration ranges belonging to the current lexical namespace block.
    /// Class/function definitions already live in their compiler vectors, so
    /// imports that follow a declaration can inspect those cold ranges without
    /// adding metadata to every declaration.
    class_declaration_scope_start: usize,
    function_declaration_scope_start: usize,
    /// Source constants do not otherwise retain one ordered declaration list.
    /// Keep their qualified names only for compile-time import collision checks.
    constant_declaration_names: Vec<String>,
    constant_declaration_scope_start: usize,
    /// Names found in a compile-time-elided conditional branch still
    /// participate in PHP's lexical import collision checks, even though no
    /// runtime declaration bytecode is emitted for that branch.
    elided_declaration_names: Vec<(UseKind, String)>,
    elided_declaration_scope_start: usize,
    /// Lexical class targets used only by generic owner metadata. Static-call
    /// bytecode keeps the original pseudo name for PHP forwarding semantics.
    lexical_static_class: Option<String>,
    lexical_static_parent: Option<String>,
    /// Trait method op arrays are shared by every consuming class, so their
    /// self/parent targets must remain dynamically keyed.
    dynamic_static_scope: bool,
    /// True if this function body contains a yield expression (makes it a generator)
    contains_yield: bool,
    /// CVs bound to global variables
    global_vars: Vec<(u32, String)>,
    /// CVs bound to static variables
    static_vars: Vec<(u32, String, Option<Value>)>,
    /// Explicit closure captures cannot be redeclared as static variables.
    closure_capture_names: HashSet<String>,
    /// Named class-like declarations compiled into a child op-array or beneath
    /// control flow must not enter the request table until execution reaches
    /// their declaration marker.
    class_declarations_are_runtime: bool,
    /// Current function name (for static variable keying)
    current_function_name: String,
    /// Property name visible to PHP 8.5's `__PROPERTY__` magic constant while
    /// compiling a property hook body. Nested closures deliberately start
    /// without this context, matching PHP's lexical boundary.
    current_property_name: Option<String>,
    /// Direct variable operands returned from a declaration using `function
    /// &name()` are acquired as references, not read in the ordinary warning
    /// context.
    returns_reference_context: bool,
    /// Declared return contract for rejecting a source-level bare `return;`
    /// during compilation. `None` means the current unit is untyped.
    return_type_context: ParamTypeHint,
    /// Source identity used by the compile-time `__FILE__` and `__DIR__`
    /// constants. Embedders may leave both empty when no file exists.
    source_file: String,
    source_directory: String,
    /// Value produced by the synthetic return at the end of this compilation
    /// unit. Included files use integer 1; ordinary scripts/functions use null.
    implicit_return_value: Value,
    /// Constants known at compile time (from `const FOO = 42;` in the same file).
    /// Used by eval_const_expr to resolve Expr::Constant in property defaults.
    known_constants: HashMap<String, Value>,
    /// Runtime materialization still belongs to a PHP constant-expression
    /// context. Nested compilers use this only while lowering a default or
    /// initializer whose value could not be fully folded.
    compiling_constant_expression: bool,
    /// Nullsafe jumps associated with a receiver TMP. A following regular
    /// postfix remains part of the same short-circuiting chain, while
    /// nullsafe expressions in arguments keep their own independent target.
    nullsafe_receiver_patches: HashMap<u16, Vec<usize>>,
}

/// Get ref_args bitmask for built-in stdlib functions.
/// Returns 0 for unknown/non-ref functions.
fn builtin_ref_args(name: &str) -> u64 {
    match name {
        "array_multisort" => u64::MAX,
        "sort"
        | "rsort"
        | "shuffle"
        | "usort"
        | "uasort"
        | "uksort"
        | "asort"
        | "arsort"
        | "ksort"
        | "krsort"
        | "array_walk"
        | "array_walk_recursive" => 0b1, // arg 0
        "array_push" | "array_unshift" => 0b1,    // arg 0
        "array_pop" | "array_shift" => 0b1,       // arg 0
        "array_splice" => 0b1,                    // arg 0
        "settype" => 0b1,                         // arg 0
        "preg_match" | "preg_match_all" => 0b100, // arg 2 (&$matches)
        "preg_replace" | "preg_replace_callback" => 0b1_0000, // arg 4 (&$count)
        "str_replace" => 0b1000,                  // arg 3 (&$count)
        "parse_str" => 0b10,                      // arg 1 (&$result)
        "extract" => 0b1,                         // arg 0 (&$array for EXTR_REFS)
        _ => 0,
    }
}

impl Compiler {
    fn property_hook_name(method: &str) -> Option<&str> {
        let (property, hook) = method.strip_prefix('$')?.split_once("::")?;
        (hook.eq_ignore_ascii_case("get") || hook.eq_ignore_ascii_case("set")).then_some(property)
    }

    fn current_hook_matches(&self, property: &str) -> bool {
        self.current_function_name
            .rsplit_once("::$")
            .and_then(|(_, suffix)| suffix.split_once("::"))
            .is_some_and(|(current, hook)| {
                current == property
                    && (hook.eq_ignore_ascii_case("get") || hook.eq_ignore_ascii_case("set"))
            })
    }

    fn nullsafe_chain_line(expr: &Expr) -> Option<usize> {
        match expr {
            Expr::PropertyAccess {
                object,
                nullsafe,
                line,
                ..
            }
            | Expr::DynamicPropertyAccess {
                object,
                nullsafe,
                line,
                ..
            }
            | Expr::MethodCall {
                object,
                nullsafe,
                line,
                ..
            } => (*nullsafe)
                .then_some(*line)
                .or_else(|| Self::nullsafe_chain_line(object)),
            Expr::ArrayAccess { array, .. } => Self::nullsafe_chain_line(array),
            Expr::DynamicStaticProperty { class, .. } | Expr::DynamicStaticCall { class, .. } => {
                Self::nullsafe_chain_line(class)
            }
            _ => None,
        }
    }

    /// PHP treats a braced name produced by a constant expression differently
    /// from a runtime string equal to `class`: the former remains an ordinary
    /// case-sensitive constant lookup, while the latter resolves `::class`.
    fn is_compile_time_class_constant_name(expr: &Expr) -> bool {
        match expr {
            Expr::Integer(_)
            | Expr::Float(_)
            | Expr::StringLiteral(_)
            | Expr::MagicConstant { .. }
            | Expr::Bool(_)
            | Expr::Null
            | Expr::ClassConstant { .. } => true,
            Expr::BinaryOp { left, right, .. }
            | Expr::NullCoalesce { left, right }
            | Expr::Elvis { left, right } => {
                Self::is_compile_time_class_constant_name(left)
                    && Self::is_compile_time_class_constant_name(right)
            }
            Expr::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                Self::is_compile_time_class_constant_name(condition)
                    && Self::is_compile_time_class_constant_name(then_expr)
                    && Self::is_compile_time_class_constant_name(else_expr)
            }
            Expr::UnaryPlus(inner)
            | Expr::UnaryMinus(inner)
            | Expr::Not(inner)
            | Expr::BitwiseNot(inner)
            | Expr::Cast { expr: inner, .. } => Self::is_compile_time_class_constant_name(inner),
            Expr::ArrayLiteral(elements) => elements.iter().all(|element| {
                !element.by_reference
                    && element
                        .key
                        .as_ref()
                        .is_none_or(Self::is_compile_time_class_constant_name)
                    && Self::is_compile_time_class_constant_name(&element.value)
            }),
            Expr::ArrayAccess { array, index, .. } => {
                Self::is_compile_time_class_constant_name(array)
                    && Self::is_compile_time_class_constant_name(index)
            }
            _ => false,
        }
    }

    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            instruction_source_lines: Vec::new(),
            literals: Vec::new(),
            cv_table: HashMap::new(),
            definitely_defined_cvs: HashSet::new(),
            next_cv: 0,
            next_tmp: 0,
            trait_class_scope_tmp: None,
            functions: Vec::new(),
            loop_stack: Vec::new(),
            labels: HashMap::new(),
            goto_patches: Vec::new(),
            goto_regions: Vec::new(),
            next_goto_region_id: 0,
            finally_jump_cv: None,
            try_entries: Vec::new(),
            class_defs: Vec::new(),
            class_declaration_keys: Vec::new(),
            constant_attributes: Rc::new(RefCell::new(HashMap::new())),
            constant_expressions: Rc::new(RefCell::new(HashMap::new())),
            generic_declarations: Vec::new(),
            generic_inheritances: Vec::new(),
            generic_use_sites: Rc::new(RefCell::new(Vec::new())),
            compile_deprecations: Rc::new(RefCell::new(Vec::new())),
            deferred_error: None,
            known_ref_args: HashMap::new(),
            strict_types: false,
            zend_assertions: 1,
            current_namespace: None,
            use_map: HashMap::new(),
            class_import_map: HashMap::new(),
            function_use_map: HashMap::new(),
            constant_use_map: HashMap::new(),
            class_declaration_scope_start: 0,
            function_declaration_scope_start: 0,
            constant_declaration_names: Vec::new(),
            constant_declaration_scope_start: 0,
            elided_declaration_names: Vec::new(),
            elided_declaration_scope_start: 0,
            lexical_static_class: None,
            lexical_static_parent: None,
            dynamic_static_scope: false,
            contains_yield: false,
            global_vars: Vec::new(),
            static_vars: Vec::new(),
            closure_capture_names: HashSet::new(),
            class_declarations_are_runtime: false,
            current_function_name: String::new(),
            current_property_name: None,
            returns_reference_context: false,
            return_type_context: ParamTypeHint::None,
            source_file: String::new(),
            source_directory: String::new(),
            implicit_return_value: Value::null(),
            known_constants: HashMap::new(),
            compiling_constant_expression: false,
            nullsafe_receiver_patches: HashMap::new(),
        }
    }

    fn take_nullsafe_receiver_patches(&mut self, operand: u16, op_type: OpType) -> Vec<usize> {
        if op_type == OpType::Tmp {
            self.nullsafe_receiver_patches
                .remove(&operand)
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    fn publish_nullsafe_receiver_patches(&mut self, result: u16, patches: Vec<usize>) {
        if patches.is_empty() {
            return;
        }
        let target = self.instructions.len() as u16;
        for &index in &patches {
            self.instructions[index].op2 = target;
        }
        self.nullsafe_receiver_patches.insert(result, patches);
    }

    pub fn with_source_context(
        mut self,
        file: impl Into<String>,
        directory: impl Into<String>,
    ) -> Self {
        self.source_file = file.into();
        self.source_directory = directory.into();
        self
    }

    pub fn with_zend_assertions(mut self, mode: i8) -> Self {
        self.zend_assertions = mode.clamp(-1, 1);
        self
    }

    pub fn with_source_path(self, path: impl Into<String>) -> Self {
        let file = path.into();
        let directory = std::path::Path::new(&file)
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.with_source_context(file, directory)
    }

    pub fn with_implicit_return_value(mut self, value: Value) -> Self {
        self.implicit_return_value = value;
        self
    }

    pub fn with_known_constants(mut self, constants: HashMap<String, Value>) -> Self {
        self.known_constants = constants;
        self
    }

    pub(crate) fn with_lexical_class_scope(
        mut self,
        class: Option<String>,
        parent: Option<String>,
    ) -> Self {
        self.lexical_static_class = class;
        self.lexical_static_parent = parent;
        self
    }

    /// Emit source provenance without widening the compact instruction. Only
    /// opcodes whose location is already observable use this path; zero stays
    /// the explicit marker for synthetic bytecode.
    fn push_instruction_at_line(&mut self, instruction: Instruction, line: usize) {
        let is_call = instruction.opcode == OpCode::DoFcall;
        let instruction_index = self.instructions.len();
        self.instructions.push(instruction);
        if line != 0 {
            self.instruction_source_lines.push((
                u32::try_from(instruction_index).unwrap_or(u32::MAX),
                u32::try_from(line).unwrap_or(u32::MAX),
            ));
        }
        if is_call {
            self.invalidate_reentrant_definitions();
        }
    }

    fn record_last_instruction_source_line(&mut self, line: usize) {
        let Some(instruction_index) = self.instructions.len().checked_sub(1) else {
            return;
        };
        if line != 0
            && self
                .instruction_source_lines
                .last()
                .is_none_or(|(last_index, _)| {
                    usize::try_from(*last_index).unwrap_or(usize::MAX) < instruction_index
                })
        {
            self.instruction_source_lines.push((
                u32::try_from(instruction_index).unwrap_or(u32::MAX),
                u32::try_from(line).unwrap_or(u32::MAX),
            ));
        }
    }

    fn mark_last_property_incdec_writeback(&mut self, increment: bool) {
        let writeback = self
            .instructions
            .last_mut()
            .expect("property inc/dec emits a writeback instruction");
        debug_assert!(matches!(
            writeback.opcode,
            OpCode::AssignObjProp | OpCode::AssignStaticProp | OpCode::AssignLateStaticProp
        ));
        writeback._pad |= if increment {
            PROPERTY_INCDEC_INCREMENT
        } else {
            PROPERTY_INCDEC_DECREMENT
        };
    }

    fn invalidate_reentrant_definitions(&mut self) {
        if self.current_function_name.is_empty() {
            // Top-level CVs are the global symbol table. Any invoked function
            // or error handler can reach them through `global` or `$GLOBALS`.
            self.definitely_defined_cvs.clear();
        }
    }

    fn materialize_source_lines(&self) -> Vec<(u32, u32)> {
        debug_assert!(
            self.instruction_source_lines
                .windows(2)
                .all(|pair| pair[0].0 < pair[1].0)
        );
        self.instruction_source_lines.clone()
    }

    fn materialize_source_lines_with_declaration(&self, line: usize) -> Vec<(u32, u32)> {
        let mut lines = self.materialize_source_lines();
        if line != 0 {
            lines.push((u32::MAX, u32::try_from(line).unwrap_or(u32::MAX)));
        }
        lines
    }

    fn child_compiler(&self) -> Self {
        let mut child = Self::new();
        child.class_declarations_are_runtime = true;
        child.generic_use_sites = Rc::clone(&self.generic_use_sites);
        child.compile_deprecations = Rc::clone(&self.compile_deprecations);
        child.constant_attributes = Rc::clone(&self.constant_attributes);
        child.constant_expressions = Rc::clone(&self.constant_expressions);
        // Nested op arrays still compile in the same file scope. Keeping this
        // context here prevents methods and closures from silently losing
        // namespace aliases or strict-types semantics when their bytecode is
        // emitted by a fresh compiler instance.
        child.strict_types = self.strict_types;
        child.zend_assertions = self.zend_assertions;
        child.current_namespace = self.current_namespace.clone();
        child.use_map = self.use_map.clone();
        child.class_import_map = self.class_import_map.clone();
        child.function_use_map = self.function_use_map.clone();
        child.constant_use_map = self.constant_use_map.clone();
        child.lexical_static_class = self.lexical_static_class.clone();
        child.lexical_static_parent = self.lexical_static_parent.clone();
        child.dynamic_static_scope = self.dynamic_static_scope;
        child.source_file = self.source_file.clone();
        child.source_directory = self.source_directory.clone();
        child.known_constants = self.known_constants.clone();
        child
    }

    fn compile_constant_expression(&mut self, expr: &Expr) -> (u16, OpType) {
        let previous = self.compiling_constant_expression;
        self.compiling_constant_expression = true;
        let result = self.compile_expr(expr);
        self.compiling_constant_expression = previous;
        result
    }

    fn magic_constant_value(&self, name: &str, line: usize) -> Value {
        match name.to_ascii_uppercase().as_str() {
            "__LINE__" => Value::long(line as i64),
            "__FILE__" => Value::string(self.source_file.clone()),
            "__DIR__" => Value::string(self.source_directory.clone()),
            "__NAMESPACE__" => Value::string(self.current_namespace.clone().unwrap_or_default()),
            "__CLASS__" => Value::string(self.lexical_static_class.clone().unwrap_or_default()),
            "__PROPERTY__" => Value::string(self.current_property_name.clone().unwrap_or_default()),
            "__TRAIT__" => Value::string(if self.dynamic_static_scope {
                self.lexical_static_class.clone().unwrap_or_default()
            } else {
                String::new()
            }),
            "__FUNCTION__" => {
                let function = if self.current_function_name.starts_with("__closure_") {
                    self.declaration_diagnostic_name()
                } else if let Some((_, hook)) = self.current_function_name.split_once("::$") {
                    format!("${hook}")
                } else if self.lexical_static_class.is_some() {
                    self.current_function_name.rsplit_once("::").map_or_else(
                        || self.current_function_name.clone(),
                        |(_, name)| name.into(),
                    )
                } else {
                    self.current_function_name.clone()
                };
                Value::string(function)
            }
            "__METHOD__" => {
                Value::string(if self.current_function_name.starts_with("__closure_") {
                    self.declaration_diagnostic_name()
                } else {
                    self.current_function_name.clone()
                })
            }
            _ => Value::null(),
        }
    }

    fn record_generic_declaration(
        &mut self,
        kind: GenericDeclarationKind,
        owner: String,
        parameters: &[crate::parser::GenericParameter],
        value_parameters: Option<&[Param]>,
        return_type: Option<&TypeHint>,
    ) {
        if parameters.is_empty() {
            return;
        }
        let is_constructor = kind == GenericDeclarationKind::Method
            && owner
                .rsplit_once("::")
                .is_some_and(|(_, method)| method.eq_ignore_ascii_case("__construct"));
        let mut variance_uses = Vec::new();
        if !is_constructor {
            variance_uses.extend(value_parameters.unwrap_or_default().iter().filter_map(
                |parameter| {
                    parameter
                        .type_hint
                        .clone()
                        .map(|hint| (hint, GenericTypePosition::Contravariant, false))
                },
            ));
            if let Some(return_type) = return_type {
                variance_uses.push((return_type.clone(), GenericTypePosition::Covariant, false));
            }
        }
        self.generic_declarations.push(PendingGenericDeclaration {
            kind,
            owner,
            parameters: parameters.to_vec(),
            value_parameters: value_parameters
                .unwrap_or_default()
                .iter()
                .map(|parameter| parameter.type_hint.clone())
                .collect(),
            return_type: return_type.cloned(),
            properties: Vec::new(),
            variance_uses,
            methods: Vec::new(),
        });
    }

    fn record_generic_class_declaration(
        &mut self,
        kind: GenericDeclarationKind,
        owner: String,
        parameters: &[crate::parser::GenericParameter],
        properties: &[crate::parser::ClassProperty],
        methods: &[crate::parser::ClassMethod],
    ) {
        let mut property_types = properties
            .iter()
            .filter_map(|property| {
                property
                    .type_hint
                    .clone()
                    .map(|hint| (property.name.clone(), hint, property.is_static))
            })
            .collect::<Vec<_>>();
        if let Some(constructor) = methods
            .iter()
            .find(|method| method.name.eq_ignore_ascii_case("__construct"))
        {
            property_types.extend(constructor.params.iter().filter_map(|parameter| {
                if parameter.promotion.is_none() {
                    return None;
                }
                parameter
                    .type_hint
                    .clone()
                    .map(|hint| (parameter.name.clone(), hint, false))
            }));
        }
        let mut variance_uses = properties
            .iter()
            .filter_map(|property| {
                property.type_hint.clone().map(|hint| {
                    (
                        hint,
                        if property.is_readonly {
                            GenericTypePosition::Covariant
                        } else {
                            GenericTypePosition::Invariant
                        },
                        property.is_static,
                    )
                })
            })
            .collect::<Vec<_>>();
        for method in methods {
            for parameter in &method.generic_params {
                variance_uses.extend(
                    parameter
                        .bound
                        .iter()
                        .chain(parameter.default.iter())
                        .cloned()
                        .map(|hint| (hint, GenericTypePosition::Invariant, method.is_static)),
                );
            }
            if method.name.eq_ignore_ascii_case("__construct") {
                variance_uses.extend(method.params.iter().filter_map(|parameter| {
                    let (_, _, is_readonly) = parameter.promotion?;
                    parameter.type_hint.clone().map(|hint| {
                        (
                            hint,
                            if is_readonly {
                                GenericTypePosition::Covariant
                            } else {
                                GenericTypePosition::Invariant
                            },
                            false,
                        )
                    })
                }));
                continue;
            }
            variance_uses.extend(method.params.iter().filter_map(|parameter| {
                parameter
                    .type_hint
                    .clone()
                    .map(|hint| (hint, GenericTypePosition::Contravariant, method.is_static))
            }));
            if let Some(return_type) = &method.return_type {
                variance_uses.push((
                    return_type.clone(),
                    GenericTypePosition::Covariant,
                    method.is_static,
                ));
            }
        }
        self.generic_declarations.push(PendingGenericDeclaration {
            kind,
            owner,
            parameters: parameters.to_vec(),
            value_parameters: Vec::new(),
            return_type: None,
            properties: property_types,
            variance_uses,
            methods: methods
                .iter()
                // Property hooks reuse method bytecode and ordinary method
                // LSP validation, but they are not generic class methods.
                .filter(|method| {
                    !(method.name.starts_with('$')
                        && (method.name.ends_with("::get") || method.name.ends_with("::set")))
                })
                .map(|method| PendingGenericMethodMetadata {
                    name: method.name.clone(),
                    parameters: method.generic_params.clone(),
                    value_parameters: method
                        .params
                        .iter()
                        .map(|parameter| parameter.type_hint.clone())
                        .collect(),
                    return_type: method.return_type.clone(),
                    required_parameters: method
                        .params
                        .iter()
                        .filter(|parameter| parameter.default.is_none() && !parameter.is_variadic)
                        .count() as u16,
                    is_variadic: method
                        .params
                        .last()
                        .is_some_and(|parameter| parameter.is_variadic),
                    is_static: method.is_static,
                })
                .collect(),
        });
    }

    fn record_generic_inheritances(
        &mut self,
        owner: &str,
        owner_parameters: &[crate::parser::GenericParameter],
        kind: GenericInheritanceKind,
        ancestors: &[GenericAncestor],
    ) {
        let owner_parameters = owner_parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .collect::<Vec<_>>();
        for ancestor in ancestors {
            self.generic_inheritances.push(PendingGenericInheritance {
                kind,
                owner: owner.to_string(),
                ancestor: self.resolve_name(&ancestor.name),
                owner_parameters: owner_parameters.clone(),
                arguments: ancestor
                    .arguments
                    .iter()
                    .map(|argument| self.resolve_generic_type_names(argument))
                    .collect(),
            });
        }
    }

    fn resolve_generic_type_names(&self, hint: &TypeHint) -> TypeHint {
        match hint {
            TypeHint::ClassName(name) => match name.as_str() {
                "self" | "parent" | "static" => hint.clone(),
                _ => TypeHint::ClassName(self.resolve_name(name)),
            },
            TypeHint::GenericParameter { name, erased } => TypeHint::GenericParameter {
                name: name.clone(),
                erased: Box::new(self.resolve_generic_type_names(erased)),
            },
            TypeHint::GenericApplication { base, arguments } => TypeHint::GenericApplication {
                base: match base.as_str() {
                    "self" | "parent" | "static" => base.clone(),
                    _ => self.resolve_name(base),
                },
                arguments: arguments
                    .iter()
                    .map(|argument| self.resolve_generic_type_names(argument))
                    .collect(),
            },
            TypeHint::Nullable(inner) => {
                TypeHint::Nullable(Box::new(self.resolve_generic_type_names(inner)))
            }
            TypeHint::Union(parts) => TypeHint::Union(
                parts
                    .iter()
                    .map(|part| self.resolve_generic_type_names(part))
                    .collect(),
            ),
            TypeHint::Intersection(parts) => TypeHint::Intersection(
                parts
                    .iter()
                    .map(|part| self.resolve_generic_type_names(part))
                    .collect(),
            ),
            concrete => concrete.clone(),
        }
    }

    fn record_generic_use_site(&mut self, arguments: &[crate::parser::TypeHint]) -> u32 {
        let mut use_sites = self.generic_use_sites.borrow_mut();
        let index = use_sites.len() as u32;
        use_sites.push(PendingGenericUseSite {
            arguments: arguments.to_vec(),
        });
        index
    }

    fn emit_generic_check(
        &mut self,
        opcode: OpCode,
        kind: GenericDeclarationKind,
        arguments: &[crate::parser::TypeHint],
        static_owner: Option<&str>,
        owner_op: u16,
        owner_type: OpType,
        secondary_op: u16,
        secondary_type: OpType,
    ) -> bool {
        if arguments.is_empty() {
            return false;
        }
        if static_owner
            .is_some_and(|owner| self.static_generic_use_is_fully_proven(kind, owner, arguments))
        {
            return false;
        }
        let use_site = self.record_generic_use_site(arguments);
        debug_assert!(matches!(
            opcode,
            OpCode::CheckGenericArgs | OpCode::CheckLateStaticGenericArgs
        ));
        let mut check = Instruction::new(opcode);
        check.op1 = owner_op;
        check.op1_type = owner_type;
        check.op2 = secondary_op;
        check.op2_type = secondary_type;
        check.extended_value = use_site;
        check._pad = kind as u16;
        self.instructions.push(check);
        true
    }

    /// Statically named declarations in the same compilation unit can prove
    /// both their RFC arity/bounds and, when reified is enabled, whether the
    /// substituted call contract is identical to bound erasure. Such sites
    /// need no runtime opcode at all.
    fn static_generic_use_is_fully_proven(
        &self,
        kind: GenericDeclarationKind,
        owner: &str,
        arguments: &[TypeHint],
    ) -> bool {
        let Some(declaration) = self.generic_declarations.iter().find(|declaration| {
            declaration.kind == kind && declaration.owner.eq_ignore_ascii_case(owner)
        }) else {
            return false;
        };
        let metadata = GenericMetadata::compile(
            vec![declaration.clone()],
            vec![PendingGenericUseSite {
                arguments: arguments.to_vec(),
            }],
        );
        if metadata
            .resolve_binding(kind, owner, 0, |actual, bound| {
                actual.eq_ignore_ascii_case(bound)
            })
            .is_err()
        {
            return false;
        }
        if !crate::generics::GenericRuntimeCapabilities::CONFIGURED.reified {
            return true;
        }
        // Reified construction has observable instance identity even when its
        // constructor signature happens to equal bound erasure. Keep the cold
        // binding opcode so properties, cloning and later Reflection can use
        // the canonical argument tuple.
        if kind == GenericDeclarationKind::Class {
            return false;
        }

        let mut effective = arguments.to_vec();
        for parameter in declaration.parameters.iter().skip(effective.len()) {
            let Some(default) = parameter.default.clone() else {
                return false;
            };
            effective.push(default);
        }
        let same_runtime_contract = |hint: &TypeHint| {
            // Erasing a named application hides relationships such as
            // `Box<T>` -> `Box<int>`. Reified mode must retain the call-site
            // check even though the executable PHP hint remains `Box`.
            if Self::generic_application_depends_on_parameter(hint) {
                return false;
            }
            let erased = self.convert_type_hint(&Some(hint.clone()));
            let substituted =
                self.substitute_generic_hint(hint, &declaration.parameters, &effective);
            erased == self.convert_type_hint(&Some(substituted))
        };
        declaration
            .value_parameters
            .iter()
            .flatten()
            .all(same_runtime_contract)
            && declaration
                .return_type
                .as_ref()
                .is_none_or(same_runtime_contract)
    }

    fn generic_application_depends_on_parameter(hint: &TypeHint) -> bool {
        match hint {
            TypeHint::GenericApplication { arguments, .. } => arguments
                .iter()
                .any(Self::type_hint_contains_generic_parameter),
            TypeHint::Nullable(inner) => Self::generic_application_depends_on_parameter(inner),
            TypeHint::Union(parts) | TypeHint::Intersection(parts) => parts
                .iter()
                .any(Self::generic_application_depends_on_parameter),
            _ => false,
        }
    }

    fn type_hint_contains_generic_parameter(hint: &TypeHint) -> bool {
        match hint {
            TypeHint::GenericParameter { .. } => true,
            TypeHint::GenericApplication { arguments, .. }
            | TypeHint::Union(arguments)
            | TypeHint::Intersection(arguments) => arguments
                .iter()
                .any(Self::type_hint_contains_generic_parameter),
            TypeHint::Nullable(inner) => Self::type_hint_contains_generic_parameter(inner),
            _ => false,
        }
    }

    fn substitute_generic_hint(
        &self,
        hint: &TypeHint,
        parameters: &[crate::parser::GenericParameter],
        arguments: &[TypeHint],
    ) -> TypeHint {
        match hint {
            TypeHint::GenericParameter { name, erased } => parameters
                .iter()
                .position(|parameter| parameter.name == *name)
                .and_then(|index| arguments.get(index))
                .cloned()
                .unwrap_or_else(|| (**erased).clone()),
            TypeHint::GenericApplication {
                base,
                arguments: inner,
            } => TypeHint::GenericApplication {
                base: base.clone(),
                arguments: inner
                    .iter()
                    .map(|argument| self.substitute_generic_hint(argument, parameters, arguments))
                    .collect(),
            },
            TypeHint::Nullable(inner) => TypeHint::Nullable(Box::new(
                self.substitute_generic_hint(inner, parameters, arguments),
            )),
            TypeHint::Union(parts) => TypeHint::Union(
                parts
                    .iter()
                    .map(|part| self.substitute_generic_hint(part, parameters, arguments))
                    .collect(),
            ),
            TypeHint::Intersection(parts) => TypeHint::Intersection(
                parts
                    .iter()
                    .map(|part| self.substitute_generic_hint(part, parameters, arguments))
                    .collect(),
            ),
            concrete => concrete.clone(),
        }
    }

    fn emit_reified_argument_check(&mut self, runtime_binding: bool) {
        if !runtime_binding || !crate::generics::GenericRuntimeCapabilities::CONFIGURED.reified {
            return;
        }
        self.instructions
            .push(Instruction::new(OpCode::CheckReifiedArgs));
    }

    fn emit_reified_return_check(
        &mut self,
        runtime_binding: bool,
        result: u16,
        result_type: OpType,
    ) {
        if !runtime_binding || !crate::generics::GenericRuntimeCapabilities::CONFIGURED.reified {
            return;
        }
        let mut check = Instruction::new(OpCode::CheckReifiedReturn);
        check.op1 = result;
        check.op1_type = result_type;
        self.instructions.push(check);
    }

    /// Pre-scan top-level `const` declarations to populate known_constants.
    /// This allows property defaults to reference constants declared later in the file.
    /// Two passes: first collect all simple constants, then re-evaluate those that
    /// reference other constants (handles `const A = 1; const B = A;`).
    fn prescan_constants(&mut self, stmts: &[Stmt]) {
        let file_context = Some((self.source_file.as_str(), self.source_directory.as_str()));
        // Two passes over the full statement tree (including namespace bodies).
        // Pass 1: collect directly evaluable constants
        Self::prescan_constants_pass(
            stmts,
            None,
            &mut HashSet::new(),
            &mut self.known_constants,
            file_context,
        );
        // Pass 2: retry with the now-larger table (handles forward refs like const B = A)
        Self::prescan_constants_pass(
            stmts,
            None,
            &mut HashSet::new(),
            &mut self.known_constants,
            file_context,
        );
    }

    /// Single pass over statements, recursing into namespace bodies.
    /// `ns` is the current namespace prefix (None = top-level).
    fn prescan_constants_pass(
        stmts: &[Stmt],
        ns: Option<&str>,
        declared: &mut HashSet<String>,
        known: &mut HashMap<String, Value>,
        file_context: Option<(&str, &str)>,
    ) {
        for stmt in stmts {
            match stmt {
                Stmt::Const { declarations, .. } => {
                    for (name, value) in declarations {
                        let fqn = match ns {
                            Some(prefix) => format!("{}\\{}", prefix, name),
                            None => name.clone(),
                        };
                        if declared.insert(fqn.clone())
                            && !known.contains_key(&fqn)
                            && let Ok(val) =
                                Self::eval_const_expr_with_context(value, known, file_context)
                        {
                            known.insert(fqn, val);
                        }
                    }
                }
                Stmt::Namespace { name, body } => {
                    Self::prescan_constants_pass(
                        body,
                        (!name.is_empty()).then_some(name.as_str()),
                        declared,
                        known,
                        file_context,
                    );
                }
                Stmt::Block(body) => {
                    Self::prescan_constants_pass(body, ns, declared, known, file_context);
                }
                _ => {}
            }
        }
    }

    /// Resolve a class-like/type name against the current namespace and class
    /// import map.
    /// Rules:
    /// - Fully qualified names (starting with \) are used as-is (without leading \)
    /// - Names in the use map are replaced with their fully qualified target
    /// - Unqualified names in a namespace get the namespace prefix
    /// - Names already containing \ (relative qualified) get namespace prefix
    fn resolve_name(&self, name: &str) -> String {
        if let Some(relative) = name.strip_prefix("namespace\\") {
            return self
                .current_namespace
                .as_ref()
                .map_or_else(|| relative.to_string(), |ns| format!("{ns}\\{relative}"));
        }
        // Fully qualified: strip leading backslash
        if name.starts_with('\\') {
            return name[1..].to_string();
        }
        // Check use map: first segment might be an alias
        let first_segment = name.split('\\').next().unwrap_or(name);
        if let Some(fqn) = self.use_map.get(first_segment) {
            if name.contains('\\') {
                // e.g. `User\Sub` where User is aliased to `App\Models\User`
                let rest = &name[first_segment.len()..]; // starts with '\'
                return format!("{}{}", fqn, rest);
            } else {
                return fqn.clone();
            }
        }
        // In a namespace: prefix with namespace
        if let Some(ns) = &self.current_namespace {
            return format!("{}\\{}", ns, name);
        }
        // Global namespace: use as-is
        name.to_string()
    }

    /// Resolve a source-level function name. Function imports are separate
    /// from class aliases and apply only to unqualified calls.
    fn resolve_function_name(&self, name: &str) -> String {
        if let Some(relative) = name.strip_prefix("namespace\\") {
            return self
                .current_namespace
                .as_ref()
                .map_or_else(|| relative.to_string(), |ns| format!("{ns}\\{relative}"));
        }
        if let Some(fully_qualified) = name.strip_prefix('\\') {
            return fully_qualified.to_string();
        }
        if !name.contains('\\')
            && let Some(fqn) = self.function_use_map.get(&name.to_ascii_lowercase())
        {
            return fqn.clone();
        }
        if let Some(namespace) = &self.current_namespace {
            return format!("{namespace}\\{name}");
        }
        name.to_string()
    }

    fn resolve_declaration_name(&self, name: &str) -> String {
        if let Some(fully_qualified) = name.strip_prefix('\\') {
            return fully_qualified.to_string();
        }
        if let Some(namespace) = &self.current_namespace {
            return format!("{namespace}\\{name}");
        }
        name.to_string()
    }

    fn class_import_exists(&self, alias: &str) -> bool {
        self.class_import_map
            .contains_key(&alias.to_ascii_lowercase())
    }

    fn imported_name(&self, kind: UseKind, alias: &str) -> Option<&str> {
        match kind {
            UseKind::Class => self
                .class_import_map
                .get(&alias.to_ascii_lowercase())
                .map(String::as_str),
            UseKind::Function => self
                .function_use_map
                .get(&alias.to_ascii_lowercase())
                .map(String::as_str),
            UseKind::Const => self.constant_use_map.get(alias).map(String::as_str),
        }
    }

    fn imported_name_matches_declaration(kind: UseKind, imported: &str, declared: &str) -> bool {
        match kind {
            UseKind::Class | UseKind::Function => imported.eq_ignore_ascii_case(declared),
            UseKind::Const => imported == declared,
        }
    }

    fn class_declaration_exists_in_scope(&self, qualified_name: &str) -> bool {
        self.class_defs[self.class_declaration_scope_start..]
            .iter()
            .any(|class| class.name.eq_ignore_ascii_case(qualified_name))
    }

    fn function_declaration_exists_in_scope(&self, qualified_name: &str) -> bool {
        self.functions[self.function_declaration_scope_start..]
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(qualified_name))
    }

    fn constant_declaration_exists_in_scope(&self, qualified_name: &str) -> bool {
        self.constant_declaration_names[self.constant_declaration_scope_start..]
            .iter()
            .any(|name| name == qualified_name)
    }

    fn elided_declaration_exists_in_scope(&self, kind: UseKind, qualified_name: &str) -> bool {
        self.elided_declaration_names[self.elided_declaration_scope_start..]
            .iter()
            .any(|(declared_kind, name)| {
                if *declared_kind != kind {
                    return false;
                }
                match kind {
                    UseKind::Class | UseKind::Function => name.eq_ignore_ascii_case(qualified_name),
                    UseKind::Const => name == qualified_name,
                }
            })
    }

    fn validate_import_alias(
        &self,
        kind: UseKind,
        fully_qualified_name: &str,
        alias: &str,
        line: usize,
    ) -> Result<(), String> {
        let declaration_name = self.resolve_declaration_name(alias);
        let duplicate_import = match kind {
            UseKind::Class => self.class_import_exists(alias),
            UseKind::Function => self
                .function_use_map
                .contains_key(&alias.to_ascii_lowercase()),
            UseKind::Const => self.constant_use_map.contains_key(alias),
        };
        let declaration_collision = match kind {
            UseKind::Class => {
                self.class_declaration_exists_in_scope(&declaration_name)
                    || self.elided_declaration_exists_in_scope(kind, &declaration_name)
            }
            UseKind::Function => {
                self.function_declaration_exists_in_scope(&declaration_name)
                    || self.elided_declaration_exists_in_scope(kind, &declaration_name)
            }
            UseKind::Const => {
                self.constant_declaration_exists_in_scope(&declaration_name)
                    || self.elided_declaration_exists_in_scope(kind, &declaration_name)
            }
        };
        let collision = duplicate_import
            || (declaration_collision
                && !Self::imported_name_matches_declaration(
                    kind,
                    fully_qualified_name,
                    &declaration_name,
                ));
        if !collision {
            return Ok(());
        }
        let kind_prefix = match kind {
            UseKind::Class => "",
            UseKind::Function => "function ",
            UseKind::Const => "const ",
        };
        Err(self.goto_error(
            &format!(
                "Cannot use {kind_prefix}{fully_qualified_name} as {alias} because the name is already in use"
            ),
            line,
        ))
    }

    fn validate_declaration_import(
        &self,
        kind: UseKind,
        source_name: &str,
        declaration_name: &str,
        line: usize,
    ) -> Result<(), String> {
        let Some(imported_name) = self.imported_name(kind, source_name) else {
            return Ok(());
        };
        if Self::imported_name_matches_declaration(kind, imported_name, declaration_name) {
            return Ok(());
        }
        let message = match kind {
            UseKind::Class => format!(
                "Cannot redeclare class {declaration_name} (previously declared as local import)"
            ),
            UseKind::Function => format!(
                "Cannot redeclare function {declaration_name}() (previously declared as local import)"
            ),
            UseKind::Const => {
                format!(
                    "Cannot declare const {declaration_name} because the name is already in use"
                )
            }
        };
        Err(self.goto_error(&message, line))
    }

    fn record_elided_declarations(&mut self, statements: &[Stmt]) -> Result<(), String> {
        for statement in statements {
            let declaration = match statement {
                Stmt::Function { line, name, .. } => {
                    Some((UseKind::Function, name.as_str(), *line))
                }
                Stmt::Class { line, name, .. }
                | Stmt::Interface { line, name, .. }
                | Stmt::Trait { line, name, .. }
                | Stmt::Enum { line, name, .. } => Some((UseKind::Class, name.as_str(), *line)),
                _ => None,
            };
            if let Some((kind, source_name, line)) = declaration {
                let declaration_name = self.resolve_declaration_name(source_name);
                if !declaration_name.starts_with("class@anonymous#") {
                    self.validate_declaration_import(kind, source_name, &declaration_name, line)?;
                    self.elided_declaration_names.push((kind, declaration_name));
                }
                continue;
            }
            match statement {
                Stmt::Const {
                    line, declarations, ..
                } => {
                    for (name, _) in declarations {
                        let declaration_name = self.resolve_declaration_name(name);
                        self.validate_declaration_import(
                            UseKind::Const,
                            name,
                            &declaration_name,
                            *line,
                        )?;
                        self.elided_declaration_names
                            .push((UseKind::Const, declaration_name));
                    }
                }
                Stmt::Block(body) => self.record_elided_declarations(body)?,
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    self.record_elided_declarations(then_body)?;
                    self.record_elided_declarations(else_body)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn resolve_constant_name(&self, name: &str) -> (String, Option<String>) {
        if let Some(relative) = name.strip_prefix("namespace\\") {
            let resolved = self
                .current_namespace
                .as_ref()
                .map_or_else(|| relative.to_string(), |ns| format!("{ns}\\{relative}"));
            return (resolved, None);
        }
        if let Some(fully_qualified) = name.strip_prefix('\\') {
            return (fully_qualified.to_string(), None);
        }
        if !name.contains('\\')
            && let Some(imported) = self.constant_use_map.get(name)
        {
            return (imported.clone(), None);
        }
        if let Some(namespace) = &self.current_namespace {
            return (
                format!("{namespace}\\{name}"),
                (!name.contains('\\')).then_some(name.to_string()),
            );
        }
        (name.to_string(), None)
    }

    fn compiled_interface_closure(&self, root: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut stack = vec![root.to_string()];
        let mut seen = std::collections::HashSet::new();
        while let Some(name) = stack.pop() {
            let key = name.to_ascii_lowercase();
            if !seen.insert(key) {
                continue;
            }
            result.push(name.clone());
            if let Some(definition) = self
                .class_defs
                .iter()
                .find(|definition| definition.name.eq_ignore_ascii_case(&name))
            {
                stack.extend(definition.implements.iter().cloned());
            }
        }
        result
    }

    fn has_function_import(&self, name: &str) -> bool {
        !name.contains('\\')
            && self
                .function_use_map
                .contains_key(&name.to_ascii_lowercase())
    }

    /// Look up ref_args for a function: check user functions, known_ref_args, then builtins.
    fn lookup_ref_args(&self, name: &str) -> u64 {
        // Check user-defined functions in the same compilation unit
        for (fname, uf) in &self.functions {
            if fname == name {
                return uf.common.sig.ref_args;
            }
        }
        // Check inherited known functions (from parent scope)
        if let Some(&ra) = self.known_ref_args.get(name) {
            return ra;
        }
        // Fall back to builtin table
        builtin_ref_args(name)
    }

    /// PHP permits the result of a user function to be used as a temporary
    /// array write target, while results of internal functions and arbitrary
    /// temporary expressions remain invalid write contexts.
    fn is_known_user_function_call(&self, expression: &Expr) -> bool {
        let Expr::FunctionCall { name, .. } = expression else {
            return false;
        };
        let resolved = self.resolve_function_name(name);
        self.functions
            .iter()
            .any(|(function_name, _)| function_name.eq_ignore_ascii_case(&resolved))
            || self
                .known_ref_args
                .keys()
                .any(|function_name| function_name.eq_ignore_ascii_case(&resolved))
    }

    fn mark_trailing_mutable_dimension_fetches(&mut self) {
        for instruction in self
            .instructions
            .iter_mut()
            .rev()
            .take_while(|instruction| instruction.opcode == OpCode::FetchDimR)
        {
            instruction._pad |= FETCH_DIM_MUTABLE;
        }
    }

    /// Build a snapshot of all currently known function ref_args
    /// (own functions + inherited known_ref_args) to pass to child compilers.
    fn build_known_ref_args(&self) -> HashMap<String, u64> {
        let mut map = self.known_ref_args.clone();
        for (fname, uf) in &self.functions {
            map.insert(fname.clone(), uf.common.sig.ref_args);
        }
        map
    }

    pub fn compile(self, stmts: &[Stmt]) -> Result<CompileResult, CompileFailure> {
        let compile_deprecations = Rc::clone(&self.compile_deprecations);
        self.compile_inner(stmts).map_err(|message| CompileFailure {
            message,
            deprecations: compile_deprecations.borrow().clone(),
        })
    }

    fn compile_inner(mut self, stmts: &[Stmt]) -> Result<CompileResult, String> {
        // The parser appends the first valid-syntax compile error at top level
        // after the entire source has parsed. Check it before constant-branch
        // elimination or any other lowering can hide it or surface a later
        // runtime-oriented diagnostic first.
        for statement in stmts {
            if let Stmt::ExprStmt(expression) = statement {
                let diagnostic = match expression {
                    Expr::CompileWarning { message, line } => Some((message, *line, true)),
                    Expr::CompileDeprecation { message, line } => Some((message, *line, false)),
                    _ => None,
                };
                if let Some((message, line, warning)) = diagnostic {
                    self.compile_deprecations
                        .borrow_mut()
                        .push(CompileDeprecation {
                            message: message.clone(),
                            file: self.source_file.clone(),
                            line,
                            warning,
                        });
                }
            }
        }
        if let Some((message, line)) = stmts.iter().find_map(|statement| match statement {
            Stmt::ExprStmt(Expr::CompileError { message, line }) => Some((message, *line)),
            _ => None,
        }) {
            return Err(self.goto_error(message, line));
        }

        // Pre-scan: collect compile-time constants from the entire file so that
        // property defaults can reference constants declared later (forward refs).
        self.prescan_constants(stmts);

        let mut runtime_reachable = true;
        for stmt in stmts {
            if runtime_reachable {
                self.compile_stmt(stmt)?;
                runtime_reachable = !self.statement_statically_returns(stmt);
            } else if matches!(
                stmt,
                Stmt::Function { .. }
                    | Stmt::Class { .. }
                    | Stmt::Interface { .. }
                    | Stmt::Trait { .. }
                    | Stmt::Enum { .. }
            ) {
                // PHP registers unconditional top-level declarations even
                // when an earlier return makes their source position
                // unreachable. Conditional declarations stay runtime-bound
                // and must not be collected from dead code (Composer's
                // version polyfills rely on this distinction).
                self.compile_stmt(stmt)?;
            }
        }
        self.finalize_gotos()?;
        // Check for deferred errors from compile_expr
        if let Some(err) = self.deferred_error.take() {
            return Err(err);
        }

        // Include units override the ordinary implicit null with integer 1.
        let implicit_return = self.implicit_return_value.clone();
        let return_idx = self.add_literal(implicit_return);
        let mut ret = Instruction::new(OpCode::Return);
        ret.op1_type = OpType::Const;
        ret.op1 = return_idx;
        self.instructions.push(ret);

        // Main script: collect all CVs for syncing to eg.globals before function calls.
        // These go into main_scope_vars (separate from explicit `global` bindings).
        let all_cvs = self.all_cvs();
        let main_scope_vars = all_cvs
            .iter()
            .filter(|(_, name)| !name.starts_with('\0'))
            .cloned()
            .collect();

        let cache = (0..self.instructions.len())
            .map(|_| InlineCache::empty())
            .collect();
        refine_function_global_access(&mut self.functions);

        // Consume exact scalar declarations after the first structural pass.
        // Quick-region shapes remain stable; typed composed bodies are rebuilt
        // below so exact String returns can enter the shared typed IR.
        let return_types = declared_function_return_types(&self.functions);
        let parameter_types = declared_function_parameter_types(&self.functions);
        let function_ref_args = declared_function_ref_args(&self.functions);
        let method_facts = declared_method_facts(&self.class_defs);
        for (_, function) in &mut self.functions {
            let signature = &function.common.sig;
            function.op_array.specialize_foreach_target_writes(
                signature.ref_args,
                signature.this_offset,
                &function.reference_cvs,
            );
            propagate_declared_scalar_types(
                &mut function.op_array,
                &function.reference_cvs,
                signature.this_offset,
                &signature.param_type_hints,
                signature.ref_args,
                &return_types,
                &parameter_types,
                &function_ref_args,
                None,
                None,
                &method_facts,
            );
        }
        for class in &mut self.class_defs {
            let class_name = class.name.clone();
            let parent_class = class.parent.clone();
            for (_, _, _, _, method) in &mut class.methods {
                let signature = &method.common.sig;
                method.op_array.specialize_foreach_target_writes(
                    signature.ref_args,
                    signature.this_offset,
                    &method.reference_cvs,
                );
                propagate_declared_scalar_types(
                    &mut method.op_array,
                    &method.reference_cvs,
                    signature.this_offset,
                    &signature.param_type_hints,
                    signature.ref_args,
                    &return_types,
                    &parameter_types,
                    &function_ref_args,
                    Some(&class_name),
                    parent_class.as_deref(),
                    &method_facts,
                );
            }
        }
        for (_, function) in &mut self.functions {
            function.scalar_string_plan = super::build_scalar_string_function_plan(function);
            function.composed_scalar_long_plan =
                super::build_composed_scalar_long_function_plan(function);
            function.composed_typed_long_plan =
                super::build_composed_typed_long_function_plan(function);
        }
        for class in &mut self.class_defs {
            for (_, _, _, _, method) in &mut class.methods {
                method.scalar_string_plan = super::build_scalar_string_function_plan(method);
                method.composed_scalar_long_plan =
                    super::build_composed_scalar_long_function_plan(method);
                method.composed_typed_long_plan =
                    super::build_composed_typed_long_function_plan(method);
            }
        }

        let source_lines = self.materialize_source_lines();
        let runtime_class_names = runtime_class_declaration_names(&self.class_defs);
        debug_assert_eq!(self.class_defs.len(), self.class_declaration_keys.len());
        let mut class_defs = Vec::with_capacity(self.class_defs.len());
        let mut runtime_class_defs = Vec::new();
        for (class_def, declaration) in self.class_defs.into_iter().zip(self.class_declaration_keys)
        {
            let declaration_is_runtime = declaration
                .as_ref()
                .is_some_and(|(_, child_runtime)| *child_runtime)
                || runtime_class_names.contains(&class_def.name.to_ascii_lowercase());
            if declaration_is_runtime && let Some((declaration_key, _)) = declaration {
                runtime_class_defs.push((declaration_key, class_def));
            } else {
                class_defs.push(class_def);
            }
        }

        let generic_use_sites = self.generic_use_sites.borrow().clone();
        let generic_metadata = GenericMetadata::compile_with_inheritance(
            self.generic_declarations,
            self.generic_inheritances,
            generic_use_sites,
        );
        generic_metadata.validate_variance()?;
        Ok(CompileResult {
            main: OpArray {
                num_cvs: self.next_cv,
                num_temps: self.next_tmp,
                trait_class_scope_tmp: self.trait_class_scope_tmp,
                instructions: self.instructions,
                source_lines,
                literals: self.literals,
                try_entries: self.try_entries,
                strict_types: self.strict_types,
                is_generator: false,
                global_vars: self.global_vars,
                static_vars: self.static_vars,
                name: if self.source_file.is_empty() {
                    "<main>".to_string()
                } else {
                    self.source_file.clone()
                },
                source_file: std::rc::Rc::new(self.source_file.clone()),
                main_scope_vars,
                all_cvs,
                cache,
                may_access_globals: false, // main script is entry point, never a callee
                block_info: Vec::new(),
                block_counters: Vec::new(),
                block_plans: Vec::new(),
                ip_to_block: Vec::new(),
            },
            functions: self.functions,
            class_defs,
            runtime_class_defs,
            constant_attributes: self.constant_attributes.borrow().clone(),
            constant_expressions: self.constant_expressions.borrow().clone(),
            generic_metadata,
            deprecations: self.compile_deprecations.borrow().clone(),
        })
    }

    fn statement_statically_returns(&self, statement: &Stmt) -> bool {
        match statement {
            Stmt::Block(body) => body.iter().any(|statement| {
                matches!(statement, Stmt::Return { .. })
                    || self.statement_statically_returns(statement)
            }),
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => self
                .eval_const_expr_in_source(condition, &self.known_constants)
                .ok()
                .is_some_and(|value| {
                    let live_body = if value.is_truthy() {
                        then_body
                    } else {
                        else_body
                    };
                    live_body.iter().any(|statement| {
                        matches!(statement, Stmt::Return { .. })
                            || self.statement_statically_returns(statement)
                    })
                }),
            _ => false,
        }
    }
}

fn magic_return_is_void(hint: &ParamTypeHint) -> bool {
    matches!(hint, ParamTypeHint::Void | ParamTypeHint::Never)
}

fn magic_return_is_bool(hint: &ParamTypeHint) -> bool {
    match hint {
        ParamTypeHint::Bool | ParamTypeHint::Never => true,
        ParamTypeHint::ClassName(name) => {
            name.eq_ignore_ascii_case("true") || name.eq_ignore_ascii_case("false")
        }
        ParamTypeHint::Union(parts) => parts.iter().all(magic_return_is_bool),
        _ => false,
    }
}

fn magic_return_is_string(hint: &ParamTypeHint) -> bool {
    matches!(hint, ParamTypeHint::String | ParamTypeHint::Never)
}

fn magic_return_is_array(hint: &ParamTypeHint) -> bool {
    matches!(hint, ParamTypeHint::Array | ParamTypeHint::Never)
}

fn magic_return_is_nullable_array(hint: &ParamTypeHint) -> bool {
    match hint {
        ParamTypeHint::Array | ParamTypeHint::Never => true,
        ParamTypeHint::Nullable(inner) => {
            matches!(inner.as_ref(), ParamTypeHint::None | ParamTypeHint::Array)
        }
        ParamTypeHint::Union(parts) => parts.iter().all(magic_return_is_nullable_array),
        _ => false,
    }
}

fn magic_return_is_object(hint: &ParamTypeHint) -> bool {
    match hint {
        ParamTypeHint::Never | ParamTypeHint::Intersection(_) => true,
        ParamTypeHint::ClassName(name) => !matches!(
            name.to_ascii_lowercase().as_str(),
            "false" | "true" | "iterable"
        ),
        ParamTypeHint::Union(parts) => parts.iter().all(magic_return_is_object),
        _ => false,
    }
}

pub(crate) fn enum_magic_method_is_forbidden(method: &str) -> bool {
    [
        "__construct",
        "__destruct",
        "__clone",
        "__get",
        "__set",
        "__unset",
        "__isset",
        "__tostring",
        "__debuginfo",
        "__serialize",
        "__unserialize",
        "__sleep",
        "__wakeup",
        "__set_state",
    ]
    .iter()
    .any(|forbidden| method.eq_ignore_ascii_case(forbidden))
}

include!("compile/statements.rs");

impl Compiler {
    fn method_parameter_default_diagnostics(
        &self,
        parameters: &[Param],
    ) -> Option<Box<[Option<Box<str>>]>> {
        let required_num_args = parameters
            .iter()
            .rposition(|parameter| !parameter.is_variadic && parameter.default.is_none())
            .map_or(0, |index| index + 1);
        if !parameters
            .iter()
            .skip(required_num_args)
            .any(|parameter| parameter.default.is_some())
        {
            return None;
        }
        Some(
            parameters
                .iter()
                .enumerate()
                .map(|(index, parameter)| {
                    if index < required_num_args {
                        return None;
                    }
                    parameter.default.as_ref().map(|default| {
                        self.method_parameter_default_diagnostic(default)
                            .into_boxed_str()
                    })
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
    }

    fn method_parameter_default_diagnostic(&self, default: &Expr) -> String {
        match default {
            Expr::Integer(value) => value.to_string(),
            Expr::Float(value) => Value::double(*value).echo_to_string_with_precision(14),
            Expr::StringLiteral(value) => {
                let mut characters = value.chars();
                let prefix = characters.by_ref().take(10).collect::<String>();
                let truncated = characters.next().is_some();
                let escaped = prefix.replace('\\', "\\\\").replace('\'', "\\'");
                format!("'{escaped}{}'", if truncated { "..." } else { "" })
            }
            Expr::Null => "null".to_string(),
            Expr::Bool(value) => value.to_string(),
            Expr::ArrayLiteral(elements) if elements.is_empty() => "[]".to_string(),
            Expr::ArrayLiteral(_) => "[...]".to_string(),
            Expr::Constant(name) if name.eq_ignore_ascii_case("null") => "null".to_string(),
            Expr::Constant(name) if name.eq_ignore_ascii_case("true") => "true".to_string(),
            Expr::Constant(name) if name.eq_ignore_ascii_case("false") => "false".to_string(),
            Expr::Constant(name) => self.resolve_constant_name(name).0,
            Expr::ClassConstant {
                class_name,
                constant,
                ..
            } => {
                let owner = if ["self", "parent", "static"]
                    .iter()
                    .any(|pseudo| class_name.eq_ignore_ascii_case(pseudo))
                {
                    class_name.clone()
                } else {
                    self.resolve_name(class_name)
                };
                format!("{owner}::{constant}")
            }
            Expr::UnaryPlus(inner) => match inner.as_ref() {
                Expr::Integer(_) | Expr::Float(_) => {
                    format!("+{}", self.method_parameter_default_diagnostic(inner))
                }
                _ => "<expression>".to_string(),
            },
            Expr::UnaryMinus(inner) => match inner.as_ref() {
                Expr::Integer(_) | Expr::Float(_) => {
                    format!("-{}", self.method_parameter_default_diagnostic(inner))
                }
                _ => "<expression>".to_string(),
            },
            _ => "<expression>".to_string(),
        }
    }

    fn record_magic_method_visibility_warning(
        &self,
        class: &str,
        method: &str,
        visibility: Visibility,
        line: usize,
    ) {
        if visibility == Visibility::Public
            || ["__construct", "__destruct", "__clone"]
                .iter()
                .any(|exception| method.eq_ignore_ascii_case(exception))
            || ![
                "__call",
                "__callstatic",
                "__get",
                "__set",
                "__isset",
                "__unset",
                "__sleep",
                "__wakeup",
                "__serialize",
                "__unserialize",
                "__tostring",
                "__invoke",
                "__set_state",
                "__debuginfo",
            ]
            .iter()
            .any(|magic| method.eq_ignore_ascii_case(magic))
        {
            return;
        }
        self.compile_deprecations
            .borrow_mut()
            .push(CompileDeprecation {
                message: format!(
                    "The magic method {class}::{method}() must have public visibility"
                ),
                file: self.source_file.clone(),
                line,
                warning: true,
            });
    }

    fn validate_magic_method_return_type(
        &self,
        class: &str,
        method: &str,
        declared: bool,
        hint: &ParamTypeHint,
        line: usize,
    ) -> Result<(), String> {
        if !declared {
            return Ok(());
        }
        if method.eq_ignore_ascii_case("__construct") || method.eq_ignore_ascii_case("__destruct") {
            return Err(self.goto_error(
                &format!("Method {class}::{method}() cannot declare a return type"),
                line,
            ));
        }

        let (requirement, compatible): (&str, fn(&ParamTypeHint) -> bool) = if method
            .eq_ignore_ascii_case("__clone")
            || method.eq_ignore_ascii_case("__set")
            || method.eq_ignore_ascii_case("__unset")
            || method.eq_ignore_ascii_case("__unserialize")
            || method.eq_ignore_ascii_case("__wakeup")
        {
            ("void", magic_return_is_void)
        } else if method.eq_ignore_ascii_case("__isset") {
            ("bool", magic_return_is_bool)
        } else if method.eq_ignore_ascii_case("__tostring") {
            ("string", magic_return_is_string)
        } else if method.eq_ignore_ascii_case("__debuginfo") {
            ("?array", magic_return_is_nullable_array)
        } else if method.eq_ignore_ascii_case("__serialize")
            || method.eq_ignore_ascii_case("__sleep")
        {
            ("array", magic_return_is_array)
        } else if method.eq_ignore_ascii_case("__set_state") {
            ("object", magic_return_is_object)
        } else {
            return Ok(());
        };
        if compatible(hint) {
            return Ok(());
        }
        Err(self.goto_error(
            &format!("{class}::{method}(): Return type must be {requirement} when declared"),
            line,
        ))
    }

    /// Evaluate a constant expression at compile time (for property defaults).
    /// Returns Err for expressions that cannot be resolved at compile time.
    #[allow(dead_code)]
    fn eval_const_expr(expr: &Expr) -> Result<Value, String> {
        Self::eval_const_expr_with_constants(expr, &HashMap::new())
    }

    /// Evaluate a constant expression with access to known compile-time constants.
    fn eval_const_expr_with_constants(
        expr: &Expr,
        known: &HashMap<String, Value>,
    ) -> Result<Value, String> {
        Self::eval_const_expr_with_context(expr, known, None)
    }

    fn eval_const_expr_in_source(
        &self,
        expr: &Expr,
        known: &HashMap<String, Value>,
    ) -> Result<Value, String> {
        self.eval_const_expr_in_source_with_property(expr, known, None)
    }

    fn eval_const_expr_in_source_with_property(
        &self,
        expr: &Expr,
        known: &HashMap<String, Value>,
        lexical_property: Option<&str>,
    ) -> Result<Value, String> {
        // Constant expressions can contain class-constant reads at any depth
        // (for example in an array property default).  The context-free
        // evaluator below deliberately knows nothing about this source unit's
        // `use` table, so expose imported class names as additional keys for
        // the duration of this evaluation rather than only resolving a
        // top-level ClassConstant node.
        let mut imported = known.clone();
        for (alias, class_name) in &self.use_map {
            imported.insert(format!("{alias}::class"), Value::string(class_name.clone()));
            let prefix = format!("{class_name}::");
            for (name, value) in known {
                if let Some(constant) = name.strip_prefix(&prefix) {
                    imported.insert(format!("{alias}::{constant}"), value.clone());
                }
            }
        }
        for (alias, constant_name) in &self.constant_use_map {
            if let Some(value) = known.get(constant_name) {
                imported.insert(alias.clone(), value.clone());
            }
        }
        if let Some(namespace) = &self.current_namespace {
            let prefix = format!("{namespace}\\");
            for (name, value) in known {
                if let Some(local_name) = name.strip_prefix(&prefix)
                    && !local_name.contains('\\')
                {
                    imported.insert(local_name.to_string(), value.clone());
                    imported.insert(format!("namespace\\{local_name}"), value.clone());
                }
            }
        } else {
            for (name, value) in known {
                if !name.contains('\\') && !name.contains("::") {
                    imported.insert(format!("namespace\\{name}"), value.clone());
                }
            }
        }
        imported.insert(
            "__PROPERTY__".to_string(),
            Value::string(lexical_property.unwrap_or_default()),
        );
        self.collect_class_name_literals(expr, &mut imported);
        Self::eval_const_expr_with_context(
            expr,
            &imported,
            Some((self.source_file.as_str(), self.source_directory.as_str())),
        )
    }

    fn compile_attributes(
        &self,
        attributes: &[Attribute],
        target: i64,
    ) -> Vec<AttributeDefinition> {
        self.compile_attributes_in_scope(
            attributes,
            target,
            self.lexical_static_class.as_deref(),
            self.lexical_static_parent.as_deref(),
        )
    }

    fn deprecated_attribute_line(&self, attributes: &[Attribute]) -> Option<usize> {
        attributes.iter().find_map(|attribute| {
            self.resolve_name(&attribute.name)
                .eq_ignore_ascii_case("Deprecated")
                .then_some(attribute.line)
        })
    }

    fn validate_class_like_name(&self, name: &str, kind: &str, line: usize) -> Result<(), String> {
        let article = if matches!(kind, "interface" | "enum") {
            "an"
        } else {
            "a"
        };
        if crate::class_names::is_semantically_reserved(name) {
            return Err(self.goto_error(
                &format!("Cannot use \"{name}\" as {article} {kind} name as it is reserved"),
                line,
            ));
        }
        if name == "_" {
            self.compile_deprecations
                .borrow_mut()
                .push(CompileDeprecation {
                    message: format!(
                        "Using \"_\" as {article} {kind} name is deprecated since 8.4"
                    ),
                    file: self.source_file.clone(),
                    line,
                    warning: false,
                });
        }
        Ok(())
    }

    fn attribute_line(&self, attributes: &[Attribute], name: &str) -> Option<usize> {
        attributes.iter().find_map(|attribute| {
            self.resolve_name(&attribute.name)
                .eq_ignore_ascii_case(name)
                .then_some(attribute.line)
        })
    }

    /// Return the first NoDiscard declaration after enforcing the built-in's
    /// non-repeatable contract. DelayedTargetValidation suppresses target and
    /// validator checks, but deliberately does not suppress repetition.
    fn no_discard_attribute_line(&self, attributes: &[Attribute]) -> Result<Option<usize>, String> {
        let mut lines = attributes.iter().filter_map(|attribute| {
            self.resolve_name(&attribute.name)
                .eq_ignore_ascii_case("NoDiscard")
                .then_some(attribute.line)
        });
        let first = lines.next();
        if let Some(line) = lines.next() {
            return Err(self.goto_error("Attribute \"NoDiscard\" must not be repeated", line));
        }
        Ok(first)
    }

    fn override_attribute_line(&self, attributes: &[Attribute]) -> Result<Option<usize>, String> {
        let mut lines = attributes.iter().filter_map(|attribute| {
            self.resolve_name(&attribute.name)
                .eq_ignore_ascii_case("Override")
                .then_some(attribute.line)
        });
        let first = lines.next();
        if let Some(line) = lines.next() {
            return Err(self.goto_error("Attribute \"Override\" must not be repeated", line));
        }
        Ok(first)
    }

    fn has_delayed_target_validation(&self, attributes: &[Attribute]) -> bool {
        self.attribute_line(attributes, "DelayedTargetValidation")
            .is_some()
    }

    fn validate_no_discard_target(
        &self,
        attributes: &[Attribute],
        target: &str,
    ) -> Result<(), String> {
        let Some(line) = self.no_discard_attribute_line(attributes)? else {
            return Ok(());
        };
        if self.has_delayed_target_validation(attributes) {
            return Ok(());
        }
        Err(self.goto_error(
            &format!(
                "Attribute \"NoDiscard\" cannot target {target} (allowed targets: function, method)"
            ),
            line,
        ))
    }

    fn validate_override_target(
        &self,
        attributes: &[Attribute],
        target: &str,
        member_target: bool,
    ) -> Result<(), String> {
        let Some(line) = self.override_attribute_line(attributes)? else {
            return Ok(());
        };
        if member_target || self.has_delayed_target_validation(attributes) {
            return Ok(());
        }
        Err(self.goto_error(
            &format!(
                "Attribute \"Override\" cannot target {target} (allowed targets: method, property)"
            ),
            line,
        ))
    }

    fn validate_no_discard_callable(
        &self,
        attributes: &[Attribute],
        owner: Option<&str>,
        name: &str,
        return_type: &ParamTypeHint,
    ) -> Result<(), String> {
        let Some(line) = self.no_discard_attribute_line(attributes)? else {
            return Ok(());
        };
        if self.has_delayed_target_validation(attributes) {
            return Ok(());
        }
        if name.starts_with('$') {
            return Err(self.goto_error("#[\\NoDiscard] is not supported for property hooks", line));
        }
        if let Some(owner) = owner
            && matches!(
                name.to_ascii_lowercase().as_str(),
                "__construct" | "__destruct" | "__clone"
            )
        {
            return Err(self.goto_error(
                &format!("Method {owner}::{name} cannot be #[\\NoDiscard]"),
                line,
            ));
        }
        let kind = if owner.is_some() {
            "method"
        } else {
            "function"
        };
        let returning = match return_type {
            ParamTypeHint::Void => Some("void"),
            ParamTypeHint::Never => Some("never returning"),
            _ => None,
        };
        if let Some(returning) = returning {
            return Err(self.goto_error(
                &format!(
                    "A {returning} {kind} does not return a value, but #[\\NoDiscard] requires a return value"
                ),
                line,
            ));
        }
        Ok(())
    }

    fn compile_attributes_in_scope(
        &self,
        attributes: &[Attribute],
        target: i64,
        lexical_class: Option<&str>,
        lexical_parent: Option<&str>,
    ) -> Vec<AttributeDefinition> {
        self.compile_attributes_in_scope_with_property(
            attributes,
            target,
            lexical_class,
            lexical_parent,
            None,
        )
    }

    fn compile_attributes_in_scope_with_property(
        &self,
        attributes: &[Attribute],
        target: i64,
        lexical_class: Option<&str>,
        lexical_parent: Option<&str>,
        lexical_property: Option<&str>,
    ) -> Vec<AttributeDefinition> {
        self.compile_attributes_in_scope_mode_with_property(
            attributes,
            target,
            lexical_class,
            lexical_parent,
            lexical_property,
            false,
        )
    }

    fn compile_attributes_in_scope_mode(
        &self,
        attributes: &[Attribute],
        target: i64,
        lexical_class: Option<&str>,
        lexical_parent: Option<&str>,
        dynamic_scope: bool,
    ) -> Vec<AttributeDefinition> {
        self.compile_attributes_in_scope_mode_with_property(
            attributes,
            target,
            lexical_class,
            lexical_parent,
            None,
            dynamic_scope,
        )
    }

    fn compile_attributes_in_scope_mode_with_property(
        &self,
        attributes: &[Attribute],
        target: i64,
        lexical_class: Option<&str>,
        lexical_parent: Option<&str>,
        lexical_property: Option<&str>,
        _dynamic_scope: bool,
    ) -> Vec<AttributeDefinition> {
        let evaluation_scope = Rc::new(AttributeEvaluationScope {
            namespace: self.current_namespace.clone(),
            class_imports: self.use_map.clone(),
            constant_imports: self.constant_use_map.clone(),
            lexical_class: lexical_class.map(str::to_owned),
            lexical_parent: lexical_parent.map(str::to_owned),
            lexical_property: lexical_property.map(str::to_owned),
            source_directory: self.source_directory.clone(),
        });
        let mut known = self.known_constants.clone();
        if let Some(class) = lexical_class {
            known.insert("self::class".to_string(), Value::string(class));
            let prefix = format!("{class}::");
            for (name, value) in &self.known_constants {
                if let Some(constant) = name.strip_prefix(&prefix) {
                    known.insert(format!("self::{constant}"), value.clone());
                }
            }
        }
        if let Some(parent) = lexical_parent {
            known.insert("parent::class".to_string(), Value::string(parent));
            let prefix = format!("{parent}::");
            for (name, value) in &self.known_constants {
                if let Some(constant) = name.strip_prefix(&prefix) {
                    known.insert(format!("parent::{constant}"), value.clone());
                }
            }
        }
        known.insert(
            "__PROPERTY__".to_string(),
            Value::string(lexical_property.unwrap_or_default()),
        );
        attributes
            .iter()
            .map(|attribute| AttributeDefinition {
                name: self.resolve_name(&attribute.name),
                arguments: attribute
                    .args
                    .iter()
                    .map(|argument| {
                        let (name, expression) = match argument {
                            CallArg::Positional(expression) => (None, expression),
                            CallArg::Named { name, value } => (Some(name.clone()), value),
                            CallArg::Unpack(expression) => (None, expression),
                        };
                        let value = self.eval_const_expr_in_source_with_property(
                            expression,
                            &known,
                            lexical_property,
                        );
                        AttributeArgument {
                            name,
                            // Attribute arguments are instantiated only on
                            // cold semantic/Reflection paths. Retaining their
                            // source expression lets PHP 8.5 diagnose a
                            // deprecated constant used as another symbol's
                            // deprecation message, including self references.
                            deferred_expression: Some(Box::new(expression.clone())),
                            value,
                        }
                    })
                    .collect(),
                evaluation_scope: evaluation_scope.clone(),
                target,
                source_file: self.source_file.clone(),
                source_line: attribute.line,
                strict_types: self.strict_types,
            })
            .collect()
    }

    /// PHP rejects an array-unpack operand during compilation only when its
    /// value is already fixed by the source expression. Ordinary user
    /// constants and runtime expressions remain catchable at execution time;
    /// class constants and built-in constants participate in compile-time
    /// evaluation. A literal array has a known array type even when its own
    /// element values are dynamic.
    fn statically_known_array_unpack_type(&self, expr: &Expr) -> Option<ValueType> {
        if matches!(expr, Expr::ArrayLiteral(_)) {
            return Some(ValueType::Array);
        }

        let class_constants = self
            .known_constants
            .iter()
            .filter(|(name, _)| name.contains("::"))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        let value = self
            .eval_const_expr_in_source(expr, &class_constants)
            .ok()?;

        // `new` is evaluated at runtime in an ordinary array expression. The
        // constant evaluator's narrow stdClass support exists for property and
        // parameter defaults and must not move this error into compilation.
        (value.value_type() != ValueType::Object).then(|| value.value_type())
    }

    fn collect_class_name_literals(&self, expr: &Expr, known: &mut HashMap<String, Value>) {
        match expr {
            Expr::ClassConstant {
                class_name,
                constant,
                ..
            } => {
                let source_key = format!("{class_name}::{constant}");
                if constant.eq_ignore_ascii_case("class") {
                    known
                        .entry(source_key)
                        .or_insert_with(|| Value::string(self.resolve_name(class_name)));
                } else if !known.contains_key(&source_key) {
                    let resolved_class = self.resolve_name(class_name);
                    let resolved_key = format!("{resolved_class}::{constant}");
                    if let Some(value) = known
                        .get(&resolved_key)
                        .cloned()
                        .or_else(|| crate::builtin_class_constant(&resolved_class, constant))
                    {
                        known.insert(source_key, value);
                    }
                }
            }
            Expr::BinaryOp { left, right, .. }
            | Expr::NullCoalesce { left, right }
            | Expr::Elvis { left, right } => {
                self.collect_class_name_literals(left, known);
                self.collect_class_name_literals(right, known);
            }
            Expr::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                self.collect_class_name_literals(condition, known);
                self.collect_class_name_literals(then_expr, known);
                self.collect_class_name_literals(else_expr, known);
            }
            Expr::UnaryPlus(inner)
            | Expr::UnaryMinus(inner)
            | Expr::Not(inner)
            | Expr::BitwiseNot(inner)
            | Expr::Cast { expr: inner, .. } => self.collect_class_name_literals(inner, known),
            Expr::ArrayLiteral(elements) => {
                for element in elements {
                    if let Some(key) = &element.key {
                        self.collect_class_name_literals(key, known);
                    }
                    self.collect_class_name_literals(&element.value, known);
                }
            }
            Expr::ArrayAccess { array, index, .. } => {
                self.collect_class_name_literals(array, known);
                self.collect_class_name_literals(index, known);
            }
            _ => {}
        }
    }

    /// Evaluate a constant expression with the source-unit context needed by
    /// file-scoped magic constants. Class/function magic constants require a
    /// declaration context and remain rejected until that context is explicit.
    fn eval_const_expr_with_context(
        expr: &Expr,
        known: &HashMap<String, Value>,
        file_context: Option<(&str, &str)>,
    ) -> Result<Value, String> {
        match expr {
            Expr::Integer(n) => Ok(Value::long(*n)),
            Expr::Float(f) => Ok(Value::double(*f)),
            Expr::StringLiteral(s) => Ok(Value::string(s.clone())),
            Expr::Bool(b) => Ok(Value::bool(*b)),
            Expr::Null => Ok(Value::null()),
            Expr::Constant(name) => {
                let name = name.strip_prefix('\\').unwrap_or(name);
                // Check user-defined constants from the same compilation unit
                if let Some(val) = known.get(name) {
                    return Ok(val.clone());
                }
                // PHP built-in constants (shared source of truth with runtime)
                if let Some(val) = crate::builtin_constant(name) {
                    return Ok(val);
                }
                // Stream constants cannot be used in constant expressions
                match name {
                    "STDIN" | "STDOUT" | "STDERR" => {
                        Err(format!("{} is not available in constant expressions", name))
                    }
                    _ => Err(format!(
                        "expression Constant(\"{}\") is not a compile-time constant",
                        name
                    )),
                }
            }
            Expr::MagicConstant { name, line } => {
                if name.eq_ignore_ascii_case("__LINE__") {
                    Ok(Value::long(*line as i64))
                } else if name.eq_ignore_ascii_case("__FILE__") {
                    file_context
                        .map(|(file, _)| Value::string(file))
                        .ok_or_else(|| {
                            "magic constant __FILE__ requires the active compilation context"
                                .to_string()
                        })
                } else if name.eq_ignore_ascii_case("__DIR__") {
                    file_context
                        .map(|(_, directory)| Value::string(directory))
                        .ok_or_else(|| {
                            "magic constant __DIR__ requires the active compilation context"
                                .to_string()
                        })
                } else if name.eq_ignore_ascii_case("__PROPERTY__") {
                    Ok(known
                        .get("__PROPERTY__")
                        .cloned()
                        .unwrap_or_else(|| Value::string("")))
                } else if name.eq_ignore_ascii_case("__CLASS__") {
                    known.get("__CLASS__").cloned().ok_or_else(|| {
                        "magic constant __CLASS__ requires the active compilation context"
                            .to_string()
                    })
                } else {
                    Err(format!(
                        "magic constant {} requires the active compilation context",
                        name
                    ))
                }
            }
            Expr::ClassConstant {
                class_name,
                constant,
                ..
            } => known
                .get(&format!("{}::{}", class_name, constant))
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "class constant {}::{} is not available in this constant expression",
                        class_name, constant
                    )
                }),
            Expr::DynamicNamedClassConstant {
                class_name,
                constant,
            } => {
                let constant = Self::eval_const_expr_with_context(constant, known, file_context)?;
                let constant = constant.as_str().ok_or_else(|| {
                    format!(
                        "value of type {} cannot be used as a class constant name",
                        constant.type_name()
                    )
                })?;
                known
                    .get(&format!("{}::{}", class_name, constant))
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "class constant {}::{} is not available in this constant expression",
                            class_name, constant
                        )
                    })
            }
            Expr::DynamicClassConstant {
                class, constant, ..
            } => {
                let class = Self::eval_const_expr_with_context(class, known, file_context)?;
                let class = class.as_str().ok_or_else(|| {
                    format!(
                        "value of type {} cannot be used as a class name",
                        class.type_name()
                    )
                })?;
                let constant = Self::eval_const_expr_with_context(constant, known, file_context)?;
                let constant = constant.as_str().ok_or_else(|| {
                    format!(
                        "value of type {} cannot be used as a class constant name",
                        constant.type_name()
                    )
                })?;
                known
                    .get(&format!("{}::{}", class, constant))
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "class constant {}::{} is not available in this constant expression",
                            class, constant
                        )
                    })
            }
            Expr::BinaryOp { op, left, right } => {
                let left = Self::eval_const_expr_with_context(left, known, file_context)?;
                // Preserve PHP's short-circuit rules even though constant
                // evaluation itself has no observable side effects.
                match op {
                    BinOp::And if !left.is_truthy() => return Ok(Value::bool(false)),
                    BinOp::Or if left.is_truthy() => return Ok(Value::bool(true)),
                    _ => {}
                }
                let right = Self::eval_const_expr_with_context(right, known, file_context)?;
                Self::eval_const_binary(*op, &left, &right)
            }
            Expr::Not(inner) => Ok(Value::bool(
                !Self::eval_const_expr_with_context(inner, known, file_context)?.is_truthy(),
            )),
            Expr::UnaryPlus(inner) => {
                let value = Self::eval_const_expr_with_context(inner, known, file_context)?;
                if let Some(number) = value.as_long() {
                    Ok(Value::long(number))
                } else if let Some(number) = value.as_double() {
                    Ok(Value::double(number))
                } else {
                    Err("unsupported unary expression".to_string())
                }
            }
            Expr::UnaryMinus(inner) => {
                let value = Self::eval_const_expr_with_context(inner, known, file_context)?;
                if let Some(number) = value.as_long() {
                    Ok(number
                        .checked_neg()
                        .map(Value::long)
                        .unwrap_or_else(|| Value::double(-(number as f64))))
                } else if let Some(number) = value.as_double() {
                    Ok(Value::double(-number))
                } else {
                    Err("unsupported unary expression".to_string())
                }
            }
            Expr::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                if Self::eval_const_expr_with_context(condition, known, file_context)?.is_truthy() {
                    Self::eval_const_expr_with_context(then_expr, known, file_context)
                } else {
                    Self::eval_const_expr_with_context(else_expr, known, file_context)
                }
            }
            Expr::Elvis { left, right } => {
                let left = Self::eval_const_expr_with_context(left, known, file_context)?;
                if left.is_truthy() {
                    Ok(left)
                } else {
                    Self::eval_const_expr_with_context(right, known, file_context)
                }
            }
            Expr::NullCoalesce { left, right } => {
                let left = Self::eval_const_expr_with_context(left, known, file_context)?;
                if left.value_type() == ValueType::Null {
                    Self::eval_const_expr_with_context(right, known, file_context)
                } else {
                    Ok(left)
                }
            }
            Expr::New {
                class_name,
                args,
                generic_args,
                ..
            } if class_name.eq_ignore_ascii_case("stdClass")
                && args.is_empty()
                && generic_args.is_empty() =>
            {
                Ok(Value::object(crate::value::PhpObject::dynamic(
                    "stdClass".into(),
                    0,
                    HashMap::new(),
                )))
            }
            Expr::ArrayLiteral(elements) => {
                let mut arr = crate::value::PhpArray::new();
                for elem in elements {
                    let val = Self::eval_const_expr_with_context(&elem.value, known, file_context)?;
                    if elem.unpack {
                        let source = val.as_array().ok_or_else(|| {
                            "Only arrays and Traversables can be unpacked".to_string()
                        })?;
                        for (key, value) in source.iter() {
                            match key {
                                crate::value::ArrayKey::Int(_) => {
                                    if !arr.try_push(value.dereferenced().clone()) {
                                        return Err(
                                            "Cannot add element to the array as the next element is already occupied"
                                                .to_string(),
                                        );
                                    }
                                }
                                crate::value::ArrayKey::String(key) => {
                                    arr.set_str(&key, value.dereferenced().clone());
                                }
                            }
                        }
                        continue;
                    }
                    if let Some(key_expr) = &elem.key {
                        let key =
                            Self::eval_const_expr_with_context(key_expr, known, file_context)?;
                        if let Some(n) = key.as_long() {
                            arr.set_int(n, val);
                        } else if let Some(s) = key.as_str() {
                            arr.set_str(s, val);
                        } else {
                            return Err(
                                "unsupported array key type in constant expression".to_string()
                            );
                        }
                    } else {
                        arr.push(val);
                    }
                }
                Ok(Value::array(arr))
            }
            Expr::ArrayAccess { array, index, .. } => {
                let array = Self::eval_const_expr_with_context(array, known, file_context)?;
                let index = Self::eval_const_expr_with_context(index, known, file_context)?;
                let array = array
                    .as_array()
                    .ok_or_else(|| "constant expression cannot index a non-array".to_string())?;
                let value = if let Some(index) = index.as_long() {
                    array.get_int(index)
                } else if let Some(index) = index.as_str() {
                    array.get_str(index)
                } else {
                    None
                };
                value
                    .cloned()
                    .ok_or_else(|| "undefined array key in constant expression".to_string())
            }
            _ => Err(format!(
                "expression {:?} is not a compile-time constant",
                expr
            )),
        }
    }

    pub(crate) fn eval_const_binary(
        op: BinOp,
        left: &Value,
        right: &Value,
    ) -> Result<Value, String> {
        let integer_pair = || left.as_long().zip(right.as_long());
        let numeric_pair = || left.to_double().zip(right.to_double());
        let integer_operator_pair = || {
            let left = crate::vm::execute::integer_operator_operand(left).ok()?;
            let right = crate::vm::execute::integer_operator_operand(right).ok()?;
            (!left.emits_diagnostic() && !right.emits_diagnostic())
                .then_some((left.value, right.value))
        };
        let unsupported = || format!("unsupported operands for {:?} in constant expression", op);

        match op {
            BinOp::Add => {
                if let (Some(left), Some(right)) = (left.as_array(), right.as_array()) {
                    Ok(Value::array(left.union(right)))
                } else if let Some((left, right)) = integer_pair() {
                    Ok(left
                        .checked_add(right)
                        .map(Value::long)
                        .unwrap_or_else(|| Value::double(left as f64 + right as f64)))
                } else if let Some((left, right)) = numeric_pair() {
                    Ok(Value::double(left + right))
                } else {
                    Err(unsupported())
                }
            }
            BinOp::Sub => {
                if let Some((left, right)) = integer_pair() {
                    Ok(left
                        .checked_sub(right)
                        .map(Value::long)
                        .unwrap_or_else(|| Value::double(left as f64 - right as f64)))
                } else if let Some((left, right)) = numeric_pair() {
                    Ok(Value::double(left - right))
                } else {
                    Err(unsupported())
                }
            }
            BinOp::Mul => {
                if let Some((left, right)) = integer_pair() {
                    Ok(left
                        .checked_mul(right)
                        .map(Value::long)
                        .unwrap_or_else(|| Value::double(left as f64 * right as f64)))
                } else if let Some((left, right)) = numeric_pair() {
                    Ok(Value::double(left * right))
                } else {
                    Err(unsupported())
                }
            }
            BinOp::Div => {
                let (left_number, right_number) = numeric_pair().ok_or_else(unsupported)?;
                if right_number == 0.0 {
                    return Err("division by zero in constant expression".into());
                }
                if let Some((left, right)) = integer_pair()
                    && let Some(quotient) = left.checked_div(right)
                    && left.checked_rem(right) == Some(0)
                {
                    return Ok(Value::long(quotient));
                }
                Ok(Value::double(left_number / right_number))
            }
            BinOp::Mod => {
                let (left, right) = integer_operator_pair().ok_or_else(unsupported)?;
                if right == 0 {
                    return Err("division by zero in constant expression".into());
                }
                Ok(Value::long(left.checked_rem(right).unwrap_or(0)))
            }
            // Float-to-string conversion observes request-local `precision`
            // and therefore cannot be folded while compiling the request.
            BinOp::Concat
                if left.value_type() == ValueType::Double
                    || right.value_type() == ValueType::Double =>
            {
                Err(unsupported())
            }
            BinOp::Concat => Ok(Value::string(format!(
                "{}{}",
                left.echo_to_string(),
                right.echo_to_string()
            ))),
            BinOp::And => Ok(Value::bool(left.is_truthy() && right.is_truthy())),
            BinOp::Or => Ok(Value::bool(left.is_truthy() || right.is_truthy())),
            BinOp::LogicalXor => Ok(Value::bool(left.is_truthy() ^ right.is_truthy())),
            BinOp::Identical | BinOp::NotIdentical => {
                let identical = left.structurally_equal(right);
                Ok(Value::bool(if op == BinOp::Identical {
                    identical
                } else {
                    !identical
                }))
            }
            BinOp::Equal | BinOp::NotEqual => {
                let equal = crate::vm::execute::values_equal_checked(left, right)
                    .map_err(|()| "recursive comparison in constant expression".to_string())?;
                Ok(Value::bool(if op == BinOp::Equal { equal } else { !equal }))
            }
            BinOp::Less
            | BinOp::LessEqual
            | BinOp::Greater
            | BinOp::GreaterEqual
            | BinOp::Spaceship => {
                let comparison = crate::vm::execute::values_compare_checked(left, right)
                    .map_err(|()| "recursive comparison in constant expression".to_string())?;
                match op {
                    BinOp::Less => Ok(Value::bool(comparison < 0)),
                    BinOp::LessEqual => Ok(Value::bool(comparison <= 0)),
                    BinOp::Greater => Ok(Value::bool(
                        comparison != crate::vm::execute::PHP_COMPARISON_UNORDERED
                            && comparison > 0,
                    )),
                    BinOp::GreaterEqual => Ok(Value::bool(
                        comparison != crate::vm::execute::PHP_COMPARISON_UNORDERED
                            && comparison >= 0,
                    )),
                    BinOp::Spaceship => Ok(Value::long(comparison.signum() as i64)),
                    _ => unreachable!(),
                }
            }
            BinOp::Pow => {
                if let Some((base, exponent)) = integer_pair()
                    && let Ok(exponent) = u32::try_from(exponent)
                    && let Some(value) = base.checked_pow(exponent)
                {
                    return Ok(Value::long(value));
                }
                let (base, exponent) = numeric_pair().ok_or_else(unsupported)?;
                Ok(Value::double(base.powf(exponent)))
            }
            BinOp::BitwiseAnd | BinOp::BitwiseOr | BinOp::BitwiseXor => {
                if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
                    let (operation, preserve_longer_tail): (fn(u8, u8) -> u8, bool) = match op {
                        BinOp::BitwiseAnd => (|left, right| left & right, false),
                        BinOp::BitwiseOr => (|left, right| left | right, true),
                        BinOp::BitwiseXor => (|left, right| left ^ right, false),
                        _ => unreachable!(),
                    };
                    return Ok(Value::string(crate::value::php_byte_string_binary(
                        left,
                        right,
                        operation,
                        preserve_longer_tail,
                    )));
                }
                let (left, right) = integer_operator_pair().ok_or_else(unsupported)?;
                Ok(Value::long(match op {
                    BinOp::BitwiseAnd => left & right,
                    BinOp::BitwiseOr => left | right,
                    BinOp::BitwiseXor => left ^ right,
                    _ => unreachable!(),
                }))
            }
            BinOp::ShiftLeft | BinOp::ShiftRight => {
                let (left, right) = integer_operator_pair().ok_or_else(unsupported)?;
                let shift = u32::try_from(right)
                    .ok()
                    .filter(|shift| *shift < i64::BITS)
                    .ok_or_else(unsupported)?;
                Ok(Value::long(if op == BinOp::ShiftLeft {
                    left.wrapping_shl(shift)
                } else {
                    left.wrapping_shr(shift)
                }))
            }
        }
    }

    /// Compile parameter list into CV slots. Returns (num_args, required_num_args, is_variadic, variadic_cv_index, ref_args).
    /// num_args counts only non-variadic params. The variadic param gets its own CV.
    fn compile_params(
        &self,
        func_compiler: &mut Compiler,
        params: &[Param],
        context: &str,
    ) -> Result<CompiledParams, String> {
        // PHP treats every parameter through the last parameter without a
        // default as required. Defaults before that boundary remain in the
        // op array for reflection/source fidelity, but calls may not use them.
        let required_num_args = params
            .iter()
            .rposition(|param| !param.is_variadic && param.default.is_none())
            .map_or(0, |index| index as u32 + 1);
        let last_required = required_num_args.checked_sub(1).map(|index| index as usize);
        let mut is_variadic = false;
        let mut variadic_cv_index = 0u32;
        let mut ref_args = 0u64;
        let mut type_hints = Vec::new();
        let mut param_names = Vec::new();
        for (i, param) in params.iter().enumerate() {
            self.validate_override_target(
                &param.attributes,
                "parameter",
                param.promoted_property.is_some(),
            )?;
            if param.name == "this" {
                return Err(self.goto_error("Cannot use $this as parameter", param.line));
            }
            func_compiler.validate_declared_type_hint(&param.type_hint, param.line)?;
            match param.type_hint.as_ref() {
                Some(crate::parser::TypeHint::Never) => {
                    return Err(
                        self.goto_error("never cannot be used as a parameter type", param.line)
                    );
                }
                Some(crate::parser::TypeHint::Void) => {
                    return Err(
                        self.goto_error("void cannot be used as a parameter type", param.line)
                    );
                }
                _ => {}
            }
            if param.is_ref && i < 64 {
                ref_args |= 1u64 << i;
            }
            // PHP 8.5 accepts legacy `T $value = null` declarations while
            // deprecating their spelling. The callable contract itself is
            // nullable, which must be visible to calls and variance checks.
            let implicitly_nullable = matches!(param.default, Some(crate::parser::Expr::Null))
                && param.promotion.is_none()
                && param
                    .type_hint
                    .as_ref()
                    .is_some_and(|hint| !Self::declared_type_allows_null(hint));
            let mut hint = self.convert_type_hint(&param.type_hint);
            if implicitly_nullable {
                let callable = func_compiler.declaration_diagnostic_name();
                func_compiler
                    .compile_deprecations
                    .borrow_mut()
                    .push(CompileDeprecation {
                        message: format!(
                            "{callable}(): Implicitly marking parameter ${} as nullable is deprecated, the explicit nullable type must be used instead",
                            param.name
                        ),
                        file: func_compiler.source_file.clone(),
                        line: param.line,
                        warning: false,
                    });
                hint = ParamTypeHint::Nullable(Box::new(hint));
            }
            if param.default.is_some()
                && !implicitly_nullable
                && last_required.is_some_and(|required| i < required)
            {
                let callable = func_compiler.declaration_diagnostic_name();
                let required_name = &params[last_required.unwrap()].name;
                func_compiler
                    .compile_deprecations
                    .borrow_mut()
                    .push(CompileDeprecation {
                        message: format!(
                            "{callable}(): Optional parameter ${} declared before required parameter ${required_name} is implicitly treated as a required parameter",
                            param.name
                        ),
                        file: func_compiler.source_file.clone(),
                        line: param.line,
                        warning: false,
                    });
            }
            type_hints.push(hint);
            // Collect param name
            param_names.push(param.name.clone());

            if param.is_variadic {
                if i != params.len() - 1 {
                    return Err(format!(
                        "Variadic parameter ${} must be last in {}",
                        param.name, context
                    ));
                }
                is_variadic = true;
                variadic_cv_index = func_compiler.resolve_cv(&param.name) as u32;
                func_compiler
                    .definitely_defined_cvs
                    .insert(variadic_cv_index as u16);
                // No default emit for variadic — VM packs extra args into array
            } else {
                let cv_idx = func_compiler.resolve_cv(&param.name);
                func_compiler.definitely_defined_cvs.insert(cv_idx);
                if let Some(default_expr) = &param.default {
                    let mut normalized_default = None;
                    if Self::parameter_default_is_compile_time_fixed(default_expr)
                        && !matches!(
                            type_hints.last(),
                            Some(ParamTypeHint::None | ParamTypeHint::Mixed)
                        )
                        && let Ok(value) = func_compiler
                            .eval_const_expr_in_source(default_expr, &func_compiler.known_constants)
                    {
                        let Ok(normalized) = normalize_typed_declaration_default(
                            value.clone(),
                            type_hints.last().unwrap(),
                        ) else {
                            return Err(func_compiler.goto_error(
                                &format!(
                                    "Cannot use {} as default value for parameter ${} of type {}",
                                    value.type_name(),
                                    param.name,
                                    type_hints.last().unwrap().diagnostic_display_name()
                                ),
                                param.line,
                            ));
                        };
                        if normalized.value_type() != value.value_type() {
                            normalized_default = Some(normalized);
                        }
                    }
                    let check_generic_default =
                        crate::generics::GenericRuntimeCapabilities::CONFIGURED.syntax_enabled()
                            && param
                                .type_hint
                                .as_ref()
                                .is_some_and(Self::type_hint_contains_generic_parameter);
                    Self::emit_default_param(
                        func_compiler,
                        cv_idx,
                        i as u16,
                        default_expr,
                        normalized_default,
                        check_generic_default,
                    );
                }
            }
        }
        // num_args = non-variadic params count
        let num_args = if is_variadic {
            (params.len() - 1) as u32
        } else {
            params.len() as u32
        };
        Ok(CompiledParams {
            num_args,
            required_num_args,
            is_variadic,
            variadic_cv_index,
            ref_args,
            type_hints,
            param_names,
            return_type_hint: crate::vm::function::ParamTypeHint::None,
        })
    }

    fn declared_type_allows_null(hint: &crate::parser::TypeHint) -> bool {
        use crate::parser::TypeHint;
        match hint {
            TypeHint::Mixed | TypeHint::Null | TypeHint::Nullable(_) => true,
            TypeHint::Union(parts) => parts.iter().any(Self::declared_type_allows_null),
            _ => false,
        }
    }

    fn parameter_default_is_compile_time_fixed(expr: &Expr) -> bool {
        match expr {
            Expr::Integer(_)
            | Expr::Float(_)
            | Expr::StringLiteral(_)
            | Expr::Null
            | Expr::Bool(_)
            | Expr::MagicConstant { .. } => true,
            Expr::BinaryOp { left, right, .. }
            | Expr::NullCoalesce { left, right }
            | Expr::Elvis { left, right } => {
                Self::parameter_default_is_compile_time_fixed(left)
                    && Self::parameter_default_is_compile_time_fixed(right)
            }
            Expr::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                Self::parameter_default_is_compile_time_fixed(condition)
                    && Self::parameter_default_is_compile_time_fixed(then_expr)
                    && Self::parameter_default_is_compile_time_fixed(else_expr)
            }
            Expr::UnaryPlus(inner)
            | Expr::UnaryMinus(inner)
            | Expr::Not(inner)
            | Expr::BitwiseNot(inner)
            | Expr::Cast { expr: inner, .. } => {
                Self::parameter_default_is_compile_time_fixed(inner)
            }
            Expr::ArrayLiteral(elements) => elements.iter().all(|element| {
                element
                    .key
                    .as_ref()
                    .is_none_or(Self::parameter_default_is_compile_time_fixed)
                    && Self::parameter_default_is_compile_time_fixed(&element.value)
            }),
            Expr::ArrayAccess { array, index, .. } => {
                Self::parameter_default_is_compile_time_fixed(array)
                    && Self::parameter_default_is_compile_time_fixed(index)
            }
            _ => false,
        }
    }

    fn declaration_diagnostic_name(&self) -> String {
        self.current_function_name
            .starts_with("__closure_")
            .then(|| self.current_function_name.split_once('@'))
            .flatten()
            .map(|(_, public)| public.to_string())
            .unwrap_or_else(|| self.current_function_name.clone())
    }

    fn generator_return_type_accepts(hint: &ParamTypeHint) -> bool {
        match hint {
            ParamTypeHint::None | ParamTypeHint::Mixed => true,
            ParamTypeHint::ClassName(name) => matches!(
                name.trim_start_matches('\\').to_ascii_lowercase().as_str(),
                "generator" | "iterator" | "traversable" | "iterable" | "object"
            ),
            ParamTypeHint::Nullable(inner) => Self::generator_return_type_accepts(inner),
            ParamTypeHint::Union(parts) => parts.iter().any(Self::generator_return_type_accepts),
            ParamTypeHint::Intersection(parts) => {
                !parts.is_empty() && parts.iter().all(Self::generator_return_type_accepts)
            }
            _ => false,
        }
    }

    fn validate_generator_return_type(
        &self,
        contains_yield: bool,
        hint: &ParamTypeHint,
        line: usize,
    ) -> Result<(), String> {
        if contains_yield && !Self::generator_return_type_accepts(hint) {
            return Err(self.goto_error(
                &format!(
                    "Generator return type must be a supertype of Generator, {} given",
                    hint.display_name()
                ),
                line,
            ));
        }
        Ok(())
    }

    fn validate_declared_type_hint(
        &self,
        hint: &Option<crate::parser::TypeHint>,
        line: usize,
    ) -> Result<(), String> {
        self.validate_declared_type_hint_in_scope(
            hint,
            line,
            self.lexical_static_class.as_deref(),
            self.lexical_static_parent.as_deref(),
        )
    }

    fn validate_declared_type_hint_in_scope(
        &self,
        hint: &Option<crate::parser::TypeHint>,
        line: usize,
        current_class: Option<&str>,
        parent_class: Option<&str>,
    ) -> Result<(), String> {
        use crate::parser::TypeHint;

        fn standalone_type_error(hint: &TypeHint) -> Option<&'static str> {
            match hint {
                TypeHint::Nullable(inner) => match inner.as_ref() {
                    TypeHint::Mixed => Some(
                        "Type mixed cannot be marked as nullable since mixed already includes null",
                    ),
                    TypeHint::Void => Some("Void can only be used as a standalone type"),
                    TypeHint::Never => Some("never can only be used as a standalone type"),
                    _ => None,
                },
                TypeHint::Union(parts) => {
                    if parts.iter().any(|part| matches!(part, TypeHint::Mixed)) {
                        Some("Type mixed can only be used as a standalone type")
                    } else if parts.iter().any(|part| matches!(part, TypeHint::Void)) {
                        Some("Void can only be used as a standalone type")
                    } else if parts.iter().any(|part| matches!(part, TypeHint::Never)) {
                        Some("never can only be used as a standalone type")
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }

        #[derive(Clone)]
        struct TypeAtom {
            identity: String,
            display: String,
            semantic: Vec<&'static str>,
            is_class: bool,
            explicit_true: bool,
            explicit_false: bool,
        }

        #[derive(Clone)]
        struct TypeBranch {
            atoms: Vec<TypeAtom>,
        }

        impl TypeBranch {
            fn display(&self) -> String {
                self.atoms
                    .iter()
                    .map(|atom| atom.display.as_str())
                    .collect::<Vec<_>>()
                    .join("&")
            }

            fn identities(&self) -> std::collections::HashSet<&str> {
                self.atoms
                    .iter()
                    .map(|atom| atom.identity.as_str())
                    .collect()
            }
        }

        fn intersection_member_name(hint: &TypeHint) -> Option<String> {
            match hint {
                TypeHint::ClassName(name) => match name.to_ascii_lowercase().as_str() {
                    "int" => Some("int".to_string()),
                    "float" => Some("float".to_string()),
                    "string" => Some("string".to_string()),
                    "bool" => Some("bool".to_string()),
                    "callable" => Some("callable".to_string()),
                    "null" => Some("null".to_string()),
                    "void" => Some("void".to_string()),
                    "mixed" => Some("mixed".to_string()),
                    "never" => Some("never".to_string()),
                    "false" => Some("false".to_string()),
                    "true" => Some("true".to_string()),
                    "object" => Some("object".to_string()),
                    "static" => Some("static".to_string()),
                    "iterable" => Some("Traversable|array".to_string()),
                    _ => None,
                },
                TypeHint::Int => Some("int".to_string()),
                TypeHint::Float => Some("float".to_string()),
                TypeHint::String => Some("string".to_string()),
                TypeHint::Bool => Some("bool".to_string()),
                TypeHint::Array => Some("array".to_string()),
                TypeHint::Callable => Some("callable".to_string()),
                TypeHint::Null => Some("null".to_string()),
                TypeHint::Void => Some("void".to_string()),
                TypeHint::Mixed => Some("mixed".to_string()),
                TypeHint::Never => Some("never".to_string()),
                TypeHint::Nullable(_) | TypeHint::Union(_) | TypeHint::Intersection(_) => None,
                TypeHint::GenericParameter { .. } | TypeHint::GenericApplication { .. } => None,
            }
        }

        fn first_invalid_intersection_member(hint: &TypeHint) -> Option<String> {
            match hint {
                TypeHint::Intersection(parts) => parts.iter().find_map(|part| {
                    intersection_member_name(part)
                        .or_else(|| first_invalid_intersection_member(part))
                }),
                TypeHint::Union(parts) => parts.iter().find_map(first_invalid_intersection_member),
                TypeHint::Nullable(inner) => first_invalid_intersection_member(inner),
                _ => None,
            }
        }

        let atom = |hint: &TypeHint| -> Option<TypeAtom> {
            let builtin = |identity: &str,
                           semantic: Vec<&'static str>,
                           is_class: bool,
                           explicit_true: bool,
                           explicit_false: bool| TypeAtom {
                identity: identity.to_string(),
                display: identity.to_string(),
                semantic,
                is_class,
                explicit_true,
                explicit_false,
            };
            Some(match hint {
                TypeHint::Int => builtin("int", vec![], false, false, false),
                TypeHint::Float => builtin("float", vec![], false, false, false),
                TypeHint::String => builtin("string", vec![], false, false, false),
                TypeHint::Bool => builtin("bool", vec!["true", "false"], false, false, false),
                TypeHint::Array => builtin("array", vec![], false, false, false),
                TypeHint::Callable => builtin("callable", vec![], false, false, false),
                TypeHint::Null => builtin("null", vec![], false, false, false),
                TypeHint::Void => builtin("void", vec![], false, false, false),
                TypeHint::Mixed => builtin("mixed", vec![], false, false, false),
                TypeHint::Never => builtin("never", vec![], false, false, false),
                TypeHint::ClassName(name) => {
                    let lower = name.to_ascii_lowercase();
                    match lower.as_str() {
                        "int" => builtin("int", vec![], false, false, false),
                        "float" => builtin("float", vec![], false, false, false),
                        "string" => builtin("string", vec![], false, false, false),
                        "bool" => builtin("bool", vec!["true", "false"], false, false, false),
                        "callable" => builtin("callable", vec![], false, false, false),
                        "null" => builtin("null", vec![], false, false, false),
                        "void" => builtin("void", vec![], false, false, false),
                        "mixed" => builtin("mixed", vec![], false, false, false),
                        "never" => builtin("never", vec![], false, false, false),
                        "false" => builtin("false", vec![], false, false, true),
                        "true" => builtin("true", vec![], false, true, false),
                        "object" => builtin("object", vec![], false, false, false),
                        "iterable" => TypeAtom {
                            identity: "iterable".to_string(),
                            display: "iterable".to_string(),
                            semantic: vec!["traversable", "array"],
                            is_class: false,
                            explicit_true: false,
                            explicit_false: false,
                        },
                        "static" => builtin("static", vec![], true, false, false),
                        "self" => {
                            let display = current_class.unwrap_or("self").to_string();
                            TypeAtom {
                                identity: display.to_ascii_lowercase(),
                                display,
                                semantic: vec![],
                                is_class: true,
                                explicit_true: false,
                                explicit_false: false,
                            }
                        }
                        "parent" => {
                            let display = parent_class.unwrap_or("parent").to_string();
                            TypeAtom {
                                identity: display.to_ascii_lowercase(),
                                display,
                                semantic: vec![],
                                is_class: true,
                                explicit_true: false,
                                explicit_false: false,
                            }
                        }
                        _ => {
                            let display = self.resolve_name(name);
                            TypeAtom {
                                identity: display.to_ascii_lowercase(),
                                display,
                                semantic: vec![],
                                is_class: true,
                                explicit_true: false,
                                explicit_false: false,
                            }
                        }
                    }
                }
                TypeHint::GenericParameter { name, .. } => TypeAtom {
                    identity: format!("generic:{name}"),
                    display: name.clone(),
                    semantic: vec![],
                    is_class: true,
                    explicit_true: false,
                    explicit_false: false,
                },
                TypeHint::GenericApplication { base, .. } => {
                    let display = self.resolve_name(base);
                    TypeAtom {
                        identity: display.to_ascii_lowercase(),
                        display,
                        semantic: vec![],
                        is_class: true,
                        explicit_true: false,
                        explicit_false: false,
                    }
                }
                TypeHint::Nullable(_) | TypeHint::Union(_) | TypeHint::Intersection(_) => {
                    return None;
                }
            })
        };

        let branch = |hint: &TypeHint| -> Option<TypeBranch> {
            match hint {
                TypeHint::Intersection(parts) => Some(TypeBranch {
                    atoms: parts.iter().filter_map(&atom).collect(),
                }),
                _ => atom(hint).map(|atom| TypeBranch { atoms: vec![atom] }),
            }
        };

        fn duplicate_in_branch(branch: &TypeBranch) -> Option<String> {
            let mut seen = std::collections::HashSet::new();
            for atom in &branch.atoms {
                if !seen.insert(atom.identity.as_str()) {
                    return Some(atom.display.clone());
                }
            }
            None
        }

        fn union_redundancy(branches: &[TypeBranch]) -> Option<String> {
            let has_explicit_true = branches
                .iter()
                .flat_map(|branch| &branch.atoms)
                .any(|atom| atom.explicit_true);
            let has_explicit_false = branches
                .iter()
                .flat_map(|branch| &branch.atoms)
                .any(|atom| atom.explicit_false);
            let has_bool = branches
                .iter()
                .flat_map(|branch| &branch.atoms)
                .any(|atom| atom.identity == "bool");
            if has_explicit_true && has_explicit_false && !has_bool {
                return Some("Type contains both true and false, bool must be used instead".into());
            }

            let mut seen_identities = std::collections::HashSet::new();
            let mut seen_semantic = std::collections::HashSet::new();
            for branch in branches.iter().filter(|branch| branch.atoms.len() == 1) {
                let atom = &branch.atoms[0];
                if !seen_identities.insert(atom.identity.as_str())
                    || seen_semantic.contains(atom.identity.as_str())
                {
                    if atom.identity == "iterable" {
                        return Some("Duplicate type array is redundant".to_string());
                    }
                    return Some(format!("Duplicate type {} is redundant", atom.display));
                }
                for semantic in &atom.semantic {
                    if seen_identities.contains(semantic) || !seen_semantic.insert(*semantic) {
                        return Some(format!("Duplicate type {semantic} is redundant"));
                    }
                }
            }

            let has_object = branches
                .iter()
                .any(|branch| branch.atoms.len() == 1 && branch.atoms[0].identity == "object");
            if has_object
                && branches
                    .iter()
                    .any(|branch| branch.atoms.iter().any(|a| a.is_class))
            {
                let mut class_displays = Vec::new();
                let mut tail_displays = Vec::new();
                for branch in branches {
                    if branch.atoms.len() > 1 {
                        class_displays.push(format!("({})", branch.display()));
                        continue;
                    }
                    let atom = &branch.atoms[0];
                    match atom.identity.as_str() {
                        "object" => {}
                        "iterable" => {
                            class_displays.push("Traversable".to_string());
                            tail_displays.push("array".to_string());
                        }
                        _ if atom.is_class => class_displays.push(atom.display.clone()),
                        _ => tail_displays.push(atom.display.clone()),
                    }
                }
                class_displays.push("object".to_string());
                class_displays.extend(tail_displays);
                return Some(format!(
                    "Type {} contains both object and a class type, which is redundant",
                    class_displays.join("|")
                ));
            }

            for left in 0..branches.len() {
                for right in (left + 1)..branches.len() {
                    let left_ids = branches[left].identities();
                    let right_ids = branches[right].identities();
                    if left_ids == right_ids && branches[left].atoms.len() > 1 {
                        let display = branches[left].display();
                        return Some(format!("Type {display} is redundant with type {display}"));
                    }
                    let (restrictive, permissive) = if left_ids.is_superset(&right_ids) {
                        (&branches[left], &branches[right])
                    } else if right_ids.is_superset(&left_ids) {
                        (&branches[right], &branches[left])
                    } else {
                        continue;
                    };
                    if restrictive.atoms.len() > permissive.atoms.len() {
                        return Some(format!(
                            "Type {} is redundant as it is more restrictive than type {}",
                            restrictive.display(),
                            permissive.display()
                        ));
                    }
                }
            }
            None
        }

        if let Some(message) = hint.as_ref().and_then(standalone_type_error) {
            return Err(self.goto_error(message, line));
        }
        if let Some(invalid) = hint.as_ref().and_then(first_invalid_intersection_member) {
            return Err(self.goto_error(
                &format!("Type {invalid} cannot be part of an intersection type"),
                line,
            ));
        }
        if matches!(hint, Some(TypeHint::Nullable(inner)) if matches!(inner.as_ref(), TypeHint::Null))
        {
            return Err(self.goto_error("null cannot be marked as nullable", line));
        }
        if let Some(TypeHint::Intersection(parts)) = hint {
            let normalized = TypeBranch {
                atoms: parts.iter().filter_map(&atom).collect(),
            };
            if let Some(duplicate) = duplicate_in_branch(&normalized) {
                return Err(
                    self.goto_error(&format!("Duplicate type {duplicate} is redundant"), line)
                );
            }
        }
        if let Some(TypeHint::Union(parts)) = hint {
            let branches = parts.iter().filter_map(branch).collect::<Vec<_>>();
            for normalized in &branches {
                if let Some(duplicate) = duplicate_in_branch(normalized) {
                    return Err(
                        self.goto_error(&format!("Duplicate type {duplicate} is redundant"), line)
                    );
                }
            }
            if let Some(message) = union_redundancy(&branches) {
                return Err(self.goto_error(&message, line));
            }
        }
        Ok(())
    }

    fn validate_property_type_hint_in_scope(
        &self,
        hint: &Option<crate::parser::TypeHint>,
        line: usize,
        declaring_class: &str,
        property_name: &str,
        parent_class: Option<&str>,
    ) -> Result<(), String> {
        self.validate_property_function_only_type(hint, line, declaring_class, property_name)?;
        self.validate_declared_type_hint_in_scope(hint, line, Some(declaring_class), parent_class)
    }

    fn validate_property_function_only_type(
        &self,
        hint: &Option<crate::parser::TypeHint>,
        line: usize,
        declaring_class: &str,
        property_name: &str,
    ) -> Result<(), String> {
        use crate::parser::TypeHint;

        fn contains_callable(hint: &TypeHint) -> bool {
            match hint {
                TypeHint::Callable => true,
                TypeHint::Nullable(inner) => contains_callable(inner),
                TypeHint::Union(parts) | TypeHint::Intersection(parts) => {
                    parts.iter().any(contains_callable)
                }
                _ => false,
            }
        }

        let forbidden = match hint {
            Some(TypeHint::Void) => Some("void".to_string()),
            Some(TypeHint::Never) => Some("never".to_string()),
            Some(hint) if contains_callable(hint) => {
                Some(self.convert_type_hint(&Some(hint.clone())).display_name())
            }
            _ => None,
        };
        if let Some(type_name) = forbidden {
            return Err(self.goto_error(
                &format!(
                    "Property {declaring_class}::${property_name} cannot have type {type_name}"
                ),
                line,
            ));
        }
        Ok(())
    }

    /// Convert parser TypeHint to runtime ParamTypeHint.
    fn convert_type_hint(
        &self,
        hint: &Option<crate::parser::TypeHint>,
    ) -> crate::vm::function::ParamTypeHint {
        use crate::parser::TypeHint;
        use crate::vm::function::ParamTypeHint;
        match hint {
            None => ParamTypeHint::None,
            Some(TypeHint::Int) => ParamTypeHint::Int,
            Some(TypeHint::Float) => ParamTypeHint::Float,
            Some(TypeHint::String) => ParamTypeHint::String,
            Some(TypeHint::Bool) => ParamTypeHint::Bool,
            Some(TypeHint::Array) => ParamTypeHint::Array,
            Some(TypeHint::Callable) => ParamTypeHint::Callable,
            Some(TypeHint::Null) => ParamTypeHint::Nullable(Box::new(ParamTypeHint::None)),
            Some(TypeHint::ClassName(name)) => {
                // Built-in and pseudo-types are ASCII case-insensitive even
                // when the lexer delivered their spelling as an identifier.
                match name.to_ascii_lowercase().as_str() {
                    "int" => ParamTypeHint::Int,
                    "float" => ParamTypeHint::Float,
                    "string" => ParamTypeHint::String,
                    "bool" => ParamTypeHint::Bool,
                    "array" => ParamTypeHint::Array,
                    "callable" => ParamTypeHint::Callable,
                    "mixed" => ParamTypeHint::Mixed,
                    "never" => ParamTypeHint::Never,
                    "void" => ParamTypeHint::Void,
                    "null" => ParamTypeHint::Nullable(Box::new(ParamTypeHint::None)),
                    builtin @ ("self" | "parent" | "static" | "object" | "iterable" | "false"
                    | "true") => ParamTypeHint::ClassName(builtin.to_string()),
                    _ => ParamTypeHint::ClassName(self.resolve_name(name)),
                }
            }
            Some(TypeHint::Nullable(inner)) => {
                let inner_hint = self.convert_type_hint(&Some(*inner.clone()));
                ParamTypeHint::Nullable(Box::new(inner_hint))
            }
            Some(TypeHint::Void) => ParamTypeHint::Void,
            Some(TypeHint::Mixed) => ParamTypeHint::Mixed,
            Some(TypeHint::Never) => ParamTypeHint::Never,
            Some(TypeHint::Union(types)) => {
                let converted: Vec<ParamTypeHint> = types
                    .iter()
                    .map(|t| self.convert_type_hint(&Some(t.clone())))
                    .collect();
                ParamTypeHint::Union(converted)
            }
            Some(TypeHint::Intersection(types)) => {
                let converted: Vec<ParamTypeHint> = types
                    .iter()
                    .map(|t| self.convert_type_hint(&Some(t.clone())))
                    .collect();
                ParamTypeHint::Intersection(converted)
            }
            Some(TypeHint::GenericParameter { erased, .. }) => {
                self.convert_type_hint(&Some(*erased.clone()))
            }
            Some(TypeHint::GenericApplication { base, .. }) => {
                ParamTypeHint::ClassName(match base.as_str() {
                    "self" | "parent" | "static" | "object" | "iterable" => base.clone(),
                    _ => self.resolve_name(base),
                })
            }
        }
    }

    fn resolve_declared_property_type_hint(
        &self,
        hint: ParamTypeHint,
        class_name: &str,
        parent_name: Option<&str>,
    ) -> ParamTypeHint {
        match hint {
            ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("self") => {
                ParamTypeHint::ClassName(class_name.to_string())
            }
            ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("parent") => {
                ParamTypeHint::ClassName(parent_name.unwrap_or("parent").to_string())
            }
            ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("static") => {
                ParamTypeHint::ClassName(class_name.to_string())
            }
            ParamTypeHint::Nullable(inner) => ParamTypeHint::Nullable(Box::new(
                self.resolve_declared_property_type_hint(*inner, class_name, parent_name),
            )),
            ParamTypeHint::Union(parts) => ParamTypeHint::Union(
                parts
                    .into_iter()
                    .map(|part| {
                        self.resolve_declared_property_type_hint(part, class_name, parent_name)
                    })
                    .collect(),
            ),
            ParamTypeHint::Intersection(parts) => ParamTypeHint::Intersection(
                parts
                    .into_iter()
                    .map(|part| {
                        self.resolve_declared_property_type_hint(part, class_name, parent_name)
                    })
                    .collect(),
            ),
            concrete => concrete,
        }
    }

    /// Emit default parameter initialization for a single param.
    /// Pattern: BindDefaultParam (skip if arg passed) → compute default →
    /// AssignCv → optional generic check → label.
    fn emit_default_param(
        compiler: &mut Compiler,
        cv_idx: u16,
        parameter_index: u16,
        default_expr: &Expr,
        normalized_default: Option<Value>,
        check_generic_default: bool,
    ) {
        // BindDefaultParam: if CV is NOT undef, jump to skip_label (op2 = target, patched later)
        let bind_idx = compiler.instructions.len();
        let mut bind = Instruction::new(OpCode::BindDefaultParam);
        bind.op1_type = OpType::Cv;
        bind.op1 = cv_idx;
        bind.op2 = 0; // placeholder — will be patched to skip_label
        compiler.instructions.push(bind);

        // Compute default expression (only reached if arg was NOT passed)
        let (val_op, val_type) = match normalized_default {
            Some(value) => (compiler.add_literal(value), OpType::Const),
            None => compiler.compile_constant_expression(default_expr),
        };

        // Assign computed default to CV
        let mut assign = Instruction::new(OpCode::AssignCv);
        assign.op1_type = OpType::Cv;
        assign.op1 = cv_idx;
        assign.op2_type = val_type;
        assign.op2 = val_op;
        compiler.instructions.push(assign);

        if check_generic_default {
            let mut check = Instruction::new(OpCode::CheckGenericDefault);
            check.op1_type = OpType::Cv;
            check.op1 = cv_idx;
            check.extended_value = u32::from(parameter_index);
            compiler.instructions.push(check);
        }

        // An explicitly supplied argument skips both initialization and its
        // post-materialization check; the existing pre-call boundary owns it.
        let skip_label = compiler.instructions.len() as u16;
        compiler.instructions[bind_idx].op2 = skip_label;
    }

    fn resolve_static_member_owner(&self, class_name: &str) -> (String, bool) {
        let pseudo_class = class_name.to_ascii_lowercase();
        let dynamic_static_scope = pseudo_class == "static"
            || (self.dynamic_static_scope && matches!(pseudo_class.as_str(), "self" | "parent"));
        let resolved = match pseudo_class.as_str() {
            "static" => class_name.to_string(),
            "self" if !self.dynamic_static_scope => self
                .lexical_static_class
                .clone()
                .unwrap_or_else(|| class_name.to_string()),
            "parent" if !self.dynamic_static_scope => self
                .lexical_static_parent
                .clone()
                .unwrap_or_else(|| class_name.to_string()),
            "self" | "parent" => class_name.to_string(),
            _ => self.resolve_name(class_name),
        };
        (resolved, dynamic_static_scope)
    }

    /// Lower every static-property owner/name form to the shared two-operand
    /// VM protocol. Runtime names are explicitly cast once, preserving PHP's
    /// integer and `__toString()` member-name conversions and evaluation order.
    fn compile_static_property_operands(
        &mut self,
        expr: &Expr,
    ) -> Option<(u16, OpType, u16, OpType, bool, bool, usize)> {
        match expr {
            Expr::StaticProperty {
                class_name,
                property,
                line,
                ..
            } => {
                let (resolved, late_static) = self.resolve_static_member_owner(class_name);
                Some((
                    self.add_literal(Value::string(resolved)),
                    OpType::Const,
                    self.add_literal(Value::string(property.clone())),
                    OpType::Const,
                    late_static,
                    false,
                    *line,
                ))
            }
            Expr::DynamicNamedStaticProperty {
                class_name,
                property,
                line,
            } => {
                let (resolved, late_static) = self.resolve_static_member_owner(class_name);
                let class = self.add_literal(Value::string(resolved));
                let (property, property_type) = self.compile_expr(property);
                let property = self.emit_string_cast(property, property_type);
                Some((
                    class,
                    OpType::Const,
                    property,
                    OpType::Tmp,
                    late_static,
                    false,
                    *line,
                ))
            }
            Expr::DynamicStaticProperty {
                class,
                property,
                line,
            } => {
                let (class, class_type) = self.compile_expr(class);
                let (property, property_type) = self.compile_expr(property);
                let property = self.emit_string_cast(property, property_type);
                Some((class, class_type, property, OpType::Tmp, false, true, *line))
            }
            _ => None,
        }
    }

    fn compile_static_property_reference_fetch(
        &mut self,
        expr: &Expr,
        destination: u16,
        internal_result: bool,
    ) -> Result<(), String> {
        let (class, class_type, property, property_type, late_static, dynamic_owner, line) = self
            .compile_static_property_operands(expr)
            .ok_or_else(|| "Expected static-property reference source".to_string())?;
        let mut fetch = Instruction::new(if late_static {
            OpCode::FetchLateStaticProp
        } else {
            OpCode::FetchStaticProp
        });
        fetch.op1 = class;
        fetch.op1_type = class_type;
        fetch.op2 = property;
        fetch.op2_type = property_type;
        fetch.result = destination;
        fetch.result_type = OpType::Cv;
        fetch._pad |= STATIC_PROP_REFERENCE_FETCH;
        fetch._pad |= STATIC_PROP_INDIRECT_MODIFY;
        if internal_result {
            fetch._pad |= REFERENCE_RESULT_INTERNAL;
        }
        if dynamic_owner {
            fetch._pad |= STATIC_PROP_DYNAMIC_OWNER;
        }
        if property_type != OpType::Const {
            fetch._pad |= STATIC_PROP_DYNAMIC_NAME;
        }
        self.push_instruction_at_line(fetch, line);
        Ok(())
    }

    fn compile_static_property_reference_assignment(
        &mut self,
        expr: &Expr,
        source: u16,
        source_type: OpType,
        source_is_internal: bool,
    ) -> Result<(), String> {
        let (class, class_type, property, property_type, late_static, dynamic_owner, line) = self
            .compile_static_property_operands(expr)
            .ok_or_else(|| "Expected static-property reference target".to_string())?;
        let mut assign = Instruction::new(if late_static {
            OpCode::AssignLateStaticProp
        } else {
            OpCode::AssignStaticProp
        });
        assign.op1 = class;
        assign.op1_type = class_type;
        assign.op2 = property;
        assign.op2_type = property_type;
        assign.result = source;
        assign.result_type = source_type;
        assign._pad |= STATIC_PROP_REFERENCE_BIND;
        assign._pad |= STATIC_PROP_INDIRECT_MODIFY;
        if source_is_internal {
            assign._pad |= REFERENCE_RESULT_INTERNAL;
        }
        if dynamic_owner {
            assign._pad |= STATIC_PROP_DYNAMIC_OWNER;
        }
        if property_type != OpType::Const {
            assign._pad |= STATIC_PROP_DYNAMIC_NAME;
        }
        self.push_instruction_at_line(assign, line);
        Ok(())
    }

    fn emit_string_cast(&mut self, operand: u16, operand_type: OpType) -> u16 {
        let result = self.alloc_tmp();
        let mut cast = Instruction::new(OpCode::Cast);
        cast.op1 = operand;
        cast.op1_type = operand_type;
        cast.result = result;
        cast.result_type = OpType::Tmp;
        cast.extended_value = CastType::String as u32;
        self.instructions.push(cast);
        result
    }

    /// Compile the receiver chain of a property write. Intermediate property
    /// reads are mutable l-values: PHP throws `Attempt to modify property`
    /// when any receiver in the chain is null or scalar.
    fn compile_property_modify_base(&mut self, expr: &Expr) -> (u16, OpType) {
        match expr {
            Expr::PropertyAccess {
                object,
                property,
                nullsafe: false,
                line,
            } => {
                let (object, object_type) = self.compile_property_modify_base(object);
                let property = self.add_literal(Value::string(property.clone()));
                let result = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchObjR);
                fetch.op1 = object;
                fetch.op1_type = object_type;
                fetch.op2 = property;
                fetch.op2_type = OpType::Const;
                fetch.result = result;
                fetch.result_type = OpType::Tmp;
                fetch._pad |= FETCH_OBJ_MODIFY;
                self.push_instruction_at_line(fetch, *line);
                (result, OpType::Tmp)
            }
            Expr::DynamicPropertyAccess {
                object,
                property,
                nullsafe: false,
                line,
            } => {
                let (object, object_type) = self.compile_property_modify_base(object);
                let (property, property_type) = self.compile_expr(property);
                let result = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchObjR);
                fetch.op1 = object;
                fetch.op1_type = object_type;
                fetch.op2 = property;
                fetch.op2_type = property_type;
                fetch.result = result;
                fetch.result_type = OpType::Tmp;
                fetch._pad |= FETCH_OBJ_MODIFY;
                self.push_instruction_at_line(fetch, *line);
                (result, OpType::Tmp)
            }
            _ => self.compile_expr(expr),
        }
    }

    /// Prepare a mutable receiver chain while deferring the property fetches
    /// until the assignment value has been evaluated. PHP evaluates the base
    /// and dynamic names first, then the RHS, and only then reports that an
    /// intermediate scalar property cannot be modified.
    fn prepare_property_modify_base(
        &mut self,
        expr: &Expr,
    ) -> (u16, OpType, Vec<(Instruction, usize)>) {
        match expr {
            Expr::PropertyAccess {
                object,
                property,
                nullsafe: false,
                line,
            } => {
                let (object, object_type, mut deferred) = self.prepare_property_modify_base(object);
                let property = self.add_literal(Value::string(property.clone()));
                let result = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchObjR);
                fetch.op1 = object;
                fetch.op1_type = object_type;
                fetch.op2 = property;
                fetch.op2_type = OpType::Const;
                fetch.result = result;
                fetch.result_type = OpType::Tmp;
                fetch._pad |= FETCH_OBJ_MODIFY;
                deferred.push((fetch, *line));
                (result, OpType::Tmp, deferred)
            }
            Expr::DynamicPropertyAccess {
                object,
                property,
                nullsafe: false,
                line,
            } => {
                let (object, object_type, mut deferred) = self.prepare_property_modify_base(object);
                let (property, property_type) = self.compile_expr(property);
                let result = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchObjR);
                fetch.op1 = object;
                fetch.op1_type = object_type;
                fetch.op2 = property;
                fetch.op2_type = property_type;
                fetch.result = result;
                fetch.result_type = OpType::Tmp;
                fetch._pad |= FETCH_OBJ_MODIFY;
                deferred.push((fetch, *line));
                (result, OpType::Tmp, deferred)
            }
            _ => {
                let (operand, operand_type) = self.compile_expr(expr);
                (operand, operand_type, Vec::new())
            }
        }
    }

    /// Compile expression. Returns (operand_index, OpType).
    fn compile_isset_object_base(&mut self, expr: &Expr) -> (u16, OpType) {
        match expr {
            Expr::Variable { name, .. } => (self.resolve_cv(name), OpType::Cv),
            Expr::PropertyAccess {
                object,
                property,
                nullsafe: _,
                line,
            } => {
                let (object_op, object_type) = self.compile_isset_object_base(object);
                let property_op = self.add_literal(Value::string(property.clone()));
                let result = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchObjR);
                fetch.op1 = object_op;
                fetch.op1_type = object_type;
                fetch.op2 = property_op;
                fetch.op2_type = OpType::Const;
                fetch.result = result;
                fetch.result_type = OpType::Tmp;
                fetch._pad |= FETCH_OBJ_SILENT;
                self.push_instruction_at_line(fetch, *line);
                (result, OpType::Tmp)
            }
            Expr::DynamicPropertyAccess {
                object,
                property,
                nullsafe: _,
                line,
            } => {
                let (object_op, object_type) = self.compile_isset_object_base(object);
                let (property_op, property_type) = self.compile_expr(property);
                let result = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchObjR);
                fetch.op1 = object_op;
                fetch.op1_type = object_type;
                fetch.op2 = property_op;
                fetch.op2_type = property_type;
                fetch.result = result;
                fetch.result_type = OpType::Tmp;
                fetch._pad |= FETCH_OBJ_SILENT;
                self.push_instruction_at_line(fetch, *line);
                (result, OpType::Tmp)
            }
            static_property @ (Expr::StaticProperty { .. }
            | Expr::DynamicNamedStaticProperty { .. }
            | Expr::DynamicStaticProperty { .. }) => {
                let (class, class_type, property, property_type, late_static, dynamic_owner, line) =
                    self.compile_static_property_operands(static_property)
                        .expect("matched static-property form");
                let result = self.alloc_tmp();
                let mut fetch = Instruction::new(if late_static {
                    OpCode::FetchLateStaticProp
                } else {
                    OpCode::FetchStaticProp
                });
                fetch.op1 = class;
                fetch.op1_type = class_type;
                fetch.op2 = property;
                fetch.op2_type = property_type;
                fetch.result = result;
                fetch.result_type = OpType::Tmp;
                fetch._pad |= STATIC_PROP_SILENT;
                if dynamic_owner {
                    fetch._pad |= STATIC_PROP_DYNAMIC_OWNER;
                }
                if property_type != OpType::Const {
                    fetch._pad |= STATIC_PROP_DYNAMIC_NAME;
                }
                self.push_instruction_at_line(fetch, line);
                (result, OpType::Tmp)
            }
            Expr::ArrayAccess { array, index, .. } => {
                if matches!(array.as_ref(), Expr::Globals { .. }) {
                    let (key, key_type) = self.compile_expr(index);
                    let result = self.alloc_tmp();
                    let mut isset = Instruction::new(OpCode::FetchGlobal);
                    isset.op1 = key;
                    isset.op1_type = key_type;
                    isset.result = result;
                    isset.result_type = OpType::Tmp;
                    isset._pad |= FETCH_DIM_ISSET;
                    self.instructions.push(isset);
                    return (result, OpType::Tmp);
                }
                let (array_op, array_type) = self.compile_isset_object_base(array);
                let (index_op, index_type) = self.compile_expr(index);
                let result = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchDimR);
                fetch.op1 = array_op;
                fetch.op1_type = array_type;
                fetch.op2 = index_op;
                fetch.op2_type = index_type;
                fetch.result = result;
                fetch.result_type = OpType::Tmp;
                fetch._pad |= FETCH_DIM_SILENT;
                self.instructions.push(fetch);
                (result, OpType::Tmp)
            }
            _ => self.compile_expr(expr),
        }
    }

    fn compile_isset_operand(&mut self, expr: &Expr) -> (u16, OpType) {
        match expr {
            Expr::DynamicVariable { name, line } => {
                let (name, name_type) = self.compile_expr(name);
                let result = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchDynamicVar);
                fetch.op1 = name;
                fetch.op1_type = name_type;
                fetch.result = result;
                fetch.result_type = OpType::Tmp;
                fetch._pad |= FETCH_DIM_ISSET;
                self.push_instruction_at_line(fetch, *line);
                (result, OpType::Tmp)
            }
            Expr::PropertyAccess {
                object,
                property,
                nullsafe: _,
                line,
            } => {
                let (object_op, object_type) = self.compile_isset_object_base(object);
                let property_op = self.add_literal(Value::string(property.clone()));
                let result = self.alloc_tmp();
                let mut isset = Instruction::new(OpCode::IssetObj);
                isset.op1 = object_op;
                isset.op1_type = object_type;
                isset.op2 = property_op;
                isset.op2_type = OpType::Const;
                isset.result = result;
                isset.result_type = OpType::Tmp;
                self.push_instruction_at_line(isset, *line);
                (result, OpType::Tmp)
            }
            Expr::ArrayAccess { array, index, .. } => {
                if matches!(array.as_ref(), Expr::Globals { .. }) {
                    let (key, key_type) = self.compile_expr(index);
                    let result = self.alloc_tmp();
                    let mut isset = Instruction::new(OpCode::FetchGlobal);
                    isset.op1 = key;
                    isset.op1_type = key_type;
                    isset.result = result;
                    isset.result_type = OpType::Tmp;
                    isset._pad |= FETCH_DIM_ISSET;
                    self.instructions.push(isset);
                    return (result, OpType::Tmp);
                }
                let (array_op, array_type) = self.compile_isset_object_base(array);
                let (index_op, index_type) = self.compile_expr(index);
                let result = self.alloc_tmp();
                let mut isset = Instruction::new(OpCode::FetchDimR);
                isset.op1 = array_op;
                isset.op1_type = array_type;
                isset.op2 = index_op;
                isset.op2_type = index_type;
                isset.result = result;
                isset.result_type = OpType::Tmp;
                isset._pad |= FETCH_DIM_ISSET;
                self.instructions.push(isset);
                (result, OpType::Tmp)
            }
            _ => self.compile_isset_object_base(expr),
        }
    }

    fn compile_variable_read(&mut self, name: &str, line: usize) -> (u16, OpType) {
        let cv = self.resolve_cv(name);
        // `$this` has its own "not in object context" contract; it is never
        // an ordinary undefined-variable warning.
        if line == 0 || name == "this" || self.definitely_defined_cvs.contains(&cv) {
            return (cv, OpType::Cv);
        }
        let name_literal = self.add_literal(Value::string(name.to_string()));
        let result = self.alloc_tmp();
        let mut fetch = Instruction::new(OpCode::FetchCvR);
        fetch.op1 = cv;
        fetch.op1_type = OpType::Cv;
        fetch.op2 = name_literal;
        fetch.op2_type = OpType::Const;
        fetch.result = result;
        fetch.result_type = OpType::Tmp;
        self.push_instruction_at_line(fetch, line);
        // A user error handler is an arbitrary re-entrant call and may mutate
        // the top-level symbol table. Function-local CV bindings themselves
        // remain defined across ordinary calls. Keep the snapshot result and
        // invalidate only the proofs reachable from this scope.
        self.invalidate_reentrant_definitions();
        (result, OpType::Tmp)
    }

    /// Lower a left-associative concatenation chain without recursively
    /// retaining one large `compile_expr` frame per operand. Real generated
    /// PHP commonly contains wide concatenations, and the compiler must not
    /// depend on the comparatively small stack assigned to a test/request
    /// thread.
    fn compile_concat_chain(&mut self, left: &Expr, right: &Expr) -> (u16, OpType) {
        let mut reversed = vec![right];
        let mut first = left;
        while let Expr::BinaryOp {
            op: BinOp::Concat,
            left,
            right,
        } = first
        {
            reversed.push(right);
            first = left;
        }

        let mut accumulated = self.compile_expr(first);
        for operand in reversed.into_iter().rev() {
            let next = self.compile_expr(operand);
            let result = self.alloc_tmp();
            let mut concat = Instruction::new(OpCode::Concat);
            concat.op1 = accumulated.0;
            concat.op1_type = accumulated.1;
            concat.op2 = next.0;
            concat.op2_type = next.1;
            concat.result = result;
            concat.result_type = OpType::Tmp;
            self.instructions.push(concat);
            accumulated = (result, OpType::Tmp);
        }
        accumulated
    }

    /// Compile a parser-produced left-deep addition chain iteratively. Dynamic
    /// eval can legitimately receive generated expressions with thousands of
    /// terms; retaining one Rust frame per `+` would turn valid PHP into a
    /// process stack overflow before the VM sees any bytecode.
    fn compile_add_chain(&mut self, left: &Expr, right: &Expr) -> (u16, OpType) {
        let mut reversed = vec![right];
        let mut first = left;
        while let Expr::BinaryOp {
            op: BinOp::Add,
            left,
            right,
        } = first
        {
            reversed.push(right);
            first = left;
        }

        let mut accumulated = self.compile_expr(first);
        for operand in reversed.into_iter().rev() {
            let next = self.compile_expr(operand);
            let result = self.alloc_tmp();
            let mut add = Instruction::new(OpCode::Add);
            add.op1 = accumulated.0;
            add.op1_type = accumulated.1;
            add.op2 = next.0;
            add.op2_type = next.1;
            add.result = result;
            add.result_type = OpType::Tmp;
            self.instructions.push(add);
            accumulated = (result, OpType::Tmp);
        }
        accumulated
    }

    fn compile_expr(&mut self, expr: &Expr) -> (u16, OpType) {
        match expr {
            Expr::Integer(n) => {
                let idx = self.add_literal(Value::long(*n));
                (idx, OpType::Const)
            }
            Expr::Float(f) => {
                let idx = self.add_literal(Value::double(*f));
                (idx, OpType::Const)
            }
            Expr::StringLiteral(s) => {
                let idx = self.add_literal(Value::string(s.clone()));
                (idx, OpType::Const)
            }
            Expr::Null => {
                let idx = self.add_literal(Value::null());
                (idx, OpType::Const)
            }
            Expr::Bool(b) => {
                let idx = self.add_literal(Value::bool(*b));
                (idx, OpType::Const)
            }
            Expr::Variable { name, line } => self.compile_variable_read(name, *line),
            Expr::DynamicVariable { name, line } => {
                let (name, name_type) = self.compile_expr(name);
                let result = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchDynamicVar);
                fetch.op1 = name;
                fetch.op1_type = name_type;
                fetch.result = result;
                fetch.result_type = OpType::Tmp;
                self.push_instruction_at_line(fetch, *line);
                self.invalidate_reentrant_definitions();
                (result, OpType::Tmp)
            }
            Expr::Globals { .. } => {
                let result = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchGlobals);
                fetch.result = result;
                fetch.result_type = OpType::Tmp;
                self.instructions.push(fetch);
                (result, OpType::Tmp)
            }
            Expr::CompileError { message, line } => {
                self.deferred_error = Some(self.goto_error(message, *line));
                let null = self.add_literal(Value::null());
                (null, OpType::Const)
            }
            Expr::CompileWarning { .. } => {
                let null = self.add_literal(Value::null());
                (null, OpType::Const)
            }
            Expr::CompileDeprecation { .. } => {
                let null = self.add_literal(Value::null());
                (null, OpType::Const)
            }
            Expr::ArrayAppendArgument { line, .. } => {
                self.deferred_error = Some(self.goto_error("Cannot use [] for reading", *line));
                let null = self.add_literal(Value::null());
                (null, OpType::Const)
            }
            Expr::Pipe {
                input,
                callable,
                line,
            } => {
                // PHP evaluates the complete input before evaluating the
                // callable expression. SendVal deliberately prevents a pipe
                // result from satisfying a by-reference parameter.
                let (input, input_type) = self.compile_expr(input);
                let (callable, callable_type) = self.compile_expr(callable);
                let mut init = Instruction::new(OpCode::InitDynamicCall);
                init.op1 = callable;
                init.op1_type = callable_type;
                init.extended_value = 1;
                self.push_instruction_at_line(init, *line);

                let mut send = Instruction::new(OpCode::SendVal);
                send.op1 = input;
                send.op1_type = input_type;
                send._pad |= SEND_FLAG_NONREFERENCEABLE;
                self.push_instruction_at_line(send, *line);

                let result = self.alloc_tmp();
                let mut call = Instruction::new(OpCode::DoFcall);
                call.result = result;
                call.result_type = OpType::Tmp;
                self.push_instruction_at_line(call, *line);
                (result, OpType::Tmp)
            }
            Expr::BinaryOp { op, left, right } => {
                if matches!(op, BinOp::Concat) {
                    return self.compile_concat_chain(left, right);
                }
                if matches!(op, BinOp::Add) {
                    return self.compile_add_chain(left, right);
                }
                // Short-circuit logical operators
                match op {
                    BinOp::And => {
                        // $a && $b: eval left, JmpZ → false, eval right, JmpZ → false,
                        // result=true, Jmp→end, false: result=false, end:
                        let (l_op, l_type) = self.compile_expr(left);
                        let conditional_entry = Box::new(self.definitely_defined_cvs.clone());
                        let tmp = self.alloc_tmp();

                        let jmpz_left = self.instructions.len();
                        let mut jmpz = Instruction::new(OpCode::JmpZ);
                        jmpz.op1 = l_op;
                        jmpz.op1_type = l_type;
                        jmpz.op2 = 0; // → false_label
                        self.instructions.push(jmpz);

                        let (r_op, r_type) = self.compile_expr(right);

                        let jmpz_right = self.instructions.len();
                        let mut jmpz2 = Instruction::new(OpCode::JmpZ);
                        jmpz2.op1 = r_op;
                        jmpz2.op1_type = r_type;
                        jmpz2.op2 = 0; // → false_label
                        self.instructions.push(jmpz2);

                        // Both truthy → true
                        let true_lit = self.add_literal(Value::bool(true));
                        let mut set_true = Instruction::new(OpCode::AssignCv);
                        set_true.op1_type = OpType::Tmp;
                        set_true.op1 = tmp;
                        set_true.op2_type = OpType::Const;
                        set_true.op2 = true_lit;
                        self.instructions.push(set_true);

                        let jmp_end = self.instructions.len();
                        let mut jmp = Instruction::new(OpCode::Jmp);
                        jmp.op1 = 0; // → end
                        self.instructions.push(jmp);

                        // false_label
                        let false_label = self.instructions.len() as u16;
                        let false_lit = self.add_literal(Value::bool(false));
                        let mut set_false = Instruction::new(OpCode::AssignCv);
                        set_false.op1_type = OpType::Tmp;
                        set_false.op1 = tmp;
                        set_false.op2_type = OpType::Const;
                        set_false.op2 = false_lit;
                        self.instructions.push(set_false);

                        let end_label = self.instructions.len() as u16;
                        self.instructions[jmpz_left].op2 = false_label;
                        self.instructions[jmpz_right].op2 = false_label;
                        self.instructions[jmp_end].op1 = end_label;
                        // The RHS is skipped when the LHS is false, so only
                        // definitions established by evaluating the LHS are
                        // guaranteed after the complete expression.
                        self.definitely_defined_cvs = *conditional_entry;

                        return (tmp, OpType::Tmp);
                    }
                    BinOp::Or => {
                        // $a || $b: evaluate $a, if true skip $b
                        let (l_op, l_type) = self.compile_expr(left);
                        let conditional_entry = Box::new(self.definitely_defined_cvs.clone());
                        let tmp = self.alloc_tmp();

                        // JmpNZ left, <true_label> — if left is true, short-circuit
                        let jmpnz_idx = self.instructions.len();
                        let mut jmpnz = Instruction::new(OpCode::JmpNZ);
                        jmpnz.op1 = l_op;
                        jmpnz.op1_type = l_type;
                        jmpnz.op2 = 0; // placeholder
                        self.instructions.push(jmpnz);

                        // Left was falsy — evaluate right
                        let (r_op, r_type) = self.compile_expr(right);

                        // JmpNZ right, <true_label>
                        let jmpnz2_idx = self.instructions.len();
                        let mut jmpnz2 = Instruction::new(OpCode::JmpNZ);
                        jmpnz2.op1 = r_op;
                        jmpnz2.op1_type = r_type;
                        jmpnz2.op2 = 0; // placeholder
                        self.instructions.push(jmpnz2);

                        // Both falsy → result = false
                        let false_lit = self.add_literal(Value::bool(false));
                        let mut set_false = Instruction::new(OpCode::AssignCv);
                        set_false.op1_type = OpType::Tmp;
                        set_false.op1 = tmp;
                        set_false.op2_type = OpType::Const;
                        set_false.op2 = false_lit;
                        self.instructions.push(set_false);

                        // Jmp to end
                        let jmp_end_idx = self.instructions.len();
                        let mut jmp_end = Instruction::new(OpCode::Jmp);
                        jmp_end.op1 = 0; // placeholder
                        self.instructions.push(jmp_end);

                        // true_label: result = true
                        let true_label = self.instructions.len() as u16;
                        let true_lit = self.add_literal(Value::bool(true));
                        let mut set_true = Instruction::new(OpCode::AssignCv);
                        set_true.op1_type = OpType::Tmp;
                        set_true.op1 = tmp;
                        set_true.op2_type = OpType::Const;
                        set_true.op2 = true_lit;
                        self.instructions.push(set_true);

                        let end_label = self.instructions.len() as u16;

                        // Patch jumps
                        self.instructions[jmpnz_idx].op2 = true_label;
                        self.instructions[jmpnz2_idx].op2 = true_label;
                        self.instructions[jmp_end_idx].op1 = end_label;
                        self.definitely_defined_cvs = *conditional_entry;

                        return (tmp, OpType::Tmp);
                    }
                    BinOp::LogicalXor => {
                        let (left, left_type) = self.compile_expr(left);
                        let left_bool = self.alloc_tmp();
                        let mut bool_left = Instruction::new(OpCode::BoolNot);
                        bool_left.op1 = left;
                        bool_left.op1_type = left_type;
                        bool_left.result = left_bool;
                        bool_left.result_type = OpType::Tmp;
                        self.instructions.push(bool_left);

                        let (right, right_type) = self.compile_expr(right);
                        let right_bool = self.alloc_tmp();
                        let mut bool_right = Instruction::new(OpCode::BoolNot);
                        bool_right.op1 = right;
                        bool_right.op1_type = right_type;
                        bool_right.result = right_bool;
                        bool_right.result_type = OpType::Tmp;
                        self.instructions.push(bool_right);

                        let result = self.alloc_tmp();
                        let mut compare = Instruction::new(OpCode::IsNotIdentical);
                        compare.op1 = left_bool;
                        compare.op1_type = OpType::Tmp;
                        compare.op2 = right_bool;
                        compare.op2_type = OpType::Tmp;
                        compare.result = result;
                        compare.result_type = OpType::Tmp;
                        self.instructions.push(compare);
                        return (result, OpType::Tmp);
                    }
                    _ => {}
                }

                let (l_op, l_type) = self.compile_expr(left);
                let (r_op, r_type) = self.compile_expr(right);
                let tmp = self.alloc_tmp();

                let opcode = match op {
                    BinOp::Add => OpCode::Add,
                    BinOp::Sub => OpCode::Sub,
                    BinOp::Mul => OpCode::Mul,
                    BinOp::Div => OpCode::Div,
                    BinOp::Mod => OpCode::Mod,
                    BinOp::Concat => unreachable!("concatenation chain handled above"),
                    BinOp::Equal => OpCode::IsEqual,
                    BinOp::NotEqual => OpCode::IsNotEqual,
                    BinOp::Identical => OpCode::IsIdentical,
                    BinOp::NotIdentical => OpCode::IsNotIdentical,
                    BinOp::Less => OpCode::IsSmaller,
                    BinOp::LessEqual => OpCode::IsSmallerOrEqual,
                    // PHP has no IS_GREATER opcode — it swaps operands
                    BinOp::Greater => OpCode::IsSmaller,
                    BinOp::GreaterEqual => OpCode::IsSmallerOrEqual,
                    BinOp::Spaceship => OpCode::Spaceship,
                    BinOp::Pow => OpCode::Pow,
                    BinOp::BitwiseAnd => OpCode::BitwiseAnd,
                    BinOp::BitwiseOr => OpCode::BitwiseOr,
                    BinOp::BitwiseXor => OpCode::BitwiseXor,
                    BinOp::ShiftLeft => OpCode::ShiftLeft,
                    BinOp::ShiftRight => OpCode::ShiftRight,
                    BinOp::And | BinOp::Or | BinOp::LogicalXor => unreachable!(), // handled above
                };

                // For > and >=, swap operands (PHP convention)
                let (l_op, l_type, r_op, r_type) = match op {
                    BinOp::Greater | BinOp::GreaterEqual => (r_op, r_type, l_op, l_type),
                    _ => (l_op, l_type, r_op, r_type),
                };

                let mut instr = Instruction::new(opcode);
                instr.op1 = l_op;
                instr.op1_type = l_type;
                instr.op2 = r_op;
                instr.op2_type = r_type;
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                self.instructions.push(instr);

                (tmp, OpType::Tmp)
            }
            Expr::CoalesceAssign { target, expr } => {
                match self.compile_coalesce_assign_expression(target, expr) {
                    Ok(result) => result,
                    Err(error) => {
                        self.deferred_error = Some(error);
                        let null = self.add_literal(Value::null());
                        (null, OpType::Const)
                    }
                }
            }
            Expr::CompoundAssignExpression { target, op, expr } => {
                let direct_cv = if let Expr::Variable { name, .. } = target.as_ref() {
                    Some(self.resolve_cv(name))
                } else {
                    None
                };
                let (left, left_type, writeback, right, right_type) = if let Some(cv) = direct_cv {
                    // PHP resolves a simple CV destination first, evaluates
                    // the RHS, and only then consumes the current CV value.
                    // A re-entrant call on the RHS may therefore unset or
                    // replace a main-scope global before the read happens.
                    let (right, right_type) = self.compile_expr(expr);
                    let (left, left_type) = self.compile_expr(target);
                    (
                        left,
                        left_type,
                        ForeachArrayWriteback::Variable(cv),
                        right,
                        right_type,
                    )
                } else {
                    match target.as_ref() {
                        Expr::PropertyAccess {
                            object,
                            property,
                            nullsafe: false,
                            line,
                        } => {
                            let (object, object_type, mut deferred) =
                                self.prepare_property_modify_base(object);
                            let property = self.add_literal(Value::string(property.clone()));
                            let left = self.alloc_tmp();
                            let mut fetch = Instruction::new(OpCode::FetchObjR);
                            fetch.op1 = object;
                            fetch.op1_type = object_type;
                            fetch.op2 = property;
                            fetch.op2_type = OpType::Const;
                            fetch.result = left;
                            fetch.result_type = OpType::Tmp;
                            fetch._pad |= FETCH_OBJ_COMPOUND;
                            deferred.push((fetch, *line));
                            let (right, right_type) = self.compile_expr(expr);
                            for (fetch, line) in deferred {
                                self.push_instruction_at_line(fetch, line);
                            }
                            (
                                left,
                                OpType::Tmp,
                                ForeachArrayWriteback::ObjectProperty {
                                    object,
                                    object_type,
                                    property,
                                    property_type: OpType::Const,
                                    line: *line,
                                },
                                right,
                                right_type,
                            )
                        }
                        Expr::DynamicPropertyAccess {
                            object,
                            property,
                            nullsafe: false,
                            line,
                        } => {
                            let (object, object_type, mut deferred) =
                                self.prepare_property_modify_base(object);
                            let (property, property_type) = self.compile_expr(property);
                            let left = self.alloc_tmp();
                            let mut fetch = Instruction::new(OpCode::FetchObjR);
                            fetch.op1 = object;
                            fetch.op1_type = object_type;
                            fetch.op2 = property;
                            fetch.op2_type = property_type;
                            fetch.result = left;
                            fetch.result_type = OpType::Tmp;
                            fetch._pad |= FETCH_OBJ_COMPOUND;
                            deferred.push((fetch, *line));
                            let (right, right_type) = self.compile_expr(expr);
                            for (fetch, line) in deferred {
                                self.push_instruction_at_line(fetch, line);
                            }
                            (
                                left,
                                OpType::Tmp,
                                ForeachArrayWriteback::ObjectProperty {
                                    object,
                                    object_type,
                                    property,
                                    property_type,
                                    line: *line,
                                },
                                right,
                                right_type,
                            )
                        }
                        _ => {
                            let mut root = target.as_ref();
                            while let Expr::ArrayAccess { array, .. } = root {
                                root = array;
                            }
                            let defer_temporary_array_fetches =
                                self.is_known_user_function_call(root);
                            let (left, left_type, writeback) =
                                match self.compile_foreach_reference_source(target, false, true) {
                                    Ok(source) => source,
                                    Err(error) => {
                                        self.deferred_error = Some(error);
                                        let null = self.add_literal(Value::null());
                                        return (null, OpType::Const);
                                    }
                                };
                            if matches!(&writeback, ForeachArrayWriteback::Array(_)) {
                                self.mark_trailing_mutable_dimension_fetches();
                            }
                            let mut deferred_fetches = Vec::new();
                            if defer_temporary_array_fetches
                                && matches!(&writeback, ForeachArrayWriteback::Array(_))
                            {
                                while self.instructions.last().is_some_and(|instruction| {
                                    instruction.opcode == OpCode::FetchDimR
                                }) {
                                    deferred_fetches.push(
                                        self.instructions
                                            .pop()
                                            .expect("checked trailing dimension fetch"),
                                    );
                                }
                                deferred_fetches.reverse();
                            }
                            let (right, right_type) = self.compile_expr(expr);
                            self.instructions.extend(deferred_fetches);
                            (left, left_type, writeback, right, right_type)
                        }
                    }
                };
                let opcode = match op {
                    BinOp::Add => OpCode::Add,
                    BinOp::Sub => OpCode::Sub,
                    BinOp::Mul => OpCode::Mul,
                    BinOp::Div => OpCode::Div,
                    BinOp::Mod => OpCode::Mod,
                    BinOp::Concat => OpCode::Concat,
                    BinOp::Pow => OpCode::Pow,
                    BinOp::BitwiseAnd => OpCode::BitwiseAnd,
                    BinOp::BitwiseOr => OpCode::BitwiseOr,
                    BinOp::BitwiseXor => OpCode::BitwiseXor,
                    BinOp::ShiftLeft => OpCode::ShiftLeft,
                    BinOp::ShiftRight => OpCode::ShiftRight,
                    _ => {
                        self.deferred_error = Some("Invalid compound assignment operator".into());
                        let null = self.add_literal(Value::null());
                        return (null, OpType::Const);
                    }
                };
                let result = self.alloc_tmp();
                let mut operation = Instruction::new(opcode);
                operation.op1 = left;
                operation.op1_type = left_type;
                operation.op2 = right;
                operation.op2_type = right_type;
                operation.result = result;
                operation.result_type = OpType::Tmp;
                self.instructions.push(operation);
                self.emit_foreach_reference_source_writeback(writeback, result, OpType::Tmp);
                if let Some(cv) = direct_cv {
                    self.definitely_defined_cvs.insert(cv);
                }
                (result, OpType::Tmp)
            }
            Expr::PostInc { name, line } => {
                let (source, source_type) = self.compile_variable_read(name, *line);
                let cv_idx = self.resolve_cv(name);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::PostInc);
                instr.op1_type = source_type;
                instr.op1 = source;
                if source_type != OpType::Cv {
                    instr.op2 = cv_idx;
                    instr.op2_type = OpType::Cv;
                }
                instr.result_type = OpType::Tmp;
                instr.result = tmp;
                self.push_instruction_at_line(instr, *line);
                self.definitely_defined_cvs.insert(cv_idx);
                (tmp, OpType::Tmp)
            }
            Expr::PostDec { name, line } => {
                let (source, source_type) = self.compile_variable_read(name, *line);
                let cv_idx = self.resolve_cv(name);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::PostDec);
                instr.op1_type = source_type;
                instr.op1 = source;
                if source_type != OpType::Cv {
                    instr.op2 = cv_idx;
                    instr.op2_type = OpType::Cv;
                }
                instr.result_type = OpType::Tmp;
                instr.result = tmp;
                self.push_instruction_at_line(instr, *line);
                self.definitely_defined_cvs.insert(cv_idx);
                (tmp, OpType::Tmp)
            }
            Expr::PostIncTarget(target) | Expr::PostDecTarget(target) => {
                let source_line = incdec_target_source_line(target);
                let (current, current_type, writeback) =
                    match self.compile_foreach_reference_source(target, false, true) {
                        Ok(source) => source,
                        Err(error) => {
                            self.deferred_error = Some(error);
                            let null = self.add_literal(Value::null());
                            return (null, OpType::Const);
                        }
                    };
                if matches!(&writeback, ForeachArrayWriteback::ObjectProperty { .. })
                    && let Some(fetch) = self.instructions.iter_mut().rev().find(|instruction| {
                        instruction.opcode == OpCode::FetchObjR
                            && instruction.result == current
                            && instruction.result_type == current_type
                    })
                {
                    fetch._pad |= FETCH_OBJ_INCDEC;
                }
                if matches!(&writeback, ForeachArrayWriteback::Array(_)) {
                    self.mark_trailing_mutable_dimension_fetches();
                    self.record_last_instruction_source_line(source_line);
                }
                let property_writeback = matches!(
                    &writeback,
                    ForeachArrayWriteback::ObjectProperty { .. }
                        | ForeachArrayWriteback::StaticProperty { .. }
                );
                let original = self.alloc_tmp();
                let mut preserve = Instruction::new(OpCode::AssignCv);
                preserve.op1 = original;
                preserve.op1_type = OpType::Tmp;
                preserve.op2 = current;
                preserve.op2_type = current_type;
                self.instructions.push(preserve);

                let updated = self.alloc_tmp();
                let mut operation = Instruction::new(if matches!(expr, Expr::PostIncTarget(_)) {
                    OpCode::PreInc
                } else {
                    OpCode::PreDec
                });
                operation.op1 = original;
                operation.op1_type = OpType::Tmp;
                operation.result = updated;
                operation.result_type = OpType::Tmp;
                self.push_instruction_at_line(operation, source_line);
                self.emit_foreach_reference_source_writeback(writeback, updated, OpType::Tmp);
                if property_writeback {
                    self.mark_last_property_incdec_writeback(matches!(
                        expr,
                        Expr::PostIncTarget(_)
                    ));
                }
                (original, OpType::Tmp)
            }
            Expr::PreInc { name, line } => {
                let (source, source_type) = self.compile_variable_read(name, *line);
                let cv_idx = self.resolve_cv(name);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::PreInc);
                instr.op1_type = source_type;
                instr.op1 = source;
                if source_type != OpType::Cv {
                    instr.op2 = cv_idx;
                    instr.op2_type = OpType::Cv;
                }
                instr.result_type = OpType::Tmp;
                instr.result = tmp;
                self.push_instruction_at_line(instr, *line);
                self.definitely_defined_cvs.insert(cv_idx);
                (tmp, OpType::Tmp)
            }
            Expr::PreDec { name, line } => {
                let (source, source_type) = self.compile_variable_read(name, *line);
                let cv_idx = self.resolve_cv(name);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::PreDec);
                instr.op1_type = source_type;
                instr.op1 = source;
                if source_type != OpType::Cv {
                    instr.op2 = cv_idx;
                    instr.op2_type = OpType::Cv;
                }
                instr.result_type = OpType::Tmp;
                instr.result = tmp;
                self.push_instruction_at_line(instr, *line);
                self.definitely_defined_cvs.insert(cv_idx);
                (tmp, OpType::Tmp)
            }
            Expr::PreIncTarget(target) | Expr::PreDecTarget(target) => {
                let source_line = incdec_target_source_line(target);
                let (left, left_type, writeback) =
                    match self.compile_foreach_reference_source(target, false, true) {
                        Ok(source) => source,
                        Err(error) => {
                            self.deferred_error = Some(error);
                            let null = self.add_literal(Value::null());
                            return (null, OpType::Const);
                        }
                    };
                if matches!(&writeback, ForeachArrayWriteback::ObjectProperty { .. })
                    && let Some(fetch) = self.instructions.iter_mut().rev().find(|instruction| {
                        instruction.opcode == OpCode::FetchObjR
                            && instruction.result == left
                            && instruction.result_type == left_type
                    })
                {
                    fetch._pad |= FETCH_OBJ_INCDEC;
                }
                if matches!(&writeback, ForeachArrayWriteback::Array(_)) {
                    self.mark_trailing_mutable_dimension_fetches();
                    self.record_last_instruction_source_line(source_line);
                }
                let property_writeback = matches!(
                    &writeback,
                    ForeachArrayWriteback::ObjectProperty { .. }
                        | ForeachArrayWriteback::StaticProperty { .. }
                );
                let result = self.alloc_tmp();
                let mut operation = Instruction::new(if matches!(expr, Expr::PreIncTarget(_)) {
                    OpCode::PreInc
                } else {
                    OpCode::PreDec
                });
                operation.op1 = left;
                operation.op1_type = left_type;
                operation.result = result;
                operation.result_type = OpType::Tmp;
                self.push_instruction_at_line(operation, source_line);
                self.emit_foreach_reference_source_writeback(writeback, result, OpType::Tmp);
                if property_writeback {
                    self.mark_last_property_incdec_writeback(matches!(expr, Expr::PreIncTarget(_)));
                }
                (result, OpType::Tmp)
            }
            Expr::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                let (cond_op, cond_type) = self.compile_expr(condition);
                let branch_entry = Box::new(self.definitely_defined_cvs.clone());
                let tmp = self.alloc_tmp();

                // JmpZ condition → else_label
                let jmpz_idx = self.instructions.len();
                let mut jmpz = Instruction::new(OpCode::JmpZ);
                jmpz.op1 = cond_op;
                jmpz.op1_type = cond_type;
                jmpz.op2 = 0; // placeholder
                self.instructions.push(jmpz);

                // Then branch: compile then_expr, assign to tmp
                let (then_op, then_type) = self.compile_expr(then_expr);
                let then_exit = Box::new(self.definitely_defined_cvs.clone());
                let mut set_then = Instruction::new(OpCode::AssignCv);
                set_then.op1_type = OpType::Tmp;
                set_then.op1 = tmp;
                set_then.op2_type = then_type;
                set_then.op2 = then_op;
                self.instructions.push(set_then);

                // Jmp → end
                let jmp_end_idx = self.instructions.len();
                let mut jmp = Instruction::new(OpCode::Jmp);
                jmp.op1 = 0; // placeholder
                self.instructions.push(jmp);

                // Else branch
                let else_label = self.instructions.len() as u16;
                self.definitely_defined_cvs = *branch_entry;
                let (else_op, else_type) = self.compile_expr(else_expr);
                let else_exit = Box::new(self.definitely_defined_cvs.clone());
                let mut set_else = Instruction::new(OpCode::AssignCv);
                set_else.op1_type = OpType::Tmp;
                set_else.op1 = tmp;
                set_else.op2_type = else_type;
                set_else.op2 = else_op;
                self.instructions.push(set_else);

                let end_label = self.instructions.len() as u16;
                self.instructions[jmpz_idx].op2 = else_label;
                self.instructions[jmp_end_idx].op1 = end_label;
                self.definitely_defined_cvs = *then_exit;
                self.definitely_defined_cvs
                    .retain(|cv| else_exit.contains(cv));

                (tmp, OpType::Tmp)
            }
            Expr::Elvis { left, right } => {
                // Evaluate LHS once, store in tmp
                let (left_op, left_type) = self.compile_expr(left);
                let conditional_entry = Box::new(self.definitely_defined_cvs.clone());
                let tmp = self.alloc_tmp();
                let mut assign_left = Instruction::new(OpCode::AssignCv);
                assign_left.op1_type = OpType::Tmp;
                assign_left.op1 = tmp;
                assign_left.op2_type = left_type;
                assign_left.op2 = left_op;
                self.instructions.push(assign_left);

                // JmpNZ tmp → end (if truthy, result is already in tmp)
                let jmpnz_idx = self.instructions.len();
                let mut jmpnz = Instruction::new(OpCode::JmpNZ);
                jmpnz.op1 = tmp;
                jmpnz.op1_type = OpType::Tmp;
                jmpnz.op2 = 0; // placeholder
                self.instructions.push(jmpnz);

                // Else branch: evaluate RHS, overwrite tmp
                let (right_op, right_type) = self.compile_expr(right);
                let mut assign_right = Instruction::new(OpCode::AssignCv);
                assign_right.op1_type = OpType::Tmp;
                assign_right.op1 = tmp;
                assign_right.op2_type = right_type;
                assign_right.op2 = right_op;
                self.instructions.push(assign_right);

                let end_label = self.instructions.len() as u16;
                self.instructions[jmpnz_idx].op2 = end_label;
                self.definitely_defined_cvs = *conditional_entry;

                (tmp, OpType::Tmp)
            }
            Expr::Not(inner) => {
                let (op, op_type) = self.compile_expr(inner);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::BoolNot);
                instr.op1 = op;
                instr.op1_type = op_type;
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                self.instructions.push(instr);
                (tmp, OpType::Tmp)
            }
            Expr::BitwiseNot(inner) => {
                let (op, op_type) = self.compile_expr(inner);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::BitwiseNot);
                instr.op1 = op;
                instr.op1_type = op_type;
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                self.instructions.push(instr);
                (tmp, OpType::Tmp)
            }
            Expr::Print(inner) => {
                // print expr: echo the expression, then result is integer 1
                let (op, op_type) = self.compile_expr(inner);
                let mut echo = Instruction::new(OpCode::Echo);
                echo.op1 = op;
                echo.op1_type = op_type;
                self.instructions.push(echo);
                // print returns 1
                let one_lit = self.add_literal(Value::long(1));
                (one_lit, OpType::Const)
            }
            Expr::Include {
                path,
                is_require,
                is_once,
            } => {
                let (path_op, path_type) = self.compile_expr(path);
                let result = self.alloc_tmp();
                let mut include = Instruction::new(OpCode::Include);
                include.op1 = path_op;
                include.op1_type = path_type;
                include.result = result;
                include.result_type = OpType::Tmp;
                include.extended_value = u32::from(*is_require) | (u32::from(*is_once) << 1);
                self.instructions.push(include);
                (result, OpType::Tmp)
            }
            Expr::Eval { source, line } => {
                let (source_op, source_type) = self.compile_expr(source);
                let result = self.alloc_tmp();
                let mut eval = Instruction::new(OpCode::Eval);
                eval.op1 = source_op;
                eval.op1_type = source_type;
                eval.result = result;
                eval.result_type = OpType::Tmp;
                eval.extended_value = u32::try_from(*line).unwrap_or(u32::MAX);
                self.instructions.push(eval);
                self.definitely_defined_cvs.clear();
                (result, OpType::Tmp)
            }
            Expr::FunctionCall {
                name,
                args,
                generic_args,
                line,
            } => {
                let assertion_construct = generic_args.is_empty()
                    && name.trim_start_matches('\\').eq_ignore_ascii_case("assert");
                if assertion_construct && self.zend_assertions < 0 {
                    let enabled = self.add_literal(Value::bool(true));
                    return (enabled, OpType::Const);
                }
                let assertion_check = assertion_construct.then(|| {
                    let index = self.instructions.len();
                    self.instructions
                        .push(Instruction::new(OpCode::AssertCheck));
                    index
                });

                // PHP's assert construct supplies canonical source text when
                // the caller omitted an explicit description.
                let synthesized_assert_args = if assertion_construct && args.len() == 1 {
                    assertion_expression_source(args[0].expr()).map(|mut source| {
                        if let CallArg::Named { name, .. } = &args[0] {
                            source.insert_str("assert(".len(), &format!("{name}: "));
                        }
                        let mut synthesized = args.clone();
                        if matches!(args[0], CallArg::Named { .. }) {
                            synthesized.push(CallArg::Named {
                                name: "description".to_string(),
                                value: Expr::StringLiteral(source),
                            });
                        } else {
                            synthesized.push(CallArg::Positional(Expr::StringLiteral(source)));
                        }
                        synthesized
                    })
                } else {
                    None
                };
                let args = synthesized_assert_args.as_deref().unwrap_or(args);

                if generic_args.is_empty()
                    && args
                        .iter()
                        .any(|argument| matches!(argument, CallArg::Unpack(_)))
                {
                    // A mixed ordinary/unpacked call has runtime arity and may
                    // acquire named arguments from string keys. Materialize the
                    // argument sequence once, preserving evaluation order,
                    // then use the same named-argument protocol as a sole unpack.
                    let resolved = self.resolve_function_name(name);
                    let ref_args = self.lookup_ref_args(&resolved);
                    let name_idx = self.add_literal(Value::string(resolved));
                    let fallback_idx = if self.current_namespace.is_some()
                        && !name.contains('\\')
                        && !self.has_function_import(name)
                    {
                        self.add_literal(Value::string(name.clone()))
                    } else {
                        0
                    };
                    let (arguments, arguments_type) =
                        self.compile_mixed_unpacked_call_arguments(args, ref_args);
                    let tmp = self.alloc_tmp();
                    let mut call = Instruction::new(OpCode::CallUserFuncArray);
                    call.op1 = name_idx;
                    call.op1_type = OpType::Const;
                    call.op2 = arguments;
                    call.op2_type = arguments_type;
                    call.result = tmp;
                    call.result_type = OpType::Tmp;
                    call.extended_value = fallback_idx as u32;
                    call._pad |= CALL_USER_FUNC_ARRAY_SOURCE_UNPACK;
                    self.push_instruction_at_line(call, *line);
                    return (tmp, OpType::Tmp);
                }

                if generic_args.is_empty() {
                    if let [CallArg::Positional(argument)] = args {
                        let direct_kind = (!self.strict_types)
                            .then(|| self.unambiguous_global_function_name(name))
                            .flatten()
                            .and_then(crate::builtin_metadata::direct_internal_spec)
                            .filter(|spec| {
                                spec.required_args <= 1
                                    && spec.max_args >= 1
                                    && spec.kind.lowering()
                                        != crate::builtin_metadata::DirectInternalLowering::Generic2
                            })
                            .map(|spec| spec.kind);

                        if let Some(direct_kind) = direct_kind {
                            let (argument_op, argument_type) = self.compile_expr(argument);
                            let tmp = self.alloc_tmp();
                            let opcode = match direct_kind.lowering() {
                                crate::builtin_metadata::DirectInternalLowering::Generic => {
                                    OpCode::DirectInternalCall1
                                }
                                crate::builtin_metadata::DirectInternalLowering::Strlen => {
                                    if argument_type == OpType::Cv {
                                        OpCode::Strlen_Cv
                                    } else {
                                        OpCode::Strlen
                                    }
                                }
                                crate::builtin_metadata::DirectInternalLowering::Generic2 => {
                                    unreachable!("binary direct builtin selected by unary lowering")
                                }
                            };
                            let mut call = Instruction::new(opcode);
                            call.op1 = argument_op;
                            call.op1_type = argument_type;
                            call.result = tmp;
                            call.result_type = OpType::Tmp;
                            if opcode == OpCode::DirectInternalCall1 {
                                call.extended_value = direct_kind as u32;
                            }
                            self.push_instruction_at_line(call, *line);
                            return (tmp, OpType::Tmp);
                        }
                    }

                    if let [CallArg::Positional(first), CallArg::Positional(second)] = args {
                        let direct_kind = self
                            .unambiguous_global_function_name(name)
                            .and_then(crate::builtin_metadata::direct_internal_spec)
                            .filter(|spec| {
                                spec.required_args <= 2
                                    && spec.max_args >= 2
                                    && spec.kind.lowering()
                                        == crate::builtin_metadata::DirectInternalLowering::Generic2
                            })
                            .map(|spec| spec.kind);

                        if let Some(direct_kind) = direct_kind {
                            let (first_op, first_type) = self.compile_expr(first);
                            let (second_op, second_type) = self.compile_expr(second);
                            let tmp = self.alloc_tmp();
                            let mut call = Instruction::new(OpCode::DirectInternalCall2);
                            call.op1 = first_op;
                            call.op1_type = first_type;
                            call.op2 = second_op;
                            call.op2_type = second_type;
                            call.result = tmp;
                            call.result_type = OpType::Tmp;
                            call.extended_value = direct_kind as u32;
                            self.push_instruction_at_line(call, *line);
                            return (tmp, OpType::Tmp);
                        }
                    }

                    if self.is_global_builtin_call(name, "call_user_func") {
                        if let Some((CallArg::Positional(callback), forwarded)) = args.split_first()
                        {
                            if forwarded
                                .iter()
                                .all(|arg| matches!(arg, CallArg::Positional(_)))
                                && !forwarded.iter().any(CallArg::contains_yield)
                            {
                                let (callback_op, callback_type) = self.compile_expr(callback);
                                let mut init = Instruction::new(OpCode::InitUserCall);
                                init.op1 = callback_op;
                                init.op1_type = callback_type;
                                init.extended_value = forwarded.len() as u32;
                                self.push_instruction_at_line(init, *line);

                                self.emit_user_call_args(forwarded);

                                let tmp = self.alloc_tmp();
                                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                                do_fcall.result = tmp;
                                do_fcall.result_type = OpType::Tmp;
                                self.push_instruction_at_line(do_fcall, *line);
                                return (tmp, OpType::Tmp);
                            }
                        }
                    }

                    if self.is_global_builtin_call(name, "call_user_func_array") {
                        if let [CallArg::Positional(callback), CallArg::Positional(array)] = args {
                            if let Expr::ArrayLiteral(elements) = array {
                                if elements.iter().all(|element| {
                                    element.key.is_none() && !element.value.contains_yield()
                                }) {
                                    // A temporary packed literal cannot be observed by PHP
                                    // code. Forward its values directly and avoid allocating,
                                    // filling and dropping a PhpArray for every invocation.
                                    let (callback_op, callback_type) = self.compile_expr(callback);
                                    let mut init = Instruction::new(OpCode::InitUserCall);
                                    init.op1 = callback_op;
                                    init.op1_type = callback_type;
                                    init.extended_value = elements.len() as u32;
                                    init._pad = 1;
                                    self.push_instruction_at_line(init, *line);

                                    for (index, element) in elements.iter().enumerate() {
                                        let (op, op_type) = self.compile_expr(&element.value);
                                        let mut send = Instruction::new(OpCode::SendUser);
                                        send.op1 = op;
                                        send.op1_type = op_type;
                                        send.op2 = index as u16;
                                        send.extended_value = index as u32;
                                        self.instructions.push(send);
                                    }

                                    let tmp = self.alloc_tmp();
                                    let mut do_fcall = Instruction::new(OpCode::DoFcall);
                                    do_fcall.result = tmp;
                                    do_fcall.result_type = OpType::Tmp;
                                    self.push_instruction_at_line(do_fcall, *line);
                                    return (tmp, OpType::Tmp);
                                }
                            }

                            // PHP treats call_user_func_array as a call construct. Compile both
                            // operands in source order, then resolve and invoke the callback
                            // directly instead of entering the variadic stdlib wrapper.
                            let (callback_op, callback_type) = self.compile_expr(callback);
                            let (array_op, array_type) = self.compile_expr(array);
                            let tmp = self.alloc_tmp();
                            let mut call = Instruction::new(OpCode::CallUserFuncArray);
                            call.op1 = callback_op;
                            call.op1_type = callback_type;
                            call.op2 = array_op;
                            call.op2_type = array_type;
                            call.result = tmp;
                            call.result_type = OpType::Tmp;
                            self.push_instruction_at_line(call, *line);
                            return (tmp, OpType::Tmp);
                        }
                    }
                }

                let resolved = self.resolve_function_name(name);
                let ref_args = {
                    let resolved_refs = self.lookup_ref_args(&resolved);
                    let has_exact_user_function = self
                        .functions
                        .iter()
                        .any(|(function, _)| function.eq_ignore_ascii_case(&resolved))
                        || self
                            .known_ref_args
                            .keys()
                            .any(|function| function.eq_ignore_ascii_case(&resolved));
                    if resolved_refs == 0
                        && !has_exact_user_function
                        && self.current_namespace.is_some()
                        && !name.contains('\\')
                        && !self.has_function_import(name)
                    {
                        builtin_ref_args(name)
                    } else {
                        resolved_refs
                    }
                };
                let name_idx = self.add_literal(Value::string(resolved.clone()));

                // For unqualified function calls in a namespace, PHP falls back to global.
                // Store the original unqualified name as a fallback literal.
                // Qualified/FQ names (containing \) get no fallback.
                let fallback_idx = if self.current_namespace.is_some()
                    && !name.contains('\\')
                    && !self.has_function_import(name)
                {
                    self.add_literal(Value::string(name.clone()))
                } else {
                    0 // no fallback
                };

                let has_reference_lvalue = args.iter().enumerate().any(|(index, arg)| {
                    index < 64
                        && ref_args & (1u64 << index) != 0
                        && matches!(
                            arg,
                            CallArg::Positional(
                                Expr::ArrayAccess { .. }
                                    | Expr::ArrayAppendArgument { .. }
                                    | Expr::PropertyAccess {
                                        nullsafe: false,
                                        ..
                                    }
                                    | Expr::StaticProperty { .. }
                                    | Expr::DynamicNamedStaticProperty { .. }
                                    | Expr::DynamicStaticProperty { .. }
                            )
                        )
                });
                let contains_yield = args.iter().any(CallArg::contains_yield);
                let mut reference_writebacks = Vec::new();
                let compiled_args = if has_reference_lvalue {
                    Some(
                        args.iter()
                            .enumerate()
                            .map(|(index, arg)| match arg {
                                CallArg::Positional(Expr::ArrayAppendArgument {
                                    target, ..
                                }) if index < 64 && ref_args & (1u64 << index) != 0 => {
                                    match self.compile_array_append_argument_reference(target, &[])
                                    {
                                        Ok((result, result_type)) => {
                                            (result, result_type, None, None)
                                        }
                                        Err(error) => {
                                            self.deferred_error = Some(error);
                                            (0, OpType::Unused, None, None)
                                        }
                                    }
                                }
                                CallArg::Positional(expr)
                                    if index < 64 && ref_args & (1u64 << index) != 0 =>
                                {
                                    match self.compile_foreach_reference_source(expr, false, false)
                                    {
                                        Ok((op, op_type, writeback)) => {
                                            reference_writebacks.push((writeback, op, op_type));
                                            (op, op_type, None, None)
                                        }
                                        Err(error) => {
                                            self.deferred_error = Some(error);
                                            (0, OpType::Unused, None, None)
                                        }
                                    }
                                }
                                CallArg::Positional(expr) | CallArg::Unpack(expr) => {
                                    let (op, op_type) = self.compile_expr(expr);
                                    let (op, op_type) = if contains_yield {
                                        let (name, line) = match expr {
                                            Expr::Variable { name, line } => (name.as_str(), *line),
                                            _ => ("argument", 0),
                                        };
                                        self.snapshot_yield_rvalue_operand(op, op_type, name, line)
                                    } else {
                                        (op, op_type)
                                    };
                                    (op, op_type, None, None)
                                }
                                CallArg::Named { name, value } => {
                                    let (op, op_type) = self.compile_expr(value);
                                    let source_cv =
                                        (contains_yield && op_type == OpType::Cv).then_some(op);
                                    let (op, op_type) = if contains_yield {
                                        let (variable, line) = match value {
                                            Expr::Variable { name, line } => (name.as_str(), *line),
                                            _ => ("argument", 0),
                                        };
                                        self.snapshot_yield_rvalue_operand(
                                            op, op_type, variable, line,
                                        )
                                    } else {
                                        (op, op_type)
                                    };
                                    let name = self.add_literal(Value::string(name.clone()));
                                    (op, op_type, Some(name), source_cv)
                                }
                            })
                            .collect(),
                    )
                } else {
                    contains_yield.then(|| self.compile_call_args(args, ref_args, false))
                };

                let runtime_generic_check = self.emit_generic_check(
                    OpCode::CheckGenericArgs,
                    GenericDeclarationKind::Function,
                    generic_args,
                    Some(&resolved),
                    name_idx,
                    OpType::Const,
                    fallback_idx,
                    if fallback_idx == 0 {
                        OpType::Unused
                    } else {
                        OpType::Const
                    },
                );

                let mut init = Instruction::new(OpCode::InitFcall);
                init.op1 = args.len() as u16;
                init.op2_type = OpType::Const;
                init.op2 = name_idx;
                init.extended_value = fallback_idx as u32;
                let init_index = self.instructions.len();
                self.instructions.push(init);

                if let Some(compiled_args) = compiled_args.as_deref() {
                    self.emit_precompiled_runtime_call_args(
                        args,
                        compiled_args,
                        0,
                        ref_args,
                        false,
                        false,
                    );
                } else {
                    self.emit_call_args(args, 0, ref_args, false, false);
                }

                if compiled_args.is_none()
                    && args.iter().all(|arg| matches!(arg, CallArg::Positional(_)))
                    && self.instructions.len() > init_index + 1 + args.len()
                {
                    self.instructions[init_index]._pad |= CALL_FLAG_DEFERRED_SCALAR_CANDIDATE;
                }

                self.emit_reified_argument_check(runtime_generic_check);

                let tmp = self.alloc_tmp();
                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                do_fcall.result = tmp;
                do_fcall.result_type = OpType::Tmp;
                self.push_instruction_at_line(do_fcall, *line);
                self.emit_reified_return_check(runtime_generic_check, tmp, OpType::Tmp);
                for (writeback, value, value_type) in reference_writebacks {
                    self.emit_foreach_reference_source_writeback(writeback, value, value_type);
                }

                if let Some(index) = assertion_check {
                    let target = u16::try_from(self.instructions.len()).unwrap_or(u16::MAX);
                    let guard = &mut self.instructions[index];
                    guard.op1 = target;
                    guard.result = tmp;
                    guard.result_type = OpType::Tmp;
                }

                (tmp, OpType::Tmp)
            }
            Expr::ArrayLiteral(elements) => {
                if self.deferred_error.is_none()
                    && let Some(element) = elements.iter().find(|element| {
                        element.unpack
                            && self
                                .statically_known_array_unpack_type(&element.value)
                                .is_some_and(|value_type| value_type != ValueType::Array)
                    })
                    && let Some(line) = element.unpack_line
                {
                    let value_type = self
                        .statically_known_array_unpack_type(&element.value)
                        .expect("filtered statically known unpack type");
                    let given = match value_type {
                        ValueType::Undef => "unknown",
                        ValueType::Null => "null",
                        ValueType::False | ValueType::True => "bool",
                        ValueType::Long => "int",
                        ValueType::Double => "float",
                        ValueType::String => "string",
                        ValueType::Array => "array",
                        ValueType::Object => "object",
                        ValueType::Resource => "resource",
                        ValueType::Reference => "reference",
                        ValueType::Closure => "Closure",
                    };
                    self.deferred_error = Some(self.goto_error(
                        &format!("Only arrays and Traversables can be unpacked, {given} given"),
                        line,
                    ));
                }

                // The literal size and an unavoidable hash transition are
                // compile-time facts. Pass them to InitArray so runtime can
                // allocate the final representation once.
                let arr_tmp = self.alloc_tmp();
                let mut init = Instruction::new(OpCode::InitArray);
                init.result_type = OpType::Tmp;
                init.result = arr_tmp;
                match array_literal_storage_hint(elements) {
                    ArrayLiteralStorageHint::Packed => {
                        init.extended_value = elements.len() as u32;
                    }
                    ArrayLiteralStorageHint::Hash => {
                        init.extended_value = elements.len() as u32;
                        init._pad |= ARRAY_INIT_HASH_HINT;
                    }
                    ArrayLiteralStorageHint::Unknown => {}
                }
                self.instructions.push(init);

                // Add elements
                for elem in elements {
                    let (val_op, val_type) = if elem.by_reference {
                        match self.compile_array_element_reference_source(&elem.value) {
                            Ok(source) => (source, OpType::Cv),
                            Err(error) => {
                                self.deferred_error = Some(error);
                                let null = self.add_literal(Value::null());
                                (null, OpType::Const)
                            }
                        }
                    } else {
                        self.compile_expr(&elem.value)
                    };
                    let mut add = Instruction::new(if elem.unpack {
                        OpCode::AddArrayUnpack
                    } else {
                        OpCode::AddArrayElement
                    });
                    if elem.unpack && self.compiling_constant_expression {
                        add._pad |= ARRAY_UNPACK_CONSTANT_EXPRESSION;
                    }
                    if elem.by_reference {
                        add._pad |= ARRAY_ELEMENT_REFERENCE;
                    }
                    add.op1_type = OpType::Tmp;
                    add.op1 = arr_tmp;
                    add.op2_type = val_type;
                    add.op2 = val_op;
                    if let Some(key) = &elem.key {
                        let (key_op, key_type) = self.compile_expr(key);
                        add.result_type = key_type;
                        add.result = key_op;
                    }
                    // result_type = Unused means auto-key
                    if elem.unpack {
                        self.push_instruction_at_line(add, elem.unpack_line.unwrap_or(0));
                    } else {
                        self.instructions.push(add);
                    }
                }

                (arr_tmp, OpType::Tmp)
            }
            Expr::ArrayAccess { array, index, line } => {
                if matches!(array.as_ref(), Expr::Globals { .. }) {
                    let (key, key_type) = self.compile_expr(index);
                    let result = self.alloc_tmp();
                    let mut fetch = Instruction::new(OpCode::FetchGlobal);
                    fetch.op1 = key;
                    fetch.op1_type = key_type;
                    fetch.result = result;
                    fetch.result_type = OpType::Tmp;
                    self.instructions.push(fetch);
                    return (result, OpType::Tmp);
                }
                let (arr_op, arr_type) = self.compile_expr(array);
                let receiver_patches = self.take_nullsafe_receiver_patches(arr_op, arr_type);
                let (idx_op, idx_type) = self.compile_expr(index);
                let tmp = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchDimR);
                fetch.op1_type = arr_type;
                fetch.op1 = arr_op;
                fetch.op2_type = idx_type;
                fetch.op2 = idx_op;
                fetch.result_type = OpType::Tmp;
                fetch.result = tmp;
                self.push_instruction_at_line(fetch, *line);
                self.publish_nullsafe_receiver_patches(tmp, receiver_patches);
                (tmp, OpType::Tmp)
            }
            Expr::DynamicClassConstant {
                class,
                constant,
                dynamic_name,
            } => {
                let (class_op, class_type) = self.compile_expr(class);
                let receiver_patches = self.take_nullsafe_receiver_patches(class_op, class_type);
                let (constant_op, constant_type) = self.compile_expr(constant);
                let tmp = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchDynamicClassConst);
                fetch._pad |= CLASS_CONST_DYNAMIC_OWNER;
                if self.compiling_constant_expression {
                    fetch._pad |= CLASS_CONST_CONSTANT_EXPRESSION;
                }
                if *dynamic_name {
                    fetch._pad |= CLASS_CONST_DYNAMIC_NAME;
                    if Self::is_compile_time_class_constant_name(constant) {
                        fetch._pad |= CLASS_CONST_COMPILE_TIME_NAME;
                    }
                }
                fetch.op1 = class_op;
                fetch.op1_type = class_type;
                fetch.op2 = constant_op;
                fetch.op2_type = constant_type;
                fetch.result = tmp;
                fetch.result_type = OpType::Tmp;
                self.instructions.push(fetch);
                self.publish_nullsafe_receiver_patches(tmp, receiver_patches);
                (tmp, OpType::Tmp)
            }
            Expr::DynamicNamedClassConstant {
                class_name,
                constant,
            } => {
                let (resolved, dynamic_static_scope) = self.resolve_static_member_owner(class_name);
                let class_op = self.add_literal(Value::string(resolved));
                let (constant_op, constant_type) = self.compile_expr(constant);
                let tmp = self.alloc_tmp();
                let mut fetch = Instruction::new(if dynamic_static_scope {
                    OpCode::FetchLateDynamicClassConst
                } else {
                    OpCode::FetchDynamicClassConst
                });
                if self.compiling_constant_expression {
                    fetch._pad |= CLASS_CONST_CONSTANT_EXPRESSION;
                }
                fetch._pad |= CLASS_CONST_DYNAMIC_NAME;
                if Self::is_compile_time_class_constant_name(constant) {
                    fetch._pad |= CLASS_CONST_COMPILE_TIME_NAME;
                }
                fetch.op1 = class_op;
                fetch.op1_type = OpType::Const;
                fetch.op2 = constant_op;
                fetch.op2_type = constant_type;
                fetch.result = tmp;
                fetch.result_type = OpType::Tmp;
                self.instructions.push(fetch);
                (tmp, OpType::Tmp)
            }
            Expr::UnaryMinus(inner) => {
                // Constant folding for literals
                match inner.as_ref() {
                    Expr::Integer(n) => {
                        let idx = self.add_literal(Value::long(-n));
                        return (idx, OpType::Const);
                    }
                    Expr::Float(f) => {
                        let idx = self.add_literal(Value::double(-f));
                        return (idx, OpType::Const);
                    }
                    _ => {}
                }
                // PHP lowers unary minus through multiplication. Keeping the
                // operand first preserves its diagnostics, while `* -1`
                // retains IEEE negative zero for a dynamic positive zero.
                let (inner_op, inner_type) = self.compile_expr(inner);
                let negative_one_idx = self.add_literal(Value::long(-1));
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::Mul);
                instr.op1 = inner_op;
                instr.op1_type = inner_type;
                instr.op2 = negative_one_idx;
                instr.op2_type = OpType::Const;
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                self.instructions.push(instr);
                (tmp, OpType::Tmp)
            }
            Expr::UnaryPlus(inner) => {
                match inner.as_ref() {
                    Expr::Integer(n) => {
                        let idx = self.add_literal(Value::long(*n));
                        return (idx, OpType::Const);
                    }
                    Expr::Float(f) => {
                        let idx = self.add_literal(Value::double(*f));
                        return (idx, OpType::Const);
                    }
                    _ => {}
                }
                // PHP specifies unary plus through numeric multiplication;
                // keeping the operand first also preserves its diagnostics.
                let (inner_op, inner_type) = self.compile_expr(inner);
                let one_idx = self.add_literal(Value::long(1));
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::Mul);
                instr.op1 = inner_op;
                instr.op1_type = inner_type;
                instr.op2 = one_idx;
                instr.op2_type = OpType::Const;
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                self.instructions.push(instr);
                (tmp, OpType::Tmp)
            }
            Expr::ErrorSuppress(inner) => {
                let first_instruction = self.instructions.len();
                let result = self.compile_expr(inner);
                for instruction in &mut self.instructions[first_instruction..] {
                    if instruction.opcode == OpCode::DoFcall {
                        instruction._pad |= CALL_FLAG_ERROR_SUPPRESS;
                    } else if instruction.opcode == OpCode::Eval {
                        instruction._pad |= EVAL_FLAG_ERROR_SUPPRESS;
                    } else if instruction.opcode == OpCode::FetchCvR {
                        instruction._pad |= crate::vm::instruction::FETCH_CV_ERROR_SUPPRESS;
                    } else if instruction.opcode == OpCode::FetchDimR
                        && instruction._pad & FETCH_DIM_ISSET == 0
                    {
                        instruction._pad |= FETCH_DIM_ERROR_SUPPRESS;
                    } else if instruction.opcode == OpCode::FetchDynamicVar
                        && instruction._pad & FETCH_DIM_ISSET == 0
                    {
                        instruction._pad |= FETCH_DYNAMIC_ERROR_SUPPRESS;
                    } else if instruction.opcode == OpCode::FetchObjR
                        && instruction._pad & FETCH_OBJ_SILENT == 0
                    {
                        instruction._pad |= FETCH_OBJ_ERROR_SUPPRESS;
                    } else if matches!(instruction.opcode, OpCode::SendVarEx | OpCode::SendNamed)
                        && instruction._pad & crate::vm::instruction::SEND_FLAG_FETCH_CV_R != 0
                    {
                        instruction._pad |= crate::vm::instruction::SEND_FLAG_ERROR_SUPPRESS;
                    }
                }
                result
            }
            Expr::Cast {
                cast_type,
                expr,
                line,
            } => {
                if *cast_type == CastType::Void {
                    let first_instruction = self.instructions.len();
                    let (inner_op, inner_type) = self.compile_expr(expr);
                    if inner_type == OpType::Tmp
                        && let Some(call) = self.instructions[first_instruction..]
                            .iter_mut()
                            .rev()
                            .find(|instruction| {
                                instruction.opcode == OpCode::DoFcall
                                    && instruction.result_type == OpType::Tmp
                                    && instruction.result == inner_op
                            })
                    {
                        call._pad |= CALL_FLAG_RETURN_EXPLICITLY_IGNORED;
                    }
                    return (inner_op, inner_type);
                }
                let (inner_op, inner_type) = self.compile_expr(expr);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::Cast);
                instr.op1 = inner_op;
                instr.op1_type = inner_type;
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                instr.extended_value = *cast_type as u32;
                self.push_instruction_at_line(instr, *line);
                (tmp, OpType::Tmp)
            }
            Expr::Isset(args) => {
                let (tmp, tmp_type) = self.compile_isset_operand(&args[0]);
                let tmp = if tmp_type == OpType::Tmp
                    && matches!(
                        args[0],
                        Expr::DynamicVariable { .. }
                            | Expr::PropertyAccess { .. }
                            | Expr::ArrayAccess { .. }
                    ) {
                    tmp
                } else {
                    let result = self.alloc_tmp();
                    let mut instr = Instruction::new(OpCode::Isset);
                    instr.op1 = tmp;
                    instr.op1_type = tmp_type;
                    instr.result = result;
                    instr.result_type = OpType::Tmp;
                    self.instructions.push(instr);
                    result
                };
                // Multi-arg `isset` short-circuits before compiling the next
                // operand's runtime instructions, just like PHP.
                for arg in args.iter().skip(1) {
                    let jmpz_idx = self.instructions.len();
                    let mut jmpz = Instruction::new(OpCode::JmpZ);
                    jmpz.op1 = tmp;
                    jmpz.op1_type = OpType::Tmp;
                    jmpz.op2 = 0;
                    self.instructions.push(jmpz);

                    let (operand, operand_type) = self.compile_isset_operand(arg);
                    let tmp2 = if operand_type == OpType::Tmp
                        && matches!(
                            arg,
                            Expr::DynamicVariable { .. }
                                | Expr::PropertyAccess { .. }
                                | Expr::ArrayAccess { .. }
                        ) {
                        operand
                    } else {
                        let result = self.alloc_tmp();
                        let mut instr2 = Instruction::new(OpCode::Isset);
                        instr2.op1 = operand;
                        instr2.op1_type = operand_type;
                        instr2.result = result;
                        instr2.result_type = OpType::Tmp;
                        self.instructions.push(instr2);
                        result
                    };
                    // Copy tmp2 into tmp
                    let mut assign = Instruction::new(OpCode::AssignCv);
                    assign.op1_type = OpType::Tmp;
                    assign.op1 = tmp;
                    assign.op2_type = OpType::Tmp;
                    assign.op2 = tmp2;
                    self.instructions.push(assign);
                    self.instructions[jmpz_idx].op2 = self.instructions.len() as u16;
                }
                (tmp, OpType::Tmp)
            }
            Expr::Empty(inner) => {
                // `empty()` reads variables, properties, and dimensions in a
                // silent probe context. In particular, an uninitialized typed
                // property behaves as unset instead of throwing before the
                // truthiness check.
                let first_instruction = self.instructions.len();
                let (op, op_type) = self.compile_isset_object_base(inner);
                for instruction in &mut self.instructions[first_instruction..] {
                    if instruction.opcode == OpCode::FetchDimR {
                        instruction._pad |= FETCH_DIM_EMPTY;
                    }
                }
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::BoolNot);
                instr.op1 = op;
                instr.op1_type = op_type;
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                self.instructions.push(instr);
                (tmp, OpType::Tmp)
            }
            Expr::NullCoalesce { left, right } => {
                // $a ?? $b → isset($a) ? $a : $b
                // Property reads in a coalescing probe are silent: an
                // uninitialized typed property behaves like an unset value.
                let (l_op, l_type) = self.compile_isset_object_base(left);
                let conditional_entry = Box::new(self.definitely_defined_cvs.clone());
                let tmp = self.alloc_tmp();

                // Check if left is set (not null/undef)
                let isset_tmp = self.alloc_tmp();
                let mut isset = Instruction::new(OpCode::Isset);
                isset.op1 = l_op;
                isset.op1_type = l_type;
                isset.result = isset_tmp;
                isset.result_type = OpType::Tmp;
                self.instructions.push(isset);

                // JmpZ → else (eval right)
                let jmpz_idx = self.instructions.len();
                let mut jmpz = Instruction::new(OpCode::JmpZ);
                jmpz.op1 = isset_tmp;
                jmpz.op1_type = OpType::Tmp;
                jmpz.op2 = 0;
                self.instructions.push(jmpz);

                // Left is set, assign to tmp
                let mut set_left = Instruction::new(OpCode::AssignCv);
                set_left.op1_type = OpType::Tmp;
                set_left.op1 = tmp;
                set_left.op2_type = l_type;
                set_left.op2 = l_op;
                self.instructions.push(set_left);

                let jmp_end_idx = self.instructions.len();
                let mut jmp = Instruction::new(OpCode::Jmp);
                jmp.op1 = 0;
                self.instructions.push(jmp);

                // Else: eval right
                let else_label = self.instructions.len() as u16;
                let (r_op, r_type) = self.compile_expr(right);
                let mut set_right = Instruction::new(OpCode::AssignCv);
                set_right.op1_type = OpType::Tmp;
                set_right.op1 = tmp;
                set_right.op2_type = r_type;
                set_right.op2 = r_op;
                self.instructions.push(set_right);

                let end_label = self.instructions.len() as u16;
                self.instructions[jmpz_idx].op2 = else_label;
                self.instructions[jmp_end_idx].op1 = end_label;
                self.definitely_defined_cvs = *conditional_entry;

                (tmp, OpType::Tmp)
            }
            Expr::Match { line, expr, arms } => {
                // match($x) { cond => body, ... default => body }
                // Compile like a chain of === checks
                let (expr_op, expr_type) = self.compile_expr(expr);
                let match_entry = Box::new(self.definitely_defined_cvs.clone());
                let result_tmp = self.alloc_tmp();
                let mut end_patches = Vec::new();
                let mut default_body: Option<&Expr> = None;

                for arm in arms {
                    if let Some(conditions) = &arm.conditions {
                        // For each condition: if expr === cond, jump to body
                        let mut body_patches = Vec::new();
                        for (i, cond) in conditions.iter().enumerate() {
                            let (cond_op, cond_type) = self.compile_expr(cond);
                            let cmp_tmp = self.alloc_tmp();
                            let mut cmp = Instruction::new(OpCode::IsIdentical);
                            cmp.op1 = expr_op;
                            cmp.op1_type = expr_type;
                            cmp.op2 = cond_op;
                            cmp.op2_type = cond_type;
                            cmp.result = cmp_tmp;
                            cmp.result_type = OpType::Tmp;
                            self.instructions.push(cmp);

                            if i < conditions.len() - 1 {
                                // JmpNZ → body
                                let jmpnz_idx = self.instructions.len();
                                let mut jmpnz = Instruction::new(OpCode::JmpNZ);
                                jmpnz.op1 = cmp_tmp;
                                jmpnz.op1_type = OpType::Tmp;
                                jmpnz.op2 = 0;
                                self.instructions.push(jmpnz);
                                body_patches.push(jmpnz_idx);
                            } else {
                                // Last condition: JmpZ → next arm
                                let jmpz_idx = self.instructions.len();
                                let mut jmpz = Instruction::new(OpCode::JmpZ);
                                jmpz.op1 = cmp_tmp;
                                jmpz.op1_type = OpType::Tmp;
                                jmpz.op2 = 0;
                                self.instructions.push(jmpz);

                                // Patch JmpNZ's to here (body start)
                                let body_start = self.instructions.len() as u16;
                                for patch in &body_patches {
                                    self.instructions[*patch].op2 = body_start;
                                }

                                // Compile body
                                let (body_op, body_type) = self.compile_expr(&arm.body);
                                let mut set = Instruction::new(OpCode::AssignCv);
                                set.op1_type = OpType::Tmp;
                                set.op1 = result_tmp;
                                set.op2_type = body_type;
                                set.op2 = body_op;
                                self.instructions.push(set);

                                let jmp_end = self.instructions.len();
                                let mut jmp = Instruction::new(OpCode::Jmp);
                                jmp.op1 = 0;
                                self.instructions.push(jmp);
                                end_patches.push(jmp_end);

                                // Patch JmpZ to next arm
                                let next = self.instructions.len() as u16;
                                self.instructions[jmpz_idx].op2 = next;
                            }
                        }
                    } else {
                        default_body = Some(&arm.body);
                    }
                }

                // Default arm or error
                if let Some(body) = default_body {
                    let (body_op, body_type) = self.compile_expr(body);
                    let mut set = Instruction::new(OpCode::AssignCv);
                    set.op1_type = OpType::Tmp;
                    set.op1 = result_tmp;
                    set.op2_type = body_type;
                    set.op2 = body_op;
                    self.instructions.push(set);
                } else {
                    // No default: throw UnhandledMatchError at runtime
                    let mut throw = Instruction::new(OpCode::Throw);
                    throw.op1 = expr_op;
                    throw.op1_type = expr_type;
                    throw._pad |= THROW_FLAG_UNHANDLED_MATCH;
                    self.push_instruction_at_line(throw, *line);
                }

                let end_label = self.instructions.len() as u16;
                for patch in end_patches {
                    self.instructions[patch].op1 = end_label;
                }
                // Conditions and arm bodies are path-dependent. Preserve the
                // discriminant's effects, which are the only definitions
                // guaranteed for every successful continuation.
                self.definitely_defined_cvs = *match_entry;

                (result_tmp, OpType::Tmp)
            }
            Expr::Closure {
                line,
                attributes,
                is_static,
                returns_by_ref,
                params,
                use_vars,
                body,
                return_type,
                generic_params,
            } => {
                let trace_scope = if let Some((_, public_name)) = self
                    .current_function_name
                    .starts_with("__closure_")
                    .then(|| self.current_function_name.split_once('@'))
                    .flatten()
                {
                    public_name.to_string()
                } else if self.current_function_name.is_empty()
                    || self.current_function_name == "<main>"
                    || self.current_function_name == self.source_file
                {
                    self.source_file.clone()
                } else {
                    format!("{}()", self.current_function_name.replace("->", "::"))
                };
                let public_trace_name = if trace_scope.is_empty() {
                    "{closure}".to_string()
                } else {
                    format!("{{closure:{trace_scope}:{line}}}")
                };
                let closure_name = format!(
                    "__closure_{}@{}",
                    CLOSURE_COUNTER.fetch_add(1, Ordering::Relaxed),
                    public_trace_name
                );
                self.record_generic_declaration(
                    GenericDeclarationKind::Closure,
                    closure_name.clone(),
                    generic_params,
                    Some(params),
                    return_type.as_ref(),
                );
                // Compile closure body into a separate function
                let mut func_compiler = self.child_compiler();
                func_compiler.known_ref_args = self.build_known_ref_args();
                func_compiler.current_function_name = closure_name.clone();
                func_compiler.returns_reference_context = *returns_by_ref;
                func_compiler.contains_yield = body.iter().any(Stmt::contains_yield);
                // params come first as CVs (args), then use_vars
                let compile_result = self.compile_params(&mut func_compiler, params, "closure");
                let mut cp = match compile_result {
                    Ok(r) => r,
                    Err(e) => {
                        self.deferred_error = Some(e);
                        CompiledParams {
                            num_args: params.len() as u32,
                            required_num_args: params.len() as u32,
                            is_variadic: false,
                            variadic_cv_index: 0,
                            ref_args: 0,
                            type_hints: vec![],
                            param_names: vec![],
                            return_type_hint: crate::vm::function::ParamTypeHint::None,
                        }
                    }
                };
                if let Err(error) = func_compiler.validate_declared_type_hint(return_type, *line) {
                    self.deferred_error = Some(error);
                }
                cp.return_type_hint = self.convert_type_hint(return_type);
                if let Err(error) = self.validate_no_discard_callable(
                    attributes,
                    None,
                    "{closure}",
                    &cp.return_type_hint,
                ) {
                    self.deferred_error = Some(error);
                }
                if let Err(error) = self.validate_override_target(attributes, "function", false) {
                    self.deferred_error = Some(error);
                }
                func_compiler.return_type_context = cp.return_type_hint.clone();
                if let Err(error) = self.validate_generator_return_type(
                    func_compiler.contains_yield,
                    &cp.return_type_hint,
                    *line,
                ) {
                    self.deferred_error = Some(error);
                }
                let mut closure_reference_cvs = Vec::new();
                for (v, by_reference, line) in use_vars {
                    let cv = func_compiler.resolve_cv(v);
                    func_compiler.closure_capture_names.insert(v.clone());
                    // Explicit `use ($value)` snapshots at closure creation,
                    // so the captured CV is initialized even when the source
                    // was missing. Arrow functions capture silently and keep
                    // an UNDEF cell; their first actual read must diagnose it.
                    if *line != 0 || *by_reference {
                        func_compiler.definitely_defined_cvs.insert(cv);
                    }
                    if *by_reference {
                        closure_reference_cvs.push(cv as u32);
                    }
                }
                for s in body {
                    if let Err(e) = func_compiler.compile_stmt(s) {
                        self.deferred_error = Some(e);
                        break;
                    }
                }
                if let Err(error) = func_compiler.finalize_gotos() {
                    self.deferred_error = Some(error);
                }
                let null_idx = func_compiler.add_literal(Value::null());
                let mut ret = Instruction::new(OpCode::Return);
                ret.op1_type = OpType::Const;
                ret.op1 = null_idx;
                func_compiler.instructions.push(ret);

                let closure_all_cvs = func_compiler.all_cvs();
                let cache = (0..func_compiler.instructions.len())
                    .map(|_| InlineCache::empty())
                    .collect();
                let may_access_globals = !func_compiler.global_vars.is_empty()
                    || instructions_may_access_globals(&func_compiler.instructions);
                let nested_generic_declarations =
                    std::mem::take(&mut func_compiler.generic_declarations);
                let op_array = OpArray {
                    num_cvs: func_compiler.next_cv,
                    num_temps: func_compiler.next_tmp,
                    trait_class_scope_tmp: func_compiler.trait_class_scope_tmp,
                    source_lines: func_compiler.materialize_source_lines_with_declaration(*line),
                    instructions: func_compiler.instructions,
                    literals: func_compiler.literals,
                    try_entries: func_compiler.try_entries,
                    strict_types: self.strict_types,
                    is_generator: func_compiler.contains_yield,
                    global_vars: func_compiler.global_vars,
                    static_vars: func_compiler.static_vars,
                    name: func_compiler.current_function_name,
                    source_file: std::rc::Rc::new(func_compiler.source_file.clone()),
                    main_scope_vars: vec![],
                    all_cvs: closure_all_cvs,
                    cache,
                    may_access_globals,
                    block_info: Vec::new(),
                    block_counters: Vec::new(),
                    block_plans: Vec::new(),
                    ip_to_block: Vec::new(),
                };
                let mut user_func = make_user_function_typed(
                    op_array,
                    cp.num_args,
                    cp.required_num_args,
                    cp.is_variadic,
                    cp.variadic_cv_index,
                    cp.ref_args,
                    cp.type_hints,
                    cp.param_names,
                    cp.return_type_hint,
                    *returns_by_ref,
                );
                user_func.set_attributes(self.compile_attributes_in_scope_mode(
                    attributes,
                    2,
                    self.lexical_static_class.as_deref(),
                    self.lexical_static_parent.as_deref(),
                    true,
                ));
                user_func.parameter_attributes = params
                    .iter()
                    .map(|parameter| {
                        self.compile_attributes_in_scope_mode(
                            &parameter.attributes,
                            32,
                            self.lexical_static_class.as_deref(),
                            self.lexical_static_parent.as_deref(),
                            true,
                        )
                    })
                    .collect();
                user_func.reference_cvs = closure_reference_cvs;
                #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
                {
                    let captured_plan = u8::try_from(use_vars.len()).ok().and_then(|count| {
                        super::build_captured_typed_long_function_plan(&user_func, count)
                    });
                    user_func.set_captured_typed_long_plan(captured_plan);
                }
                self.functions.extend(func_compiler.functions);
                self.class_declaration_keys
                    .extend(func_compiler.class_declaration_keys);
                self.class_defs.extend(func_compiler.class_defs);
                self.generic_declarations
                    .extend(nested_generic_declarations);
                let has_static_vars = !user_func.op_array.static_vars.is_empty();
                let binds_trait_class_scope = user_func.common.plan.needs_trait_class_scope();
                self.functions.push((closure_name.clone(), user_func));

                // Build closure value with direct function pointer + captured values.
                // CreateClosure resolves the function pointer at creation time (not call time).
                // ClosureUseVar pushes each captured value into the closure.
                let name_idx = self.add_literal(Value::string(closure_name));
                let tmp = self.alloc_tmp();

                let mut create = Instruction::new(OpCode::CreateClosure);
                create.op1 = name_idx;
                create.op1_type = OpType::Const;
                // A closure declared in a class retains that lexical visibility
                // scope even when another object invokes it later. Trait bodies
                // remain dynamically scoped to their concrete consumer.
                if !self.dynamic_static_scope
                    && let Some(scope) = self.lexical_static_class.clone()
                {
                    create.op2 = self.add_literal(Value::string(scope));
                    create.op2_type = OpType::Const;
                }
                create.result = tmp;
                create.result_type = OpType::Tmp;
                create.extended_value = use_vars.len() as u32;
                if *is_static {
                    create._pad |= crate::vm::instruction::CLOSURE_FLAG_STATIC;
                }
                if has_static_vars {
                    create._pad |= crate::vm::instruction::CLOSURE_FLAG_HAS_STATICS;
                }
                if binds_trait_class_scope {
                    let scope_tmp = if let Some(scope_tmp) = self.trait_class_scope_tmp {
                        scope_tmp
                    } else {
                        let scope_tmp = self.alloc_tmp();
                        self.trait_class_scope_tmp = Some(scope_tmp);
                        scope_tmp
                    };
                    create.op2 = scope_tmp;
                    create.op2_type = OpType::Tmp;
                    create._pad |= crate::vm::instruction::CLOSURE_FLAG_TRAIT_LEXICAL_SCOPE;
                }
                self.instructions.push(create);

                // Add captured use_var values
                for (v, by_reference, line) in use_vars {
                    let cv = self.resolve_cv(v);
                    let mut use_var = Instruction::new(OpCode::ClosureUseVar);
                    use_var.op1 = tmp;
                    use_var.op1_type = OpType::Tmp;
                    if *by_reference {
                        use_var.op2 = cv;
                        use_var.op2_type = OpType::Cv;
                        use_var._pad |= crate::vm::instruction::CLOSURE_USE_REFERENCE;
                    } else if self.definitely_defined_cvs.contains(&cv) {
                        use_var.op2 = cv;
                        use_var.op2_type = OpType::Cv;
                    } else if *line != 0 {
                        let capture = self.alloc_tmp();
                        let name = self.add_literal(Value::string(v.clone()));
                        let mut fetch = Instruction::new(OpCode::FetchCvR);
                        fetch.op1 = cv;
                        fetch.op1_type = OpType::Cv;
                        fetch.op2 = name;
                        fetch.op2_type = OpType::Const;
                        fetch.result = capture;
                        fetch.result_type = OpType::Tmp;
                        self.push_instruction_at_line(fetch, *line);
                        self.invalidate_reentrant_definitions();
                        use_var.op2 = capture;
                        use_var.op2_type = OpType::Tmp;
                    } else {
                        use_var.op2 = cv;
                        use_var.op2_type = OpType::Cv;
                    }
                    self.instructions.push(use_var);
                }

                (tmp, OpType::Tmp)
            }
            Expr::New {
                class_name,
                args,
                generic_args,
                line,
                call_line,
            } => {
                if generic_args.is_empty()
                    && args
                        .iter()
                        .any(|argument| matches!(argument, CallArg::Unpack(_)))
                {
                    let (arguments, arguments_type) =
                        self.compile_mixed_unpacked_call_arguments(args, 0);
                    let (resolved_class, dynamic_static_scope) =
                        self.resolve_static_member_owner(class_name);
                    let name_idx = self.add_literal(Value::string(resolved_class));
                    let tmp = self.alloc_tmp();
                    let mut new_obj = Instruction::new(OpCode::NewObj);
                    new_obj.op1 = name_idx;
                    new_obj.op1_type = OpType::Const;
                    new_obj.op2 = arguments;
                    new_obj.op2_type = arguments_type;
                    new_obj.result = tmp;
                    new_obj.result_type = OpType::Tmp;
                    new_obj._pad |= NEW_FLAG_UNPACKED_ARGUMENTS;
                    if dynamic_static_scope {
                        new_obj._pad |= NEW_FLAG_DYNAMIC_STATIC_SCOPE;
                    }
                    self.push_instruction_at_line(new_obj, *line);
                    return (tmp, OpType::Tmp);
                }
                // Pre-compile arg expressions BEFORE NewObj so side effects
                // always execute, even when the class has no __construct.
                // Compile args, tracking which are named for SendNamed emission
                let compiled_args: Vec<CompiledCallArg> =
                    if args.iter().any(CallArg::contains_yield) {
                        self.compile_call_args(args, 0, true)
                    } else {
                        args.iter()
                            .map(|arg| match arg {
                                CallArg::Positional(expr) | CallArg::Unpack(expr) => {
                                    let (op, op_type) = self.compile_expr(expr);
                                    (op, op_type, None, None)
                                }
                                CallArg::Named { name, value } => {
                                    let (op, op_type) = self.compile_expr(value);
                                    let name_idx = self.add_literal(Value::string(name.clone()));
                                    (op, op_type, Some(name_idx), None)
                                }
                            })
                            .collect()
                    };

                let (resolved_class, dynamic_static_scope) =
                    self.resolve_static_member_owner(class_name);
                let name_idx = self.add_literal(Value::string(resolved_class.clone()));
                let runtime_generic_check = self.emit_generic_check(
                    if dynamic_static_scope {
                        OpCode::CheckLateStaticGenericArgs
                    } else {
                        OpCode::CheckGenericArgs
                    },
                    GenericDeclarationKind::Class,
                    generic_args,
                    Some(&resolved_class),
                    name_idx,
                    OpType::Const,
                    0,
                    OpType::Unused,
                );
                let tmp = self.alloc_tmp();
                let mut new_obj = Instruction::new(OpCode::NewObj);
                new_obj.op1 = name_idx;
                new_obj.op1_type = OpType::Const;
                new_obj.result = tmp;
                new_obj.result_type = OpType::Tmp;
                new_obj.extended_value = args.len() as u32;
                if dynamic_static_scope {
                    new_obj._pad |= NEW_FLAG_DYNAMIC_STATIC_SCOPE;
                }
                self.push_instruction_at_line(new_obj, *line);

                // Send constructor args — offset by 1 because CV 0 is $this
                self.emit_precompiled_call_args(&compiled_args, 1);
                self.emit_reified_argument_check(runtime_generic_check);

                // DoFcall to run __construct (VM skips if no constructor exists)
                let discard = self.alloc_tmp();
                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                do_fcall.result = discard;
                do_fcall.result_type = OpType::Tmp;
                self.push_instruction_at_line(do_fcall, *call_line);
                self.emit_reified_return_check(runtime_generic_check, tmp, OpType::Tmp);

                (tmp, OpType::Tmp)
            }
            Expr::DynamicNew {
                class,
                args,
                line,
                call_line,
            } => {
                if args
                    .iter()
                    .any(|argument| matches!(argument, CallArg::Unpack(_)))
                {
                    let (arguments, arguments_type) =
                        self.compile_mixed_unpacked_call_arguments(args, 0);
                    let (class, class_type) = self.compile_expr(class);
                    let tmp = self.alloc_tmp();
                    let mut new_obj = Instruction::new(OpCode::NewObj);
                    new_obj.op1 = class;
                    new_obj.op1_type = class_type;
                    new_obj.op2 = arguments;
                    new_obj.op2_type = arguments_type;
                    new_obj.result = tmp;
                    new_obj.result_type = OpType::Tmp;
                    new_obj._pad |= NEW_FLAG_DYNAMIC_CLASS_NAME | NEW_FLAG_UNPACKED_ARGUMENTS;
                    self.push_instruction_at_line(new_obj, *line);
                    return (tmp, OpType::Tmp);
                }
                let compiled_args: Vec<CompiledCallArg> =
                    if args.iter().any(CallArg::contains_yield) {
                        self.compile_call_args(args, 0, true)
                    } else {
                        args.iter()
                            .map(|arg| match arg {
                                CallArg::Positional(expr) | CallArg::Unpack(expr) => {
                                    let (op, op_type) = self.compile_expr(expr);
                                    (op, op_type, None, None)
                                }
                                CallArg::Named { name, value } => {
                                    let (op, op_type) = self.compile_expr(value);
                                    let name_idx = self.add_literal(Value::string(name.clone()));
                                    (op, op_type, Some(name_idx), None)
                                }
                            })
                            .collect()
                    };
                let (class, class_type) = self.compile_expr(class);
                let tmp = self.alloc_tmp();
                let mut new_obj = Instruction::new(OpCode::NewObj);
                new_obj.op1 = class;
                new_obj.op1_type = class_type;
                new_obj.result = tmp;
                new_obj.result_type = OpType::Tmp;
                new_obj.extended_value = args.len() as u32;
                new_obj._pad |= NEW_FLAG_DYNAMIC_CLASS_NAME;
                self.push_instruction_at_line(new_obj, *line);

                self.emit_precompiled_call_args(&compiled_args, 1);
                let discard = self.alloc_tmp();
                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                do_fcall.result = discard;
                do_fcall.result_type = OpType::Tmp;
                self.push_instruction_at_line(do_fcall, *call_line);

                (tmp, OpType::Tmp)
            }
            Expr::AnonymousNew {
                attributes,
                args,
                is_readonly,
                allow_dynamic_properties,
                parent,
                implements,
                properties,
                constants,
                methods,
                uses,
                trait_aliases,
                line,
                call_line,
            } => {
                let unpacked_arguments = args
                    .iter()
                    .any(|argument| matches!(argument, CallArg::Unpack(_)))
                    .then(|| self.compile_mixed_unpacked_call_arguments(args, 0));
                let compiled_args: Vec<CompiledCallArg> =
                    if unpacked_arguments.is_none() && args.iter().any(CallArg::contains_yield) {
                        self.compile_call_args(args, 0, true)
                    } else {
                        args.iter()
                            .filter(|_| unpacked_arguments.is_none())
                            .map(|arg| match arg {
                                CallArg::Positional(expr) | CallArg::Unpack(expr) => {
                                    let (op, op_type) = self.compile_expr(expr);
                                    (op, op_type, None, None)
                                }
                                CallArg::Named { name, value } => {
                                    let (op, op_type) = self.compile_expr(value);
                                    let name_idx = self.add_literal(Value::string(name.clone()));
                                    (op, op_type, Some(name_idx), None)
                                }
                            })
                            .collect()
                    };
                let sequence = ANONYMOUS_CLASS_COUNTER.fetch_add(1, Ordering::Relaxed);
                let class_name = format!("class@anonymous#{sequence}");
                let declaration = Stmt::Class {
                    line: *call_line,
                    attributes: attributes.clone(),
                    name: format!("\\{class_name}"),
                    parent: parent.clone(),
                    implements: implements.clone(),
                    is_abstract: false,
                    is_final: false,
                    is_readonly: *is_readonly,
                    allow_dynamic_properties: *allow_dynamic_properties,
                    properties: properties.clone(),
                    constants: constants.clone(),
                    methods: methods.clone(),
                    uses: uses.clone(),
                    trait_aliases: trait_aliases.clone(),
                    trait_precedences: Vec::new(),
                    generic_params: Vec::new(),
                };
                if let Err(error) = self.compile_stmt(&declaration) {
                    self.deferred_error = Some(error);
                }

                let name_idx = self.add_literal(Value::string(class_name));
                let tmp = self.alloc_tmp();
                let mut new_obj = Instruction::new(OpCode::NewObj);
                new_obj.op1 = name_idx;
                new_obj.op1_type = OpType::Const;
                if let Some((arguments, arguments_type)) = unpacked_arguments {
                    new_obj.op2 = arguments;
                    new_obj.op2_type = arguments_type;
                    new_obj._pad |= NEW_FLAG_UNPACKED_ARGUMENTS;
                }
                new_obj.result = tmp;
                new_obj.result_type = OpType::Tmp;
                new_obj.extended_value = args.len() as u32;
                self.push_instruction_at_line(new_obj, *line);
                if unpacked_arguments.is_some() {
                    return (tmp, OpType::Tmp);
                }
                self.emit_precompiled_call_args(&compiled_args, 1);
                let discard = self.alloc_tmp();
                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                do_fcall.result = discard;
                do_fcall.result_type = OpType::Tmp;
                self.push_instruction_at_line(do_fcall, *call_line);
                (tmp, OpType::Tmp)
            }
            Expr::PropertyAccess {
                object,
                property,
                nullsafe,
                line,
            } => {
                let (obj_op, obj_type) = self.compile_expr(object);
                let mut receiver_patches = self.take_nullsafe_receiver_patches(obj_op, obj_type);
                let tmp = self.alloc_tmp();

                let nullsafe_patch = if *nullsafe {
                    let mut check = Instruction::new(OpCode::NullSafeCheck);
                    check.op1 = obj_op;
                    check.op1_type = obj_type;
                    check.op2 = 0;
                    check.result = tmp;
                    check.result_type = OpType::Tmp;
                    check.extended_value = 0; // 0 = property access (warn + null on scalar)
                    let idx = self.instructions.len();
                    self.instructions.push(check);
                    Some(idx)
                } else {
                    None
                };

                let prop_idx = self.add_literal(Value::string(property.clone()));
                let mut fetch = Instruction::new(OpCode::FetchObjR);
                fetch.op1 = obj_op;
                fetch.op1_type = obj_type;
                fetch.op2 = prop_idx;
                fetch.op2_type = OpType::Const;
                fetch.result = tmp;
                fetch.result_type = OpType::Tmp;
                if matches!(object.as_ref(), Expr::Variable { name, .. } if name == "this")
                    && self.current_hook_matches(property)
                {
                    fetch._pad |= OBJ_PROP_HOOK_BYPASS;
                }
                self.push_instruction_at_line(fetch, *line);

                if let Some(idx) = nullsafe_patch {
                    receiver_patches.push(idx);
                }
                self.publish_nullsafe_receiver_patches(tmp, receiver_patches);

                (tmp, OpType::Tmp)
            }
            Expr::DynamicPropertyAccess {
                object,
                property,
                nullsafe,
                line,
            } => {
                let (obj_op, obj_type) = self.compile_expr(object);
                let mut receiver_patches = self.take_nullsafe_receiver_patches(obj_op, obj_type);
                let tmp = self.alloc_tmp();
                let nullsafe_patch = if *nullsafe {
                    let mut check = Instruction::new(OpCode::NullSafeCheck);
                    check.op1 = obj_op;
                    check.op1_type = obj_type;
                    check.op2 = 0;
                    check.result = tmp;
                    check.result_type = OpType::Tmp;
                    check.extended_value = 0;
                    let index = self.instructions.len();
                    self.instructions.push(check);
                    Some(index)
                } else {
                    None
                };
                let (property_op, property_type) = self.compile_expr(property);
                let mut fetch = Instruction::new(OpCode::FetchObjR);
                fetch.op1 = obj_op;
                fetch.op1_type = obj_type;
                fetch.op2 = property_op;
                fetch.op2_type = property_type;
                fetch.result = tmp;
                fetch.result_type = OpType::Tmp;
                self.push_instruction_at_line(fetch, *line);
                if let Some(index) = nullsafe_patch {
                    receiver_patches.push(index);
                }
                self.publish_nullsafe_receiver_patches(tmp, receiver_patches);
                (tmp, OpType::Tmp)
            }
            Expr::MethodCall {
                object,
                method,
                args,
                generic_args,
                nullsafe,
                line,
            } => {
                if generic_args.is_empty()
                    && args
                        .iter()
                        .any(|argument| matches!(argument, CallArg::Unpack(_)))
                {
                    let (obj_op, obj_type) = self.compile_expr(object);
                    let mut receiver_patches =
                        self.take_nullsafe_receiver_patches(obj_op, obj_type);
                    let tmp = self.alloc_tmp();
                    let nullsafe_patch = if *nullsafe {
                        let mut check = Instruction::new(OpCode::NullSafeCheck);
                        check.op1 = obj_op;
                        check.op1_type = obj_type;
                        check.op2 = 0;
                        check.result = tmp;
                        check.result_type = OpType::Tmp;
                        check.extended_value = 1;
                        check._pad = self.add_literal(Value::string(method.clone()));
                        let index = self.instructions.len();
                        self.push_instruction_at_line(check, *line);
                        Some(index)
                    } else {
                        None
                    };

                    let callback = self.alloc_tmp();
                    let mut init = Instruction::new(OpCode::InitArray);
                    init.result = callback;
                    init.result_type = OpType::Tmp;
                    init.extended_value = 2;
                    self.instructions.push(init);
                    let mut receiver = Instruction::new(OpCode::AddArrayElement);
                    receiver.op1 = callback;
                    receiver.op1_type = OpType::Tmp;
                    receiver.op2 = obj_op;
                    receiver.op2_type = obj_type;
                    self.instructions.push(receiver);
                    let method = self.add_literal(Value::string(method.clone()));
                    let mut method_element = Instruction::new(OpCode::AddArrayElement);
                    method_element.op1 = callback;
                    method_element.op1_type = OpType::Tmp;
                    method_element.op2 = method;
                    method_element.op2_type = OpType::Const;
                    self.instructions.push(method_element);
                    let (arguments, arguments_type) =
                        self.compile_mixed_unpacked_call_arguments(args, 0);
                    let mut call = Instruction::new(OpCode::CallUserFuncArray);
                    call.op1 = callback;
                    call.op1_type = OpType::Tmp;
                    call.op2 = arguments;
                    call.op2_type = arguments_type;
                    call.result = tmp;
                    call.result_type = OpType::Tmp;
                    call._pad |= CALL_USER_FUNC_ARRAY_SOURCE_UNPACK;
                    self.push_instruction_at_line(call, *line);
                    if let Some(index) = nullsafe_patch {
                        receiver_patches.push(index);
                    }
                    self.publish_nullsafe_receiver_patches(tmp, receiver_patches);
                    return (tmp, OpType::Tmp);
                }
                if args.iter().any(CallArg::contains_yield) {
                    // InitMethodCall owns a pending VM call frame. A yield in
                    // an argument suspends before SendVal/DoFcall and cannot
                    // keep that raw frame alive across generator detachment.
                    // Evaluate the receiver and arguments first, then start
                    // the call protocol from their stable TMP/CV operands.
                    let (obj_op, obj_type) = self.compile_expr(object);
                    let (receiver_name, receiver_line) = match object.as_ref() {
                        Expr::Variable { name, line } => (name.as_str(), *line),
                        _ => ("receiver", 0),
                    };
                    let (obj_op, obj_type) = self.snapshot_yield_rvalue_operand(
                        obj_op,
                        obj_type,
                        receiver_name,
                        receiver_line,
                    );
                    let mut receiver_patches =
                        self.take_nullsafe_receiver_patches(obj_op, obj_type);
                    let tmp = self.alloc_tmp();
                    let nullsafe_patch = if *nullsafe {
                        let mut check = Instruction::new(OpCode::NullSafeCheck);
                        check.op1 = obj_op;
                        check.op1_type = obj_type;
                        check.op2 = 0;
                        check.result = tmp;
                        check.result_type = OpType::Tmp;
                        check.extended_value = 1;
                        check._pad = self.add_literal(Value::string(method.clone()));
                        let index = self.instructions.len();
                        self.push_instruction_at_line(check, *line);
                        Some(index)
                    } else {
                        None
                    };
                    let compiled_args = self.compile_call_args(args, 0, true);
                    let result = self.compile_method_call_from_operands(
                        obj_op,
                        obj_type,
                        tmp,
                        nullsafe_patch,
                        method,
                        args,
                        &compiled_args,
                        generic_args,
                        *line,
                    );
                    if let Some(index) = nullsafe_patch {
                        receiver_patches.push(index);
                    }
                    self.publish_nullsafe_receiver_patches(result.0, receiver_patches);
                    return result;
                }
                let (obj_op, obj_type) = self.compile_expr(object);
                let mut receiver_patches = self.take_nullsafe_receiver_patches(obj_op, obj_type);
                let tmp = self.alloc_tmp();

                let nullsafe_patch = if *nullsafe {
                    let mut check = Instruction::new(OpCode::NullSafeCheck);
                    check.op1 = obj_op;
                    check.op1_type = obj_type;
                    check.op2 = 0;
                    check.result = tmp;
                    check.result_type = OpType::Tmp;
                    check.extended_value = 1; // 1 = method call (fatal on scalar)
                    check._pad = self.add_literal(Value::string(method.clone()));
                    let idx = self.instructions.len();
                    self.push_instruction_at_line(check, *line);
                    Some(idx)
                } else {
                    None
                };

                let method_idx = self.add_literal(Value::string(method.clone()));

                let runtime_generic_check = self.emit_generic_check(
                    OpCode::CheckGenericArgs,
                    GenericDeclarationKind::Method,
                    generic_args,
                    None,
                    obj_op,
                    obj_type,
                    method_idx,
                    OpType::Const,
                );

                let mut init = Instruction::new(OpCode::InitMethodCall);
                init.op1 = obj_op;
                init.op1_type = obj_type;
                init.op2 = method_idx;
                init.op2_type = OpType::Const;
                init.extended_value = args.len() as u32;
                let init_index = self.instructions.len();
                self.push_instruction_at_line(init, *line);

                self.emit_call_args(args, 1, 0, true, true);

                if args.iter().all(|arg| matches!(arg, CallArg::Positional(_)))
                    && self.instructions.len() > init_index + 1 + args.len()
                {
                    self.instructions[init_index]._pad |= CALL_FLAG_DEFERRED_SCALAR_CANDIDATE;
                }

                self.emit_reified_argument_check(runtime_generic_check);

                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                do_fcall.result = tmp;
                do_fcall.result_type = OpType::Tmp;
                self.push_instruction_at_line(do_fcall, *line);
                self.emit_temporary_method_receiver_release(obj_op, obj_type, *line);
                self.emit_reified_return_check(runtime_generic_check, tmp, OpType::Tmp);

                if let Some(idx) = nullsafe_patch {
                    receiver_patches.push(idx);
                }
                self.publish_nullsafe_receiver_patches(tmp, receiver_patches);

                (tmp, OpType::Tmp)
            }
            Expr::StaticCall {
                class_name,
                method,
                args,
                generic_args,
                line,
            } => {
                // Pseudo-class names are runtime call-scope tokens, not names
                // that can be namespace-qualified. Their spelling is also
                // needed later to recover forwarding late-static scope.
                let pseudo_class = class_name.to_ascii_lowercase();
                let resolved_class =
                    if matches!(pseudo_class.as_str(), "self" | "parent" | "static") {
                        class_name.clone()
                    } else {
                        self.resolve_name(class_name)
                    };
                if generic_args.is_empty()
                    && !matches!(pseudo_class.as_str(), "self" | "parent" | "static")
                    && args
                        .iter()
                        .any(|argument| matches!(argument, CallArg::Unpack(_)))
                {
                    let callback = self.alloc_tmp();
                    let mut init = Instruction::new(OpCode::InitArray);
                    init.result = callback;
                    init.result_type = OpType::Tmp;
                    init.extended_value = 2;
                    self.instructions.push(init);
                    for value in [resolved_class.clone(), method.clone()] {
                        let value = self.add_literal(Value::string(value));
                        let mut add = Instruction::new(OpCode::AddArrayElement);
                        add.op1 = callback;
                        add.op1_type = OpType::Tmp;
                        add.op2 = value;
                        add.op2_type = OpType::Const;
                        self.instructions.push(add);
                    }
                    let (arguments, arguments_type) =
                        self.compile_mixed_unpacked_call_arguments(args, 0);
                    let tmp = self.alloc_tmp();
                    let mut call = Instruction::new(OpCode::CallUserFuncArray);
                    call.op1 = callback;
                    call.op1_type = OpType::Tmp;
                    call.op2 = arguments;
                    call.op2_type = arguments_type;
                    call.result = tmp;
                    call.result_type = OpType::Tmp;
                    call._pad |= CALL_USER_FUNC_ARRAY_SOURCE_UNPACK;
                    self.push_instruction_at_line(call, *line);
                    return (tmp, OpType::Tmp);
                }
                let generic_class = match pseudo_class.as_str() {
                    "self" if !self.dynamic_static_scope => self
                        .lexical_static_class
                        .as_ref()
                        .unwrap_or(&resolved_class),
                    "parent" if !self.dynamic_static_scope => self
                        .lexical_static_parent
                        .as_ref()
                        .unwrap_or(&resolved_class),
                    _ => &resolved_class,
                };
                let generic_owner = format!("{}::{}", generic_class, method);
                let class_idx = self.add_literal(Value::string(resolved_class));
                let method_idx = self.add_literal(Value::string(method.clone()));
                let generic_owner_idx = self.add_literal(Value::string(generic_owner.clone()));
                let compiled_args = args
                    .iter()
                    .any(CallArg::contains_yield)
                    .then(|| self.compile_call_args(args, 0, true));
                let dynamic_static_scope = (self.dynamic_static_scope
                    && matches!(pseudo_class.as_str(), "self" | "parent"))
                    || pseudo_class == "static";
                let runtime_generic_check = self.emit_generic_check(
                    if dynamic_static_scope {
                        OpCode::CheckLateStaticGenericArgs
                    } else {
                        OpCode::CheckGenericArgs
                    },
                    GenericDeclarationKind::Method,
                    generic_args,
                    Some(&generic_owner),
                    generic_owner_idx,
                    OpType::Const,
                    0,
                    OpType::Unused,
                );
                #[cfg(target_arch = "x86_64")]
                let late_generic_check_ip = (runtime_generic_check && pseudo_class == "static")
                    .then(|| (self.instructions.len() - 1) as u16);
                #[cfg(not(target_arch = "x86_64"))]
                let late_generic_check_ip: Option<u16> = {
                    let _ = runtime_generic_check;
                    None
                };

                let mut init = Instruction::new(if pseudo_class == "static" {
                    OpCode::InitLateStaticCall
                } else {
                    OpCode::InitStaticCall
                });
                init.op1 = class_idx;
                init.op1_type = OpType::Const;
                init.op2 = method_idx;
                init.op2_type = OpType::Const;
                init.extended_value = args.len() as u32;
                if let Some(check_ip) = late_generic_check_ip {
                    // On x86_64 the immediately preceding late-generic guard has
                    // already proved the current called class. Reuse its keyed
                    // cache instead of resolving the frame scope a second time.
                    // ARM keeps the smaller ordinary initializer path because
                    // this extra marker measurably perturbs its hot-code layout.
                    init.result = check_ip;
                    init.result_type = OpType::Const;
                }
                if self.dynamic_static_scope
                    && matches!(pseudo_class.as_str(), "self" | "parent" | "static")
                {
                    init._pad |= CALL_FLAG_DYNAMIC_STATIC_SCOPE;
                }
                self.instructions.push(init);

                if let Some(compiled_args) = compiled_args.as_deref() {
                    self.emit_precompiled_runtime_call_args(args, compiled_args, 1, 0, true, true);
                } else {
                    self.emit_call_args(args, 1, 0, true, true);
                }
                self.emit_reified_argument_check(runtime_generic_check);

                let tmp = self.alloc_tmp();
                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                do_fcall.result = tmp;
                do_fcall.result_type = OpType::Tmp;
                self.push_instruction_at_line(do_fcall, *line);
                self.emit_reified_return_check(runtime_generic_check, tmp, OpType::Tmp);

                (tmp, OpType::Tmp)
            }
            static_property @ (Expr::StaticProperty { .. }
            | Expr::DynamicNamedStaticProperty { .. }
            | Expr::DynamicStaticProperty { .. }) => {
                let (
                    class_idx,
                    class_type,
                    prop_idx,
                    prop_type,
                    dynamic_static_scope,
                    dynamic_owner,
                    line,
                ) = self
                    .compile_static_property_operands(static_property)
                    .expect("matched static-property form");
                let receiver_patches = self.take_nullsafe_receiver_patches(class_idx, class_type);
                let tmp = self.alloc_tmp();
                let mut fetch = Instruction::new(if dynamic_static_scope {
                    OpCode::FetchLateStaticProp
                } else {
                    OpCode::FetchStaticProp
                });
                fetch.op1 = class_idx;
                fetch.op1_type = class_type;
                fetch.op2 = prop_idx;
                fetch.op2_type = prop_type;
                fetch.result = tmp;
                fetch.result_type = OpType::Tmp;
                if dynamic_owner {
                    fetch._pad |= STATIC_PROP_DYNAMIC_OWNER;
                }
                if prop_type != OpType::Const {
                    fetch._pad |= STATIC_PROP_DYNAMIC_NAME;
                }
                self.push_instruction_at_line(fetch, line);
                self.publish_nullsafe_receiver_patches(tmp, receiver_patches);
                (tmp, OpType::Tmp)
            }
            Expr::ClassConstant {
                class_name,
                constant,
                line,
            } => {
                let (resolved, dynamic_static_scope) = self.resolve_static_member_owner(class_name);
                if constant.eq_ignore_ascii_case("class") && !dynamic_static_scope {
                    let literal = self.add_literal(Value::string(resolved));
                    return (literal, OpType::Const);
                }
                let class_idx = self.add_literal(Value::string(resolved));
                let constant_idx = self.add_literal(Value::string(constant.clone()));
                let tmp = self.alloc_tmp();
                let mut fetch = Instruction::new(if dynamic_static_scope {
                    OpCode::FetchLateClassConst
                } else {
                    OpCode::FetchClassConst
                });
                if self.compiling_constant_expression {
                    fetch._pad |= CLASS_CONST_CONSTANT_EXPRESSION;
                }
                fetch.op1 = class_idx;
                fetch.op1_type = OpType::Const;
                fetch.op2 = constant_idx;
                fetch.op2_type = OpType::Const;
                fetch.result = tmp;
                fetch.result_type = OpType::Tmp;
                self.push_instruction_at_line(fetch, *line);
                (tmp, OpType::Tmp)
            }
            Expr::Throw { expr, line } => {
                let (op, op_type) = self.compile_expr(expr);
                let mut instr = Instruction::new(OpCode::Throw);
                instr.op1 = op;
                instr.op1_type = op_type;
                self.push_instruction_at_line(instr, *line);
                // Throw never returns, but we need to return something
                let null_idx = self.add_literal(Value::null());
                (null_idx, OpType::Const)
            }
            Expr::DynamicCall {
                callable,
                args,
                generic_args,
                line,
                ..
            } => {
                // Compile the callable expression (e.g. $var, $arr[0])
                let (callable_op, callable_type) = self.compile_expr(callable);
                let receiver_patches =
                    self.take_nullsafe_receiver_patches(callable_op, callable_type);
                let (tmp, result_type) = self.compile_dynamic_call_from_operand(
                    callable_op,
                    callable_type,
                    args,
                    generic_args,
                    *line,
                );
                self.publish_nullsafe_receiver_patches(tmp, receiver_patches);
                (tmp, result_type)
            }
            Expr::DynamicStaticCall {
                class,
                method,
                args,
                generic_args,
                line,
            } => {
                if let (
                    Expr::StaticProperty {
                        class_name,
                        property,
                        parenthesized: false,
                        ..
                    },
                    Expr::StringLiteral(hook),
                ) = (class.as_ref(), method.as_ref())
                    && class_name.eq_ignore_ascii_case("parent")
                    && (hook.eq_ignore_ascii_case("get") || hook.eq_ignore_ascii_case("set"))
                {
                    if self.lexical_static_class.is_none() {
                        self.deferred_error = Some(self.goto_error(
                            "Cannot use \"parent\" when no class scope is active",
                            *line,
                        ));
                        let null = self.add_literal(Value::null());
                        return (null, OpType::Const);
                    }
                    let current_hook = self
                        .current_function_name
                        .rsplit_once("::$")
                        .and_then(|(_, suffix)| suffix.split_once("::"));
                    let Some((current_property, current_kind)) = current_hook else {
                        self.deferred_error = Some(self.goto_error(
                            &format!(
                                "Must not use parent::${property}::{hook}() outside a property hook"
                            ),
                            *line,
                        ));
                        let null = self.add_literal(Value::null());
                        return (null, OpType::Const);
                    };
                    if current_property != property {
                        self.deferred_error = Some(self.goto_error(
                            &format!(
                                "Must not use parent::${property}::{hook}() in a different property (${current_property})"
                            ),
                            *line,
                        ));
                        let null = self.add_literal(Value::null());
                        return (null, OpType::Const);
                    }
                    if !current_kind.eq_ignore_ascii_case(hook) {
                        self.deferred_error = Some(self.goto_error(
                            &format!(
                                "Must not use parent::${property}::{hook}() in a different property hook ({current_kind})"
                            ),
                            *line,
                        ));
                        let null = self.add_literal(Value::null());
                        return (null, OpType::Const);
                    }
                    return self.compile_expr(&Expr::StaticCall {
                        class_name: "parent".to_string(),
                        method: format!("${property}::{}", hook.to_ascii_lowercase()),
                        args: args.clone(),
                        generic_args: generic_args.clone(),
                        line: *line,
                    });
                }
                let (class_op, class_type) = self.compile_expr(class);
                let receiver_patches = self.take_nullsafe_receiver_patches(class_op, class_type);
                let callable = self.alloc_tmp();
                let mut init = Instruction::new(OpCode::InitArray);
                init.op1 = class_op;
                init.op1_type = class_type;
                init.result = callable;
                init.result_type = OpType::Tmp;
                init.extended_value = 2;
                init._pad |= ARRAY_INIT_DYNAMIC_CALL_CLASS;
                self.push_instruction_at_line(init, *line);
                let mut class_element = Instruction::new(OpCode::AddArrayElement);
                class_element.op1 = callable;
                class_element.op1_type = OpType::Tmp;
                class_element.op2 = class_op;
                class_element.op2_type = class_type;
                self.instructions.push(class_element);
                let (method_op, method_type) = self.compile_expr(method);
                let mut method_element = Instruction::new(OpCode::AddArrayElement);
                method_element.op1 = callable;
                method_element.op1_type = OpType::Tmp;
                method_element.op2 = method_op;
                method_element.op2_type = method_type;
                self.instructions.push(method_element);
                let (result, result_type) = self.compile_dynamic_call_from_operand(
                    callable,
                    OpType::Tmp,
                    args,
                    generic_args,
                    *line,
                );
                self.publish_nullsafe_receiver_patches(result, receiver_patches);
                (result, result_type)
            }
            Expr::Instanceof { expr, class_name } => {
                let (obj_op, obj_type) = self.compile_expr(expr);
                let (resolved_class, dynamic_static_scope) =
                    self.resolve_static_member_owner(class_name);
                let name_idx = self.add_literal(Value::string(resolved_class));
                let tmp = self.alloc_tmp();
                let mut inst = Instruction::new(OpCode::Instanceof);
                inst.op1 = obj_op;
                inst.op1_type = obj_type;
                inst.op2 = name_idx;
                inst.op2_type = OpType::Const;
                inst.result = tmp;
                inst.result_type = OpType::Tmp;
                if dynamic_static_scope {
                    inst._pad |= INSTANCEOF_DYNAMIC_STATIC_SCOPE;
                }
                self.instructions.push(inst);
                (tmp, OpType::Tmp)
            }
            Expr::DynamicInstanceof { expr, class } => {
                let (obj_op, obj_type) = self.compile_expr(expr);
                let (class_op, class_type) = self.compile_expr(class);
                let tmp = self.alloc_tmp();
                let mut inst = Instruction::new(OpCode::Instanceof);
                inst.op1 = obj_op;
                inst.op1_type = obj_type;
                inst.op2 = class_op;
                inst.op2_type = class_type;
                inst.result = tmp;
                inst.result_type = OpType::Tmp;
                self.instructions.push(inst);
                (tmp, OpType::Tmp)
            }
            Expr::Assign { var, expr } => {
                let (op, op_type) = self.compile_expr(expr);
                let cv_idx = self.resolve_cv(var);
                let mut assign = Instruction::new(OpCode::AssignCv);
                assign.op1_type = OpType::Cv;
                assign.op1 = cv_idx;
                assign.op2_type = op_type;
                assign.op2 = op;
                assign.result_type = OpType::Tmp;
                let tmp = self.alloc_tmp();
                assign.result = tmp;
                self.instructions.push(assign);
                self.definitely_defined_cvs.insert(cv_idx);
                (tmp, OpType::Tmp)
            }
            Expr::AssignReference { var, target } => {
                if let Expr::Globals { line } = target.as_ref() {
                    self.deferred_error =
                        Some(self.goto_error("Cannot acquire reference to $GLOBALS", *line));
                    let null = self.add_literal(Value::null());
                    return (null, OpType::Const);
                }
                let destination = self.resolve_cv(var);
                self.definitely_defined_cvs.insert(destination);
                match target.as_ref() {
                    Expr::Variable { name, .. } => {
                        let source = self.resolve_cv(name);
                        let mut bind = Instruction::new(OpCode::BindCvRef);
                        bind.op1 = source;
                        bind.op1_type = OpType::Cv;
                        bind.result = destination;
                        bind.result_type = OpType::Cv;
                        self.instructions.push(bind);
                        (destination, OpType::Cv)
                    }
                    Expr::DynamicVariable { name, line } => {
                        let (key, key_type) = self.compile_expr(name);
                        let mut bind = Instruction::new(OpCode::BindDynamicVarRef);
                        bind.op1 = key;
                        bind.op1_type = key_type;
                        bind.result = destination;
                        bind.result_type = OpType::Cv;
                        self.push_instruction_at_line(bind, *line);
                        (destination, OpType::Cv)
                    }
                    Expr::PropertyAccess {
                        object,
                        property,
                        nullsafe: false,
                        line,
                    } => {
                        let (object, object_type) = self.compile_property_modify_base(object);
                        let property = self.add_literal(Value::string(property.clone()));
                        let mut bind = Instruction::new(OpCode::BindObjPropRef);
                        bind.op1 = object;
                        bind.op1_type = object_type;
                        bind.op2 = property;
                        bind.op2_type = OpType::Const;
                        bind.result = destination;
                        bind.result_type = OpType::Cv;
                        self.push_instruction_at_line(bind, *line);
                        (destination, OpType::Cv)
                    }
                    Expr::DynamicPropertyAccess {
                        object,
                        property,
                        nullsafe: false,
                        line,
                    } => {
                        let (object, object_type) = self.compile_property_modify_base(object);
                        let (property, property_type) = self.compile_expr(property);
                        let mut bind = Instruction::new(OpCode::BindObjPropRef);
                        bind.op1 = object;
                        bind.op1_type = object_type;
                        bind.op2 = property;
                        bind.op2_type = property_type;
                        bind.result = destination;
                        bind.result_type = OpType::Cv;
                        self.push_instruction_at_line(bind, *line);
                        (destination, OpType::Cv)
                    }
                    static_property @ (Expr::StaticProperty { .. }
                    | Expr::DynamicNamedStaticProperty { .. }
                    | Expr::DynamicStaticProperty { .. }) => {
                        if let Err(error) = self.compile_static_property_reference_fetch(
                            static_property,
                            destination,
                            false,
                        ) {
                            self.deferred_error = Some(error);
                            let null = self.add_literal(Value::null());
                            return (null, OpType::Const);
                        }
                        (destination, OpType::Cv)
                    }
                    Expr::ArrayAccess { array, index, .. } => {
                        if matches!(array.as_ref(), Expr::Globals { .. }) {
                            let (key, key_type) = self.compile_expr(index);
                            let mut bind = Instruction::new(OpCode::BindGlobalRef);
                            bind.op1 = key;
                            bind.op1_type = key_type;
                            bind.result = destination;
                            bind.result_type = OpType::Cv;
                            self.instructions.push(bind);
                            return (destination, OpType::Cv);
                        }
                        if let Err(error) =
                            self.compile_array_element_reference_binding(target, destination, false)
                        {
                            self.deferred_error = Some(error);
                            let null = self.add_literal(Value::null());
                            return (null, OpType::Const);
                        }
                        (destination, OpType::Cv)
                    }
                    _ => {
                        // A call result may carry the stable reference cell of
                        // a function declared with `function &name()`. Bind the
                        // destination to that result instead of degrading the
                        // `=&` expression to an ordinary value assignment.
                        let (source, source_type) = self.compile_expr(target);
                        let mut bind = Instruction::new(OpCode::BindCvRef);
                        bind.op1 = source;
                        bind.op1_type = source_type;
                        bind.result = destination;
                        bind.result_type = OpType::Cv;
                        let line = match target.as_ref() {
                            Expr::FunctionCall { line, .. }
                            | Expr::MethodCall { line, .. }
                            | Expr::StaticCall { line, .. }
                            | Expr::DynamicCall { line, .. }
                            | Expr::DynamicStaticCall { line, .. } => *line,
                            _ => 0,
                        };
                        self.push_instruction_at_line(bind, line);
                        (destination, OpType::Cv)
                    }
                }
            }
            Expr::AssignTargetReference { target, source } => {
                match self.compile_target_reference_assignment(target, source) {
                    Ok(result) => result,
                    Err(error) => {
                        self.deferred_error = Some(error);
                        let null = self.add_literal(Value::null());
                        (null, OpType::Const)
                    }
                }
            }
            Expr::AssignTarget { target, expr } => {
                match self.compile_assignment_target_expression(target, expr) {
                    Ok(result) => result,
                    Err(error) => {
                        self.deferred_error = Some(error);
                        let null = self.add_literal(Value::null());
                        (null, OpType::Const)
                    }
                }
            }
            Expr::ArrayAppendAssign {
                target,
                expr,
                by_ref,
            } => {
                let direct_cv = if let Expr::Variable { name, .. } = target.as_ref() {
                    Some(self.resolve_cv(name))
                } else {
                    None
                };
                let mutable_source = if direct_cv.is_none() {
                    match self.compile_foreach_reference_source(target, true, false) {
                        Ok(source) => Some(source),
                        Err(error) => {
                            self.deferred_error = Some(error);
                            let null = self.add_literal(Value::null());
                            return (null, OpType::Const);
                        }
                    }
                } else {
                    None
                };
                let reference_source = if *by_ref {
                    match self.compile_array_element_reference_source(expr) {
                        Ok(source) => Some(source),
                        Err(error) => {
                            self.deferred_error = Some(error);
                            let null = self.add_literal(Value::null());
                            return (null, OpType::Const);
                        }
                    }
                } else {
                    None
                };
                let (value, value_type) = reference_source
                    .map_or_else(|| self.compile_expr(expr), |source| (source, OpType::Cv));
                let (assigned, assigned_type) = if !*by_ref && value_type == OpType::Cv {
                    let assigned = self.alloc_tmp();
                    let mut preserve = Instruction::new(OpCode::AssignCv);
                    preserve.op1 = assigned;
                    preserve.op1_type = OpType::Tmp;
                    preserve.op2 = value;
                    preserve.op2_type = value_type;
                    self.instructions.push(preserve);
                    (assigned, OpType::Tmp)
                } else {
                    (value, value_type)
                };
                let (array, array_type) = direct_cv.map_or_else(
                    || {
                        let (array, array_type, _) = mutable_source
                            .as_ref()
                            .expect("non-variable append retains its mutable source");
                        (*array, *array_type)
                    },
                    |cv| (cv, OpType::Cv),
                );
                let mut append = Instruction::new(OpCode::ArrayPushOp);
                append.op1 = array;
                append.op1_type = array_type;
                append.op2 = assigned;
                append.op2_type = assigned_type;
                if *by_ref {
                    append._pad |= ARRAY_ELEMENT_REFERENCE;
                }
                self.instructions.push(append);
                if let Some((_, _, writeback)) = mutable_source {
                    self.emit_foreach_reference_source_writeback(writeback, array, array_type);
                }
                if let Some(cv) = direct_cv {
                    self.definitely_defined_cvs.insert(cv);
                }
                (assigned, assigned_type)
            }
            Expr::ListAssign {
                targets,
                expr,
                line,
            } => {
                let contains_reference = targets.iter().any(ListTarget::contains_reference);
                let (retained, retained_type, writeback, diagnose_nonreferenceable) = match self
                    .compile_list_assignment_source(
                        expr,
                        contains_reference,
                        targets
                            .iter()
                            .map(ListTarget::source_line)
                            .find(|line| *line != 0)
                            .unwrap_or(0),
                    ) {
                    Ok(source) => source,
                    Err(error) => {
                        self.deferred_error = Some(error);
                        let null = self.add_literal(Value::null());
                        return (null, OpType::Const);
                    }
                };
                if let Err(error) = self.compile_list_targets(
                    targets,
                    retained,
                    retained_type,
                    0,
                    *line,
                    diagnose_nonreferenceable,
                ) {
                    self.deferred_error = Some(error);
                }
                if contains_reference
                    && !matches!(&writeback, ForeachArrayWriteback::ReleaseInternalCv(_))
                {
                    self.emit_foreach_reference_source_writeback(
                        writeback,
                        retained,
                        retained_type,
                    );
                }
                (retained, retained_type)
            }
            Expr::FirstClassCallable(callable) => {
                let (callable, callable_type) = self.compile_expr(callable);
                let result = self.alloc_tmp();
                let mut create = Instruction::new(OpCode::CreateFirstClassCallable);
                create.op1 = callable;
                create.op1_type = callable_type;
                create.result = result;
                create.result_type = OpType::Tmp;
                self.instructions.push(create);
                (result, OpType::Tmp)
            }
            Expr::FirstClassFunctionCallable(name) => {
                let resolved = self.resolve_function_name(name);
                let callable = self.add_literal(Value::string(resolved));
                let fallback = if self.current_namespace.is_some()
                    && !name.contains('\\')
                    && !self.has_function_import(name)
                {
                    self.add_literal(Value::string(name.clone()))
                } else {
                    0
                };
                let result = self.alloc_tmp();
                let mut create = Instruction::new(OpCode::CreateFirstClassCallable);
                create.op1 = callable;
                create.op1_type = OpType::Const;
                create.result = result;
                create.result_type = OpType::Tmp;
                create.extended_value = fallback as u32;
                self.instructions.push(create);
                (result, OpType::Tmp)
            }
            Expr::Constant(name) => {
                // Fetch a named constant at runtime
                let (runtime_name, fallback) = self.resolve_constant_name(name);
                let name_idx = self.add_literal(Value::string(runtime_name));
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::FetchConst);
                instr.op1 = name_idx;
                instr.op1_type = OpType::Const;
                // extended_value = 0 means exact read; 2 enables the PHP
                // unqualified-constant fallback from namespace to global.
                if let Some(fallback) = fallback {
                    instr.op2 = self.add_literal(Value::string(fallback));
                    instr.op2_type = OpType::Const;
                    instr.extended_value = 2;
                }
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                self.instructions.push(instr);
                (tmp, OpType::Tmp)
            }
            Expr::MagicConstant { name, line } => {
                if self.dynamic_static_scope && name.eq_ignore_ascii_case("__CLASS__") {
                    let tmp = if let Some(tmp) = self.trait_class_scope_tmp {
                        tmp
                    } else {
                        let tmp = self.alloc_tmp();
                        self.trait_class_scope_tmp = Some(tmp);
                        tmp
                    };
                    return (tmp, OpType::Tmp);
                }
                let value = self.magic_constant_value(name, *line);
                let literal = self.add_literal(value);
                (literal, OpType::Const)
            }
            Expr::Yield { value, key } => {
                self.contains_yield = true;
                let mut instr = Instruction::new(OpCode::Yield);
                // op1 = yielded value
                if let Some(val_expr) = value {
                    let (val_op, val_type) = self.compile_expr(val_expr);
                    instr.op1 = val_op;
                    instr.op1_type = val_type;
                } else {
                    let null_idx = self.add_literal(Value::null());
                    instr.op1 = null_idx;
                    instr.op1_type = OpType::Const;
                }
                // op2 = key (if yield $key => $value)
                if let Some(key_expr) = key {
                    let (key_op, key_type) = self.compile_expr(key_expr);
                    instr.op2 = key_op;
                    instr.op2_type = key_type;
                }
                // result = value received from send()
                let tmp = self.alloc_tmp();
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                self.instructions.push(instr);
                (tmp, OpType::Tmp)
            }
            Expr::YieldFrom {
                expr: sub_expr,
                line,
            } => {
                self.contains_yield = true;
                let (sub_op, sub_type) = self.compile_expr(sub_expr);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::YieldFrom);
                instr.op1 = sub_op;
                instr.op1_type = sub_type;
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                self.push_instruction_at_line(instr, *line);
                (tmp, OpType::Tmp)
            }
            Expr::Clone {
                expr: inner,
                with_properties,
                line,
            } => {
                let (src_op, src_type) = self.compile_expr(inner);
                let properties = with_properties
                    .as_ref()
                    .map(|properties| self.compile_expr(properties));
                if let Some((properties_op, properties_type)) = properties {
                    let mut validate = Instruction::new(OpCode::ValidateCloneWith);
                    validate.op1 = properties_op;
                    validate.op1_type = properties_type;
                    self.push_instruction_at_line(validate, *line);
                }
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::CloneObj);
                instr.op1 = src_op;
                instr.op1_type = src_type;
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                if properties.is_some() {
                    instr._pad |= CLONE_OBJ_WITH_PROPERTIES;
                }
                self.push_instruction_at_line(instr, *line);
                if let Some((properties_op, properties_type)) = properties {
                    // Reuse the canonical iteration and property-write paths so
                    // clone-with inherits PHP ordering, visibility, hooks,
                    // magic methods, type checks and exception behavior.
                    let array_tmp = self.alloc_tmp();
                    let position_tmp = self.alloc_tmp();
                    let mut init = Instruction::new(OpCode::ForeachInit);
                    init.op1 = properties_op;
                    init.op1_type = properties_type;
                    init.result = array_tmp;
                    init.result_type = OpType::Tmp;
                    init.extended_value = position_tmp as u32;
                    let init_index = self.instructions.len();
                    self.push_instruction_at_line(init, *line);

                    let value_cv = self.resolve_cv(&format!("\0clone_with_value_{init_index}"));
                    let key_cv = self.resolve_cv(&format!("\0clone_with_key_{init_index}"));
                    let loop_start = self.instructions.len();
                    let has_entry_tmp = self.alloc_tmp();
                    let mut next = Instruction::new(OpCode::ForeachNext);
                    next.op1 = array_tmp;
                    next.op1_type = OpType::Tmp;
                    next.op2 = position_tmp;
                    next.op2_type = OpType::Tmp;
                    next.result = has_entry_tmp;
                    next.result_type = OpType::Tmp;
                    next.extended_value = (((key_cv as u32) + 1) << 16) | value_cv as u32;
                    self.instructions.push(next);

                    let mut done = Instruction::new(OpCode::JmpZ);
                    done.op1 = has_entry_tmp;
                    done.op1_type = OpType::Tmp;
                    let done_index = self.instructions.len();
                    self.instructions.push(done);

                    let mut assign = Instruction::new(OpCode::AssignObjProp);
                    assign.op1 = tmp;
                    assign.op1_type = OpType::Tmp;
                    assign.op2 = key_cv;
                    assign.op2_type = OpType::Cv;
                    assign.result = value_cv;
                    assign.result_type = OpType::Cv;
                    assign._pad |= ASSIGN_OBJ_CLONE_WITH;
                    self.push_instruction_at_line(assign, *line);

                    let mut repeat = Instruction::new(OpCode::Jmp);
                    repeat.op1 = loop_start as u16;
                    self.instructions.push(repeat);

                    let end = self.instructions.len() as u16;
                    self.instructions[init_index].op2 = end;
                    self.instructions[done_index].op2 = end;
                    let finish = Instruction::new(OpCode::EndCloneWith);
                    self.push_instruction_at_line(finish, *line);
                }
                (tmp, OpType::Tmp)
            }
        }
    }

    fn add_literal(&mut self, val: Value) -> u16 {
        let idx = self.literals.len() as u16;
        self.literals.push(val);
        idx
    }

    /// Preserve the side effect of a standalone expression while suppressing
    /// an immediately-produced TMP that no consumer can observe.
    ///
    /// Only opcodes whose runtime handlers explicitly support an Unused result
    /// belong here. Other expression kinds keep materializing their value.
    fn discard_unused_expr_result(&mut self, result: u16, result_type: OpType) {
        if result_type != OpType::Tmp {
            return;
        }
        // A temporary method receiver is retired immediately after DoFcall.
        // That ReleaseTemps does not consume the return value and must not
        // prevent a standalone call from publishing an Unused result. Stop at
        // any other trailing opcode because return/generic checks do consume
        // the materialized value.
        if let Some(instruction) = self
            .instructions
            .iter_mut()
            .rev()
            .find(|instruction| instruction.opcode != OpCode::ReleaseTemps)
        {
            if matches!(
                instruction.opcode,
                OpCode::DirectInternalCall1
                    | OpCode::DirectInternalCall2
                    | OpCode::Strlen
                    | OpCode::Strlen_Cv
                    | OpCode::DoFcall
                    | OpCode::PreInc
                    | OpCode::PreDec
                    | OpCode::PostInc
                    | OpCode::PostDec
            ) && instruction.result == result
                && instruction.result_type == OpType::Tmp
            {
                instruction.result_type = OpType::Unused;
            }
        }
    }

    /// Whether a source-level function name unambiguously addresses a global
    /// builtin. An unqualified name inside a namespace must retain the normal
    /// fallback lookup because a namespaced user function may shadow it.
    fn unambiguous_global_function_name<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        if let Some(fully_qualified) = name.strip_prefix('\\') {
            return Some(fully_qualified);
        }
        if !name.contains('\\')
            && let Some(imported) = self.function_use_map.get(&name.to_ascii_lowercase())
        {
            return (!imported.contains('\\')).then_some(imported.as_str());
        }
        if self.current_namespace.is_none() && !name.contains('\\') {
            Some(name)
        } else {
            None
        }
    }

    fn is_global_builtin_call(&self, name: &str, builtin: &str) -> bool {
        self.unambiguous_global_function_name(name)
            .is_some_and(|name| name.eq_ignore_ascii_case(builtin))
    }

    fn resolve_cv(&mut self, name: &str) -> u16 {
        if let Some(&idx) = self.cv_table.get(name) {
            idx as u16
        } else {
            let idx = self.next_cv;
            self.next_cv += 1;
            self.cv_table.insert(name.to_string(), idx);
            idx as u16
        }
    }

    fn resolve_finally_jump_cv(&mut self) -> u32 {
        if let Some(cv) = self.finally_jump_cv {
            return cv;
        }
        let cv = self.resolve_cv("\0finally_jump") as u32;
        self.finally_jump_cv = Some(cv);
        cv
    }

    fn enter_goto_region(&mut self, kind: GotoRegionKind) {
        let region = GotoRegion {
            id: self.next_goto_region_id,
            kind,
        };
        self.next_goto_region_id = self
            .next_goto_region_id
            .checked_add(1)
            .expect("too many structural goto regions");
        self.goto_regions.push(region);
    }

    fn leave_goto_region(&mut self) {
        self.goto_regions
            .pop()
            .expect("goto region stack must remain balanced");
    }

    fn goto_error(&self, message: &str, line: usize) -> String {
        if self.source_file.is_empty() {
            format!("{message} on line {line}")
        } else {
            format!("{message} in {} on line {line}", self.source_file)
        }
    }

    fn validate_goto_regions(
        &self,
        source: &[GotoRegion],
        target: &[GotoRegion],
        line: usize,
    ) -> Result<(), String> {
        if source
            .iter()
            .any(|region| region.kind == GotoRegionKind::Finally && !target.contains(region))
        {
            return Err(self.goto_error("jump out of a finally block is disallowed", line));
        }
        if target
            .iter()
            .any(|region| region.kind == GotoRegionKind::Finally && !source.contains(region))
        {
            return Err(self.goto_error("jump into a finally block is disallowed", line));
        }
        if target
            .iter()
            .any(|region| region.kind == GotoRegionKind::LoopOrSwitch && !source.contains(region))
        {
            return Err(self.goto_error("'goto' into loop or switch statement is disallowed", line));
        }
        Ok(())
    }

    fn goto_leaves_finally_region(source: &[GotoRegion], target: &[GotoRegion]) -> bool {
        source
            .iter()
            .any(|region| region.kind == GotoRegionKind::TryFinally && !target.contains(region))
    }

    fn define_label(&mut self, name: &str) -> Result<(), String> {
        if self.labels.contains_key(name) {
            return Err(format!("Label '{name}' already defined"));
        }
        let target = self.instructions.len() as u16;
        let target_regions = self.goto_regions.clone();
        let mut index = 0;
        while index < self.goto_patches.len() {
            if self.goto_patches[index].label == name {
                let patch = self.goto_patches.swap_remove(index);
                self.validate_goto_regions(&patch.regions, &target_regions, patch.line)?;
                self.instructions[patch.instruction].op1 = target;
                if Self::goto_leaves_finally_region(&patch.regions, &target_regions) {
                    self.instructions[patch.instruction]._pad |=
                        crate::vm::instruction::JMP_FLAG_TARGET_OUTSIDE_TRY;
                }
            } else {
                index += 1;
            }
        }
        self.labels.insert(
            name.to_string(),
            GotoLabel {
                instruction: target,
                regions: target_regions,
            },
        );
        Ok(())
    }

    fn emit_goto(&mut self, name: &str, line: usize) -> Result<(), String> {
        let mut instruction = Instruction::new(OpCode::Jmp);
        if let Some(target) = self.labels.get(name) {
            self.validate_goto_regions(&self.goto_regions, &target.regions, line)?;
            instruction.op1 = target.instruction;
            if Self::goto_leaves_finally_region(&self.goto_regions, &target.regions) {
                instruction._pad |= crate::vm::instruction::JMP_FLAG_TARGET_OUTSIDE_TRY;
            }
        } else {
            self.goto_patches.push(GotoPatch {
                instruction: self.instructions.len(),
                label: name.to_string(),
                regions: self.goto_regions.clone(),
                line,
            });
        }
        self.instructions.push(instruction);
        Ok(())
    }

    fn finalize_gotos(&mut self) -> Result<(), String> {
        if let Some(patch) = self.goto_patches.first() {
            return Err(format!("'goto' to undefined label '{}'", patch.label));
        }
        // Resolve non-local transfers once, after forward labels and every
        // try/finally range are known. Ordinary loop backedges retain Jmp's
        // original hot path; only an actual crossing creates a continuation.
        for instruction_index in 0..self.instructions.len() {
            let instruction = self.instructions[instruction_index];
            if instruction.opcode != OpCode::Jmp {
                continue;
            }
            let target = u32::from(instruction.op1);
            let target_outside_try =
                instruction._pad & crate::vm::instruction::JMP_FLAG_TARGET_OUTSIDE_TRY != 0;
            let source = instruction_index as u32;
            if self.try_entries.iter().any(|entry| {
                entry.finally_start != u32::MAX
                    && source >= entry.try_start
                    && source < entry.finally_start
                    && !(target >= entry.try_start
                        && target < entry.finally_end
                        && !(target_outside_try && target == entry.try_start))
            }) {
                self.instructions[instruction_index].opcode = OpCode::JmpFinally;
            }
        }
        Ok(())
    }

    /// Build CV metadata in allocation order rather than randomized map order.
    fn all_cvs(&self) -> Vec<(u32, String)> {
        let mut cvs: Vec<_> = self
            .cv_table
            .iter()
            .map(|(name, &idx)| (idx, name.clone()))
            .collect();
        cvs.sort_unstable_by_key(|(idx, _)| *idx);
        cvs
    }

    /// Retain variable names only when a compiled method can execute another
    /// source unit in its local symbol-table scope.
    fn dynamic_scope_cvs(&self) -> Vec<(u32, String)> {
        self.instructions
            .iter()
            .any(|instruction| matches!(instruction.opcode, OpCode::Include | OpCode::Eval))
            .then(|| self.all_cvs())
            .unwrap_or_default()
    }

    /// Controls how a positional argument's Send opcode is chosen.
    /// - `RefAware`: compile-time ref check (FunctionCall with known ref_args)
    /// - `ValOnly`: always SendVal (New — constructor ref_args unknown at compile time)
    /// - `VarEx`: runtime ref check via SendVarEx (MethodCall, StaticCall, DynamicCall)
    fn positional_opcode(ref_args: u64, index: usize, op_type: OpType, use_var_ex: bool) -> OpCode {
        if ref_args != 0 && !use_var_ex {
            // RefAware mode (FunctionCall)
            let is_ref = index < 64 && (ref_args & (1u64 << index)) != 0;
            if is_ref && matches!(op_type, OpType::Cv | OpType::Tmp | OpType::Var) {
                OpCode::SendRef
            } else {
                OpCode::SendVal
            }
        } else if use_var_ex && op_type == OpType::Cv {
            OpCode::SendVarEx
        } else {
            OpCode::SendVal
        }
    }

    /// Emit Send instructions for a call's argument list.
    ///
    /// `args` — the CallArg slice from the AST.
    /// `cv_offset` — added to each positional index for op2 (0 for functions, 1 for methods/$this).
    /// `ref_args` — compile-time by-ref bitmask (0 when unknown).
    /// `use_var_ex` — true to emit SendVarEx for CV operands (method/static/dynamic calls).
    /// `set_extended_value` — true to set extended_value = param index on positional sends.
    fn emit_call_args(
        &mut self,
        args: &[CallArg],
        cv_offset: u32,
        ref_args: u64,
        use_var_ex: bool,
        set_extended_value: bool,
    ) {
        for (i, arg) in args.iter().enumerate() {
            match arg {
                CallArg::Positional(Expr::ArrayAccess { array, index, .. })
                    if matches!(array.as_ref(), Expr::ArrayAppendArgument { .. })
                        && (i >= 64 || ref_args & (1u64 << i) == 0) =>
                {
                    let Expr::ArrayAppendArgument { target, .. } = array.as_ref() else {
                        unreachable!();
                    };
                    // PHP evaluates the lvalue base and following key before
                    // raising the catchable read Error for this one-level
                    // intermediate append argument. A direct variable key is
                    // fetched in the FUNC_ARG lvalue context and is therefore
                    // silent; compound key expressions retain ordinary read
                    // diagnostics. The operation does not append.
                    let _ = self.compile_expr(target);
                    if let Expr::Variable { name, .. } = index.as_ref() {
                        let _ = self.resolve_cv(name);
                    } else {
                        let _ = self.compile_expr(index);
                    }
                    let error = self.add_literal(crate::value::make_error_value(
                        "Error",
                        "Cannot use [] for reading",
                    ));
                    let mut throw = Instruction::new(OpCode::Throw);
                    throw.op1 = error;
                    throw.op1_type = OpType::Const;
                    self.instructions.push(throw);

                    let null = self.add_literal(Value::null());
                    let mut send = Instruction::new(OpCode::SendVal);
                    send.op1 = null;
                    send.op1_type = OpType::Const;
                    send.op2 = (i as u32 + cv_offset) as u16;
                    if set_extended_value {
                        send.extended_value = i as u32;
                    }
                    self.instructions.push(send);
                }
                CallArg::Positional(Expr::Variable { name, .. })
                    if !use_var_ex && i < 64 && ref_args & (1u64 << i) != 0 =>
                {
                    let cv = self.resolve_cv(name);
                    let mut send = Instruction::new(OpCode::SendRef);
                    send.op1 = cv;
                    send.op1_type = OpType::Cv;
                    send.op2 = (i as u32 + cv_offset) as u16;
                    if set_extended_value {
                        send.extended_value = i as u32;
                    }
                    self.instructions.push(send);
                }
                CallArg::Positional(Expr::Variable { name, line }) if use_var_ex => {
                    let cv = self.resolve_cv(name);
                    let mut send = Instruction::new(OpCode::SendVarEx);
                    send.op1 = cv;
                    send.op1_type = OpType::Cv;
                    send.op2 = (i as u32 + cv_offset) as u16;
                    if set_extended_value {
                        send.extended_value = i as u32;
                    }
                    if *line != 0 {
                        send.result = self.add_literal(Value::string(name.clone()));
                        send.result_type = OpType::Const;
                        send._pad |= crate::vm::instruction::SEND_FLAG_FETCH_CV_R;
                        self.push_instruction_at_line(send, *line);
                    } else {
                        self.instructions.push(send);
                    }
                }
                CallArg::Positional(expr)
                    if (!use_var_ex
                        && i < 64
                        && ref_args & (1u64 << i) != 0
                        && matches!(
                            expr,
                            Expr::DynamicVariable { .. }
                                | Expr::PropertyAccess {
                                    nullsafe: false,
                                    ..
                                }
                                | Expr::DynamicPropertyAccess {
                                    nullsafe: false,
                                    ..
                                }
                                | Expr::StaticProperty { .. }
                                | Expr::DynamicNamedStaticProperty { .. }
                                | Expr::DynamicStaticProperty { .. }
                        ))
                        || (use_var_ex
                            && (matches!(
                                expr,
                                Expr::DynamicVariable { .. }
                                    | Expr::StaticProperty { .. }
                                    | Expr::DynamicNamedStaticProperty { .. }
                                    | Expr::DynamicStaticProperty { .. }
                            ) || matches!(
                                expr,
                                Expr::PropertyAccess {
                                    object,
                                    nullsafe: false,
                                    ..
                                } | Expr::DynamicPropertyAccess {
                                    object,
                                    nullsafe: false,
                                    ..
                                } if matches!(object.as_ref(), Expr::Variable { name, .. } if name == "this")
                            ))) =>
                {
                    let source = self
                        .compile_array_element_reference_source(expr)
                        .expect("matched mutable call argument must compile as a reference source");
                    let mut send = Instruction::new(if use_var_ex {
                        OpCode::SendVarEx
                    } else {
                        OpCode::SendRef
                    });
                    send.op1 = source;
                    send.op1_type = OpType::Cv;
                    send.op2 = (i as u32 + cv_offset) as u16;
                    if set_extended_value {
                        send.extended_value = i as u32;
                    }
                    self.instructions.push(send);
                }
                CallArg::Positional(expr) | CallArg::Unpack(expr) => {
                    let (op, op_type) = self.compile_expr(expr);
                    let nonreferenceable = Self::nullsafe_chain_line(expr).is_some();
                    let opcode = if nonreferenceable {
                        OpCode::SendVal
                    } else {
                        Self::positional_opcode(ref_args, i, op_type, use_var_ex)
                    };
                    let mut send = Instruction::new(opcode);
                    send.op1 = op;
                    send.op1_type = op_type;
                    send.op2 = (i as u32 + cv_offset) as u16;
                    if matches!(expr, Expr::Globals { .. }) {
                        send._pad |= SEND_FLAG_GLOBALS;
                        send.extended_value = i as u32;
                    }
                    if set_extended_value {
                        send.extended_value = i as u32;
                    }
                    if nonreferenceable {
                        send._pad |= SEND_FLAG_NONREFERENCEABLE;
                        send.extended_value = i as u32;
                    }
                    self.instructions.push(send);
                }
                CallArg::Named {
                    name,
                    value:
                        Expr::Variable {
                            name: variable,
                            line,
                        },
                } => {
                    let cv = self.resolve_cv(variable);
                    let mut send = Instruction::new(OpCode::SendNamed);
                    send.op1 = cv;
                    send.op1_type = OpType::Cv;
                    send.op2 = self.add_literal(Value::string(name.clone()));
                    send.op2_type = OpType::Const;
                    send.extended_value = i as u32;
                    if *line != 0 {
                        send.result = self.add_literal(Value::string(variable.clone()));
                        send.result_type = OpType::Const;
                        send._pad |= crate::vm::instruction::SEND_FLAG_FETCH_CV_R;
                        self.push_instruction_at_line(send, *line);
                    } else {
                        self.instructions.push(send);
                    }
                }
                CallArg::Named { name, value } => {
                    let (op, op_type) = self.compile_expr(value);
                    let name_idx = self.add_literal(Value::string(name.clone()));
                    let mut send = Instruction::new(OpCode::SendNamed);
                    send.op1 = op;
                    send.op1_type = op_type;
                    send.op2 = name_idx;
                    send.op2_type = OpType::Const;
                    // The first named send uses this source position to
                    // initialize only the argument slots that no preceding
                    // positional send could have written.
                    send.extended_value = i as u32;
                    if Self::nullsafe_chain_line(value).is_some() {
                        send._pad |= SEND_FLAG_NONREFERENCEABLE;
                    }
                    self.instructions.push(send);
                }
            }
        }
    }

    /// Evaluate arguments that precede a later `yield` into stable operands.
    /// A plain CV operand would otherwise be reread only after the generator
    /// resumes. For known by-value parameters we retain its current value in a
    /// TMP. Late-bound positional and named sends additionally retain the
    /// original CV index as instruction metadata, allowing the VM to select
    /// that l-value only when runtime signature resolution proves that the
    /// parameter is by-reference.
    fn snapshot_yield_rvalue_operand(
        &mut self,
        op: u16,
        op_type: OpType,
        name: &str,
        line: usize,
    ) -> (u16, OpType) {
        if op_type != OpType::Cv {
            return (op, op_type);
        }
        let result = self.alloc_tmp();
        let mut snapshot = Instruction::new(OpCode::FetchCvR);
        snapshot.op1 = op;
        snapshot.op1_type = OpType::Cv;
        snapshot.op2 = self.add_literal(Value::string(name));
        snapshot.op2_type = OpType::Const;
        snapshot.result = result;
        snapshot.result_type = OpType::Tmp;
        self.push_instruction_at_line(snapshot, line);
        (result, OpType::Tmp)
    }

    fn compile_call_args(
        &mut self,
        args: &[CallArg],
        ref_args: u64,
        runtime_reference_check: bool,
    ) -> Vec<CompiledCallArg> {
        args.iter()
            .enumerate()
            .map(|(index, arg)| {
                let (mut op, mut op_type, name_literal) = match arg {
                    CallArg::Positional(expr) | CallArg::Unpack(expr) => {
                        let (op, op_type) = self.compile_expr(expr);
                        (op, op_type, None)
                    }
                    CallArg::Named { name, value } => {
                        let (op, op_type) = self.compile_expr(value);
                        let name_idx = self.add_literal(Value::string(name.clone()));
                        (op, op_type, Some(name_idx))
                    }
                };
                let mut source_cv = None;

                let positional = matches!(arg, CallArg::Positional(_) | CallArg::Unpack(_));
                let known_reference = positional
                    && !runtime_reference_check
                    && index < 64
                    && ref_args & (1u64 << index) != 0;
                if op_type == OpType::Cv && !known_reference {
                    let original_cv = op;
                    let (name, line) = match arg.expr() {
                        Expr::Variable { name, line } => (name.clone(), *line),
                        _ => ("argument".to_string(), 0),
                    };
                    let result = self.alloc_tmp();
                    let mut snapshot = Instruction::new(OpCode::FetchCvR);
                    snapshot.op1 = original_cv;
                    snapshot.op1_type = OpType::Cv;
                    snapshot.op2 = self.add_literal(Value::string(name));
                    snapshot.op2_type = OpType::Const;
                    snapshot.result = result;
                    snapshot.result_type = OpType::Tmp;
                    self.push_instruction_at_line(snapshot, line);
                    op = result;
                    op_type = OpType::Tmp;
                    if runtime_reference_check || !positional {
                        source_cv = Some(original_cv);
                    }
                }
                (op, op_type, name_literal, source_cv)
            })
            .collect()
    }

    fn compile_mixed_unpacked_call_arguments(
        &mut self,
        args: &[CallArg],
        ref_args: u64,
    ) -> (u16, OpType) {
        let arguments = self.alloc_tmp();
        let mut init = Instruction::new(OpCode::InitArray);
        init.result = arguments;
        init.result_type = OpType::Tmp;
        init.extended_value = args.len() as u32;
        if args
            .iter()
            .any(|argument| matches!(argument, CallArg::Named { .. }))
        {
            init._pad |= ARRAY_INIT_HASH_HINT;
        }
        self.instructions.push(init);

        let mut saw_unpack = false;
        for (index, argument) in args.iter().enumerate() {
            match argument {
                CallArg::Unpack(expression) => {
                    saw_unpack = true;
                    let (value, value_type) = self.compile_expr(expression);
                    let mut unpack = Instruction::new(OpCode::AddCallUnpack);
                    unpack.op1 = arguments;
                    unpack.op1_type = OpType::Tmp;
                    unpack.op2 = value;
                    unpack.op2_type = value_type;
                    self.instructions.push(unpack);
                }
                CallArg::Positional(expression) => {
                    // Before the first unpack, a known by-reference parameter
                    // still has a stable public index. Preserve a direct CV so
                    // AddCallArgument can materialize its alias (including a
                    // missing variable as null) instead of snapshotting an
                    // ordinary by-value read. Once an unpack was seen, runtime
                    // arity makes this compile-time mapping ambiguous.
                    let (value, value_type) = if !saw_unpack
                        && index < 64
                        && ref_args & (1u64 << index) != 0
                        && let Expr::Variable { name, .. } = expression
                    {
                        (self.resolve_cv(name), OpType::Cv)
                    } else {
                        self.compile_expr(expression)
                    };
                    let mut add = Instruction::new(OpCode::AddCallArgument);
                    add.op1 = arguments;
                    add.op1_type = OpType::Tmp;
                    add.op2 = value;
                    add.op2_type = value_type;
                    self.instructions.push(add);
                }
                CallArg::Named { name, value } => {
                    let (value, value_type) = self.compile_expr(value);
                    let key = self.add_literal(Value::string(name.clone()));
                    let mut add = Instruction::new(OpCode::AddCallArgument);
                    add.op1 = arguments;
                    add.op1_type = OpType::Tmp;
                    add.op2 = value;
                    add.op2_type = value_type;
                    add.result = key;
                    add.result_type = OpType::Const;
                    self.instructions.push(add);
                }
            }
        }

        (arguments, OpType::Tmp)
    }

    fn compile_method_call_from_operands(
        &mut self,
        obj_op: u16,
        obj_type: OpType,
        tmp: u16,
        nullsafe_patch: Option<usize>,
        method: &str,
        args: &[CallArg],
        compiled_args: &[CompiledCallArg],
        generic_args: &[TypeHint],
        line: usize,
    ) -> (u16, OpType) {
        let method_idx = self.add_literal(Value::string(method.to_string()));
        let runtime_generic_check = self.emit_generic_check(
            OpCode::CheckGenericArgs,
            GenericDeclarationKind::Method,
            generic_args,
            None,
            obj_op,
            obj_type,
            method_idx,
            OpType::Const,
        );
        let mut init = Instruction::new(OpCode::InitMethodCall);
        init.op1 = obj_op;
        init.op1_type = obj_type;
        init.op2 = method_idx;
        init.op2_type = OpType::Const;
        init.extended_value = args.len() as u32;
        self.push_instruction_at_line(init, line);
        self.emit_precompiled_runtime_call_args(args, compiled_args, 1, 0, true, true);
        self.emit_reified_argument_check(runtime_generic_check);

        let mut do_fcall = Instruction::new(OpCode::DoFcall);
        do_fcall.result = tmp;
        do_fcall.result_type = OpType::Tmp;
        self.push_instruction_at_line(do_fcall, line);
        self.emit_temporary_method_receiver_release(obj_op, obj_type, line);
        self.emit_reified_return_check(runtime_generic_check, tmp, OpType::Tmp);
        if let Some(index) = nullsafe_patch {
            self.instructions[index].op2 = self.instructions.len() as u16;
        }
        (tmp, OpType::Tmp)
    }

    /// Zend releases a one-use object receiver as soon as its method call
    /// completes, before a surrounding fluent expression continues. Keep CV
    /// receivers on the existing hot path and retire only compiler temporaries.
    fn emit_temporary_method_receiver_release(
        &mut self,
        receiver: u16,
        receiver_type: OpType,
        line: usize,
    ) {
        if receiver_type != OpType::Tmp {
            return;
        }
        let mut release = Instruction::new(OpCode::ReleaseTemps);
        release.op1 = receiver;
        release.op1_type = OpType::Tmp;
        release.op2 = receiver + 1;
        release.op2_type = OpType::Tmp;
        self.push_instruction_at_line(release, line);
    }

    fn compile_dynamic_call_from_operand(
        &mut self,
        callable: u16,
        callable_type: OpType,
        args: &[CallArg],
        generic_args: &[TypeHint],
        line: usize,
    ) -> (u16, OpType) {
        let (callable, callable_type) = if args.iter().any(CallArg::contains_yield) {
            self.snapshot_yield_rvalue_operand(callable, callable_type, "callable", line)
        } else {
            (callable, callable_type)
        };
        if generic_args.is_empty()
            && args
                .iter()
                .any(|argument| matches!(argument, CallArg::Unpack(_)))
        {
            let (arguments, arguments_type) = self.compile_mixed_unpacked_call_arguments(args, 0);
            let result = self.alloc_tmp();
            let mut call = Instruction::new(OpCode::CallUserFuncArray);
            call.op1 = callable;
            call.op1_type = callable_type;
            call.op2 = arguments;
            call.op2_type = arguments_type;
            call.result = result;
            call.result_type = OpType::Tmp;
            call._pad |= CALL_USER_FUNC_ARRAY_SOURCE_UNPACK;
            self.push_instruction_at_line(call, line);
            return (result, OpType::Tmp);
        }

        let compiled_args = args
            .iter()
            .any(CallArg::contains_yield)
            .then(|| self.compile_call_args(args, 0, true));
        let runtime_generic_check = self.emit_generic_check(
            OpCode::CheckGenericArgs,
            GenericDeclarationKind::Function,
            generic_args,
            None,
            callable,
            callable_type,
            0,
            OpType::Unused,
        );
        let mut init = Instruction::new(OpCode::InitDynamicCall);
        init.op1 = callable;
        init.op1_type = callable_type;
        init.extended_value = args.len() as u32;
        self.push_instruction_at_line(init, line);
        if let Some(compiled_args) = compiled_args.as_deref() {
            self.emit_precompiled_runtime_call_args(args, compiled_args, 0, 0, true, true);
        } else {
            self.emit_call_args(args, 0, 0, true, true);
        }
        self.emit_reified_argument_check(runtime_generic_check);
        let result = self.alloc_tmp();
        let mut do_fcall = Instruction::new(OpCode::DoFcall);
        do_fcall.result = result;
        do_fcall.result_type = OpType::Tmp;
        self.push_instruction_at_line(do_fcall, line);
        self.emit_reified_return_check(runtime_generic_check, result, OpType::Tmp);
        (result, OpType::Tmp)
    }

    /// Emit arguments for compiler-lowered call_user_func. Unlike an ordinary
    /// dynamic call, the callback may resolve to a method, so the VM computes
    /// the hidden `$this` CV offset from the resolved signature.
    fn emit_user_call_args(&mut self, args: &[CallArg]) {
        for (index, arg) in args.iter().enumerate() {
            let CallArg::Positional(expr) = arg else {
                unreachable!("user-call lowering only accepts positional arguments");
            };
            let (op, op_type) = self.compile_expr(expr);
            let mut send = Instruction::new(OpCode::SendUser);
            send.op1 = op;
            send.op1_type = op_type;
            send.op2 = index as u16;
            send.extended_value = index as u32;
            self.instructions.push(send);
        }
    }

    /// Emit Send instructions from pre-compiled argument tuples.
    /// Used by `Expr::New` where side effects must execute before NewObj.
    /// Each tuple carries the operand, optional name literal, and optional
    /// original CV for runtime by-reference selection.
    fn emit_precompiled_call_args(&mut self, compiled_args: &[CompiledCallArg], cv_offset: u32) {
        for (i, (op, op_type, named_idx, source_cv)) in compiled_args.iter().enumerate() {
            if let Some(name_const) = named_idx {
                let mut send = Instruction::new(OpCode::SendNamed);
                send.op1 = *op;
                send.op1_type = *op_type;
                send.op2 = *name_const;
                send.op2_type = OpType::Const;
                send.extended_value = i as u32;
                if let Some(source_cv) = source_cv {
                    send.result = *source_cv;
                    send.result_type = OpType::Unused;
                    send._pad |= SEND_FLAG_YIELD_SNAPSHOT;
                }
                self.instructions.push(send);
            } else if let Some(source_cv) = source_cv {
                let mut send = Instruction::new(OpCode::SendVarEx);
                send.op1 = *op;
                send.op1_type = *op_type;
                send.op2 = (i as u32 + cv_offset) as u16;
                send.extended_value = i as u32;
                send.result = *source_cv;
                send.result_type = OpType::Unused;
                send._pad |= SEND_FLAG_YIELD_SNAPSHOT;
                self.instructions.push(send);
            } else {
                let mut send = Instruction::new(OpCode::SendVal);
                send.op1 = *op;
                send.op1_type = *op_type;
                send.op2 = (i as u32 + cv_offset) as u16;
                self.instructions.push(send);
            }
        }
    }

    fn emit_precompiled_runtime_call_args(
        &mut self,
        args: &[CallArg],
        compiled_args: &[CompiledCallArg],
        cv_offset: u32,
        ref_args: u64,
        use_var_ex: bool,
        set_extended_value: bool,
    ) {
        debug_assert_eq!(args.len(), compiled_args.len());
        for (index, (arg, (op, op_type, name_idx, source_cv))) in
            args.iter().zip(compiled_args).enumerate()
        {
            match arg {
                CallArg::Positional(expr) | CallArg::Unpack(expr) => {
                    let nonreferenceable = Self::nullsafe_chain_line(expr).is_some();
                    let yield_snapshot = use_var_ex && source_cv.is_some();
                    let opcode = if nonreferenceable {
                        OpCode::SendVal
                    } else if yield_snapshot {
                        OpCode::SendVarEx
                    } else {
                        Self::positional_opcode(ref_args, index, *op_type, use_var_ex)
                    };
                    let mut send = Instruction::new(opcode);
                    send.op1 = *op;
                    send.op1_type = *op_type;
                    send.op2 = (index as u32 + cv_offset) as u16;
                    if matches!(expr, Expr::Globals { .. }) {
                        send._pad |= SEND_FLAG_GLOBALS;
                        send.extended_value = index as u32;
                    }
                    if set_extended_value {
                        send.extended_value = index as u32;
                    }
                    if yield_snapshot && let Some(source_cv) = source_cv {
                        send.result = *source_cv;
                        // Metadata only: keeping result_type unused prevents
                        // scalar/dataflow passes from treating the source CV
                        // as a value written by SendVarEx.
                        send.result_type = OpType::Unused;
                        send._pad |= SEND_FLAG_YIELD_SNAPSHOT;
                    }
                    if nonreferenceable {
                        send._pad |= SEND_FLAG_NONREFERENCEABLE;
                        send.extended_value = index as u32;
                    }
                    self.instructions.push(send);
                }
                CallArg::Named { .. } => {
                    let mut send = Instruction::new(OpCode::SendNamed);
                    send.op1 = *op;
                    send.op1_type = *op_type;
                    send.op2 = name_idx.expect("compiled named argument must retain its name");
                    send.op2_type = OpType::Const;
                    send.extended_value = index as u32;
                    if let Some(source_cv) = source_cv {
                        send.result = *source_cv;
                        send.result_type = OpType::Unused;
                        send._pad |= SEND_FLAG_YIELD_SNAPSHOT;
                    }
                    if Self::nullsafe_chain_line(arg.expr()).is_some() {
                        send._pad |= SEND_FLAG_NONREFERENCEABLE;
                    }
                    self.instructions.push(send);
                }
            }
        }
    }

    fn alloc_tmp(&mut self) -> u16 {
        let idx = self.next_tmp;
        self.next_tmp += 1;
        idx as u16
    }

    /// Compile list destructuring targets. Each target gets a FetchDimR + AssignCv.
    fn compile_list_reference_target(
        &mut self,
        target: &Expr,
        array: u16,
        array_type: OpType,
        key: u16,
        key_type: OpType,
        diagnose_nonreferenceable: bool,
    ) -> Result<(), String> {
        let internal_name = format!("\0list_reference_{}", self.next_cv);
        let source = self.resolve_cv(&internal_name);
        let mut bind_source = Instruction::new(OpCode::BindArrayDimRef);
        bind_source.op1 = array;
        bind_source.op1_type = array_type;
        bind_source.op2 = key;
        bind_source.op2_type = key_type;
        bind_source.result = source;
        bind_source.result_type = OpType::Cv;
        bind_source._pad |= REFERENCE_RESULT_INTERNAL;
        if diagnose_nonreferenceable {
            bind_source._pad |= REFERENCE_SOURCE_MAY_BE_NONREFERENCEABLE;
        }
        let line = match target {
            Expr::Variable { line, .. }
            | Expr::DynamicVariable { line, .. }
            | Expr::ArrayAccess { line, .. }
            | Expr::PropertyAccess { line, .. }
            | Expr::DynamicPropertyAccess { line, .. }
            | Expr::CompileError { line, .. } => *line,
            _ => 0,
        };
        self.push_instruction_at_line(bind_source, line);

        if let Expr::Variable { name, .. } = target {
            let destination = self.resolve_cv(name);
            let mut bind = Instruction::new(OpCode::BindCvRef);
            bind.op1 = source;
            bind.op1_type = OpType::Cv;
            bind.result = destination;
            bind.result_type = OpType::Cv;
            self.instructions.push(bind);
            self.definitely_defined_cvs.insert(destination);
        } else {
            let source_expr = Expr::Variable {
                name: internal_name,
                line: 0,
            };
            self.compile_target_reference_assignment(target, &source_expr)?;
        }
        Ok(())
    }

    fn compile_list_targets(
        &mut self,
        targets: &[crate::parser::ListTarget],
        array: u16,
        array_type: OpType,
        start_index: usize,
        source_line: usize,
        diagnose_nonreferenceable: bool,
    ) -> Result<(), String> {
        use crate::parser::ListTarget;
        let mut idx = start_index;
        for target in targets {
            match target {
                ListTarget::Variable(var_name) => {
                    // result = array_tmp[idx]
                    let idx_literal = self.add_literal(Value::long(idx as i64));
                    let fetch_tmp = self.alloc_tmp();
                    let mut fetch = Instruction::new(OpCode::FetchDimR);
                    fetch.op1_type = array_type;
                    fetch.op1 = array;
                    fetch.op2_type = OpType::Const;
                    fetch.op2 = idx_literal;
                    fetch.result_type = OpType::Tmp;
                    fetch.result = fetch_tmp;
                    fetch._pad |= FETCH_DIM_DESTRUCTURE;
                    self.push_instruction_at_line(fetch, source_line);
                    // assign to CV
                    let cv_idx = self.resolve_cv(var_name);
                    let mut assign = Instruction::new(OpCode::AssignCv);
                    assign.op1_type = OpType::Cv;
                    assign.op1 = cv_idx;
                    assign.op2_type = OpType::Tmp;
                    assign.op2 = fetch_tmp;
                    self.instructions.push(assign);
                    self.definitely_defined_cvs.insert(cv_idx);
                    idx += 1;
                }
                ListTarget::Reference(target) => {
                    let key = self.add_literal(Value::long(idx as i64));
                    self.compile_list_reference_target(
                        target,
                        array,
                        array_type,
                        key,
                        OpType::Const,
                        diagnose_nonreferenceable,
                    )?;
                    idx += 1;
                }
                ListTarget::Target(target) => {
                    let idx_literal = self.add_literal(Value::long(idx as i64));
                    let fetch_tmp = self.alloc_tmp();
                    let mut fetch = Instruction::new(OpCode::FetchDimR);
                    fetch.op1_type = array_type;
                    fetch.op1 = array;
                    fetch.op2_type = OpType::Const;
                    fetch.op2 = idx_literal;
                    fetch.result_type = OpType::Tmp;
                    fetch.result = fetch_tmp;
                    fetch._pad |= FETCH_DIM_DESTRUCTURE;
                    self.push_instruction_at_line(fetch, source_line);

                    match target {
                        Expr::CompileError { message, line } => {
                            return Err(self.goto_error(message, *line));
                        }
                        Expr::DynamicVariable { name, line } => {
                            let (key, key_type) = self.compile_expr(name);
                            let mut assign = Instruction::new(OpCode::AssignDynamicVar);
                            assign.op1 = key;
                            assign.op1_type = key_type;
                            assign.op2 = fetch_tmp;
                            assign.op2_type = OpType::Tmp;
                            self.push_instruction_at_line(assign, *line);
                        }
                        Expr::PropertyAccess {
                            object,
                            property,
                            nullsafe: false,
                            ..
                        } => {
                            let (object, object_type) = self.compile_property_modify_base(object);
                            let property = self.add_literal(Value::string(property.clone()));
                            let mut assign = Instruction::new(OpCode::AssignObjProp);
                            assign.op1 = object;
                            assign.op1_type = object_type;
                            assign.op2 = property;
                            assign.op2_type = OpType::Const;
                            assign.result = fetch_tmp;
                            assign.result_type = OpType::Tmp;
                            self.instructions.push(assign);
                        }
                        Expr::DynamicPropertyAccess {
                            object,
                            property,
                            nullsafe: false,
                            ..
                        } => {
                            let (object, object_type) = self.compile_property_modify_base(object);
                            let (property, property_type) = self.compile_expr(property);
                            let mut assign = Instruction::new(OpCode::AssignObjProp);
                            assign.op1 = object;
                            assign.op1_type = object_type;
                            assign.op2 = property;
                            assign.op2_type = property_type;
                            assign.result = fetch_tmp;
                            assign.result_type = OpType::Tmp;
                            self.instructions.push(assign);
                        }
                        static_property @ (Expr::StaticProperty { .. }
                        | Expr::DynamicNamedStaticProperty { .. }
                        | Expr::DynamicStaticProperty { .. }) => {
                            let (
                                class,
                                class_type,
                                property,
                                property_type,
                                late_static,
                                dynamic_owner,
                                line,
                            ) = self
                                .compile_static_property_operands(static_property)
                                .expect("matched static-property form");
                            let mut assign = Instruction::new(if late_static {
                                OpCode::AssignLateStaticProp
                            } else {
                                OpCode::AssignStaticProp
                            });
                            assign.op1 = class;
                            assign.op1_type = class_type;
                            assign.op2 = property;
                            assign.op2_type = property_type;
                            assign.result = fetch_tmp;
                            assign.result_type = OpType::Tmp;
                            if dynamic_owner {
                                assign._pad |= STATIC_PROP_DYNAMIC_OWNER;
                            }
                            if property_type != OpType::Const {
                                assign._pad |= STATIC_PROP_DYNAMIC_NAME;
                            }
                            self.push_instruction_at_line(assign, line);
                        }
                        Expr::ArrayAccess { .. } => {
                            let mut root = target;
                            let mut reversed_indices = Vec::new();
                            while let Expr::ArrayAccess { array, index, .. } = root {
                                reversed_indices.push(index.as_ref().clone());
                                root = array.as_ref();
                            }
                            reversed_indices.reverse();
                            let path = self.compile_mutable_array_path(
                                root,
                                &reversed_indices,
                                false,
                                false,
                            )?;
                            let &(container, container_type) = path.containers.last().unwrap();
                            let &(key, key_type) = path.keys.last().unwrap();
                            let mut assign = Instruction::new(OpCode::AssignDim);
                            assign.op1 = container;
                            assign.op1_type = container_type;
                            assign.op2 = key;
                            assign.op2_type = key_type;
                            assign.result = fetch_tmp;
                            assign.result_type = OpType::Tmp;
                            self.instructions.push(assign);
                            self.rebuild_mutable_array_path(&path);
                            self.write_back_mutable_array_root(&path);
                            if let Expr::Variable { name, .. } = root {
                                let cv = self.resolve_cv(name);
                                self.definitely_defined_cvs.insert(cv);
                            }
                        }
                        _ => return Err("Invalid destructuring assignment target".into()),
                    }
                    idx += 1;
                }
                ListTarget::AppendTarget(target) => {
                    let idx_literal = self.add_literal(Value::long(idx as i64));
                    let fetch_tmp = self.alloc_tmp();
                    let mut fetch = Instruction::new(OpCode::FetchDimR);
                    fetch.op1_type = array_type;
                    fetch.op1 = array;
                    fetch.op2_type = OpType::Const;
                    fetch.op2 = idx_literal;
                    fetch.result_type = OpType::Tmp;
                    fetch.result = fetch_tmp;
                    fetch._pad |= FETCH_DIM_DESTRUCTURE;
                    self.instructions.push(fetch);

                    if let Expr::Variable { name, .. } = target {
                        let cv = self.resolve_cv(name);
                        let mut append = Instruction::new(OpCode::ArrayPushOp);
                        append.op1 = cv;
                        append.op1_type = OpType::Cv;
                        append.op2 = fetch_tmp;
                        append.op2_type = OpType::Tmp;
                        self.instructions.push(append);
                        self.definitely_defined_cvs.insert(cv);
                    } else {
                        let (target, target_type, writeback) =
                            self.compile_foreach_reference_source(target, true, false)?;
                        let mut append = Instruction::new(OpCode::ArrayPushOp);
                        append.op1 = target;
                        append.op1_type = target_type;
                        append.op2 = fetch_tmp;
                        append.op2_type = OpType::Tmp;
                        self.instructions.push(append);
                        self.emit_foreach_reference_source_writeback(
                            writeback,
                            target,
                            target_type,
                        );
                    }
                    idx += 1;
                }
                ListTarget::Skip => {
                    idx += 1;
                }
                ListTarget::Nested(inner_targets) => {
                    // Fetch the sub-array at this index
                    let idx_literal = self.add_literal(Value::long(idx as i64));
                    if inner_targets.iter().any(ListTarget::contains_reference) {
                        let sub_name = format!("\0list_nested_reference_{}", self.next_cv);
                        let sub = self.resolve_cv(&sub_name);
                        let mut bind = Instruction::new(OpCode::BindArrayDimRef);
                        bind.op1_type = array_type;
                        bind.op1 = array;
                        bind.op2_type = OpType::Const;
                        bind.op2 = idx_literal;
                        bind.result_type = OpType::Cv;
                        bind.result = sub;
                        bind._pad |= REFERENCE_RESULT_INTERNAL;
                        self.push_instruction_at_line(bind, source_line);
                        self.compile_list_targets(
                            inner_targets,
                            sub,
                            OpType::Cv,
                            0,
                            source_line,
                            diagnose_nonreferenceable,
                        )?;
                    } else {
                        let sub_tmp = self.alloc_tmp();
                        let mut fetch = Instruction::new(OpCode::FetchDimR);
                        fetch.op1_type = array_type;
                        fetch.op1 = array;
                        fetch.op2_type = OpType::Const;
                        fetch.op2 = idx_literal;
                        fetch.result_type = OpType::Tmp;
                        fetch.result = sub_tmp;
                        fetch._pad |= FETCH_DIM_DESTRUCTURE;
                        self.push_instruction_at_line(fetch, source_line);
                        let nested_start = self.instructions.len();
                        self.compile_list_targets(
                            inner_targets,
                            sub_tmp,
                            OpType::Tmp,
                            0,
                            source_line,
                            diagnose_nonreferenceable,
                        )?;
                        for instruction in &mut self.instructions[nested_start..] {
                            if instruction.opcode == OpCode::FetchDimR {
                                instruction._pad |= FETCH_DIM_SILENT;
                            }
                        }
                    }
                    idx += 1;
                }
                ListTarget::KeyedVariable { key, var } => {
                    // Use explicit key instead of sequential index
                    let (key_op, key_type) = self.compile_expr(key);
                    let fetch_tmp = self.alloc_tmp();
                    let mut fetch = Instruction::new(OpCode::FetchDimR);
                    fetch.op1_type = array_type;
                    fetch.op1 = array;
                    fetch.op2_type = key_type;
                    fetch.op2 = key_op;
                    fetch.result_type = OpType::Tmp;
                    fetch.result = fetch_tmp;
                    fetch._pad |= FETCH_DIM_DESTRUCTURE;
                    self.push_instruction_at_line(fetch, source_line);
                    let cv_idx = self.resolve_cv(var);
                    let mut assign = Instruction::new(OpCode::AssignCv);
                    assign.op1_type = OpType::Cv;
                    assign.op1 = cv_idx;
                    assign.op2_type = OpType::Tmp;
                    assign.op2 = fetch_tmp;
                    self.instructions.push(assign);
                    self.definitely_defined_cvs.insert(cv_idx);
                    // Don't increment idx for keyed — they use explicit keys
                }
                ListTarget::KeyedReference { key, target } => {
                    let (key, key_type) = self.compile_expr(key);
                    self.compile_list_reference_target(
                        target,
                        array,
                        array_type,
                        key,
                        key_type,
                        diagnose_nonreferenceable,
                    )?;
                }
                ListTarget::KeyedNested { key, targets } => {
                    let (key, key_type) = self.compile_expr(key);
                    if targets.iter().any(ListTarget::contains_reference) {
                        let sub_name = format!("\0list_nested_reference_{}", self.next_cv);
                        let sub = self.resolve_cv(&sub_name);
                        let mut bind = Instruction::new(OpCode::BindArrayDimRef);
                        bind.op1_type = array_type;
                        bind.op1 = array;
                        bind.op2_type = key_type;
                        bind.op2 = key;
                        bind.result_type = OpType::Cv;
                        bind.result = sub;
                        bind._pad |= REFERENCE_RESULT_INTERNAL;
                        self.push_instruction_at_line(bind, source_line);
                        self.compile_list_targets(
                            targets,
                            sub,
                            OpType::Cv,
                            0,
                            source_line,
                            diagnose_nonreferenceable,
                        )?;
                    } else {
                        let sub = self.alloc_tmp();
                        let mut fetch = Instruction::new(OpCode::FetchDimR);
                        fetch.op1_type = array_type;
                        fetch.op1 = array;
                        fetch.op2_type = key_type;
                        fetch.op2 = key;
                        fetch.result_type = OpType::Tmp;
                        fetch.result = sub;
                        fetch._pad |= FETCH_DIM_DESTRUCTURE;
                        self.push_instruction_at_line(fetch, source_line);
                        let nested_start = self.instructions.len();
                        self.compile_list_targets(
                            targets,
                            sub,
                            OpType::Tmp,
                            0,
                            source_line,
                            diagnose_nonreferenceable,
                        )?;
                        for instruction in &mut self.instructions[nested_start..] {
                            if instruction.opcode == OpCode::FetchDimR {
                                instruction._pad |= FETCH_DIM_SILENT;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
