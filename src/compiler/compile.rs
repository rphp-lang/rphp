/// AST → OpArray compiler.
/// Converts parsed statements into VM instructions.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

/// Global closure counter — ensures unique names across nested compilers.
static CLOSURE_COUNTER: AtomicU32 = AtomicU32::new(0);

use crate::value::Value;
use crate::parser::{Stmt, Expr, BinOp, CastType, Visibility, Param};
use crate::vm::opcode::OpCode;
use crate::vm::instruction::{Instruction, OpType};
use super::OpArray;

use super::{make_user_function_with_args, make_user_function_full};
use crate::vm::function::UserFunction;

/// Result of compiling a script — main OpArray + declared functions + class defs.
pub struct CompileResult {
    pub main: OpArray,
    pub functions: Vec<(String, UserFunction)>,
    pub class_defs: Vec<ClassDef>,
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

/// Compiled class definition
pub struct ClassDef {
    pub name: String,
    pub parent: Option<String>,
    pub implements: Vec<String>,
    pub is_interface: bool,
    pub is_abstract: bool,
    pub properties: Vec<(String, Option<Value>, Visibility, String)>,  // (name, default_value, visibility, declaring_class)
    pub methods: Vec<(String, Visibility, bool, UserFunction)>, // (name, vis, is_static, func)
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
}

/// Get ref_args bitmask for built-in stdlib functions.
/// Returns 0 for unknown/non-ref functions.
fn builtin_ref_args(name: &str) -> u64 {
    match name {
        "sort" | "rsort" | "shuffle" => 0b1,           // arg 0
        "array_push" | "array_unshift" => 0b1,          // arg 0
        "array_pop" | "array_shift" => 0b1,             // arg 0
        "array_splice" => 0b1,                           // arg 0
        "settype" => 0b1,                                // arg 0
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
        }
    }

    /// Look up ref_args for a function: check user functions, known_ref_args, then builtins.
    fn lookup_ref_args(&self, name: &str) -> u64 {
        // Check user-defined functions in the same compilation unit
        for (fname, uf) in &self.functions {
            if fname == name {
                return uf.common.ref_args;
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
            map.insert(fname.clone(), uf.common.ref_args);
        }
        map
    }

    pub fn compile(mut self, stmts: &[Stmt]) -> Result<CompileResult, String> {
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

        Ok(CompileResult {
            main: OpArray {
                num_cvs: self.next_cv,
                num_temps: self.next_tmp,
                instructions: self.instructions,
                literals: self.literals,
                try_entries: self.try_entries,
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
                let (operand, op_type) = self.compile_expr(expr);
                let cv_idx = self.resolve_cv(var);
                let mut assign = Instruction::new(OpCode::AssignCv);
                assign.op1_type = OpType::Cv;
                assign.op1 = cv_idx;
                assign.op2_type = op_type;
                assign.op2 = operand;
                self.instructions.push(assign);
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
                    let after_then = self.instructions.len() as u32;
                    self.instructions[jmpz_idx].op2 = after_then;
                } else {
                    // Jmp <after_else> (skip else body when then completes)
                    let jmp_idx = self.instructions.len();
                    let mut jmp = Instruction::new(OpCode::Jmp);
                    jmp.op1 = 0; // placeholder
                    self.instructions.push(jmp);

                    // Patch JmpZ to jump to else body
                    let else_start = self.instructions.len() as u32;
                    self.instructions[jmpz_idx].op2 = else_start;

                    // Compile else body
                    for s in else_body {
                        self.compile_stmt(s)?;
                    }

                    // Patch Jmp to jump past else body
                    let after_else = self.instructions.len() as u32;
                    self.instructions[jmp_idx].op1 = after_else;
                }
            }
            Stmt::Function { name, params, body } => {
                // Compile function body into a separate OpArray
                let mut func_compiler = Compiler::new();
                func_compiler.known_ref_args = self.build_known_ref_args();
                let (num_args, required_num_args, is_variadic, variadic_cv, ref_args) =
                    Self::compile_params(&mut func_compiler, params, name)?;
                for s in body {
                    func_compiler.compile_stmt(s)?;
                }
                let null_idx = func_compiler.add_literal(Value::null());
                let mut ret = Instruction::new(OpCode::Return);
                ret.op1_type = OpType::Const;
                ret.op1 = null_idx;
                func_compiler.instructions.push(ret);

                let op_array = OpArray {
                    num_cvs: func_compiler.next_cv,
                    num_temps: func_compiler.next_tmp,
                    instructions: func_compiler.instructions,
                    literals: func_compiler.literals,
                    try_entries: func_compiler.try_entries,
                };
                let user_func = make_user_function_full(op_array, num_args, required_num_args, is_variadic, variadic_cv, ref_args);

                // Collect any nested function declarations
                self.functions.extend(func_compiler.functions);
                self.functions.push((name.clone(), user_func));
            }
            Stmt::Return(expr) => {
                let (op, op_type) = if let Some(e) = expr {
                    self.compile_expr(e)
                } else {
                    let idx = self.add_literal(Value::null());
                    (idx, OpType::Const)
                };
                let mut ret = Instruction::new(OpCode::Return);
                ret.op1 = op;
                ret.op1_type = op_type;
                self.instructions.push(ret);
            }
            Stmt::ExprStmt(expr) => {
                // Compile expression for side effects (e.g. function call), discard result
                self.compile_expr(expr);
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
                jmp_back.op1 = loop_start as u32;
                self.instructions.push(jmp_back);

                // Patch JmpZ, break and continue jumps
                let after_loop = self.instructions.len() as u32;
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
                jmpnz.op2 = loop_start as u32;
                self.instructions.push(jmpnz);

                // Patch break and continue jumps
                let after_loop = self.instructions.len() as u32;
                let ctx = self.loop_stack.pop().unwrap();
                for patch_idx in ctx.break_patches {
                    self.instructions[patch_idx].op1 = after_loop;
                }
                for patch_idx in ctx.continue_patches {
                    self.instructions[patch_idx].op1 = cond_pos as u32;
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
                    self.compile_expr(upd);
                }

                // Jmp back to loop start
                let mut jmp_back = Instruction::new(OpCode::Jmp);
                jmp_back.op1 = loop_start as u32;
                self.instructions.push(jmp_back);

                // Patch JmpZ, break and continue jumps
                let after_loop = self.instructions.len() as u32;
                if let Some(idx) = jmpz_idx {
                    self.instructions[idx].op2 = after_loop;
                }
                let ctx = self.loop_stack.pop().unwrap();
                for patch_idx in ctx.break_patches {
                    self.instructions[patch_idx].op1 = after_loop;
                }
                for patch_idx in ctx.continue_patches {
                    self.instructions[patch_idx].op1 = update_pos as u32;
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
                        jmp.op1 = target as u32;
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
                        let next = self.instructions.len() as u32;
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
                let mut default_body_start: Option<u32> = None;
                for case in cases.iter() {
                    let body_start = self.instructions.len() as u32;
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

                let after_switch = self.instructions.len() as u32;

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
                init.extended_value = pos_tmp;
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
                let key_encoded = match key_cv {
                    Some(k) => (k + 1) << 16,
                    None => 0,
                };
                next.extended_value = key_encoded | val_cv;
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
                jmp_back.op1 = loop_start as u32;
                self.instructions.push(jmp_back);

                // Patch jumps
                let after_loop = self.instructions.len() as u32;
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
                    let catch_cv = self.resolve_cv(&catch.var);

                    catch_entries.push(CatchEntry {
                        types: catch.types.clone(),
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

                let after_all = self.instructions.len() as u32;

                // Patch all jumps to after_all
                self.instructions[jmp_past_catch].op1 = if let Some(fs) = finally_start {
                    fs as u32
                } else {
                    after_all
                };

                // Patch catch-end jumps
                for jmp_idx in &catch_end_jumps {
                    self.instructions[*jmp_idx].op1 = if let Some(fs) = finally_start {
                        fs as u32
                    } else {
                        after_all
                    };
                }

                // Build TryEntry with catch entries and finally info
                let (entry_finally_start, entry_finally_end) = if let Some(fs) = finally_start {
                    (fs as u32, after_all)
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
            Stmt::Const { name, value } => {
                // Compile the value expression and emit FetchConst to define it
                // For const, we evaluate at compile time if possible, otherwise at runtime
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
            Stmt::Class { name, parent, implements, is_abstract, properties, methods } => {
                // Compile class declaration — store class info as a literal
                // Each class method gets compiled like a function
                let mut compiled_methods = Vec::new();
                for method in methods {
                    let mut func_compiler = Compiler::new();
                    func_compiler.known_ref_args = self.build_known_ref_args();
                    // $this is always CV 0 in methods
                    func_compiler.resolve_cv("this");
                    let context = format!("method {}::{}", name, method.name);
                    let (num_args, required_num_args, is_variadic, variadic_cv, ref_args) =
                        Self::compile_params(&mut func_compiler, &method.params, &context)?;
                    for s in &method.body {
                        func_compiler.compile_stmt(s)?;
                    }
                    let null_idx = func_compiler.add_literal(Value::null());
                    let mut ret = Instruction::new(OpCode::Return);
                    ret.op1_type = OpType::Const;
                    ret.op1 = null_idx;
                    func_compiler.instructions.push(ret);

                    let op_array = OpArray {
                        num_cvs: func_compiler.next_cv,
                        num_temps: func_compiler.next_tmp,
                        instructions: func_compiler.instructions,
                        literals: func_compiler.literals,
                        try_entries: func_compiler.try_entries,
                    };
                    // Methods have $this at CV 0 — add 1 to num_args to include $this
                    // and set this_offset=1 so arity check and visibility detection work correctly
                    let mut user_func = make_user_function_full(op_array, num_args + 1, required_num_args, is_variadic, variadic_cv, ref_args);
                    user_func.common.this_offset = 1;
                    self.functions.extend(func_compiler.functions);
                    compiled_methods.push((method.name.clone(), method.visibility, method.is_static, user_func));
                }

                // Evaluate property defaults (constant expressions only)
                let mut compiled_props: Vec<(String, Option<Value>, Visibility, String)> = Vec::new();
                for prop in properties {
                    let default = match &prop.default {
                        Some(expr) => Some(Self::eval_const_expr(expr).map_err(|e| {
                            format!("Cannot use non-constant expression as default value for property {}::${}: {}", name, prop.name, e)
                        })?),
                        None => None,
                    };
                    compiled_props.push((prop.name.clone(), default, prop.visibility, name.clone()));
                }

                // Store class definition for runtime
                self.class_defs.push(ClassDef {
                    name: name.clone(),
                    parent: parent.clone(),
                    implements: implements.clone(),
                    is_interface: false,
                    is_abstract: *is_abstract,
                    properties: compiled_props,
                    methods: compiled_methods,
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
                    let (num_args, required_num_args, is_variadic, variadic_cv, ref_args) =
                        Self::compile_params(&mut func_compiler, &method.params, &context)?;
                    let null_idx = func_compiler.add_literal(Value::null());
                    let mut ret = Instruction::new(OpCode::Return);
                    ret.op1_type = OpType::Const;
                    ret.op1 = null_idx;
                    func_compiler.instructions.push(ret);

                    let op_array = OpArray {
                        num_cvs: func_compiler.next_cv,
                        num_temps: func_compiler.next_tmp,
                        instructions: func_compiler.instructions,
                        literals: func_compiler.literals,
                        try_entries: func_compiler.try_entries,
                    };
                    let user_func = make_user_function_full(op_array, num_args, required_num_args, is_variadic, variadic_cv, ref_args);
                    self.functions.extend(func_compiler.functions);
                    compiled_methods.push((method.name.clone(), method.visibility, method.is_static, user_func));
                }

                // For interface "extends", all parent interfaces become the implements list
                self.class_defs.push(ClassDef {
                    name: name.clone(),
                    parent: None,
                    implements: extends.clone(),
                    is_interface: true,
                    is_abstract: false,
                    properties: vec![],
                    methods: compiled_methods,
                });
            }
        }
        Ok(())
    }

    /// Evaluate a constant expression at compile time (for property defaults).
    /// Returns Err for expressions that cannot be resolved at compile time.
    fn eval_const_expr(expr: &Expr) -> Result<Value, String> {
        match expr {
            Expr::Integer(n) => Ok(Value::long(*n)),
            Expr::Float(f) => Ok(Value::double(*f)),
            Expr::StringLiteral(s) => Ok(Value::string(s.clone())),
            Expr::Bool(b) => Ok(Value::bool(*b)),
            Expr::Null => Ok(Value::null()),
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
                    let val = Self::eval_const_expr(&elem.value)?;
                    if let Some(key_expr) = &elem.key {
                        let key = Self::eval_const_expr(key_expr)?;
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
    fn compile_params(func_compiler: &mut Compiler, params: &[Param], context: &str) -> Result<(u32, u32, bool, u32, u64), String> {
        let mut required_num_args = 0u32;
        let mut seen_default = false;
        let mut is_variadic = false;
        let mut variadic_cv_index = 0u32;
        let mut ref_args = 0u64;
        for (i, param) in params.iter().enumerate() {
            if param.is_ref && i < 64 {
                ref_args |= 1u64 << i;
            }
            if param.is_variadic {
                if i != params.len() - 1 {
                    return Err(format!("Variadic parameter ${} must be last in {}", param.name, context));
                }
                is_variadic = true;
                variadic_cv_index = func_compiler.resolve_cv(&param.name);
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
        Ok((num_args, required_num_args, is_variadic, variadic_cv_index, ref_args))
    }

    /// Emit default parameter initialization for a single param.
    /// Pattern: BindDefaultParam (skip if arg passed) → compute default → AssignCv → label
    fn emit_default_param(compiler: &mut Compiler, cv_idx: u32, default_expr: &Expr) {
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
        let skip_label = compiler.instructions.len() as u32;
        compiler.instructions[bind_idx].op2 = skip_label;
    }

    /// Compile expression. Returns (operand_index, OpType).
    fn compile_expr(&mut self, expr: &Expr) -> (u32, OpType) {
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
                        let false_label = self.instructions.len() as u32;
                        let false_lit = self.add_literal(Value::bool(false));
                        let mut set_false = Instruction::new(OpCode::AssignCv);
                        set_false.op1_type = OpType::Tmp;
                        set_false.op1 = tmp;
                        set_false.op2_type = OpType::Const;
                        set_false.op2 = false_lit;
                        self.instructions.push(set_false);

                        let end_label = self.instructions.len() as u32;
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
                        let true_label = self.instructions.len() as u32;
                        let true_lit = self.add_literal(Value::bool(true));
                        let mut set_true = Instruction::new(OpCode::AssignCv);
                        set_true.op1_type = OpType::Tmp;
                        set_true.op1 = tmp;
                        set_true.op2_type = OpType::Const;
                        set_true.op2 = true_lit;
                        self.instructions.push(set_true);

                        let end_label = self.instructions.len() as u32;

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
                let else_label = self.instructions.len() as u32;
                let (else_op, else_type) = self.compile_expr(else_expr);
                let mut set_else = Instruction::new(OpCode::AssignCv);
                set_else.op1_type = OpType::Tmp;
                set_else.op1 = tmp;
                set_else.op2_type = else_type;
                set_else.op2 = else_op;
                self.instructions.push(set_else);

                let end_label = self.instructions.len() as u32;
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

                let end_label = self.instructions.len() as u32;
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
            Expr::FunctionCall { name, args } => {
                let ref_args = self.lookup_ref_args(name);
                let name_idx = self.add_literal(Value::string(name.clone()));

                let mut init = Instruction::new(OpCode::InitFcall);
                init.op1 = args.len() as u32;
                init.op2_type = OpType::Const;
                init.op2 = name_idx;
                self.instructions.push(init);

                for (i, arg) in args.iter().enumerate() {
                    let is_ref_param = i < 64 && (ref_args & (1u64 << i)) != 0;
                    let (operand, op_type) = self.compile_expr(arg);
                    // Use SendRef only when param is by-ref AND arg is a CV (variable)
                    let opcode = if is_ref_param && op_type == OpType::Cv {
                        OpCode::SendRef
                    } else {
                        OpCode::SendVal
                    };
                    let mut send = Instruction::new(opcode);
                    send.op1 = operand;
                    send.op1_type = op_type;
                    send.op2 = i as u32;
                    self.instructions.push(send);
                }

                let tmp = self.alloc_tmp();
                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                do_fcall.result = tmp;
                do_fcall.result_type = OpType::Tmp;
                self.instructions.push(do_fcall);

                (tmp, OpType::Tmp)
            }
            Expr::ArrayLiteral(elements) => {
                // Create empty array in a TMP
                let arr_tmp = self.alloc_tmp();
                let mut init = Instruction::new(OpCode::InitArray);
                init.result_type = OpType::Tmp;
                init.result = arr_tmp;
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
                    let end = self.instructions.len() as u32;
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
                let else_label = self.instructions.len() as u32;
                let (r_op, r_type) = self.compile_expr(right);
                let mut set_right = Instruction::new(OpCode::AssignCv);
                set_right.op1_type = OpType::Tmp;
                set_right.op1 = tmp;
                set_right.op2_type = r_type;
                set_right.op2 = r_op;
                self.instructions.push(set_right);

                let end_label = self.instructions.len() as u32;
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
                                let body_start = self.instructions.len() as u32;
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
                                let next = self.instructions.len() as u32;
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
                    let msg = self.add_literal(Value::string("Unhandled match case"));
                    let mut throw = Instruction::new(OpCode::Throw);
                    throw.op1 = msg;
                    throw.op1_type = OpType::Const;
                    self.instructions.push(throw);
                }

                let end_label = self.instructions.len() as u32;
                for patch in end_patches {
                    self.instructions[patch].op1 = end_label;
                }

                (result_tmp, OpType::Tmp)
            }
            Expr::Closure { params, use_vars, body } => {
                // Compile closure body into a separate function
                let mut func_compiler = Compiler::new();
                func_compiler.known_ref_args = self.build_known_ref_args();
                // params come first as CVs (args), then use_vars
                let compile_result = Self::compile_params(&mut func_compiler, params, "closure");
                let (num_args, required_num_args, is_variadic, variadic_cv, ref_args) = match compile_result {
                    Ok(r) => r,
                    Err(e) => {
                        self.deferred_error = Some(e);
                        (params.len() as u32, params.len() as u32, false, 0, 0)
                    }
                };
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

                let op_array = OpArray {
                    num_cvs: func_compiler.next_cv,
                    num_temps: func_compiler.next_tmp,
                    instructions: func_compiler.instructions,
                    literals: func_compiler.literals,
                    try_entries: func_compiler.try_entries,
                };
                let user_func = make_user_function_full(op_array, num_args, required_num_args, is_variadic, variadic_cv, ref_args);

                // Register closure as anonymous function with unique name
                let closure_name = format!("__closure_{}", CLOSURE_COUNTER.fetch_add(1, Ordering::Relaxed));
                self.functions.extend(func_compiler.functions);
                self.functions.push((closure_name.clone(), user_func));

                // Build closure as array: [function_name, use_val1, use_val2, ...]
                // At call time, InitDynamicCall unpacks this.
                let name_idx = self.add_literal(Value::string(closure_name));
                let tmp = self.alloc_tmp();

                // InitArray for closure descriptor
                let mut init_arr = Instruction::new(OpCode::InitArray);
                init_arr.result = tmp;
                init_arr.result_type = OpType::Tmp;
                self.instructions.push(init_arr);

                // First element: function name
                let mut add_name = Instruction::new(OpCode::AddArrayElement);
                add_name.op1 = tmp;
                add_name.op1_type = OpType::Tmp;
                add_name.op2 = name_idx;
                add_name.op2_type = OpType::Const;
                add_name.result_type = OpType::Unused; // auto-index
                self.instructions.push(add_name);

                // Add captured use_var values
                for v in use_vars {
                    let cv = self.resolve_cv(v);
                    let mut add_use = Instruction::new(OpCode::AddArrayElement);
                    add_use.op1 = tmp;
                    add_use.op1_type = OpType::Tmp;
                    add_use.op2 = cv;
                    add_use.op2_type = OpType::Cv;
                    add_use.result_type = OpType::Unused;
                    self.instructions.push(add_use);
                }

                (tmp, OpType::Tmp)
            }
            Expr::New { class_name, args } => {
                // Pre-compile arg expressions BEFORE NewObj so side effects
                // always execute, even when the class has no __construct.
                let compiled_args: Vec<(u32, OpType)> = args.iter()
                    .map(|arg| self.compile_expr(arg))
                    .collect();

                let name_idx = self.add_literal(Value::string(class_name.clone()));
                let tmp = self.alloc_tmp();
                let mut new_obj = Instruction::new(OpCode::NewObj);
                new_obj.op1 = name_idx;
                new_obj.op1_type = OpType::Const;
                new_obj.result = tmp;
                new_obj.result_type = OpType::Tmp;
                new_obj.extended_value = args.len() as u32;
                self.instructions.push(new_obj);

                // Send constructor args — offset by 1 because CV 0 is $this
                for (i, (op, op_type)) in compiled_args.iter().enumerate() {
                    let mut send = Instruction::new(OpCode::SendVal);
                    send.op1 = *op;
                    send.op1_type = *op_type;
                    send.op2 = (i + 1) as u32; // +1 to skip $this at CV 0
                    self.instructions.push(send);
                }

                // DoFcall to run __construct (VM skips if no constructor exists)
                let discard = self.alloc_tmp();
                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                do_fcall.result = discard;
                do_fcall.result_type = OpType::Tmp;
                self.instructions.push(do_fcall);

                (tmp, OpType::Tmp)
            }
            Expr::PropertyAccess { object, property } => {
                let (obj_op, obj_type) = self.compile_expr(object);
                let prop_idx = self.add_literal(Value::string(property.clone()));
                let tmp = self.alloc_tmp();
                let mut fetch = Instruction::new(OpCode::FetchObjR);
                fetch.op1 = obj_op;
                fetch.op1_type = obj_type;
                fetch.op2 = prop_idx;
                fetch.op2_type = OpType::Const;
                fetch.result = tmp;
                fetch.result_type = OpType::Tmp;
                self.instructions.push(fetch);
                (tmp, OpType::Tmp)
            }
            Expr::MethodCall { object, method, args } => {
                let (obj_op, obj_type) = self.compile_expr(object);
                let method_idx = self.add_literal(Value::string(method.clone()));

                let mut init = Instruction::new(OpCode::InitMethodCall);
                init.op1 = obj_op;
                init.op1_type = obj_type;
                init.op2 = method_idx;
                init.op2_type = OpType::Const;
                init.extended_value = args.len() as u32;
                self.instructions.push(init);

                for (i, arg) in args.iter().enumerate() {
                    let (op, op_type) = self.compile_expr(arg);
                    // Use SendVarEx for runtime ref_args check (method not known at compile time)
                    let opcode = if op_type == OpType::Cv { OpCode::SendVarEx } else { OpCode::SendVal };
                    let mut send = Instruction::new(opcode);
                    send.op1 = op;
                    send.op1_type = op_type;
                    send.op2 = (i + 1) as u32; // +1 to skip CV 0 ($this)
                    send.extended_value = i as u32; // param index for ref_args check
                    self.instructions.push(send);
                }

                let tmp = self.alloc_tmp();
                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                do_fcall.result = tmp;
                do_fcall.result_type = OpType::Tmp;
                self.instructions.push(do_fcall);

                (tmp, OpType::Tmp)
            }
            Expr::StaticCall { class_name, method, args } => {
                let class_idx = self.add_literal(Value::string(class_name.clone()));
                let method_idx = self.add_literal(Value::string(method.clone()));

                let mut init = Instruction::new(OpCode::InitStaticCall);
                init.op1 = class_idx;
                init.op1_type = OpType::Const;
                init.op2 = method_idx;
                init.op2_type = OpType::Const;
                init.extended_value = args.len() as u32;
                self.instructions.push(init);

                for (i, arg) in args.iter().enumerate() {
                    let (op, op_type) = self.compile_expr(arg);
                    let opcode = if op_type == OpType::Cv { OpCode::SendVarEx } else { OpCode::SendVal };
                    let mut send = Instruction::new(opcode);
                    send.op1 = op;
                    send.op1_type = op_type;
                    send.op2 = (i + 1) as u32; // +1: CV 0 is $this even for static methods
                    send.extended_value = i as u32; // param index for ref_args check
                    self.instructions.push(send);
                }

                let tmp = self.alloc_tmp();
                let mut do_fcall = Instruction::new(OpCode::DoFcall);
                do_fcall.result = tmp;
                do_fcall.result_type = OpType::Tmp;
                self.instructions.push(do_fcall);

                (tmp, OpType::Tmp)
            }
            Expr::StaticProperty { class_name, property } => {
                let class_idx = self.add_literal(Value::string(class_name.clone()));
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
                for (i, arg) in args.iter().enumerate() {
                    let (op, op_type) = self.compile_expr(arg);
                    let opcode = if op_type == OpType::Cv { OpCode::SendVarEx } else { OpCode::SendVal };
                    let mut send = Instruction::new(opcode);
                    send.op1 = op;
                    send.op1_type = op_type;
                    send.op2 = i as u32;
                    send.extended_value = i as u32; // param index for ref_args check
                    self.instructions.push(send);
                }

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
                let name_idx = self.add_literal(Value::string(class_name.clone()));
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
        }
    }

    fn add_literal(&mut self, val: Value) -> u32 {
        let idx = self.literals.len() as u32;
        self.literals.push(val);
        idx
    }

    fn resolve_cv(&mut self, name: &str) -> u32 {
        if let Some(&idx) = self.cv_table.get(name) {
            idx
        } else {
            let idx = self.next_cv;
            self.next_cv += 1;
            self.cv_table.insert(name.to_string(), idx);
            idx
        }
    }

    fn alloc_tmp(&mut self) -> u32 {
        let idx = self.next_tmp;
        self.next_tmp += 1;
        idx
    }
}
