/// AST → OpArray compiler.
/// Converts parsed statements into VM instructions.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

/// Global closure counter — ensures unique names across nested compilers.
static CLOSURE_COUNTER: AtomicU32 = AtomicU32::new(0);

use crate::value::{ObjectLayout, Value, ValueType};
use crate::parser::{Stmt, Expr, BinOp, CastType, Visibility, Param, CallArg, ListTarget};
use crate::vm::opcode::OpCode;
use crate::vm::instruction::{
    Instruction, InlineCache, KnownScalarType, OpType, ARRAY_INIT_HASH_HINT,
    CALL_FLAG_DEFERRED_SCALAR_CANDIDATE, CALL_FLAG_EXACT_SCALAR_ARGS,
};
use super::OpArray;

use super::{
    finalize_user_method, make_user_function_full, make_user_function_typed,
    make_user_function_with_args,
};
use crate::vm::function::{CallStrategy, ParamTypeHint, UserFunction};

/// Result of compiling a script — main OpArray + declared functions + class defs.
pub struct CompileResult {
    pub main: OpArray,
    pub functions: Vec<(String, UserFunction)>,
    pub class_defs: Vec<ClassDef>,
}

/// PHP normalizes only canonical decimal string array keys to integers.
fn canonical_string_literal_array_key(value: &str) -> Option<i64> {
    value
        .parse::<i64>()
        .ok()
        .filter(|parsed| parsed.to_string() == value)
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
fn array_literal_storage_hint(
    elements: &[crate::parser::ArrayElement],
) -> ArrayLiteralStorageHint {
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
            Some(Expr::StringLiteral(key)) => {
                canonical_string_literal_array_key(key).unwrap()
            }
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
        ArrayLiteralStorageHint, array_literal_storage_hint,
        canonical_string_literal_array_key,
    };
    use crate::parser::{ArrayElement, Expr};

    fn element(key: Option<Expr>) -> ArrayElement {
        ArrayElement { key, value: Expr::Integer(1) }
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
            array_literal_storage_hint(&[element(Some(Expr::Variable("key".into())))]),
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
        direct_global_access[index] = !op_array.global_vars.is_empty();

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
        function.composed_scalar_long_plan =
            super::build_composed_scalar_long_function_plan(function);
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
            (known != KnownScalarType::Unknown)
                .then_some((name.to_ascii_lowercase(), known))
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

fn known_argument_satisfies_hint(
    known: KnownScalarType,
    hint: &ParamTypeHint,
    strict: bool,
) -> bool {
    match hint {
        ParamTypeHint::None | ParamTypeHint::Mixed => true,
        ParamTypeHint::Int => known == KnownScalarType::Long,
        ParamTypeHint::Float => {
            known == KnownScalarType::Double
                || (!strict && known == KnownScalarType::Long)
        }
        ParamTypeHint::String => known == KnownScalarType::String,
        ParamTypeHint::Bool => known == KnownScalarType::Bool,
        ParamTypeHint::Nullable(inner) => {
            known_argument_satisfies_hint(known, inner, strict)
        }
        ParamTypeHint::Union(types) => types
            .iter()
            .any(|member| known_argument_satisfies_hint(known, member, strict)),
        _ => false,
    }
}

struct PendingScalarCallFacts {
    return_type: KnownScalarType,
    parameter_types: Option<Vec<ParamTypeHint>>,
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
    this_offset: u32,
    param_type_hints: &[ParamTypeHint],
    ref_args: u64,
    return_types: &HashMap<String, KnownScalarType>,
    parameter_types: &HashMap<String, Vec<ParamTypeHint>>,
) {
    let slot_count = (op_array.num_cvs + op_array.num_temps) as usize;
    let mut slots = vec![KnownScalarType::Unknown; slot_count];
    let mut mutable_params = vec![false; param_type_hints.len()];

    for instruction in &op_array.instructions {
        let mut mark_mutable = |slot: u16| {
            let slot = slot as u32;
            if slot >= this_offset && slot < this_offset + param_type_hints.len() as u32 {
                mutable_params[(slot - this_offset) as usize] = true;
            }
        };
        match instruction.opcode {
            OpCode::AssignCv
            | OpCode::AssignConcat
            | OpCode::PreInc
            | OpCode::PreDec
            | OpCode::PostInc
            | OpCode::PostDec
            | OpCode::BindDefaultParam
            | OpCode::BindGlobal
            | OpCode::BindStatic => mark_mutable(instruction.op1),
            OpCode::SendRef | OpCode::SendVarEx if instruction.op1_type == OpType::Cv => {
                mark_mutable(instruction.op1)
            }
            OpCode::ForeachNext => mutable_params.fill(true),
            _ => {}
        }
    }

    for (index, hint) in param_type_hints.iter().enumerate() {
        if !mutable_params[index] && (index >= 64 || ref_args & (1u64 << index) == 0) {
            let cv = this_offset as usize + index;
            if cv < slots.len() {
                slots[cv] = exact_declared_scalar_type(hint);
            }
        }
    }

    let straight_line = !op_array.instructions.iter().any(|instruction| {
        matches!(
            instruction.opcode,
            OpCode::Jmp
                | OpCode::JmpZ
                | OpCode::JmpNZ
                | OpCode::QuickLongLoopJmp
                | OpCode::ForeachInit
                | OpCode::ForeachNext
                | OpCode::BindDefaultParam
        )
    });
    let mut pending_calls = Vec::new();

    for ip in 0..op_array.instructions.len() {
        let instruction = op_array.instructions[ip];

        // A CV exposed by reference can be changed by code outside this body.
        // Forget any straight-line fact before later instructions consume it.
        match instruction.opcode {
            OpCode::SendRef | OpCode::SendVarEx if instruction.op1_type == OpType::Cv => {
                if let Some(slot) = slots.get_mut(instruction.op1 as usize) {
                    *slot = KnownScalarType::Unknown;
                }
            }
            OpCode::BindGlobal | OpCode::BindStatic => {
                if let Some(slot) = slots.get_mut(instruction.op1 as usize) {
                    *slot = KnownScalarType::Unknown;
                }
            }
            OpCode::ForeachNext => slots.fill(KnownScalarType::Unknown),
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
                arguments_proven: true,
            }),
            OpCode::InitMethodCall
            | OpCode::InitStaticCall
            | OpCode::InitDynamicCall
            | OpCode::InitUserCall
            | OpCode::NewObj => pending_calls.push(PendingScalarCallFacts {
                return_type: KnownScalarType::Unknown,
                parameter_types: None,
                arguments_proven: false,
            }),
            _ => {}
        }

        let left = operand_scalar_type(
            op_array,
            &slots,
            instruction.op1_type,
            instruction.op1,
        );
        let right = operand_scalar_type(
            op_array,
            &slots,
            instruction.op2_type,
            instruction.op2,
        );
        if matches!(instruction.opcode, OpCode::SendVal) {
            if let Some(call) = pending_calls.last_mut() {
                call.arguments_proven &= call
                    .parameter_types
                    .as_ref()
                    .and_then(|hints| hints.get(instruction.op2 as usize))
                    .is_some_and(|hint| {
                        known_argument_satisfies_hint(left, hint, op_array.strict_types)
                    });
            }
        } else if matches!(
            instruction.opcode,
            OpCode::SendRef | OpCode::SendVarEx | OpCode::SendNamed | OpCode::SendUser
        ) {
            if let Some(call) = pending_calls.last_mut() {
                call.arguments_proven = false;
            }
        }
        let mut result = KnownScalarType::Unknown;
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
                    exact_call_arguments =
                        call.arguments_proven && call.parameter_types.is_some();
                }
            }
            OpCode::Strlen | OpCode::Strlen_Cv | OpCode::Strlen_String => {
                result = KnownScalarType::Long;
            }
            OpCode::Concat | OpCode::Concat_StringString => {
                result = KnownScalarType::String;
            }
            OpCode::Mod_LongLong | OpCode::BitwiseXor_LongLong => {
                result = KnownScalarType::Long;
            }
            OpCode::BitwiseAnd
            | OpCode::BitwiseOr
            | OpCode::BitwiseXor
            | OpCode::ShiftLeft
            | OpCode::ShiftRight
            | OpCode::BitwiseNot => result = KnownScalarType::Long,
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
                    if let Some(destination) = slots.get_mut(instruction.op1 as usize) {
                        *destination = right;
                    }
                }
                result = right;
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
            && matches!(instruction.result_type, OpType::Cv | OpType::Tmp | OpType::Var)
        {
            if let Some(destination) = slots.get_mut(instruction.result as usize) {
                *destination = result;
            }
        }
    }
}

/// A single catch clause within a try entry
#[derive(Debug, Clone)]
pub struct CatchEntry {
    pub types: Vec<String>,   // catch type names (e.g., ["Exception"], ["Foo", "Bar"] for multi-catch)
    pub catch_start: u32,     // instruction offset of catch body
    pub catch_cv: u32,        // CV index for the exception variable
}

/// Exception handler entry for try/catch
#[derive(Debug, Clone)]
pub struct TryEntry {
    pub try_start: u32,
    pub try_end: u32,
    pub catches: Vec<CatchEntry>,  // ordered list of catch clauses
    pub finally_start: u32,  // 0xFFFFFFFF if no finally
    pub finally_end: u32,    // instruction after finally block
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

/// Compiled class definition
pub struct ClassDef {
    pub name: String,
    pub parent: Option<String>,
    pub implements: Vec<String>,
    pub is_interface: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_trait: bool,
    pub is_enum: bool,
    pub uses: Vec<String>,  // trait names from `use Foo, Bar;`
    pub properties: Vec<(String, Option<Value>, Visibility, String)>,  // (name, default_value, visibility, declaring_class)
    /// Shared declared-property storage-key → numeric slot layout.
    /// Rebuilt after inheritance and trait properties are merged.
    pub property_layout: std::rc::Rc<ObjectLayout>,
    pub readonly_props: Vec<String>,  // names of readonly properties
    pub methods: Vec<(String, Visibility, bool, bool, UserFunction)>, // (name, vis, is_static, is_final, func)
    /// Stable numeric ID assigned at registration time. Used as inline cache key.
    /// 0 = not yet assigned (set by ExecutorGlobals::register_class).
    pub class_id: u32,
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

pub struct Compiler {
    instructions: Vec<Instruction>,
    literals: Vec<Value>,
    /// Variable name → CV index
    cv_table: HashMap<String, u32>,
    next_cv: u32,
    next_tmp: u32,
    /// Collected function declarations
    functions: Vec<(String, UserFunction)>,
    /// Loop context stack for break/continue
    loop_stack: Vec<LoopContext>,
    /// Try/catch entries
    try_entries: Vec<TryEntry>,
    /// Class definitions
    class_defs: Vec<ClassDef>,
    /// Deferred error from compile_expr (which can't return Result)
    deferred_error: Option<String>,
    /// ref_args for functions known from parent scope (inherited by child compilers)
    known_ref_args: HashMap<String, u64>,
    /// Per-file strict_types flag from `declare(strict_types=1);`
    strict_types: bool,
    /// Current namespace (None = global namespace)
    current_namespace: Option<String>,
    /// Use aliases: alias → fully qualified name
    use_map: HashMap<String, String>,
    /// True if this function body contains a yield expression (makes it a generator)
    contains_yield: bool,
    /// CVs bound to global variables
    global_vars: Vec<(u32, String)>,
    /// CVs bound to static variables
    static_vars: Vec<(u32, String)>,
    /// Current function name (for static variable keying)
    current_function_name: String,
    /// Constants known at compile time (from `const FOO = 42;` in the same file).
    /// Used by eval_const_expr to resolve Expr::Constant in property defaults.
    known_constants: HashMap<String, Value>,
}

/// Get ref_args bitmask for built-in stdlib functions.
/// Returns 0 for unknown/non-ref functions.
fn builtin_ref_args(name: &str) -> u64 {
    match name {
        "sort" | "rsort" | "shuffle"
        | "usort" | "asort" | "arsort" | "ksort" | "krsort"
        | "array_walk" => 0b1,                           // arg 0
        "array_push" | "array_unshift" => 0b1,          // arg 0
        "array_pop" | "array_shift" => 0b1,             // arg 0
        "array_splice" => 0b1,                           // arg 0
        "settype" => 0b1,                                // arg 0
        "preg_match" | "preg_match_all" => 0b100,          // arg 2 (&$matches)
        "parse_str" => 0b10,                                // arg 1 (&$result)
        _ => 0,
    }
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            literals: Vec::new(),
            cv_table: HashMap::new(),
            next_cv: 0,
            next_tmp: 0,
            functions: Vec::new(),
            loop_stack: Vec::new(),
            try_entries: Vec::new(),
            class_defs: Vec::new(),
            deferred_error: None,
            known_ref_args: HashMap::new(),
            strict_types: false,
            current_namespace: None,
            use_map: HashMap::new(),
            contains_yield: false,
            global_vars: Vec::new(),
            static_vars: Vec::new(),
            current_function_name: String::new(),
            known_constants: HashMap::new(),
        }
    }

    /// Pre-scan top-level `const` declarations to populate known_constants.
    /// This allows property defaults to reference constants declared later in the file.
    /// Two passes: first collect all simple constants, then re-evaluate those that
    /// reference other constants (handles `const A = 1; const B = A;`).
    fn prescan_constants(&mut self, stmts: &[Stmt]) {
        // Two passes over the full statement tree (including namespace bodies).
        // Pass 1: collect directly evaluable constants
        Self::prescan_constants_pass(stmts, None, &mut self.known_constants);
        // Pass 2: retry with the now-larger table (handles forward refs like const B = A)
        Self::prescan_constants_pass(stmts, None, &mut self.known_constants);
    }

    /// Single pass over statements, recursing into namespace bodies.
    /// `ns` is the current namespace prefix (None = top-level).
    fn prescan_constants_pass(
        stmts: &[Stmt],
        ns: Option<&str>,
        known: &mut HashMap<String, Value>,
    ) {
        for stmt in stmts {
            match stmt {
                Stmt::Const { name, value } => {
                    // Qualify constant name with namespace prefix
                    let fqn = match ns {
                        Some(prefix) => format!("{}\\{}", prefix, name),
                        None => name.clone(),
                    };
                    if !known.contains_key(&fqn) {
                        if let Ok(val) = Self::eval_const_expr_with_constants(value, known) {
                            known.insert(fqn, val);
                        }
                    }
                }
                Stmt::Namespace { name, body } => {
                    Self::prescan_constants_pass(body, Some(name), known);
                }
                _ => {}
            }
        }
    }

    /// Resolve a class/function name against current namespace and use map.
    /// Rules:
    /// - Fully qualified names (starting with \) are used as-is (without leading \)
    /// - Names in the use map are replaced with their fully qualified target
    /// - Unqualified names in a namespace get the namespace prefix
    /// - Names already containing \ (relative qualified) get namespace prefix
    fn resolve_name(&self, name: &str) -> String {
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

    /// Build a snapshot of all currently known function ref_args
    /// (own functions + inherited known_ref_args) to pass to child compilers.
    fn build_known_ref_args(&self) -> HashMap<String, u64> {
        let mut map = self.known_ref_args.clone();
        for (fname, uf) in &self.functions {
            map.insert(fname.clone(), uf.common.sig.ref_args);
        }
        map
    }

    pub fn compile(mut self, stmts: &[Stmt]) -> Result<CompileResult, String> {
        // Pre-scan: collect compile-time constants from the entire file so that
        // property defaults can reference constants declared later (forward refs).
        self.prescan_constants(stmts);

        for stmt in stmts {
            self.compile_stmt(stmt)?;
        }
        // Check for deferred errors from compile_expr
        if let Some(err) = self.deferred_error.take() {
            return Err(err);
        }

        // Implicit return null
        let null_idx = self.add_literal(Value::null());
        let mut ret = Instruction::new(OpCode::Return);
        ret.op1_type = OpType::Const;
        ret.op1 = null_idx;
        self.instructions.push(ret);

        // Main script: collect all CVs for syncing to eg.globals before function calls.
        // These go into main_scope_vars (separate from explicit `global` bindings).
        let mut main_scope_vars: Vec<(u32, String)> = Vec::new();
        for (name, &cv_idx) in &self.cv_table {
            main_scope_vars.push((cv_idx, name.clone()));
        }
        let all_cvs = self.all_cvs();

        let cache = (0..self.instructions.len()).map(|_| InlineCache::empty()).collect();
        refine_function_global_access(&mut self.functions);

        // Consume exact scalar declarations only after call and quick-region
        // plans have been selected. This keeps those structural decisions
        // stable while enriching their canonical fallback bytecode.
        let return_types = declared_function_return_types(&self.functions);
        let parameter_types = declared_function_parameter_types(&self.functions);
        for (_, function) in &mut self.functions {
            let signature = &function.common.sig;
            propagate_declared_scalar_types(
                &mut function.op_array,
                signature.this_offset,
                &signature.param_type_hints,
                signature.ref_args,
                &return_types,
                &parameter_types,
            );
        }
        for class in &mut self.class_defs {
            for (_, _, _, _, method) in &mut class.methods {
                let signature = &method.common.sig;
                propagate_declared_scalar_types(
                    &mut method.op_array,
                    signature.this_offset,
                    &signature.param_type_hints,
                    signature.ref_args,
                    &return_types,
                    &parameter_types,
                );
            }
        }

        Ok(CompileResult {
            main: OpArray {
                num_cvs: self.next_cv,
                num_temps: self.next_tmp,
                instructions: self.instructions,
                literals: self.literals,
                try_entries: self.try_entries,
                strict_types: self.strict_types,
                is_generator: false,
                global_vars: self.global_vars,
                static_vars: self.static_vars,
                name: "<main>".to_string(),
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
            class_defs: self.class_defs,
        })
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        // Check for deferred errors from compile_expr (e.g. closure body errors)
        if let Some(err) = self.deferred_error.take() {
            return Err(err);
        }
        match stmt {
            Stmt::Echo(expr) => {
                let (operand, op_type) = self.compile_expr(expr);
                let mut echo = Instruction::new(OpCode::Echo);
                echo.op1 = operand;
                echo.op1_type = op_type;
                self.instructions.push(echo);
            }
            Stmt::Assign { var, expr } => {
                // Detect $x .= expr pattern → emit AssignConcat (in-place string append)
                if let Expr::BinaryOp { op: crate::parser::BinOp::Concat, left, right } = expr {
                    if let Expr::Variable(ref lhs_var) = **left {
                        if lhs_var == var {
                            let (rhs_op, rhs_type) = self.compile_expr(right);
                            let cv_idx = self.resolve_cv(var);
                            let mut instr = Instruction::new(OpCode::AssignConcat);
                            instr.op1_type = OpType::Cv;
                            instr.op1 = cv_idx;
                            instr.op2_type = rhs_type;
                            instr.op2 = rhs_op;
                            self.instructions.push(instr);
                            // Early return from this match arm
                        } else {
                            let (operand, op_type) = self.compile_expr(expr);
                            let cv_idx = self.resolve_cv(var);
                            let mut assign = Instruction::new(OpCode::AssignCv);
                            assign.op1_type = OpType::Cv;
                            assign.op1 = cv_idx;
                            assign.op2_type = op_type;
                            assign.op2 = operand;
                            self.instructions.push(assign);
                        }
                    } else {
                        let (operand, op_type) = self.compile_expr(expr);
                        let cv_idx = self.resolve_cv(var);
                        let mut assign = Instruction::new(OpCode::AssignCv);
                        assign.op1_type = OpType::Cv;
                        assign.op1 = cv_idx;
                        assign.op2_type = op_type;
                        assign.op2 = operand;
                        self.instructions.push(assign);
                    }
                } else {
                    let (operand, op_type) = self.compile_expr(expr);
                    let cv_idx = self.resolve_cv(var);
                    let mut assign = Instruction::new(OpCode::AssignCv);
                    assign.op1_type = OpType::Cv;
                    assign.op1 = cv_idx;
                    assign.op2_type = op_type;
                    assign.op2 = operand;
                    self.instructions.push(assign);
                }
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                // Compile condition
                let (cond_op, cond_type) = self.compile_expr(condition);

                // JmpZ condition, <then_end>
                let jmpz_idx = self.instructions.len();
                let mut jmpz = Instruction::new(OpCode::JmpZ);
                jmpz.op1 = cond_op;
                jmpz.op1_type = cond_type;
                jmpz.op2 = 0; // placeholder, will be patched
                self.instructions.push(jmpz);

                // Compile then body
                for s in then_body {
                    self.compile_stmt(s)?;
                }

                if else_body.is_empty() {
                    // Patch JmpZ to jump past then body
                    let after_then = self.instructions.len() as u16;
                    self.instructions[jmpz_idx].op2 = after_then;
                } else {
                    // Jmp <after_else> (skip else body when then completes)
                    let jmp_idx = self.instructions.len();
                    let mut jmp = Instruction::new(OpCode::Jmp);
                    jmp.op1 = 0; // placeholder
                    self.instructions.push(jmp);

                    // Patch JmpZ to jump to else body
                    let else_start = self.instructions.len() as u16;
                    self.instructions[jmpz_idx].op2 = else_start;

                    // Compile else body
                    for s in else_body {
                        self.compile_stmt(s)?;
                    }

                    // Patch Jmp to jump past else body
                    let after_else = self.instructions.len() as u16;
                    self.instructions[jmp_idx].op1 = after_else;
                }
            }
            Stmt::Function { name, params, body, return_type } => {
                // Compile function body into a separate OpArray
                let mut func_compiler = Compiler::new();
                func_compiler.known_ref_args = self.build_known_ref_args();
                func_compiler.current_function_name = self.resolve_name(name);
                let mut cp = self.compile_params(&mut func_compiler, params, name)?;
                cp.return_type_hint = self.convert_type_hint(return_type);
                for s in body {
                    func_compiler.compile_stmt(s)?;
                }
                let null_idx = func_compiler.add_literal(Value::null());
                let mut ret = Instruction::new(OpCode::Return);
                ret.op1_type = OpType::Const;
                ret.op1 = null_idx;
                func_compiler.instructions.push(ret);

                let func_name = func_compiler.current_function_name.clone();
                let func_all_cvs = func_compiler.all_cvs();
                let cache = (0..func_compiler.instructions.len()).map(|_| InlineCache::empty()).collect();
                let may_access_globals = !func_compiler.global_vars.is_empty()
                    || func_compiler.instructions.iter().any(|i| matches!(i.opcode,
                        OpCode::InitFcall | OpCode::InitDynamicCall | OpCode::InitUserCall | OpCode::CallUserFuncArray
                        | OpCode::InitMethodCall | OpCode::InitStaticCall
                        | OpCode::Include));
                let op_array = OpArray {
                    num_cvs: func_compiler.next_cv,
                    num_temps: func_compiler.next_tmp,
                    instructions: func_compiler.instructions,
                    literals: func_compiler.literals,
                    try_entries: func_compiler.try_entries,
                    strict_types: self.strict_types,
                    is_generator: func_compiler.contains_yield,
                    global_vars: func_compiler.global_vars,
                    static_vars: func_compiler.static_vars,
                    name: func_name,
                    main_scope_vars: vec![],
                    all_cvs: func_all_cvs,
                    cache,
                    may_access_globals,
                    block_info: Vec::new(),
                    block_counters: Vec::new(),
                    block_plans: Vec::new(),
                    ip_to_block: Vec::new(),
                };
                let user_func = make_user_function_typed(op_array, cp.num_args, cp.required_num_args, cp.is_variadic, cp.variadic_cv_index, cp.ref_args, cp.type_hints, cp.param_names, cp.return_type_hint);

                // Collect any nested function declarations
                self.functions.extend(func_compiler.functions);
                let resolved_name = self.resolve_name(name);
                self.functions.push((resolved_name, user_func));
            }
            Stmt::Return(expr) => {
                let (op, op_type, has_explicit_value) = if let Some(e) = expr {
                    let (o, t) = self.compile_expr(e);
                    (o, t, true)
                } else {
                    let idx = self.add_literal(Value::null());
                    (idx, OpType::Const, false)
                };
                let mut ret = Instruction::new(OpCode::Return);
                ret.op1 = op;
                ret.op1_type = op_type;
                // extended_value=1 means explicit "return expr;", 0 means bare "return;"
                ret.extended_value = if has_explicit_value { 1 } else { 0 };
                self.instructions.push(ret);
            }
            Stmt::ExprStmt(expr) => {
                // Compile expression for side effects (e.g. function call), discard result
                let (result, result_type) = self.compile_expr(expr);
                self.discard_unused_expr_result(result, result_type);
            }
            Stmt::While { condition, body } => {
                // Loop start: compile condition
                let loop_start = self.instructions.len();
                let (cond_op, cond_type) = self.compile_expr(condition);

                // JmpZ condition, <after_loop>
                let jmpz_idx = self.instructions.len();
                let mut jmpz = Instruction::new(OpCode::JmpZ);
                jmpz.op1 = cond_op;
                jmpz.op1_type = cond_type;
                jmpz.op2 = 0; // placeholder
                self.instructions.push(jmpz);

                // Push loop context — continue jumps to loop_start (re-test condition)
                self.loop_stack.push(LoopContext {
                    continue_target: Some(loop_start),
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                    is_switch: false,
                });

                // Compile body
                for s in body {
                    self.compile_stmt(s)?;
                }

                // Jmp back to loop start
                let mut jmp_back = Instruction::new(OpCode::Jmp);
                jmp_back.op1 = loop_start as u16;
                self.instructions.push(jmp_back);

                // Patch JmpZ, break and continue jumps
                let after_loop = self.instructions.len() as u16;
                self.instructions[jmpz_idx].op2 = after_loop;
                let ctx = self.loop_stack.pop().unwrap();
                for patch_idx in ctx.break_patches {
                    self.instructions[patch_idx].op1 = after_loop;
                }
                // continue_patches already resolved (target was known at compile time)
            }
            Stmt::DoWhile { condition, body } => {
                let loop_start = self.instructions.len();

                // Push loop context — continue target not yet known
                self.loop_stack.push(LoopContext {
                    continue_target: None,
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                    is_switch: false,
                });

                // Compile body
                for s in body {
                    self.compile_stmt(s)?;
                }

                // continue target = condition check position
                let cond_pos = self.instructions.len();
                if let Some(ctx) = self.loop_stack.last_mut() {
                    ctx.continue_target = Some(cond_pos);
                }

                // Compile condition, JmpNZ back to loop start
                let (cond_op, cond_type) = self.compile_expr(condition);
                let mut jmpnz = Instruction::new(OpCode::JmpNZ);
                jmpnz.op1 = cond_op;
                jmpnz.op1_type = cond_type;
                jmpnz.op2 = loop_start as u16;
                self.instructions.push(jmpnz);

                // Patch break and continue jumps
                let after_loop = self.instructions.len() as u16;
                let ctx = self.loop_stack.pop().unwrap();
                for patch_idx in ctx.break_patches {
                    self.instructions[patch_idx].op1 = after_loop;
                }
                for patch_idx in ctx.continue_patches {
                    self.instructions[patch_idx].op1 = cond_pos as u16;
                }
            }
            Stmt::For { init, condition, update, body } => {
                // Compile init statements
                for s in init {
                    self.compile_stmt(s)?;
                }

                // Loop start: compile condition (or always true)
                let loop_start = self.instructions.len();

                let jmpz_idx = if let Some(cond) = condition {
                    let (cond_op, cond_type) = self.compile_expr(cond);
                    let idx = self.instructions.len();
                    let mut jmpz = Instruction::new(OpCode::JmpZ);
                    jmpz.op1 = cond_op;
                    jmpz.op1_type = cond_type;
                    jmpz.op2 = 0; // placeholder
                    self.instructions.push(jmpz);
                    Some(idx)
                } else {
                    None
                };

                // Push loop context — continue target not yet known
                self.loop_stack.push(LoopContext {
                    continue_target: None,
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                    is_switch: false,
                });

                // Compile body
                for s in body {
                    self.compile_stmt(s)?;
                }

                // Continue target = update expression position
                let update_pos = self.instructions.len();
                if let Some(ctx) = self.loop_stack.last_mut() {
                    ctx.continue_target = Some(update_pos);
                }

                // Compile update expression (discard result)
                if let Some(upd) = update {
                    let (result, result_type) = self.compile_expr(upd);
                    self.discard_unused_expr_result(result, result_type);
                }

                // Jmp back to loop start
                let mut jmp_back = Instruction::new(OpCode::Jmp);
                jmp_back.op1 = loop_start as u16;
                self.instructions.push(jmp_back);

                // Patch JmpZ, break and continue jumps
                let after_loop = self.instructions.len() as u16;
                if let Some(idx) = jmpz_idx {
                    self.instructions[idx].op2 = after_loop;
                }
                let ctx = self.loop_stack.pop().unwrap();
                for patch_idx in ctx.break_patches {
                    self.instructions[patch_idx].op1 = after_loop;
                }
                for patch_idx in ctx.continue_patches {
                    self.instructions[patch_idx].op1 = update_pos as u16;
                }
            }
            Stmt::Break(level) => {
                let depth = level.unwrap_or(1) as usize;
                if depth == 0 || depth > self.loop_stack.len() {
                    return Err(format!(
                        "'break {}' is not in a deep enough nesting level",
                        depth
                    ));
                }
                let target_idx = self.loop_stack.len() - depth;
                let jmp_idx = self.instructions.len();
                let mut jmp = Instruction::new(OpCode::Jmp);
                jmp.op1 = 0; // placeholder — patched when loop ends
                self.instructions.push(jmp);
                self.loop_stack[target_idx].break_patches.push(jmp_idx);
            }
            Stmt::Continue(level) => {
                let depth = level.unwrap_or(1) as usize;
                if depth == 0 || depth > self.loop_stack.len() {
                    return Err(format!(
                        "'continue {}' is not in a deep enough nesting level",
                        depth
                    ));
                }
                let target_idx = self.loop_stack.len() - depth;
                let ctx = &mut self.loop_stack[target_idx];
                if ctx.is_switch {
                    // PHP: "continue" targeting switch is equivalent to "break"
                    let jmp_idx = self.instructions.len();
                    let mut jmp = Instruction::new(OpCode::Jmp);
                    jmp.op1 = 0; // placeholder — patched as break
                    self.instructions.push(jmp);
                    ctx.break_patches.push(jmp_idx);
                } else {
                    let jmp_idx = self.instructions.len();
                    let mut jmp = Instruction::new(OpCode::Jmp);
                    if let Some(target) = ctx.continue_target {
                        jmp.op1 = target as u16;
                    } else {
                        jmp.op1 = 0; // placeholder — patched when target is known
                        ctx.continue_patches.push(jmp_idx);
                    }
                    self.instructions.push(jmp);
                }
            }
            Stmt::Switch { expr, cases } => {
                // Compile the switch expression into a TMP
                let (expr_op, expr_type) = self.compile_expr(expr);
                let switch_tmp = self.alloc_tmp();
                let mut assign = Instruction::new(OpCode::AssignCv);
                assign.op1_type = OpType::Tmp;
                assign.op1 = switch_tmp;
                assign.op2_type = expr_type;
                assign.op2 = expr_op;
                self.instructions.push(assign);

                // Push switch context — break works, continue acts as break
                self.loop_stack.push(LoopContext {
                    continue_target: None,
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                    is_switch: true,
                });

                // Phase 1: emit comparison chain for ALL cases (skip default)
                // For each case value: compare switch_tmp == value, JmpZ → next, Jmp → body
                let mut case_body_patches: Vec<usize> = Vec::new(); // Jmp instructions to body start

                for case in cases.iter() {
                    if let Some(value) = &case.value {
                        // Compare: switch_tmp == case_value
                        let (val_op, val_type) = self.compile_expr(value);
                        let cmp_tmp = self.alloc_tmp();
                        let mut cmp = Instruction::new(OpCode::IsEqual);
                        cmp.op1 = switch_tmp;
                        cmp.op1_type = OpType::Tmp;
                        cmp.op2 = val_op;
                        cmp.op2_type = val_type;
                        cmp.result = cmp_tmp;
                        cmp.result_type = OpType::Tmp;
                        self.instructions.push(cmp);

                        // JmpZ → next case check
                        let jmpz_idx = self.instructions.len();
                        let mut jmpz = Instruction::new(OpCode::JmpZ);
                        jmpz.op1 = cmp_tmp;
                        jmpz.op1_type = OpType::Tmp;
                        jmpz.op2 = 0; // placeholder
                        self.instructions.push(jmpz);

                        // Jmp → this case's body
                        let jmp_idx = self.instructions.len();
                        let mut jmp = Instruction::new(OpCode::Jmp);
                        jmp.op1 = 0; // placeholder → body
                        self.instructions.push(jmp);
                        case_body_patches.push(jmp_idx);

                        // Patch JmpZ to next comparison (which is the next instruction)
                        let next = self.instructions.len() as u16;
                        self.instructions[jmpz_idx].op2 = next;
                    }
                    // default is skipped here — handled after all comparisons
                }

                // After all case comparisons: emit Jmp to default body or past all bodies
                let default_jmp_idx = {
                    let jmp_idx = self.instructions.len();
                    let mut jmp = Instruction::new(OpCode::Jmp);
                    jmp.op1 = 0; // placeholder → default body or after switch
                    self.instructions.push(jmp);
                    jmp_idx
                };

                // Phase 2: emit case bodies with fall-through
                let mut body_idx = 0;
                let mut default_body_start: Option<u16> = None;
                for case in cases.iter() {
                    let body_start = self.instructions.len() as u16;
                    if case.value.is_some() {
                        // Patch the Jmp from phase 1 to point here
                        self.instructions[case_body_patches[body_idx]].op1 = body_start;
                        body_idx += 1;
                    } else {
                        default_body_start = Some(body_start);
                    }
                    // Compile body statements (fall-through — no automatic break)
                    for s in &case.body {
                        self.compile_stmt(s)?;
                    }
                }

                let after_switch = self.instructions.len() as u16;

                // Patch the default/end jump
                if let Some(def_start) = default_body_start {
                    self.instructions[default_jmp_idx].op1 = def_start;
                } else {
                    self.instructions[default_jmp_idx].op1 = after_switch;
                }

                // Patch break jumps
                let ctx = self.loop_stack.pop().unwrap();
                for patch_idx in ctx.break_patches {
                    self.instructions[patch_idx].op1 = after_switch;
                }
            }
            Stmt::ArrayAssign { var, index, expr } => {
                // $var[index] = expr
                let cv_idx = self.resolve_cv(var);
                let (idx_op, idx_type) = self.compile_expr(index);
                let (val_op, val_type) = self.compile_expr(expr);
                let mut instr = Instruction::new(OpCode::AssignDim);
                instr.op1_type = OpType::Cv;
                instr.op1 = cv_idx;
                instr.op2_type = idx_type;
                instr.op2 = idx_op;
                instr.result_type = val_type;
                instr.result = val_op;
                self.instructions.push(instr);
            }
            Stmt::ArrayPush { var, expr } => {
                // $var[] = expr
                let cv_idx = self.resolve_cv(var);
                let (val_op, val_type) = self.compile_expr(expr);
                let mut instr = Instruction::new(OpCode::ArrayPushOp);
                instr.op1_type = OpType::Cv;
                instr.op1 = cv_idx;
                instr.op2_type = val_type;
                instr.op2 = val_op;
                self.instructions.push(instr);
            }
            Stmt::Foreach { array, value_var, key_var, body } => {
                // Compile array expression
                let (arr_op, arr_type) = self.compile_expr(array);

                // ForeachInit: copy array to TMP, position counter TMP
                let arr_copy_tmp = self.alloc_tmp();
                let pos_tmp = self.alloc_tmp();
                let foreach_init_idx = self.instructions.len();
                let mut init = Instruction::new(OpCode::ForeachInit);
                init.op1_type = arr_type;
                init.op1 = arr_op;
                init.result_type = OpType::Tmp;
                init.result = arr_copy_tmp;
                init.extended_value = pos_tmp as u32;
                init.op2 = 0; // placeholder: jump target if empty
                self.instructions.push(init);

                // Loop start: ForeachNext fetches key/value, jumps if done
                let loop_start = self.instructions.len();
                let val_cv = self.resolve_cv(value_var);
                let key_cv = key_var.as_ref().map(|k| self.resolve_cv(k));

                let done_tmp = self.alloc_tmp();
                let mut next = Instruction::new(OpCode::ForeachNext);
                next.op1_type = OpType::Tmp;
                next.op1 = arr_copy_tmp;       // array copy
                next.op2_type = OpType::Tmp;
                next.op2 = pos_tmp;             // position counter
                next.result_type = OpType::Tmp;
                next.result = done_tmp;         // 0 if done, 1 if has entry
                // Encode value_cv and key_cv in extended_value
                // Low 16 bits = value_cv, high 16 bits = key_cv + 1 (0 = no key)
                let key_encoded: u32 = match key_cv {
                    Some(k) => ((k as u32) + 1) << 16,
                    None => 0,
                };
                next.extended_value = key_encoded | (val_cv as u32);
                self.instructions.push(next);

                // JmpZ done_tmp → after_loop
                let jmpz_idx = self.instructions.len();
                let mut jmpz = Instruction::new(OpCode::JmpZ);
                jmpz.op1 = done_tmp;
                jmpz.op1_type = OpType::Tmp;
                jmpz.op2 = 0; // placeholder: after loop
                self.instructions.push(jmpz);

                // Push loop context — continue jumps to loop_start (ForeachNext)
                self.loop_stack.push(LoopContext {
                    continue_target: Some(loop_start),
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                    is_switch: false,
                });

                // Compile body
                for s in body {
                    self.compile_stmt(s)?;
                }

                // Jmp back to loop start (ForeachNext)
                let mut jmp_back = Instruction::new(OpCode::Jmp);
                jmp_back.op1 = loop_start as u16;
                self.instructions.push(jmp_back);

                // Patch jumps
                let after_loop = self.instructions.len() as u16;
                self.instructions[foreach_init_idx].op2 = after_loop; // empty array jump
                self.instructions[jmpz_idx].op2 = after_loop;
                let ctx = self.loop_stack.pop().unwrap();
                for patch_idx in ctx.break_patches {
                    self.instructions[patch_idx].op1 = after_loop;
                }
                // continue_patches already resolved (target was known)
            }
            Stmt::Unset(targets) => {
                for target in targets {
                    match target {
                        Expr::Variable(name) => {
                            let cv_idx = self.resolve_cv(name);
                            let undef_idx = self.add_literal(Value::undef());
                            let mut assign = Instruction::new(OpCode::AssignCv);
                            assign.op1_type = OpType::Cv;
                            assign.op1 = cv_idx;
                            assign.op2_type = OpType::Const;
                            assign.op2 = undef_idx;
                            self.instructions.push(assign);
                        }
                        Expr::ArrayAccess { array, index } => {
                            if let Expr::Variable(name) = array.as_ref() {
                                let cv_idx = self.resolve_cv(name);
                                let (idx_op, idx_type) = self.compile_expr(index);
                                let mut instr = Instruction::new(OpCode::UnsetDim);
                                instr.op1_type = OpType::Cv;
                                instr.op1 = cv_idx;
                                instr.op2_type = idx_type;
                                instr.op2 = idx_op;
                                self.instructions.push(instr);
                            } else {
                                return Err("unset() only supports simple variable array access".into());
                            }
                        }
                        _ => return Err("unset() requires a variable".into()),
                    }
                }
            }
            Stmt::TryCatch { try_body, catches, finally_body } => {
                // Simple implementation: compile try body, if throw happens, jump to catch
                // For now: mark try region start/end for runtime, emit catch handlers
                // We use a "try table" approach: store try/catch info as metadata

                // Record try start
                let try_start = self.instructions.len();

                // Compile try body
                for s in try_body {
                    self.compile_stmt(s)?;
                }

                // Jmp past all catch/finally blocks (no exception)
                let jmp_past_catch = self.instructions.len();
                let mut jmp = Instruction::new(OpCode::Jmp);
                jmp.op1 = 0; // placeholder
                self.instructions.push(jmp);

                let try_end = self.instructions.len();

                // For each catch clause: compile body and record catch metadata
                let mut catch_entries = Vec::new();
                let mut catch_end_jumps = Vec::new();
                for catch in catches {
                    let catch_start = self.instructions.len() as u32;
                    let catch_cv = self.resolve_cv(&catch.var) as u32;

                    let resolved_types: Vec<String> = catch.types.iter().map(|t| self.resolve_name(t)).collect();
                    catch_entries.push(CatchEntry {
                        types: resolved_types,
                        catch_start,
                        catch_cv,
                    });

                    for s in &catch.body {
                        self.compile_stmt(s)?;
                    }
                    // Jmp past remaining catches and finally
                    let jmp_idx = self.instructions.len();
                    let mut jmp_end = Instruction::new(OpCode::Jmp);
                    jmp_end.op1 = 0; // placeholder
                    self.instructions.push(jmp_end);
                    catch_end_jumps.push(jmp_idx);
                }

                // Finally block (if any)
                let finally_start = if let Some(body) = finally_body {
                    let start = self.instructions.len();
                    for s in body {
                        self.compile_stmt(s)?;
                    }
                    Some(start)
                } else {
                    None
                };

                let after_all = self.instructions.len();

                // Patch all jumps to after_all
                self.instructions[jmp_past_catch].op1 = if let Some(fs) = finally_start {
                    fs as u16
                } else {
                    after_all as u16
                };

                // Patch catch-end jumps
                for jmp_idx in &catch_end_jumps {
                    self.instructions[*jmp_idx].op1 = if let Some(fs) = finally_start {
                        fs as u16
                    } else {
                        after_all as u16
                    };
                }

                // Build TryEntry with catch entries and finally info
                let (entry_finally_start, entry_finally_end) = if let Some(fs) = finally_start {
                    (fs as u32, after_all as u32)
                } else {
                    (0xFFFFFFFF, 0)
                };
                self.try_entries.push(TryEntry {
                    try_start: try_start as u32,
                    try_end: try_end as u32,
                    catches: catch_entries,
                    finally_start: entry_finally_start,
                    finally_end: entry_finally_end,
                });
            }
            Stmt::Throw(expr) => {
                let (op, op_type) = self.compile_expr(expr);
                let mut instr = Instruction::new(OpCode::Throw);
                instr.op1 = op;
                instr.op1_type = op_type;
                self.instructions.push(instr);
            }
            Stmt::AssignProp { object, property, expr } => {
                let (obj_op, obj_type) = self.compile_expr(object);
                let (val_op, val_type) = self.compile_expr(expr);
                let prop_idx = self.add_literal(Value::string(property.clone()));

                let mut assign = Instruction::new(OpCode::AssignObjProp);
                assign.op1 = obj_op;
                assign.op1_type = obj_type;
                assign.op2 = prop_idx;
                assign.op2_type = OpType::Const;
                assign.result = val_op;
                assign.result_type = val_type;
                self.instructions.push(assign);
            }
            Stmt::AssignObjArrayDim { object, property, index, expr } => {
                let (obj_op, obj_type) = self.compile_expr(object);
                let (idx_op, idx_type) = self.compile_expr(index);
                let (val_op, val_type) = self.compile_expr(expr);
                let prop_idx = self.add_literal(Value::string(property.clone()));

                let mut instr = Instruction::new(OpCode::AssignObjDim);
                instr.op1 = obj_op;
                instr.op1_type = obj_type;
                instr.op2 = idx_op;
                instr.op2_type = idx_type;
                instr.result = val_op;
                instr.result_type = val_type;
                instr.extended_value = prop_idx as u32;
                self.instructions.push(instr);
            }
            Stmt::Include { path, is_require, is_once } => {
                let (path_op, path_type) = self.compile_expr(path);
                let mut instr = Instruction::new(OpCode::Include);
                instr.op1 = path_op;
                instr.op1_type = path_type;
                let mut flags: u32 = 0;
                if *is_require { flags |= 1; }
                if *is_once { flags |= 2; }
                instr.extended_value = flags;
                self.instructions.push(instr);
            }
            Stmt::Declare { directive, value } => {
                match directive.as_str() {
                    "strict_types" => {
                        self.strict_types = *value != 0;
                    }
                    _ => {
                        // Ignore unknown directives (encoding, ticks)
                    }
                }
            }
            Stmt::Namespace { name, body } => {
                let prev_ns = self.current_namespace.clone();
                let prev_use_map = self.use_map.clone();
                self.current_namespace = Some(name.clone());
                self.use_map.clear();
                for stmt in body {
                    self.compile_stmt(stmt)?;
                }
                self.current_namespace = prev_ns;
                self.use_map = prev_use_map;
            }
            Stmt::UseDecl { imports } => {
                for (fqn, alias) in imports {
                    self.use_map.insert(alias.clone(), fqn.clone());
                }
            }
            Stmt::Const { name, value } => {
                // Compile the value expression and emit FetchConst to define it
                // For const, we evaluate at compile time if possible, otherwise at runtime
                // Also record known compile-time constants for property default resolution.
                if let Ok(ct_val) = Self::eval_const_expr_with_constants(value, &self.known_constants) {
                    self.known_constants.insert(name.clone(), ct_val);
                }
                let (val_op, val_type) = self.compile_expr(value);
                let name_idx = self.add_literal(Value::string(name.clone()));
                let mut instr = Instruction::new(OpCode::FetchConst);
                instr.op1 = name_idx;
                instr.op1_type = OpType::Const;
                instr.op2 = val_op;
                instr.op2_type = val_type;
                // extended_value = 1 means "define mode" (store constant)
                instr.extended_value = 1;
                self.instructions.push(instr);
            }
            Stmt::ListAssign { targets, expr } => {
                // Compile the RHS expression
                let (rhs_op, rhs_type) = self.compile_expr(expr);
                // Store the RHS into a temp so we can index into it multiple times
                let rhs_tmp = self.alloc_tmp();
                let mut assign = Instruction::new(OpCode::AssignCv);
                assign.op1_type = OpType::Tmp;
                assign.op1 = rhs_tmp;
                assign.op2_type = rhs_type;
                assign.op2 = rhs_op;
                self.instructions.push(assign);
                // For each target, emit FetchDimR + AssignCv
                self.compile_list_targets(targets, rhs_tmp, 0)?;
            }
            Stmt::Global(vars) => {
                for var_name in vars {
                    let cv_idx = self.resolve_cv(var_name);
                    let name_idx = self.add_literal(Value::string(var_name.clone()));
                    let mut instr = Instruction::new(OpCode::BindGlobal);
                    instr.op1_type = OpType::Cv;
                    instr.op1 = cv_idx;
                    instr.op2_type = OpType::Const;
                    instr.op2 = name_idx;
                    self.instructions.push(instr);
                    self.global_vars.push((cv_idx as u32, var_name.clone()));
                }
            }
            Stmt::StaticVar { vars } => {
                for (var_name, default) in vars {
                    let cv_idx = self.resolve_cv(var_name);
                    let name_idx = self.add_literal(Value::string(var_name.clone()));
                    let func_name_idx = self.add_literal(Value::string(self.current_function_name.clone()));
                    // If there's a default, compile it and store as extended_value
                    // We encode: op1=CV, op2=CONST(var_name), extended_value=CONST(func_name)
                    // result = default value (or Unused)
                    let mut instr = Instruction::new(OpCode::BindStatic);
                    instr.op1_type = OpType::Cv;
                    instr.op1 = cv_idx;
                    instr.op2_type = OpType::Const;
                    instr.op2 = name_idx;
                    instr.extended_value = func_name_idx as u32;
                    if let Some(def_expr) = default {
                        let (def_op, def_type) = self.compile_expr(def_expr);
                        instr.result_type = def_type;
                        instr.result = def_op;
                    } else {
                        instr.result_type = OpType::Unused;
                    }
                    self.instructions.push(instr);
                    self.static_vars.push((cv_idx as u32, var_name.clone()));
                }
            }
            Stmt::Class { name, parent, implements, is_abstract, is_final, uses, properties, methods } => {
                // Compile class declaration — store class info as a literal
                // Each class method gets compiled like a function
                let mut compiled_methods = Vec::new();
                // Collect promoted properties from constructor
                let mut promoted_props: Vec<(String, Visibility, bool)> = Vec::new(); // (name, vis, is_readonly)
                for method in methods {
                    let mut func_compiler = Compiler::new();
                    func_compiler.known_ref_args = self.build_known_ref_args();
                    // $this is always CV 0 in methods
                    func_compiler.resolve_cv("this");
                    let context = format!("method {}::{}", name, method.name);
                    let mut cp = self.compile_params(&mut func_compiler, &method.params, &context)?;
                    cp.return_type_hint = self.convert_type_hint(&method.return_type);

                    // Constructor property promotion: generate $this->param = $param assignments
                    if method.name == "__construct" {
                        for param in &method.params {
                            if let Some((vis, is_ro)) = &param.promotion {
                                promoted_props.push((param.name.clone(), *vis, *is_ro));
                                // Generate: $this->paramName = $paramName;
                                let this_cv = 0u16; // $this is always CV 0
                                let param_cv = func_compiler.resolve_cv(&param.name);
                                let prop_name_idx = func_compiler.add_literal(Value::string(param.name.clone()));
                                let mut assign = Instruction::new(OpCode::AssignObjProp);
                                assign.op1_type = OpType::Cv;
                                assign.op1 = this_cv;
                                assign.op2_type = OpType::Const;
                                assign.op2 = prop_name_idx;
                                assign.result_type = OpType::Cv;
                                assign.result = param_cv;
                                func_compiler.instructions.push(assign);
                            }
                        }
                    }

                    for s in &method.body {
                        func_compiler.compile_stmt(s)?;
                    }
                    let null_idx = func_compiler.add_literal(Value::null());
                    let mut ret = Instruction::new(OpCode::Return);
                    ret.op1_type = OpType::Const;
                    ret.op1 = null_idx;
                    func_compiler.instructions.push(ret);

                    let cache = (0..func_compiler.instructions.len()).map(|_| InlineCache::empty()).collect();
                    let may_access_globals = !func_compiler.global_vars.is_empty()
                    || func_compiler.instructions.iter().any(|i| matches!(i.opcode,
                        OpCode::InitFcall | OpCode::InitDynamicCall | OpCode::InitUserCall | OpCode::CallUserFuncArray
                        | OpCode::InitMethodCall | OpCode::InitStaticCall
                        | OpCode::Include));
                    let op_array = OpArray {
                        num_cvs: func_compiler.next_cv,
                        num_temps: func_compiler.next_tmp,
                        instructions: func_compiler.instructions,
                        literals: func_compiler.literals,
                        try_entries: func_compiler.try_entries,
                        strict_types: self.strict_types,
                        is_generator: func_compiler.contains_yield,
                        global_vars: func_compiler.global_vars,
                        static_vars: func_compiler.static_vars,
                        name: func_compiler.current_function_name,
                        main_scope_vars: vec![],
                        all_cvs: vec![],
                        cache,
                        may_access_globals,
                        block_info: Vec::new(),
                        block_counters: Vec::new(),
                        block_plans: Vec::new(),
                        ip_to_block: Vec::new(),
                    };
                    // Methods have $this at CV 0 — add 1 to num_args to include $this
                    // and set this_offset=1 so arity check and visibility detection work correctly
                    let user_func = finalize_user_method(
                        make_user_function_typed(
                            op_array,
                            cp.num_args + 1,
                            cp.required_num_args,
                            cp.is_variadic,
                            cp.variadic_cv_index,
                            cp.ref_args,
                            cp.type_hints,
                            cp.param_names,
                            cp.return_type_hint,
                        ),
                        &method.name,
                    );
                    self.functions.extend(func_compiler.functions);
                    compiled_methods.push((method.name.clone(), method.visibility, method.is_static, method.is_final, user_func));
                }

                // Evaluate property defaults (constant expressions only)
                let mut compiled_props: Vec<(String, Option<Value>, Visibility, String)> = Vec::new();
                let mut readonly_props: Vec<String> = Vec::new();
                for prop in properties {
                    let default = match &prop.default {
                        Some(expr) => Some(Self::eval_const_expr_with_constants(expr, &self.known_constants).map_err(|e| {
                            format!("Cannot use non-constant expression as default value for property {}::${}: {}", name, prop.name, e)
                        })?),
                        None => None,
                    };
                    if prop.is_readonly {
                        readonly_props.push(prop.name.clone());
                    }
                    compiled_props.push((prop.name.clone(), default, prop.visibility, name.clone()));
                }

                // Add promoted properties
                for (pname, pvis, p_readonly) in &promoted_props {
                    compiled_props.push((pname.clone(), None, *pvis, name.clone()));
                    if *p_readonly {
                        readonly_props.push(pname.clone());
                    }
                }

                // Store class definition for runtime
                let resolved_class = self.resolve_name(name);
                let resolved_parent = parent.as_ref().map(|p| self.resolve_name(p));
                let resolved_implements: Vec<String> = implements.iter().map(|i| self.resolve_name(i)).collect();
                let resolved_uses: Vec<String> = uses.iter().map(|u| self.resolve_name(u)).collect();
                self.class_defs.push(ClassDef {
                    name: resolved_class,
                    parent: resolved_parent,
                    implements: resolved_implements,
                    is_interface: false,
                    is_abstract: *is_abstract,
                    is_final: *is_final,
                    is_trait: false,
                    is_enum: false,
                    uses: resolved_uses,
                    properties: compiled_props,
                    property_layout: std::rc::Rc::new(ObjectLayout::empty()),
                    readonly_props,
                    methods: compiled_methods,
                    class_id: 0,
                });
            }
            Stmt::Interface { name, extends, methods } => {
                // Interface methods have no body — we still create stub UserFunctions
                // so they appear in the class_def for type checking, but they should
                // never be called directly (implementing class provides the body).
                let mut compiled_methods = Vec::new();
                for method in methods {
                    // Create a minimal op_array that just returns null
                    let mut func_compiler = Compiler::new();
                    func_compiler.known_ref_args = self.build_known_ref_args();
                    func_compiler.resolve_cv("this");
                    let context = format!("interface method {}::{}", name, method.name);
                    let mut cp = self.compile_params(&mut func_compiler, &method.params, &context)?;
                    cp.return_type_hint = self.convert_type_hint(&method.return_type);
                    let null_idx = func_compiler.add_literal(Value::null());
                    let mut ret = Instruction::new(OpCode::Return);
                    ret.op1_type = OpType::Const;
                    ret.op1 = null_idx;
                    func_compiler.instructions.push(ret);

                    let cache = (0..func_compiler.instructions.len()).map(|_| InlineCache::empty()).collect();
                    let may_access_globals = !func_compiler.global_vars.is_empty()
                    || func_compiler.instructions.iter().any(|i| matches!(i.opcode,
                        OpCode::InitFcall | OpCode::InitDynamicCall | OpCode::InitUserCall | OpCode::CallUserFuncArray
                        | OpCode::InitMethodCall | OpCode::InitStaticCall
                        | OpCode::Include));
                    let op_array = OpArray {
                        num_cvs: func_compiler.next_cv,
                        num_temps: func_compiler.next_tmp,
                        instructions: func_compiler.instructions,
                        literals: func_compiler.literals,
                        try_entries: func_compiler.try_entries,
                        strict_types: self.strict_types,
                        is_generator: func_compiler.contains_yield,
                        global_vars: func_compiler.global_vars,
                        static_vars: func_compiler.static_vars,
                        name: func_compiler.current_function_name,
                        main_scope_vars: vec![],
                        all_cvs: vec![],
                        cache,
                        may_access_globals,
                        block_info: Vec::new(),
                        block_counters: Vec::new(),
                        block_plans: Vec::new(),
                        ip_to_block: Vec::new(),
                    };
                    let user_func = make_user_function_typed(op_array, cp.num_args, cp.required_num_args, cp.is_variadic, cp.variadic_cv_index, cp.ref_args, cp.type_hints, cp.param_names, cp.return_type_hint);
                    self.functions.extend(func_compiler.functions);
                    compiled_methods.push((method.name.clone(), method.visibility, method.is_static, false, user_func));
                }

                // For interface "extends", all parent interfaces become the implements list
                let resolved_iface = self.resolve_name(name);
                let resolved_extends: Vec<String> = extends.iter().map(|e| self.resolve_name(e)).collect();
                self.class_defs.push(ClassDef {
                    name: resolved_iface,
                    parent: None,
                    implements: resolved_extends,
                    is_interface: true,
                    is_abstract: false,
                    is_final: false,
                    is_trait: false,
                    is_enum: false,
                    uses: vec![],
                    properties: vec![],
                    property_layout: std::rc::Rc::new(ObjectLayout::empty()),
                    readonly_props: vec![],
                    methods: compiled_methods,
                    class_id: 0,
                });
            }
            Stmt::Trait { name, properties, methods } => {
                // Compile trait — very similar to class, but flagged as is_trait=true.
                // Trait methods get compiled exactly like class methods.
                let mut compiled_methods = Vec::new();
                for method in methods {
                    let mut func_compiler = Compiler::new();
                    func_compiler.known_ref_args = self.build_known_ref_args();
                    func_compiler.resolve_cv("this");
                    let context = format!("trait method {}::{}", name, method.name);
                    let mut cp = self.compile_params(&mut func_compiler, &method.params, &context)?;
                    cp.return_type_hint = self.convert_type_hint(&method.return_type);
                    for s in &method.body {
                        func_compiler.compile_stmt(s)?;
                    }
                    let null_idx = func_compiler.add_literal(Value::null());
                    let mut ret = Instruction::new(OpCode::Return);
                    ret.op1_type = OpType::Const;
                    ret.op1 = null_idx;
                    func_compiler.instructions.push(ret);

                    let cache = (0..func_compiler.instructions.len()).map(|_| InlineCache::empty()).collect();
                    let may_access_globals = !func_compiler.global_vars.is_empty()
                    || func_compiler.instructions.iter().any(|i| matches!(i.opcode,
                        OpCode::InitFcall | OpCode::InitDynamicCall | OpCode::InitUserCall | OpCode::CallUserFuncArray
                        | OpCode::InitMethodCall | OpCode::InitStaticCall
                        | OpCode::Include));
                    let op_array = OpArray {
                        num_cvs: func_compiler.next_cv,
                        num_temps: func_compiler.next_tmp,
                        instructions: func_compiler.instructions,
                        literals: func_compiler.literals,
                        try_entries: func_compiler.try_entries,
                        strict_types: self.strict_types,
                        is_generator: func_compiler.contains_yield,
                        global_vars: func_compiler.global_vars,
                        static_vars: func_compiler.static_vars,
                        name: func_compiler.current_function_name,
                        main_scope_vars: vec![],
                        all_cvs: vec![],
                        cache,
                        may_access_globals,
                        block_info: Vec::new(),
                        block_counters: Vec::new(),
                        block_plans: Vec::new(),
                        ip_to_block: Vec::new(),
                    };
                    let user_func = finalize_user_method(
                        make_user_function_typed(
                            op_array,
                            cp.num_args + 1,
                            cp.required_num_args,
                            cp.is_variadic,
                            cp.variadic_cv_index,
                            cp.ref_args,
                            cp.type_hints,
                            cp.param_names,
                            cp.return_type_hint,
                        ),
                        &method.name,
                    );
                    self.functions.extend(func_compiler.functions);
                    compiled_methods.push((method.name.clone(), method.visibility, method.is_static, method.is_final, user_func));
                }

                let mut compiled_props: Vec<(String, Option<Value>, Visibility, String)> = Vec::new();
                for prop in properties {
                    let default = match &prop.default {
                        Some(expr) => Some(Self::eval_const_expr_with_constants(expr, &self.known_constants).map_err(|e| {
                            format!("Cannot use non-constant expression as default value for trait property {}::${}: {}", name, prop.name, e)
                        })?),
                        None => None,
                    };
                    compiled_props.push((prop.name.clone(), default, prop.visibility, name.clone()));
                }

                let resolved_trait = self.resolve_name(name);
                self.class_defs.push(ClassDef {
                    name: resolved_trait,
                    parent: None,
                    implements: vec![],
                    is_interface: false,
                    is_abstract: false,
                    is_final: false,
                    is_trait: true,
                    is_enum: false,
                    uses: vec![],
                    properties: compiled_props,
                    property_layout: std::rc::Rc::new(ObjectLayout::empty()),
                    readonly_props: vec![],
                    methods: compiled_methods,
                    class_id: 0,
                });
            }
            Stmt::Enum { name, backing_type, cases, methods } => {
                // Compile enum as a class. Each case becomes a static property
                // holding a singleton object with `name` (and optionally `value`) properties.
                let is_backed = backing_type.is_some();

                // Compile methods
                let mut compiled_methods = Vec::new();
                for method in methods {
                    let mut func_compiler = Compiler::new();
                    func_compiler.known_ref_args = self.build_known_ref_args();
                    func_compiler.resolve_cv("this");
                    let context = format!("enum method {}::{}", name, method.name);
                    let mut cp = self.compile_params(&mut func_compiler, &method.params, &context)?;
                    cp.return_type_hint = self.convert_type_hint(&method.return_type);
                    for s in &method.body {
                        func_compiler.compile_stmt(s)?;
                    }
                    let null_idx = func_compiler.add_literal(Value::null());
                    let mut ret = Instruction::new(OpCode::Return);
                    ret.op1_type = OpType::Const;
                    ret.op1 = null_idx;
                    func_compiler.instructions.push(ret);

                    let cache = (0..func_compiler.instructions.len()).map(|_| InlineCache::empty()).collect();
                    let may_access_globals = !func_compiler.global_vars.is_empty()
                    || func_compiler.instructions.iter().any(|i| matches!(i.opcode,
                        OpCode::InitFcall | OpCode::InitDynamicCall | OpCode::InitUserCall | OpCode::CallUserFuncArray
                        | OpCode::InitMethodCall | OpCode::InitStaticCall
                        | OpCode::Include));
                    let op_array = OpArray {
                        num_cvs: func_compiler.next_cv,
                        num_temps: func_compiler.next_tmp,
                        instructions: func_compiler.instructions,
                        literals: func_compiler.literals,
                        try_entries: func_compiler.try_entries,
                        strict_types: self.strict_types,
                        is_generator: func_compiler.contains_yield,
                        global_vars: func_compiler.global_vars,
                        static_vars: func_compiler.static_vars,
                        name: func_compiler.current_function_name,
                        main_scope_vars: vec![],
                        all_cvs: vec![],
                        cache,
                        may_access_globals,
                        block_info: Vec::new(),
                        block_counters: Vec::new(),
                        block_plans: Vec::new(),
                        ip_to_block: Vec::new(),
                    };
                    let user_func = finalize_user_method(
                        make_user_function_typed(
                            op_array,
                            cp.num_args + 1,
                            cp.required_num_args,
                            cp.is_variadic,
                            cp.variadic_cv_index,
                            cp.ref_args,
                            cp.type_hints,
                            cp.param_names,
                            cp.return_type_hint,
                        ),
                        &method.name,
                    );
                    self.functions.extend(func_compiler.functions);
                    compiled_methods.push((method.name.clone(), method.visibility, method.is_static, method.is_final, user_func));
                }

                // Build properties for enum cases — each case is stored as a property
                // with a default value that is a PhpObject with name/value fields.
                // Static properties (cases) are stored as class properties with is_enum_case flag.
                let mut compiled_props: Vec<(String, Option<Value>, Visibility, String)> = Vec::new();
                for (case_name, case_value) in cases {
                    use crate::value::{PhpObject, PhpArray};
                    let mut props = std::collections::HashMap::new();
                    props.insert("name".to_string(), Value::string(case_name.clone()));
                    if is_backed {
                        if let Some(expr) = case_value {
                            let val = Self::eval_const_expr_with_constants(expr, &self.known_constants).map_err(|e| {
                                format!("Cannot use non-constant expression as enum case value for {}::{}: {}", name, case_name, e)
                            })?;
                            props.insert("value".to_string(), val);
                        }
                    }
                    let obj = Value::object(PhpObject::dynamic(
                        name.clone(),
                        0, // assigned at runtime registration
                        props,
                    ));
                    compiled_props.push((case_name.clone(), Some(obj), Visibility::Public, name.clone()));
                }

                let resolved_enum = self.resolve_name(name);
                self.class_defs.push(ClassDef {
                    name: resolved_enum,
                    parent: None,
                    implements: vec![],
                    is_interface: false,
                    is_abstract: false,
                    is_final: true, // enums are implicitly final
                    is_trait: false,
                    is_enum: true,
                    uses: vec![],
                    properties: compiled_props,
                    property_layout: std::rc::Rc::new(ObjectLayout::empty()),
                    readonly_props: vec![],
                    methods: compiled_methods,
                    class_id: 0,
                });
            }
        }
        Ok(())
    }

    /// Evaluate a constant expression at compile time (for property defaults).
    /// Returns Err for expressions that cannot be resolved at compile time.
    #[allow(dead_code)]
    fn eval_const_expr(expr: &Expr) -> Result<Value, String> {
        Self::eval_const_expr_with_constants(expr, &HashMap::new())
    }

    /// Evaluate a constant expression with access to known compile-time constants.
    fn eval_const_expr_with_constants(expr: &Expr, known: &HashMap<String, Value>) -> Result<Value, String> {
        match expr {
            Expr::Integer(n) => Ok(Value::long(*n)),
            Expr::Float(f) => Ok(Value::double(*f)),
            Expr::StringLiteral(s) => Ok(Value::string(s.clone())),
            Expr::Bool(b) => Ok(Value::bool(*b)),
            Expr::Null => Ok(Value::null()),
            Expr::Constant(name) => {
                // Check user-defined constants from the same compilation unit
                if let Some(val) = known.get(name) {
                    return Ok(val.clone());
                }
                // PHP built-in constants (shared source of truth with runtime)
                if let Some(val) = crate::builtin_constant(name) {
                    return Ok(val);
                }
                // Stream constants cannot be used in constant expressions
                match name.as_str() {
                    "STDIN" | "STDOUT" | "STDERR" => Err(format!("{} is not available in constant expressions", name)),
                    _ => Err(format!("expression Constant(\"{}\") is not a compile-time constant", name)),
                }
            }
            Expr::UnaryMinus(inner) => {
                match inner.as_ref() {
                    Expr::Integer(n) => Ok(Value::long(-n)),
                    Expr::Float(f) => Ok(Value::double(-f)),
                    _ => Err("unsupported unary expression".to_string()),
                }
            }
            Expr::ArrayLiteral(elements) => {
                let mut arr = crate::value::PhpArray::new();
                for elem in elements {
                    let val = Self::eval_const_expr_with_constants(&elem.value, known)?;
                    if let Some(key_expr) = &elem.key {
                        let key = Self::eval_const_expr_with_constants(key_expr, known)?;
                        if let Some(n) = key.as_long() {
                            arr.set_int(n, val);
                        } else if let Some(s) = key.as_str() {
                            arr.set_str(s, val);
                        } else {
                            return Err("unsupported array key type in constant expression".to_string());
                        }
                    } else {
                        arr.push(val);
                    }
                }
                Ok(Value::array(arr))
            }
            _ => Err(format!("expression {:?} is not a compile-time constant", expr)),
        }
    }

    /// Compile parameter list into CV slots. Returns (num_args, required_num_args, is_variadic, variadic_cv_index, ref_args).
    /// num_args counts only non-variadic params. The variadic param gets its own CV.
    fn compile_params(&self, func_compiler: &mut Compiler, params: &[Param], context: &str) -> Result<CompiledParams, String> {
        let mut required_num_args = 0u32;
        let mut seen_default = false;
        let mut is_variadic = false;
        let mut variadic_cv_index = 0u32;
        let mut ref_args = 0u64;
        let mut type_hints = Vec::new();
        let mut param_names = Vec::new();
        for (i, param) in params.iter().enumerate() {
            if param.is_ref && i < 64 {
                ref_args |= 1u64 << i;
            }
            // Collect type hint
            let hint = self.convert_type_hint(&param.type_hint);
            type_hints.push(hint);
            // Collect param name
            param_names.push(param.name.clone());

            if param.is_variadic {
                if i != params.len() - 1 {
                    return Err(format!("Variadic parameter ${} must be last in {}", param.name, context));
                }
                is_variadic = true;
                variadic_cv_index = func_compiler.resolve_cv(&param.name) as u32;
                // No default emit for variadic — VM packs extra args into array
            } else {
                let cv_idx = func_compiler.resolve_cv(&param.name);
                if let Some(default_expr) = &param.default {
                    seen_default = true;
                    Self::emit_default_param(func_compiler, cv_idx, default_expr);
                } else {
                    if seen_default {
                        return Err(format!(
                            "Required parameter ${} follows optional parameter in {}",
                            param.name, context
                        ));
                    }
                    required_num_args = (i as u32) + 1;
                }
            }
        }
        // num_args = non-variadic params count
        let num_args = if is_variadic { (params.len() - 1) as u32 } else { params.len() as u32 };
        Ok(CompiledParams { num_args, required_num_args, is_variadic, variadic_cv_index, ref_args, type_hints, param_names, return_type_hint: crate::vm::function::ParamTypeHint::None })
    }

    /// Convert parser TypeHint to runtime ParamTypeHint.
    fn convert_type_hint(&self, hint: &Option<crate::parser::TypeHint>) -> crate::vm::function::ParamTypeHint {
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
                // `self` and `parent` are special PHP pseudo-types — don't resolve through namespaces
                match name.as_str() {
                    "self" | "parent" | "static" => ParamTypeHint::ClassName(name.clone()),
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
                let converted: Vec<ParamTypeHint> = types.iter()
                    .map(|t| self.convert_type_hint(&Some(t.clone())))
                    .collect();
                ParamTypeHint::Union(converted)
            }
        }
    }

    /// Emit default parameter initialization for a single param.
    /// Pattern: BindDefaultParam (skip if arg passed) → compute default → AssignCv → label
    fn emit_default_param(compiler: &mut Compiler, cv_idx: u16, default_expr: &Expr) {
        // BindDefaultParam: if CV is NOT undef, jump to skip_label (op2 = target, patched later)
        let bind_idx = compiler.instructions.len();
        let mut bind = Instruction::new(OpCode::BindDefaultParam);
        bind.op1_type = OpType::Cv;
        bind.op1 = cv_idx;
        bind.op2 = 0; // placeholder — will be patched to skip_label
        compiler.instructions.push(bind);

        // Compute default expression (only reached if arg was NOT passed)
        let (val_op, val_type) = compiler.compile_expr(default_expr);

        // Assign computed default to CV
        let mut assign = Instruction::new(OpCode::AssignCv);
        assign.op1_type = OpType::Cv;
        assign.op1 = cv_idx;
        assign.op2_type = val_type;
        assign.op2 = val_op;
        compiler.instructions.push(assign);

        // Patch BindDefaultParam to skip past the assign
        let skip_label = compiler.instructions.len() as u16;
        compiler.instructions[bind_idx].op2 = skip_label;
    }

    /// Compile expression. Returns (operand_index, OpType).
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
            Expr::Variable(name) => {
                let idx = self.resolve_cv(name);
                (idx, OpType::Cv)
            }
            Expr::BinaryOp { op, left, right } => {
                // Short-circuit logical operators
                match op {
                    BinOp::And => {
                        // $a && $b: eval left, JmpZ → false, eval right, JmpZ → false,
                        // result=true, Jmp→end, false: result=false, end:
                        let (l_op, l_type) = self.compile_expr(left);
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

                        return (tmp, OpType::Tmp);
                    }
                    BinOp::Or => {
                        // $a || $b: evaluate $a, if true skip $b
                        let (l_op, l_type) = self.compile_expr(left);
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

                        return (tmp, OpType::Tmp);
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
                    BinOp::Concat => OpCode::Concat,
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
                    BinOp::And | BinOp::Or => unreachable!(), // handled above
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
            Expr::PostInc(name) => {
                let cv_idx = self.resolve_cv(name);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::PostInc);
                instr.op1_type = OpType::Cv;
                instr.op1 = cv_idx;
                instr.result_type = OpType::Tmp;
                instr.result = tmp;
                self.instructions.push(instr);
                (tmp, OpType::Tmp)
            }
            Expr::PostDec(name) => {
                let cv_idx = self.resolve_cv(name);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::PostDec);
                instr.op1_type = OpType::Cv;
                instr.op1 = cv_idx;
                instr.result_type = OpType::Tmp;
                instr.result = tmp;
                self.instructions.push(instr);
                (tmp, OpType::Tmp)
            }
            Expr::PreInc(name) => {
                let cv_idx = self.resolve_cv(name);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::PreInc);
                instr.op1_type = OpType::Cv;
                instr.op1 = cv_idx;
                instr.result_type = OpType::Tmp;
                instr.result = tmp;
                self.instructions.push(instr);
                (tmp, OpType::Tmp)
            }
            Expr::PreDec(name) => {
                let cv_idx = self.resolve_cv(name);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::PreDec);
                instr.op1_type = OpType::Cv;
                instr.op1 = cv_idx;
                instr.result_type = OpType::Tmp;
                instr.result = tmp;
                self.instructions.push(instr);
                (tmp, OpType::Tmp)
            }
            Expr::Ternary { condition, then_expr, else_expr } => {
                let (cond_op, cond_type) = self.compile_expr(condition);
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
                let (else_op, else_type) = self.compile_expr(else_expr);
                let mut set_else = Instruction::new(OpCode::AssignCv);
                set_else.op1_type = OpType::Tmp;
                set_else.op1 = tmp;
                set_else.op2_type = else_type;
                set_else.op2 = else_op;
                self.instructions.push(set_else);

                let end_label = self.instructions.len() as u16;
                self.instructions[jmpz_idx].op2 = else_label;
                self.instructions[jmp_end_idx].op1 = end_label;

                (tmp, OpType::Tmp)
            }
            Expr::Elvis { left, right } => {
                // Evaluate LHS once, store in tmp
                let (left_op, left_type) = self.compile_expr(left);
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
            Expr::FunctionCall { name, args } => {
                if let [CallArg::Positional(argument)] = args.as_slice() {
                    let direct_kind = self
                        .unambiguous_global_function_name(name)
                        .and_then(crate::builtin_metadata::direct_internal_spec)
                        .filter(|spec| {
                            spec.required_args <= 1 && spec.max_args >= 1
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
                        self.instructions.push(call);
                        return (tmp, OpType::Tmp);
                    }
                }

                if let [CallArg::Positional(first), CallArg::Positional(second)] = args.as_slice() {
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
                        self.instructions.push(call);
                        return (tmp, OpType::Tmp);
                    }
                }

                if self.is_global_builtin_call(name, "call_user_func") {
                    if let Some((CallArg::Positional(callback), forwarded)) = args.split_first() {
                        if forwarded.iter().all(|arg| matches!(arg, CallArg::Positional(_))) {
                            let (callback_op, callback_type) = self.compile_expr(callback);
                            let mut init = Instruction::new(OpCode::InitUserCall);
                            init.op1 = callback_op;
                            init.op1_type = callback_type;
                            init.extended_value = forwarded.len() as u32;
                            self.instructions.push(init);

                            self.emit_user_call_args(forwarded);

                            let tmp = self.alloc_tmp();
                            let mut do_fcall = Instruction::new(OpCode::DoFcall);
                            do_fcall.result = tmp;
                            do_fcall.result_type = OpType::Tmp;
                            self.instructions.push(do_fcall);
                            return (tmp, OpType::Tmp);
                        }
                    }
                }

                if self.is_global_builtin_call(name, "call_user_func_array") {
                    if let [CallArg::Positional(callback), CallArg::Positional(array)] = args.as_slice() {
                        if let Expr::ArrayLiteral(elements) = array {
                            if elements.iter().all(|element| element.key.is_none()) {
                                // A temporary packed literal cannot be observed by PHP
                                // code. Forward its values directly and avoid allocating,
                                // filling and dropping a PhpArray for every invocation.
                                let (callback_op, callback_type) = self.compile_expr(callback);
                                let mut init = Instruction::new(OpCode::InitUserCall);
                                init.op1 = callback_op;
                                init.op1_type = callback_type;
                                init.extended_value = elements.len() as u32;
                                self.instructions.push(init);

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
                                self.instructions.push(do_fcall);
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
                        self.instructions.push(call);
                        return (tmp, OpType::Tmp);
                    }
                }

                let resolved = self.resolve_name(name);
                let ref_args = self.lookup_ref_args(&resolved);
                let name_idx = self.add_literal(Value::string(resolved));

                // For unqualified function calls in a namespace, PHP falls back to global.
                // Store the original unqualified name as a fallback literal.
                // Qualified/FQ names (containing \) get no fallback.
                let fallback_idx = if self.current_namespace.is_some() && !name.contains('\\') {
                    self.add_literal(Value::string(name.clone()))
                } else {
                    0 // no fallback
                };

                let mut init = Instruction::new(OpCode::InitFcall);
                init.op1 = args.len() as u16;
                init.op2_type = OpType::Const;
                init.op2 = name_idx;
                init.extended_value = fallback_idx as u32;
                let init_index = self.instructions.len();
                self.instructions.push(init);

                self.emit_call_args(args, 0, ref_args, false, false);

                if args.iter().all(|arg| matches!(arg, CallArg::Positional(_)))
                    && self.instructions.len() > init_index + 1 + args.len()
                {
                    self.instructions[init_index]._pad |= CALL_FLAG_DEFERRED_SCALAR_CANDIDATE;
                }

                let tmp = self.alloc_tmp();
                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                do_fcall.result = tmp;
                do_fcall.result_type = OpType::Tmp;
                self.instructions.push(do_fcall);

                (tmp, OpType::Tmp)
            }
            Expr::ArrayLiteral(elements) => {
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
                    let (val_op, val_type) = self.compile_expr(&elem.value);
                    let mut add = Instruction::new(OpCode::AddArrayElement);
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
                    self.instructions.push(add);
                }

                (arr_tmp, OpType::Tmp)
            }
            Expr::ArrayAccess { array, index } => {
                let (arr_op, arr_type) = self.compile_expr(array);
                let (idx_op, idx_type) = self.compile_expr(index);
                let tmp = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchDimR);
                fetch.op1_type = arr_type;
                fetch.op1 = arr_op;
                fetch.op2_type = idx_type;
                fetch.op2 = idx_op;
                fetch.result_type = OpType::Tmp;
                fetch.result = tmp;
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
                let zero_idx = self.add_literal(Value::long(0));
                let (inner_op, inner_type) = self.compile_expr(inner);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::Sub);
                instr.op1 = zero_idx;
                instr.op1_type = OpType::Const;
                instr.op2 = inner_op;
                instr.op2_type = inner_type;
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                self.instructions.push(instr);
                (tmp, OpType::Tmp)
            }
            Expr::Cast { cast_type, expr } => {
                let (inner_op, inner_type) = self.compile_expr(expr);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::Cast);
                instr.op1 = inner_op;
                instr.op1_type = inner_type;
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                instr.extended_value = *cast_type as u32;
                self.instructions.push(instr);
                (tmp, OpType::Tmp)
            }
            Expr::Isset(args) => {
                let (op, op_type) = self.compile_expr(&args[0]);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::Isset);
                instr.op1 = op;
                instr.op1_type = op_type;
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                self.instructions.push(instr);
                // Multi-arg: AND each additional isset check
                // Simple non-short-circuit implementation for now
                for arg in args.iter().skip(1) {
                    let (op2, op_type2) = self.compile_expr(arg);
                    let tmp2 = self.alloc_tmp();
                    let mut instr2 = Instruction::new(OpCode::Isset);
                    instr2.op1 = op2;
                    instr2.op1_type = op_type2;
                    instr2.result = tmp2;
                    instr2.result_type = OpType::Tmp;
                    self.instructions.push(instr2);
                    // Combine: if first was false, result is false
                    let jmpz_idx = self.instructions.len();
                    let mut jmpz = Instruction::new(OpCode::JmpZ);
                    jmpz.op1 = tmp;
                    jmpz.op1_type = OpType::Tmp;
                    jmpz.op2 = 0; // placeholder
                    self.instructions.push(jmpz);
                    // Copy tmp2 into tmp
                    let mut assign = Instruction::new(OpCode::AssignCv);
                    assign.op1_type = OpType::Tmp;
                    assign.op1 = tmp;
                    assign.op2_type = OpType::Tmp;
                    assign.op2 = tmp2;
                    self.instructions.push(assign);
                    let end = self.instructions.len() as u16;
                    self.instructions[jmpz_idx].op2 = end;
                }
                (tmp, OpType::Tmp)
            }
            Expr::Empty(inner) => {
                // empty($x) ≡ !is_truthy($x)
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
            Expr::NullCoalesce { left, right } => {
                // $a ?? $b → isset($a) ? $a : $b
                let (l_op, l_type) = self.compile_expr(left);
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

                (tmp, OpType::Tmp)
            }
            Expr::Match { expr, arms } => {
                // match($x) { cond => body, ... default => body }
                // Compile like a chain of === checks
                let (expr_op, expr_type) = self.compile_expr(expr);
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
                    let err_obj = crate::value::make_error_value("UnhandledMatchError", "Unhandled match case");
                    let err_idx = self.add_literal(err_obj);
                    let mut throw = Instruction::new(OpCode::Throw);
                    throw.op1 = err_idx;
                    throw.op1_type = OpType::Const;
                    self.instructions.push(throw);
                }

                let end_label = self.instructions.len() as u16;
                for patch in end_patches {
                    self.instructions[patch].op1 = end_label;
                }

                (result_tmp, OpType::Tmp)
            }
            Expr::Closure { params, use_vars, body, return_type } => {
                // Compile closure body into a separate function
                let mut func_compiler = Compiler::new();
                func_compiler.known_ref_args = self.build_known_ref_args();
                // params come first as CVs (args), then use_vars
                let compile_result = self.compile_params(&mut func_compiler, params, "closure");
                let mut cp = match compile_result {
                    Ok(r) => r,
                    Err(e) => {
                        self.deferred_error = Some(e);
                        CompiledParams { num_args: params.len() as u32, required_num_args: params.len() as u32, is_variadic: false, variadic_cv_index: 0, ref_args: 0, type_hints: vec![], param_names: vec![], return_type_hint: crate::vm::function::ParamTypeHint::None }
                    }
                };
                cp.return_type_hint = self.convert_type_hint(return_type);
                for v in use_vars {
                    func_compiler.resolve_cv(v);
                }
                for s in body {
                    if let Err(e) = func_compiler.compile_stmt(s) {
                        self.deferred_error = Some(e);
                        break;
                    }
                }
                let null_idx = func_compiler.add_literal(Value::null());
                let mut ret = Instruction::new(OpCode::Return);
                ret.op1_type = OpType::Const;
                ret.op1 = null_idx;
                func_compiler.instructions.push(ret);

                let closure_all_cvs = func_compiler.all_cvs();
                let cache = (0..func_compiler.instructions.len()).map(|_| InlineCache::empty()).collect();
                let may_access_globals = !func_compiler.global_vars.is_empty()
                    || func_compiler.instructions.iter().any(|i| matches!(i.opcode,
                        OpCode::InitFcall | OpCode::InitDynamicCall | OpCode::InitUserCall | OpCode::CallUserFuncArray
                        | OpCode::InitMethodCall | OpCode::InitStaticCall
                        | OpCode::Include));
                let op_array = OpArray {
                    num_cvs: func_compiler.next_cv,
                    num_temps: func_compiler.next_tmp,
                    instructions: func_compiler.instructions,
                    literals: func_compiler.literals,
                    try_entries: func_compiler.try_entries,
                    strict_types: self.strict_types,
                    is_generator: func_compiler.contains_yield,
                    global_vars: func_compiler.global_vars,
                    static_vars: func_compiler.static_vars,
                    name: func_compiler.current_function_name,
                    main_scope_vars: vec![],
                    all_cvs: closure_all_cvs,
                    cache,
                    may_access_globals,
                    block_info: Vec::new(),
                    block_counters: Vec::new(),
                    block_plans: Vec::new(),
                    ip_to_block: Vec::new(),
                };
                let user_func = make_user_function_typed(op_array, cp.num_args, cp.required_num_args, cp.is_variadic, cp.variadic_cv_index, cp.ref_args, cp.type_hints, cp.param_names, cp.return_type_hint);

                // Register closure as anonymous function with unique name
                let closure_name = format!("__closure_{}", CLOSURE_COUNTER.fetch_add(1, Ordering::Relaxed));
                self.functions.extend(func_compiler.functions);
                self.functions.push((closure_name.clone(), user_func));

                // Build closure value with direct function pointer + captured values.
                // CreateClosure resolves the function pointer at creation time (not call time).
                // ClosureUseVar pushes each captured value into the closure.
                let name_idx = self.add_literal(Value::string(closure_name));
                let tmp = self.alloc_tmp();

                let mut create = Instruction::new(OpCode::CreateClosure);
                create.op1 = name_idx;
                create.op1_type = OpType::Const;
                create.result = tmp;
                create.result_type = OpType::Tmp;
                create.extended_value = use_vars.len() as u32;
                self.instructions.push(create);

                // Add captured use_var values
                for v in use_vars {
                    let cv = self.resolve_cv(v);
                    let mut use_var = Instruction::new(OpCode::ClosureUseVar);
                    use_var.op1 = tmp;
                    use_var.op1_type = OpType::Tmp;
                    use_var.op2 = cv;
                    use_var.op2_type = OpType::Cv;
                    self.instructions.push(use_var);
                }

                (tmp, OpType::Tmp)
            }
            Expr::New { class_name, args } => {
                // Pre-compile arg expressions BEFORE NewObj so side effects
                // always execute, even when the class has no __construct.
                // Compile args, tracking which are named for SendNamed emission
                let compiled_args: Vec<(u16, OpType, Option<u16>)> = args.iter()
                    .map(|arg| match arg {
                        CallArg::Positional(expr) => {
                            let (op, op_type) = self.compile_expr(expr);
                            (op, op_type, None)
                        }
                        CallArg::Named { name, value } => {
                            let (op, op_type) = self.compile_expr(value);
                            let name_idx = self.add_literal(Value::string(name.clone()));
                            (op, op_type, Some(name_idx))
                        }
                    })
                    .collect();

                let resolved_class = self.resolve_name(class_name);
                let name_idx = self.add_literal(Value::string(resolved_class));
                let tmp = self.alloc_tmp();
                let mut new_obj = Instruction::new(OpCode::NewObj);
                new_obj.op1 = name_idx;
                new_obj.op1_type = OpType::Const;
                new_obj.result = tmp;
                new_obj.result_type = OpType::Tmp;
                new_obj.extended_value = args.len() as u32;
                self.instructions.push(new_obj);

                // Send constructor args — offset by 1 because CV 0 is $this
                self.emit_precompiled_call_args(&compiled_args, 1);

                // DoFcall to run __construct (VM skips if no constructor exists)
                let discard = self.alloc_tmp();
                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                do_fcall.result = discard;
                do_fcall.result_type = OpType::Tmp;
                self.instructions.push(do_fcall);

                (tmp, OpType::Tmp)
            }
            Expr::PropertyAccess { object, property, nullsafe } => {
                let (obj_op, obj_type) = self.compile_expr(object);
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
                self.instructions.push(fetch);

                if let Some(idx) = nullsafe_patch {
                    self.instructions[idx].op2 = self.instructions.len() as u16;
                }

                (tmp, OpType::Tmp)
            }
            Expr::MethodCall { object, method, args, nullsafe } => {
                let (obj_op, obj_type) = self.compile_expr(object);
                let tmp = self.alloc_tmp();

                let nullsafe_patch = if *nullsafe {
                    let mut check = Instruction::new(OpCode::NullSafeCheck);
                    check.op1 = obj_op;
                    check.op1_type = obj_type;
                    check.op2 = 0;
                    check.result = tmp;
                    check.result_type = OpType::Tmp;
                    check.extended_value = 1; // 1 = method call (fatal on scalar)
                    let idx = self.instructions.len();
                    self.instructions.push(check);
                    Some(idx)
                } else {
                    None
                };

                let method_idx = self.add_literal(Value::string(method.clone()));

                let mut init = Instruction::new(OpCode::InitMethodCall);
                init.op1 = obj_op;
                init.op1_type = obj_type;
                init.op2 = method_idx;
                init.op2_type = OpType::Const;
                init.extended_value = args.len() as u32;
                let init_index = self.instructions.len();
                self.instructions.push(init);

                self.emit_call_args(args, 1, 0, true, true);

                if args.iter().all(|arg| matches!(arg, CallArg::Positional(_)))
                    && self.instructions.len() > init_index + 1 + args.len()
                {
                    self.instructions[init_index]._pad |= CALL_FLAG_DEFERRED_SCALAR_CANDIDATE;
                }

                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                do_fcall.result = tmp;
                do_fcall.result_type = OpType::Tmp;
                self.instructions.push(do_fcall);

                if let Some(idx) = nullsafe_patch {
                    self.instructions[idx].op2 = self.instructions.len() as u16;
                }

                (tmp, OpType::Tmp)
            }
            Expr::StaticCall { class_name, method, args } => {
                let resolved_class = self.resolve_name(class_name);
                let class_idx = self.add_literal(Value::string(resolved_class));
                let method_idx = self.add_literal(Value::string(method.clone()));

                let mut init = Instruction::new(OpCode::InitStaticCall);
                init.op1 = class_idx;
                init.op1_type = OpType::Const;
                init.op2 = method_idx;
                init.op2_type = OpType::Const;
                init.extended_value = args.len() as u32;
                self.instructions.push(init);

                self.emit_call_args(args, 1, 0, true, true);

                let tmp = self.alloc_tmp();
                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                do_fcall.result = tmp;
                do_fcall.result_type = OpType::Tmp;
                self.instructions.push(do_fcall);

                (tmp, OpType::Tmp)
            }
            Expr::StaticProperty { class_name, property } => {
                let resolved = self.resolve_name(class_name);
                let class_idx = self.add_literal(Value::string(resolved));
                let prop_idx = self.add_literal(Value::string(property.clone()));
                let tmp = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchStaticProp);
                fetch.op1 = class_idx;
                fetch.op1_type = OpType::Const;
                fetch.op2 = prop_idx;
                fetch.op2_type = OpType::Const;
                fetch.result = tmp;
                fetch.result_type = OpType::Tmp;
                self.instructions.push(fetch);
                (tmp, OpType::Tmp)
            }
            Expr::Throw(inner) => {
                let (op, op_type) = self.compile_expr(inner);
                let mut instr = Instruction::new(OpCode::Throw);
                instr.op1 = op;
                instr.op1_type = op_type;
                self.instructions.push(instr);
                // Throw never returns, but we need to return something
                let null_idx = self.add_literal(Value::null());
                (null_idx, OpType::Const)
            }
            Expr::DynamicCall { callable, args } => {
                // Compile the callable expression (e.g. $var, $arr[0])
                let (callable_op, callable_type) = self.compile_expr(callable);

                // InitDynamicCall: op1=callable, extended_value=num_args
                let mut init = Instruction::new(OpCode::InitDynamicCall);
                init.op1 = callable_op;
                init.op1_type = callable_type;
                init.extended_value = args.len() as u32;
                self.instructions.push(init);

                // Send arguments
                self.emit_call_args(args, 0, 0, true, true);

                // DoFcall
                let tmp = self.alloc_tmp();
                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                do_fcall.result = tmp;
                do_fcall.result_type = OpType::Tmp;
                self.instructions.push(do_fcall);

                (tmp, OpType::Tmp)
            }
            Expr::Instanceof { expr, class_name } => {
                let (obj_op, obj_type) = self.compile_expr(expr);
                let resolved_class = self.resolve_name(class_name);
                let name_idx = self.add_literal(Value::string(resolved_class));
                let tmp = self.alloc_tmp();
                let mut inst = Instruction::new(OpCode::Instanceof);
                inst.op1 = obj_op;
                inst.op1_type = obj_type;
                inst.op2 = name_idx;
                inst.op2_type = OpType::Const;
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
                (tmp, OpType::Tmp)
            }
            Expr::Constant(name) => {
                // Fetch a named constant at runtime
                let name_idx = self.add_literal(Value::string(name.clone()));
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::FetchConst);
                instr.op1 = name_idx;
                instr.op1_type = OpType::Const;
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                // extended_value = 0 means "read mode" (fetch constant)
                instr.extended_value = 0;
                self.instructions.push(instr);
                (tmp, OpType::Tmp)
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
            Expr::YieldFrom(sub_expr) => {
                self.contains_yield = true;
                let (sub_op, sub_type) = self.compile_expr(sub_expr);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::YieldFrom);
                instr.op1 = sub_op;
                instr.op1_type = sub_type;
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                self.instructions.push(instr);
                (tmp, OpType::Tmp)
            }
            Expr::Clone(inner) => {
                let (src_op, src_type) = self.compile_expr(inner);
                let tmp = self.alloc_tmp();
                let mut instr = Instruction::new(OpCode::CloneObj);
                instr.op1 = src_op;
                instr.op1_type = src_type;
                instr.result = tmp;
                instr.result_type = OpType::Tmp;
                self.instructions.push(instr);
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
        if let Some(instruction) = self.instructions.last_mut() {
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
            )
                && instruction.result == result
                && instruction.result_type == OpType::Tmp
            {
                instruction.result_type = OpType::Unused;
            }
        }
    }

    /// Whether a source-level function name unambiguously addresses a global
    /// builtin. An unqualified name inside a namespace must retain the normal
    /// fallback lookup because a namespaced user function may shadow it.
    fn unambiguous_global_function_name<'a>(&self, name: &'a str) -> Option<&'a str> {
        if let Some(fully_qualified) = name.strip_prefix('\\') {
            return Some(fully_qualified);
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

    /// Build list of all CVs from cv_table.
    fn all_cvs(&self) -> Vec<(u32, String)> {
        self.cv_table.iter().map(|(name, &idx)| (idx, name.clone())).collect()
    }

    /// Controls how a positional argument's Send opcode is chosen.
    /// - `RefAware`: compile-time ref check (FunctionCall with known ref_args)
    /// - `ValOnly`: always SendVal (New — constructor ref_args unknown at compile time)
    /// - `VarEx`: runtime ref check via SendVarEx (MethodCall, StaticCall, DynamicCall)
    fn positional_opcode(ref_args: u64, index: usize, op_type: OpType, use_var_ex: bool) -> OpCode {
        if ref_args != 0 && !use_var_ex {
            // RefAware mode (FunctionCall)
            let is_ref = index < 64 && (ref_args & (1u64 << index)) != 0;
            if is_ref && op_type == OpType::Cv { OpCode::SendRef } else { OpCode::SendVal }
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
                CallArg::Positional(expr) => {
                    let (op, op_type) = self.compile_expr(expr);
                    let opcode = Self::positional_opcode(ref_args, i, op_type, use_var_ex);
                    let mut send = Instruction::new(opcode);
                    send.op1 = op;
                    send.op1_type = op_type;
                    send.op2 = (i as u32 + cv_offset) as u16;
                    if set_extended_value {
                        send.extended_value = i as u32;
                    }
                    self.instructions.push(send);
                }
                CallArg::Named { name, value } => {
                    let (op, op_type) = self.compile_expr(value);
                    let name_idx = self.add_literal(Value::string(name.clone()));
                    let mut send = Instruction::new(OpCode::SendNamed);
                    send.op1 = op;
                    send.op1_type = op_type;
                    send.op2 = name_idx;
                    send.op2_type = OpType::Const;
                    self.instructions.push(send);
                }
            }
        }
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
    /// Each tuple: (operand, op_type, Option<name_literal_idx>).
    fn emit_precompiled_call_args(
        &mut self,
        compiled_args: &[(u16, OpType, Option<u16>)],
        cv_offset: u32,
    ) {
        for (i, (op, op_type, named_idx)) in compiled_args.iter().enumerate() {
            if let Some(name_const) = named_idx {
                let mut send = Instruction::new(OpCode::SendNamed);
                send.op1 = *op;
                send.op1_type = *op_type;
                send.op2 = *name_const;
                send.op2_type = OpType::Const;
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

    fn alloc_tmp(&mut self) -> u16 {
        let idx = self.next_tmp;
        self.next_tmp += 1;
        idx as u16
    }

    /// Compile list destructuring targets. Each target gets a FetchDimR + AssignCv.
    fn compile_list_targets(&mut self, targets: &[crate::parser::ListTarget], array_tmp: u16, start_index: usize) -> Result<(), String> {
        use crate::parser::ListTarget;
        let mut idx = start_index;
        for target in targets {
            match target {
                ListTarget::Variable(var_name) => {
                    // result = array_tmp[idx]
                    let idx_literal = self.add_literal(Value::long(idx as i64));
                    let fetch_tmp = self.alloc_tmp();
                    let mut fetch = Instruction::new(OpCode::FetchDimR);
                    fetch.op1_type = OpType::Tmp;
                    fetch.op1 = array_tmp;
                    fetch.op2_type = OpType::Const;
                    fetch.op2 = idx_literal;
                    fetch.result_type = OpType::Tmp;
                    fetch.result = fetch_tmp;
                    self.instructions.push(fetch);
                    // assign to CV
                    let cv_idx = self.resolve_cv(var_name);
                    let mut assign = Instruction::new(OpCode::AssignCv);
                    assign.op1_type = OpType::Cv;
                    assign.op1 = cv_idx;
                    assign.op2_type = OpType::Tmp;
                    assign.op2 = fetch_tmp;
                    self.instructions.push(assign);
                    idx += 1;
                }
                ListTarget::Skip => {
                    idx += 1;
                }
                ListTarget::Nested(inner_targets) => {
                    // Fetch the sub-array at this index
                    let idx_literal = self.add_literal(Value::long(idx as i64));
                    let sub_tmp = self.alloc_tmp();
                    let mut fetch = Instruction::new(OpCode::FetchDimR);
                    fetch.op1_type = OpType::Tmp;
                    fetch.op1 = array_tmp;
                    fetch.op2_type = OpType::Const;
                    fetch.op2 = idx_literal;
                    fetch.result_type = OpType::Tmp;
                    fetch.result = sub_tmp;
                    self.instructions.push(fetch);
                    // Recurse
                    self.compile_list_targets(inner_targets, sub_tmp, 0)?;
                    idx += 1;
                }
                ListTarget::KeyedVariable { key, var } => {
                    // Use explicit key instead of sequential index
                    let (key_op, key_type) = self.compile_expr(key);
                    let fetch_tmp = self.alloc_tmp();
                    let mut fetch = Instruction::new(OpCode::FetchDimR);
                    fetch.op1_type = OpType::Tmp;
                    fetch.op1 = array_tmp;
                    fetch.op2_type = key_type;
                    fetch.op2 = key_op;
                    fetch.result_type = OpType::Tmp;
                    fetch.result = fetch_tmp;
                    self.instructions.push(fetch);
                    let cv_idx = self.resolve_cv(var);
                    let mut assign = Instruction::new(OpCode::AssignCv);
                    assign.op1_type = OpType::Cv;
                    assign.op1 = cv_idx;
                    assign.op2_type = OpType::Tmp;
                    assign.op2 = fetch_tmp;
                    self.instructions.push(assign);
                    // Don't increment idx for keyed — they use explicit keys
                }
            }
        }
        Ok(())
    }
}
