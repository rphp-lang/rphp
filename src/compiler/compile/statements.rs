impl Compiler {
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
                if let Expr::BinaryOp {
                    op: crate::parser::BinOp::Concat,
                    left,
                    right,
                } = expr
                {
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
            Stmt::Function {
                name,
                params,
                body,
                return_type,
                generic_params,
            } => {
                // Compile function body into a separate OpArray
                let mut func_compiler = self.child_compiler();
                func_compiler.known_ref_args = self.build_known_ref_args();
                let resolved_name = self.resolve_name(name);
                self.record_generic_declaration(
                    crate::generics::GenericDeclarationKind::Function,
                    resolved_name.clone(),
                    generic_params,
                    Some(params),
                    return_type.as_ref(),
                );
                func_compiler.current_function_name = resolved_name.clone();
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
                let cache = (0..func_compiler.instructions.len())
                    .map(|_| InlineCache::empty())
                    .collect();
                let may_access_globals = !func_compiler.global_vars.is_empty()
                    || func_compiler.instructions.iter().any(|i| {
                        matches!(
                            i.opcode,
                            OpCode::InitFcall
                                | OpCode::InitDynamicCall
                                | OpCode::InitUserCall
                                | OpCode::CallUserFuncArray
                                | OpCode::InitMethodCall
                                | OpCode::InitStaticCall
                                | OpCode::Include
                        )
                    });
                let nested_generic_declarations =
                    std::mem::take(&mut func_compiler.generic_declarations);
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
                let user_func = make_user_function_typed(
                    op_array,
                    cp.num_args,
                    cp.required_num_args,
                    cp.is_variadic,
                    cp.variadic_cv_index,
                    cp.ref_args,
                    cp.type_hints,
                    cp.param_names,
                    cp.return_type_hint,
                );

                // Collect any nested function declarations
                self.functions.extend(func_compiler.functions);
                self.generic_declarations
                    .extend(nested_generic_declarations);
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
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
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
            Stmt::Foreach {
                array,
                value_var,
                key_var,
                body,
            } => {
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
                next.op1 = arr_copy_tmp; // array copy
                next.op2_type = OpType::Tmp;
                next.op2 = pos_tmp; // position counter
                next.result_type = OpType::Tmp;
                next.result = done_tmp; // 0 if done, 1 if has entry
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
                                return Err(
                                    "unset() only supports simple variable array access".into()
                                );
                            }
                        }
                        _ => return Err("unset() requires a variable".into()),
                    }
                }
            }
            Stmt::TryCatch {
                try_body,
                catches,
                finally_body,
            } => {
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

                    let resolved_types: Vec<String> =
                        catch.types.iter().map(|t| self.resolve_name(t)).collect();
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
            Stmt::AssignProp {
                object,
                property,
                expr,
            } => {
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
            Stmt::AssignObjArrayDim {
                object,
                property,
                index,
                expr,
            } => {
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
            Stmt::Include {
                path,
                is_require,
                is_once,
            } => {
                let (path_op, path_type) = self.compile_expr(path);
                let mut instr = Instruction::new(OpCode::Include);
                instr.op1 = path_op;
                instr.op1_type = path_type;
                let mut flags: u32 = 0;
                if *is_require {
                    flags |= 1;
                }
                if *is_once {
                    flags |= 2;
                }
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
                if let Ok(ct_val) =
                    Self::eval_const_expr_with_constants(value, &self.known_constants)
                {
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
                    let func_name_idx =
                        self.add_literal(Value::string(self.current_function_name.clone()));
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
            Stmt::Class {
                name,
                parent,
                implements,
                is_abstract,
                is_final,
                uses,
                properties,
                methods,
                generic_params,
            } => {
                let resolved_class = self.resolve_name(name);
                self.record_generic_declaration(
                    crate::generics::GenericDeclarationKind::Class,
                    resolved_class.clone(),
                    generic_params,
                    None,
                    None,
                );
                // Compile class declaration — store class info as a literal
                // Each class method gets compiled like a function
                let mut compiled_methods = Vec::new();
                // Collect promoted properties from constructor
                let mut promoted_props: Vec<(String, Visibility, bool)> = Vec::new(); // (name, vis, is_readonly)
                for method in methods {
                    self.record_generic_declaration(
                        crate::generics::GenericDeclarationKind::Method,
                        format!("{}::{}", resolved_class, method.name),
                        &method.generic_params,
                        Some(&method.params),
                        method.return_type.as_ref(),
                    );
                    let mut func_compiler = self.child_compiler();
                    func_compiler.known_ref_args = self.build_known_ref_args();
                    // $this is always CV 0 in methods
                    func_compiler.resolve_cv("this");
                    let context = format!("method {}::{}", name, method.name);
                    let mut cp =
                        self.compile_params(&mut func_compiler, &method.params, &context)?;
                    cp.return_type_hint = self.convert_type_hint(&method.return_type);

                    // Constructor property promotion: generate $this->param = $param assignments
                    if method.name == "__construct" {
                        for param in &method.params {
                            if let Some((vis, is_ro)) = &param.promotion {
                                promoted_props.push((param.name.clone(), *vis, *is_ro));
                                // Generate: $this->paramName = $paramName;
                                let this_cv = 0u16; // $this is always CV 0
                                let param_cv = func_compiler.resolve_cv(&param.name);
                                let prop_name_idx =
                                    func_compiler.add_literal(Value::string(param.name.clone()));
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

                    let cache = (0..func_compiler.instructions.len())
                        .map(|_| InlineCache::empty())
                        .collect();
                    let may_access_globals = !func_compiler.global_vars.is_empty()
                        || func_compiler.instructions.iter().any(|i| {
                            matches!(
                                i.opcode,
                                OpCode::InitFcall
                                    | OpCode::InitDynamicCall
                                    | OpCode::InitUserCall
                                    | OpCode::CallUserFuncArray
                                    | OpCode::InitMethodCall
                                    | OpCode::InitStaticCall
                                    | OpCode::Include
                            )
                        });
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
                    compiled_methods.push((
                        method.name.clone(),
                        method.visibility,
                        method.is_static,
                        method.is_final,
                        user_func,
                    ));
                }

                // Evaluate property defaults (constant expressions only)
                let mut compiled_props: Vec<(String, Option<Value>, Visibility, String)> =
                    Vec::new();
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
                    compiled_props.push((
                        prop.name.clone(),
                        default,
                        prop.visibility,
                        name.clone(),
                    ));
                }

                // Add promoted properties
                for (pname, pvis, p_readonly) in &promoted_props {
                    compiled_props.push((pname.clone(), None, *pvis, name.clone()));
                    if *p_readonly {
                        readonly_props.push(pname.clone());
                    }
                }

                // Store class definition for runtime
                let resolved_parent = parent.as_ref().map(|p| self.resolve_name(p));
                let resolved_implements: Vec<String> =
                    implements.iter().map(|i| self.resolve_name(i)).collect();
                let resolved_uses: Vec<String> =
                    uses.iter().map(|u| self.resolve_name(u)).collect();
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
                    property_defaults: std::rc::Rc::from([]),
                    readonly_props,
                    methods: compiled_methods,
                    class_id: 0,
                });
            }
            Stmt::Interface {
                name,
                extends,
                methods,
                generic_params,
            } => {
                let resolved_iface = self.resolve_name(name);
                self.record_generic_declaration(
                    crate::generics::GenericDeclarationKind::Interface,
                    resolved_iface.clone(),
                    generic_params,
                    None,
                    None,
                );
                // Interface methods have no body — we still create stub UserFunctions
                // so they appear in the class_def for type checking, but they should
                // never be called directly (implementing class provides the body).
                let mut compiled_methods = Vec::new();
                for method in methods {
                    self.record_generic_declaration(
                        crate::generics::GenericDeclarationKind::Method,
                        format!("{}::{}", resolved_iface, method.name),
                        &method.generic_params,
                        Some(&method.params),
                        method.return_type.as_ref(),
                    );
                    // Create a minimal op_array that just returns null
                    let mut func_compiler = self.child_compiler();
                    func_compiler.known_ref_args = self.build_known_ref_args();
                    func_compiler.resolve_cv("this");
                    let context = format!("interface method {}::{}", name, method.name);
                    let mut cp =
                        self.compile_params(&mut func_compiler, &method.params, &context)?;
                    cp.return_type_hint = self.convert_type_hint(&method.return_type);
                    let null_idx = func_compiler.add_literal(Value::null());
                    let mut ret = Instruction::new(OpCode::Return);
                    ret.op1_type = OpType::Const;
                    ret.op1 = null_idx;
                    func_compiler.instructions.push(ret);

                    let cache = (0..func_compiler.instructions.len())
                        .map(|_| InlineCache::empty())
                        .collect();
                    let may_access_globals = !func_compiler.global_vars.is_empty()
                        || func_compiler.instructions.iter().any(|i| {
                            matches!(
                                i.opcode,
                                OpCode::InitFcall
                                    | OpCode::InitDynamicCall
                                    | OpCode::InitUserCall
                                    | OpCode::CallUserFuncArray
                                    | OpCode::InitMethodCall
                                    | OpCode::InitStaticCall
                                    | OpCode::Include
                            )
                        });
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
                    let user_func = make_user_function_typed(
                        op_array,
                        cp.num_args,
                        cp.required_num_args,
                        cp.is_variadic,
                        cp.variadic_cv_index,
                        cp.ref_args,
                        cp.type_hints,
                        cp.param_names,
                        cp.return_type_hint,
                    );
                    self.functions.extend(func_compiler.functions);
                    compiled_methods.push((
                        method.name.clone(),
                        method.visibility,
                        method.is_static,
                        false,
                        user_func,
                    ));
                }

                // For interface "extends", all parent interfaces become the implements list
                let resolved_extends: Vec<String> =
                    extends.iter().map(|e| self.resolve_name(e)).collect();
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
                    property_defaults: std::rc::Rc::from([]),
                    readonly_props: vec![],
                    methods: compiled_methods,
                    class_id: 0,
                });
            }
            Stmt::Trait {
                name,
                properties,
                methods,
                generic_params,
            } => {
                let resolved_trait = self.resolve_name(name);
                self.record_generic_declaration(
                    crate::generics::GenericDeclarationKind::Trait,
                    resolved_trait.clone(),
                    generic_params,
                    None,
                    None,
                );
                // Compile trait — very similar to class, but flagged as is_trait=true.
                // Trait methods get compiled exactly like class methods.
                let mut compiled_methods = Vec::new();
                for method in methods {
                    self.record_generic_declaration(
                        crate::generics::GenericDeclarationKind::Method,
                        format!("{}::{}", resolved_trait, method.name),
                        &method.generic_params,
                        Some(&method.params),
                        method.return_type.as_ref(),
                    );
                    let mut func_compiler = self.child_compiler();
                    func_compiler.known_ref_args = self.build_known_ref_args();
                    func_compiler.resolve_cv("this");
                    let context = format!("trait method {}::{}", name, method.name);
                    let mut cp =
                        self.compile_params(&mut func_compiler, &method.params, &context)?;
                    cp.return_type_hint = self.convert_type_hint(&method.return_type);
                    for s in &method.body {
                        func_compiler.compile_stmt(s)?;
                    }
                    let null_idx = func_compiler.add_literal(Value::null());
                    let mut ret = Instruction::new(OpCode::Return);
                    ret.op1_type = OpType::Const;
                    ret.op1 = null_idx;
                    func_compiler.instructions.push(ret);

                    let cache = (0..func_compiler.instructions.len())
                        .map(|_| InlineCache::empty())
                        .collect();
                    let may_access_globals = !func_compiler.global_vars.is_empty()
                        || func_compiler.instructions.iter().any(|i| {
                            matches!(
                                i.opcode,
                                OpCode::InitFcall
                                    | OpCode::InitDynamicCall
                                    | OpCode::InitUserCall
                                    | OpCode::CallUserFuncArray
                                    | OpCode::InitMethodCall
                                    | OpCode::InitStaticCall
                                    | OpCode::Include
                            )
                        });
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
                    compiled_methods.push((
                        method.name.clone(),
                        method.visibility,
                        method.is_static,
                        method.is_final,
                        user_func,
                    ));
                }

                let mut compiled_props: Vec<(String, Option<Value>, Visibility, String)> =
                    Vec::new();
                for prop in properties {
                    let default = match &prop.default {
                        Some(expr) => Some(Self::eval_const_expr_with_constants(expr, &self.known_constants).map_err(|e| {
                            format!("Cannot use non-constant expression as default value for trait property {}::${}: {}", name, prop.name, e)
                        })?),
                        None => None,
                    };
                    compiled_props.push((
                        prop.name.clone(),
                        default,
                        prop.visibility,
                        name.clone(),
                    ));
                }

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
                    property_defaults: std::rc::Rc::from([]),
                    readonly_props: vec![],
                    methods: compiled_methods,
                    class_id: 0,
                });
            }
            Stmt::Enum {
                name,
                backing_type,
                cases,
                methods,
            } => {
                let resolved_enum = self.resolve_name(name);
                // Compile enum as a class. Each case becomes a static property
                // holding a singleton object with `name` (and optionally `value`) properties.
                let is_backed = backing_type.is_some();

                // Compile methods
                let mut compiled_methods = Vec::new();
                for method in methods {
                    self.record_generic_declaration(
                        crate::generics::GenericDeclarationKind::Method,
                        format!("{}::{}", resolved_enum, method.name),
                        &method.generic_params,
                        Some(&method.params),
                        method.return_type.as_ref(),
                    );
                    let mut func_compiler = self.child_compiler();
                    func_compiler.known_ref_args = self.build_known_ref_args();
                    func_compiler.resolve_cv("this");
                    let context = format!("enum method {}::{}", name, method.name);
                    let mut cp =
                        self.compile_params(&mut func_compiler, &method.params, &context)?;
                    cp.return_type_hint = self.convert_type_hint(&method.return_type);
                    for s in &method.body {
                        func_compiler.compile_stmt(s)?;
                    }
                    let null_idx = func_compiler.add_literal(Value::null());
                    let mut ret = Instruction::new(OpCode::Return);
                    ret.op1_type = OpType::Const;
                    ret.op1 = null_idx;
                    func_compiler.instructions.push(ret);

                    let cache = (0..func_compiler.instructions.len())
                        .map(|_| InlineCache::empty())
                        .collect();
                    let may_access_globals = !func_compiler.global_vars.is_empty()
                        || func_compiler.instructions.iter().any(|i| {
                            matches!(
                                i.opcode,
                                OpCode::InitFcall
                                    | OpCode::InitDynamicCall
                                    | OpCode::InitUserCall
                                    | OpCode::CallUserFuncArray
                                    | OpCode::InitMethodCall
                                    | OpCode::InitStaticCall
                                    | OpCode::Include
                            )
                        });
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
                    compiled_methods.push((
                        method.name.clone(),
                        method.visibility,
                        method.is_static,
                        method.is_final,
                        user_func,
                    ));
                }

                // Build properties for enum cases — each case is stored as a property
                // with a default value that is a PhpObject with name/value fields.
                // Static properties (cases) are stored as class properties with is_enum_case flag.
                let mut compiled_props: Vec<(String, Option<Value>, Visibility, String)> =
                    Vec::new();
                for (case_name, case_value) in cases {
                    use crate::value::{PhpArray, PhpObject};
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
                    compiled_props.push((
                        case_name.clone(),
                        Some(obj),
                        Visibility::Public,
                        name.clone(),
                    ));
                }

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
                    property_defaults: std::rc::Rc::from([]),
                    readonly_props: vec![],
                    methods: compiled_methods,
                    class_id: 0,
                });
            }
        }
        Ok(())
    }
}
