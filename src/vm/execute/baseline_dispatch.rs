// Kept in the execute module through include! so this structural split does not change visibility or code generation.

/// Inner execute loop — equivalent to zend_execute_ex.
fn execute_ex(eg: &mut ExecutorGlobals, initial_frame: *mut ExecuteData) -> Result<(), VmError> {
    let mut frame = initial_frame;
    let mut op_array = unsafe { (*frame).op_array() };
    let mut tick: u8 = 255; // First iteration checks immediately (wraps to 0)
    'vm: loop {
        // Batch interrupt check: every 256 opcodes instead of every opcode.
        // Placed at loop top so all `continue` paths also pass through it.
        tick = tick.wrapping_add(1);
        if tick == 0 {
            if eg.vm_interrupt.load(Ordering::Relaxed) {
                handle_interrupt(eg)?;
            }
        }

        let mut opline_ptr: *const Instruction = unsafe { (*frame).opline };
        let opline = unsafe { &*opline_ptr };
        stats::inc_opcode(opline.opcode as usize);

        // Check for pending return or exception after finally block ends
        let frame_pending = unsafe { (*frame).pending_return_after_finally };
        let check_finally = frame_pending || eg.exception.is_some();
        if check_finally {
            let current_ip = unsafe {
                (*frame).opline.offset_from(op_array.instructions.as_ptr()) as u32
            };
            let at_finally_end = op_array.try_entries.iter().any(|e| {
                e.finally_start != 0xFFFFFFFF && current_ip == e.finally_end
            });
            if at_finally_end {
                if frame_pending {
                    unsafe { (*frame).pending_return_after_finally = false; }
                    // Deferred return — pop frame now (return value already written)
                    let prev = unsafe { (*frame).prev_execute_data };
                    if prev.is_null() {
                        return Ok(());
                    }
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    frame = prev;
                    op_array = unsafe { (*frame).op_array() };
                    continue;
                } else {
                    // Real exception — re-enter throw/unwind to find outer handler
                    let pending = eg.exception.take().unwrap();
                    // Start from current frame (outer try/catch may be in same frame)
                    let mut search_frame = frame;
                    let mut found = false;
                    loop {
                        let sf_op_array = unsafe { (*search_frame).op_array() };
                        let sf_ip = unsafe {
                            (*search_frame).opline.offset_from(sf_op_array.instructions.as_ptr()) as u32
                        };
                        for entry in &sf_op_array.try_entries {
                            // Skip the entry whose finally we just finished
                            if entry.finally_start != 0xFFFFFFFF && sf_ip == entry.finally_end {
                                continue;
                            }
                            if sf_ip >= entry.try_start && sf_ip < entry.try_end {
                                // Unwind frames between current and search_frame
                                while frame != search_frame {
                                    let prev = unsafe { (*frame).prev_execute_data };
                                    eg.current_execute_data.set(prev);
                                    unsafe { cleanup_frame_slots(frame) };
                                    eg.vm_stack.pop_call_frame(frame);
                                    frame = prev;
                                }
                                let base_ptr = sf_op_array.instructions.as_ptr();
                                let matched_catch = entry.catches.iter().find(|c| {
                                    exception_matches_catch(&pending, &c.types, eg)
                                });
                                if let Some(catch) = matched_catch {
                                    let catch_cv_ptr = unsafe { (*search_frame).get_op_mut(catch.catch_cv, OpType::Cv) };
                                    unsafe { slot_set(catch_cv_ptr, pending.clone()) };
                                    unsafe { (*frame).opline = base_ptr.add(catch.catch_start as usize) };
                                } else if entry.finally_start != 0xFFFFFFFF {
                                    eg.exception = Some(pending.clone());
                                    unsafe { (*frame).opline = base_ptr.add(entry.finally_start as usize) };
                                }
                                found = true;
                                break;
                            }
                        }
                        if found { break; }
                        let prev = unsafe { (*search_frame).prev_execute_data };
                        if prev.is_null() { break; }
                        search_frame = prev;
                    }
                    if found {
                        op_array = unsafe { (*frame).op_array() };
                        continue;
                    }
                    // Propagate via eg.exception for re-entry boundary crossing
                    eg.exception = Some(pending);
                    return Ok(());
                }
            }
        }

        match opline.opcode {
            OpCode::AssignCv => {
                // ASSIGN_CV op1=CV(dest), op2=value, result=optional copy
                let val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let cloned = val.clone();
                let dest = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
                if opline.result_type != OpType::Unused {
                    // Need two copies: one for dest, one for result
                    unsafe { slot_set(dest, cloned.clone()) };
                    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                    unsafe { slot_set(result_ptr, cloned) };
                } else {
                    // Common path: just move the single clone into dest
                    unsafe { slot_set(dest, cloned) };
                }
            }

            OpCode::AssignConcat => {
                // $x .= expr: in-place string append
                // COW: if dest is sole owner, push_str in place (no allocation).
                // If shared, as_string_mut() detaches first.
                let rhs = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let dest = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
                let dest_ref = unsafe { &mut *dest };
                if dest_ref.value_type() == ValueType::String {
                    // Fast path: avoid echo_to_string() allocation when RHS is string
                    if rhs.value_type() == ValueType::String {
                        let rhs_s = rhs.as_str().unwrap();
                        let s = unsafe { dest_ref.as_string_mut().unwrap_unchecked() };
                        s.push_str(rhs_s);
                    } else {
                        let rhs_str = rhs.echo_to_string();
                        let s = unsafe { dest_ref.as_string_mut().unwrap_unchecked() };
                        s.push_str(&rhs_str);
                    }
                } else {
                    let lhs_str = dest_ref.echo_to_string();
                    let rhs_str = if rhs.value_type() == ValueType::String {
                        rhs.as_str().unwrap().to_string()
                    } else {
                        rhs.echo_to_string()
                    };
                    let mut new_s = lhs_str;
                    new_s.push_str(&rhs_str);
                    unsafe { slot_set(dest, Value::string(new_s)) };
                }
            }

            OpCode::Echo => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                if val.value_type() == ValueType::String {
                    // Fast path: string → write bytes directly, no allocation
                    eg.write_output(val.as_str().unwrap().as_bytes());
                } else if val.value_type() == ValueType::Long {
                    // Fast path: integer → stack-local write, no heap allocation
                    use std::io::Write;
                    let mut buf = [0u8; 20]; // i64 max is 19 digits + sign
                    let s = {
                        let mut cursor = std::io::Cursor::new(&mut buf[..]);
                        write!(cursor, "{}", unsafe { val.raw_long() }).unwrap();
                        cursor.position() as usize
                    };
                    eg.write_output(&buf[..s]);
                } else if val.value_type() == ValueType::Object {
                    if let Some(result) = call_magic_method(eg, val, "__tostring", &[])? {
                        let output = result.echo_to_string();
                        eg.write_output(output.as_bytes());
                    } else {
                        let output = val.echo_to_string();
                        eg.write_output(output.as_bytes());
                    }
                } else {
                    let output = val.echo_to_string();
                    eg.write_output(output.as_bytes());
                }
            }

            OpCode::Echo_String => {
                let value = unsafe {
                    &*proven_scalar_op_ptr(frame, op_array, opline.op1, opline.op1_type)
                };
                debug_assert_eq!(value.value_type(), ValueType::String);
                let string = unsafe { value.as_str().unwrap_unchecked() };
                eg.write_output(string.as_bytes());
            }

            OpCode::Echo_Long => {
                let value = unsafe {
                    &*proven_scalar_op_ptr(frame, op_array, opline.op1, opline.op1_type)
                };
                debug_assert_eq!(value.value_type(), ValueType::Long);
                use std::io::Write;
                let mut buffer = [0u8; 20];
                let length = {
                    let mut cursor = std::io::Cursor::new(&mut buffer[..]);
                    write!(cursor, "{}", unsafe { value.raw_long() }).unwrap();
                    cursor.position() as usize
                };
                eg.write_output(&buffer[..length]);
            }

            // ── Specialized arithmetic opcodes ──────────────────────────
            // Inline operand access: no get_op_ptr match, no ref check.
            // Fall through to general handler on non-Long operands.

            OpCode::Add_LongLong
            | OpCode::Sub_LongLong
            | OpCode::Mul_LongLong
            | OpCode::Mod_LongLong
            | OpCode::BitwiseXor_LongLong => {
                let left = unsafe {
                    &*proven_scalar_op_ptr(frame, op_array, opline.op1, opline.op1_type)
                };
                let right = unsafe {
                    &*proven_scalar_op_ptr(frame, op_array, opline.op2, opline.op2_type)
                };
                debug_assert_eq!(left.value_type(), ValueType::Long);
                debug_assert_eq!(right.value_type(), ValueType::Long);
                let lhs = unsafe { left.raw_long() };
                let rhs = unsafe { right.raw_long() };
                let result_ptr = unsafe {
                    (*frame).get_op_mut(opline.result as u32, opline.result_type)
                };
                match opline.opcode {
                    OpCode::Add_LongLong => match lhs.checked_add(rhs) {
                        Some(result) => unsafe { frame_tmp_set_long(frame, result_ptr, result) },
                        None => unsafe {
                            frame_tmp_set(
                                frame,
                                result_ptr,
                                Value::double(lhs as f64 + rhs as f64),
                            )
                        },
                    },
                    OpCode::Sub_LongLong => match lhs.checked_sub(rhs) {
                        Some(result) => unsafe { frame_tmp_set_long(frame, result_ptr, result) },
                        None => unsafe {
                            frame_tmp_set(
                                frame,
                                result_ptr,
                                Value::double(lhs as f64 - rhs as f64),
                            )
                        },
                    },
                    OpCode::Mul_LongLong => match lhs.checked_mul(rhs) {
                        Some(result) => unsafe { frame_tmp_set_long(frame, result_ptr, result) },
                        None => unsafe {
                            frame_tmp_set(
                                frame,
                                result_ptr,
                                Value::double(lhs as f64 * rhs as f64),
                            )
                        },
                    },
                    OpCode::Mod_LongLong => {
                        if rhs == 0 {
                            return Err(VmError::Fatal("Division by zero".into()));
                        }
                        let remainder = lhs.checked_rem(rhs).unwrap_or(0);
                        unsafe { frame_tmp_set_long(frame, result_ptr, remainder) };
                    }
                    OpCode::BitwiseXor_LongLong => unsafe {
                        frame_tmp_set_long(frame, result_ptr, lhs ^ rhs)
                    },
                    _ => unreachable!(),
                }
            }

            OpCode::Add_TmpTmp => {
                let base = frame as *const Value;
                let op1 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op1 as usize) };
                let op2 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op2 as usize) };
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    if let Some(sum) = l1.checked_add(l2) {
                        // Peek ahead: if next is Return consuming our result TMP,
                        // and frame is FastScalar with no heap — skip TMP write + Return dispatch.
                        // Write sum directly to caller's return_value, pop frame inline.
                        let next = unsafe { &*opline_ptr.add(1) };
                        if next.opcode == OpCode::Return
                            && next.op1_type == OpType::Tmp
                            && next.op1 == opline.result
                            && !unsafe { (*frame).has_heap_slots }
                        {
                            let return_target = unsafe { (*frame).return_value };
                            if !return_target.is_null() {
                                let prev = unsafe { (*frame).prev_execute_data };
                                if !prev.is_null() && unsafe { (*prev).has_heap_slots } {
                                    unsafe { std::ptr::drop_in_place(return_target) };
                                }
                                unsafe { Value::write_long(return_target, sum) };
                            }
                            stats::inc_return_fast();
                            let prev = unsafe { (*frame).prev_execute_data };
                            if prev.is_null() {
                                return Ok(());
                            }
                            if frame == initial_frame {
                                eg.current_execute_data.set(prev);
                                eg.vm_stack.pop_call_frame(frame);
                                return Ok(());
                            }
                            eg.current_execute_data.set(prev);
                            eg.vm_stack.pop_call_frame(frame);
                            frame = prev;
                            op_array = unsafe { (*frame).op_array() };
            
                            continue;
                        }
                        // Normal path: write to TMP
                        let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                        unsafe { frame_tmp_set_long(frame, result_ptr, sum) };
                    } else {
                        let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                        unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 + l2 as f64))
                        };
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 + d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for +".into()));
                }
            }

            OpCode::Add_CvTmp => {
                let base = frame as *const Value;
                let cv_ptr = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op1 as usize) };
                let op1 = if cv_ptr.is_reference() { unsafe { &*cv_ptr.as_ref_ptr() } } else { cv_ptr };
                let op2 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op2 as usize) };
                let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match l1.checked_add(l2) {
                        Some(sum) => unsafe { frame_tmp_set_long(frame, result_ptr, sum) },
                        None => unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 + l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 + d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for +".into()));
                }
            }

            OpCode::Sub_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    // Peek ahead: if next instruction is SendVal consuming our TMP result,
                    // write directly to the call arg slot and skip the SendVal dispatch.
                    let next = unsafe { &*opline_ptr.add(1) };
                    if next.opcode == OpCode::SendVal
                        && next.op1_type == OpType::Tmp
                        && next.op1 == opline.result
                    {
                        let call = unsafe { (*frame).call };
                        let dst = unsafe {
                            (call as *mut Value).add(CALL_FRAME_SLOTS + next.op2 as usize)
                        };
                        match l1.checked_sub(l2) {
                            Some(diff) => unsafe { Value::write_long(dst, diff) },
                            None => unsafe { dst.write(Value::double(l1 as f64 - l2 as f64)) },
                        }
                        // Skip SendVal: advance local ptr +1, loop bottom adds +1 → net +2
                        opline_ptr = unsafe { opline_ptr.add(1) };
                    } else {
                        let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                        match l1.checked_sub(l2) {
                            Some(diff) => unsafe { frame_tmp_set_long(frame, result_ptr, diff) },
                            None => unsafe {
                                frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 - l2 as f64))
                            },
                        }
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 - d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for -".into()));
                }
            }

            OpCode::Sub_TmpTmp => {
                let base = frame as *const Value;
                let op1 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op1 as usize) };
                let op2 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op2 as usize) };
                let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match l1.checked_sub(l2) {
                        Some(diff) => unsafe { frame_tmp_set_long(frame, result_ptr, diff) },
                        None => unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 - l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 - d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for -".into()));
                }
            }

            OpCode::IsSmaller_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, l1 < l2) };
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, d1 < d2) };
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, s1 < s2) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                }
            }

            OpCode::IsSmallerOrEqual_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, l1 <= l2) };
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, d1 <= d2) };
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, s1 <= s2) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                }
            }

            OpCode::IsEqual_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, l1 == l2) };
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, d1 == d2) };
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    unsafe { frame_tmp_set_bool(frame, result_ptr, s1 == s2) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                }
            }

            // ── Superinstructions: fused comparison + conditional jump ──
            // Eliminates TMP write/read and one dispatch cycle.
            // On fall-through, advances opline by 2 (skipping the dead JmpZ/JmpNZ).

            OpCode::JmpZ_Le_CvConst => {
                // Fused: IsSmallerOrEqual_CvConst + JmpZ
                // Jump to result if !(CV <= Const), else fall through (+2).
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let cmp_result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    l1 <= l2
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    d1 <= d2
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    s1 <= s2
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                };
                if !cmp_result {
                    unsafe { (*frame).opline = op_array.instructions().as_ptr().add(opline.result as usize) };
    
                    continue;
                }
                // Fall through: advance local +1, loop bottom adds +1 more → net +2
                opline_ptr = unsafe { opline_ptr.add(1) };

            }

            OpCode::JmpNZ_Le_CvConst => {
                // Fused: IsSmallerOrEqual_CvConst + JmpNZ
                // Jump to result if CV <= Const, else fall through (+2).
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let cmp_result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    l1 <= l2
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    d1 <= d2
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    s1 <= s2
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                };
                if cmp_result {
                    unsafe { (*frame).opline = op_array.instructions().as_ptr().add(opline.result as usize) };
    
                    continue;
                }
                opline_ptr = unsafe { opline_ptr.add(1) };

            }

            OpCode::JmpZ_Lt_CvConst => {
                // Fused: IsSmaller_CvConst + JmpZ
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let cmp_result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    l1 < l2
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    d1 < d2
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    s1 < s2
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                };
                if !cmp_result {
                    unsafe { (*frame).opline = op_array.instructions().as_ptr().add(opline.result as usize) };
    
                    continue;
                }
                opline_ptr = unsafe { opline_ptr.add(1) };

            }

            OpCode::JmpNZ_Lt_CvConst => {
                // Fused: IsSmaller_CvConst + JmpNZ
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let cmp_result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    l1 < l2
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    d1 < d2
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    s1 < s2
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                };
                if cmp_result {
                    unsafe { (*frame).opline = op_array.instructions().as_ptr().add(opline.result as usize) };

                    continue;
                }
                opline_ptr = unsafe { opline_ptr.add(1) };

            }

            OpCode::JmpZ_Eq_CvConst => {
                // Fused: IsEqual_CvConst + JmpZ
                // Jump to result if !(CV == Const), else fall through (+2).
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let cmp_result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    l1 == l2
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    d1 == d2
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    s1 == s2
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                };
                if !cmp_result {
                    unsafe { (*frame).opline = op_array.instructions().as_ptr().add(opline.result as usize) };

                    continue;
                }
                opline_ptr = unsafe { opline_ptr.add(1) };

            }

            OpCode::JmpNZ_Eq_CvConst => {
                // Fused: IsEqual_CvConst + JmpNZ
                // Jump to result if CV == Const, else fall through (+2).
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let cmp_result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    l1 == l2
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    d1 == d2
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    s1 == s2
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                };
                if cmp_result {
                    unsafe { (*frame).opline = op_array.instructions().as_ptr().add(opline.result as usize) };

                    continue;
                }
                opline_ptr = unsafe { opline_ptr.add(1) };

            }

            OpCode::Add => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match l1.checked_add(l2) {
                        Some(sum) => unsafe { frame_tmp_set_long(frame, result_ptr, sum) },
                        None => unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 + l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 + d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for +".into()));
                }
            }

            OpCode::Sub => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match l1.checked_sub(l2) {
                        Some(diff) => unsafe { frame_tmp_set_long(frame, result_ptr, diff) },
                        None => unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 - l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 - d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for -".into()));
                }
            }

            OpCode::Mul => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match l1.checked_mul(l2) {
                        Some(prod) => unsafe { frame_tmp_set_long(frame, result_ptr, prod) },
                        None => unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 * l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 * d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for *".into()));
                }
            }

            OpCode::Div => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    if d2 == 0.0 {
                        return Err(VmError::Fatal("Division by zero".into()));
                    }
                    // PHP: if both are long and divisible, result is long
                    if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                        if let Some(quotient) = l1.checked_div(l2) {
                            if l1.checked_rem(l2) == Some(0) {
                                unsafe { frame_tmp_set_long(frame, result_ptr, quotient) };
                            } else {
                                unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 / d2)) };
                            }
                        } else {
                            unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 / d2)) };
                        }
                    } else {
                        unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 / d2)) };
                    }
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for /".into()));
                }
            }

            OpCode::Mod => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    if l2 == 0 {
                        return Err(VmError::Fatal("Division by zero".into()));
                    }
                    let remainder = l1.checked_rem(l2).unwrap_or(0);
                    unsafe { frame_tmp_set_long(frame, result_ptr, remainder) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for %".into()));
                }
            }

            OpCode::Concat_StringString => {
                let left = unsafe {
                    &*proven_scalar_op_ptr(frame, op_array, opline.op1, opline.op1_type)
                };
                let right = unsafe {
                    &*proven_scalar_op_ptr(frame, op_array, opline.op2, opline.op2_type)
                };
                debug_assert_eq!(left.value_type(), ValueType::String);
                debug_assert_eq!(right.value_type(), ValueType::String);
                let lhs = unsafe { left.as_str().unwrap_unchecked() };
                let rhs = unsafe { right.as_str().unwrap_unchecked() };
                let mut concatenated = String::with_capacity(lhs.len() + rhs.len());
                concatenated.push_str(lhs);
                concatenated.push_str(rhs);
                let result_ptr = unsafe {
                    (*frame).get_op_mut(opline.result as u32, opline.result_type)
                };
                unsafe {
                    frame_tmp_set(frame, result_ptr, Value::string(concatenated))
                };
            }

            OpCode::Concat => {
                op_concat(eg, frame, op_array, opline)?;
            }

            OpCode::Spaceship => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let cmp = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    l1.cmp(&l2)
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    d1.partial_cmp(&d2).unwrap_or(std::cmp::Ordering::Equal)
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    s1.cmp(s2)
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for <=>".into()));
                };
                let val = match cmp {
                    std::cmp::Ordering::Less => -1i64,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                };
                unsafe { frame_tmp_set_long(frame, result_ptr, val) };
            }

            OpCode::Pow => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    if l2 >= 0 {
                        unsafe { frame_tmp_set_long(frame, result_ptr, l1.wrapping_pow(l2 as u32)) };
                    } else {
                        unsafe { frame_tmp_set(frame, result_ptr, Value::double((l1 as f64).powf(l2 as f64))) };
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1.powf(d2))) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for **".into()));
                }
            }

            OpCode::BitwiseAnd => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let l1 = op1.to_long_val();
                let l2 = op2.to_long_val();
                unsafe { frame_tmp_set_long(frame, result_ptr, l1 & l2) };
            }

            OpCode::BitwiseOr => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let l1 = op1.to_long_val();
                let l2 = op2.to_long_val();
                unsafe { frame_tmp_set_long(frame, result_ptr, l1 | l2) };
            }

            OpCode::BitwiseXor => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let l1 = op1.to_long_val();
                let l2 = op2.to_long_val();
                unsafe { frame_tmp_set_long(frame, result_ptr, l1 ^ l2) };
            }

            OpCode::ShiftLeft => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let l1 = op1.to_long_val();
                let l2 = op2.to_long_val();
                unsafe { frame_tmp_set_long(frame, result_ptr, l1.wrapping_shl(l2 as u32)) };
            }

            OpCode::ShiftRight => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let l1 = op1.to_long_val();
                let l2 = op2.to_long_val();
                unsafe { frame_tmp_set_long(frame, result_ptr, l1.wrapping_shr(l2 as u32)) };
            }

            OpCode::BitwiseNot => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                let l = val.to_long_val();
                unsafe { frame_tmp_set_long(frame, result_ptr, !l) };
            }

            OpCode::IsEqual | OpCode::IsNotEqual | OpCode::IsSmaller | OpCode::IsSmallerOrEqual => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match opline.opcode {
                        OpCode::IsEqual => l1 == l2,
                        OpCode::IsNotEqual => l1 != l2,
                        OpCode::IsSmaller => l1 < l2,
                        OpCode::IsSmallerOrEqual => l1 <= l2,
                        _ => unreachable!(),
                    }
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    match opline.opcode {
                        OpCode::IsEqual => s1 == s2,
                        OpCode::IsNotEqual => s1 != s2,
                        OpCode::IsSmaller => s1 < s2,
                        OpCode::IsSmallerOrEqual => s1 <= s2,
                        _ => unreachable!(),
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    match opline.opcode {
                        OpCode::IsEqual => d1 == d2,
                        OpCode::IsNotEqual => d1 != d2,
                        OpCode::IsSmaller => d1 < d2,
                        OpCode::IsSmallerOrEqual => d1 <= d2,
                        _ => unreachable!(),
                    }
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                };

                unsafe { frame_tmp_set_bool(frame, result_ptr, result) };
            }

            OpCode::IsIdentical | OpCode::IsNotIdentical => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let identical = values_identical(op1, op2);

                let result = match opline.opcode {
                    OpCode::IsIdentical => identical,
                    _ => !identical,
                };
                unsafe { frame_tmp_set_bool(frame, result_ptr, result) };
            }

            OpCode::Isset => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                let is_set = val.value_type() != ValueType::Undef && val.value_type() != ValueType::Null;
                unsafe { frame_tmp_set_bool(frame, result_ptr, is_set) };
            }

            OpCode::Cast => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                let casted = match opline.extended_value {
                    0 => Value::long(val.to_long_val()),    // (int)
                    1 => Value::double(val.to_float_val()), // (float)
                    2 => {                                   // (string)
                        if val.value_type() == ValueType::Object {
                            if let Some(result) = call_magic_method(eg, val, "__tostring", &[])? {
                                Value::string(result.echo_to_string())
                            } else {
                                Value::string(val.echo_to_string())
                            }
                        } else {
                            Value::string(val.echo_to_string())
                        }
                    }
                    3 => Value::bool(val.is_truthy()),      // (bool)
                    4 => {                                   // (array)
                        match val.value_type() {
                            ValueType::Array => val.clone(),
                            ValueType::Null | ValueType::Undef => Value::array(PhpArray::new()),
                            _ => {
                                let mut arr = PhpArray::new();
                                arr.push(val.clone());
                                Value::array(arr)
                            }
                        }
                    }
                    _ => val.clone(),
                };
                unsafe { frame_tmp_set(frame, result_ptr, casted) };
            }

            OpCode::BoolNot => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                let negated = !val.is_truthy();
                unsafe { frame_tmp_set_bool(frame, result_ptr, negated) };
            }

            OpCode::Jmp => {
                // op1 = absolute instruction index to jump to
                let target = opline.op1 as usize;
                unsafe {
                    (*frame).opline = op_array.instructions().as_ptr().add(target);
                }

                continue; // skip normal advance
            }

            #[cfg(feature = "quick-loops")]
            OpCode::QuickLongLoopJmp => {
                unsafe { execute_quick_loop_backedge(eg, frame, op_array, opline)? };
                continue;
            }

            #[cfg(not(feature = "quick-loops"))]
            OpCode::QuickLongLoopJmp => {
                let target = opline.op1 as usize;
                unsafe {
                    (*frame).opline = op_array.instructions().as_ptr().add(target);
                }
                continue;
            }

            OpCode::JmpZ => {
                // op1 = value to test, op2 = absolute jump target
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                if !val.is_truthy() {
                    let target = opline.op2 as usize;
                    unsafe {
                        (*frame).opline = op_array.instructions().as_ptr().add(target);
                    }
    
                    continue;
                }
                // Fall-through after JmpZ is also a block leader

            }

            OpCode::JmpNZ => {
                // op1 = value to test, op2 = absolute jump target
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                if val.is_truthy() {
                    let target = opline.op2 as usize;
                    unsafe {
                        (*frame).opline = op_array.instructions().as_ptr().add(target);
                    }
    
                    continue;
                }
                // Fall-through after JmpNZ is also a block leader

            }

            OpCode::DirectInternalCall1 => {
                // The handler ID is emitted from the same metadata used to
                // register the direct ABI. No function lookup, cache probe or
                // FunctionType check remains in this hot path.
                let argument = unsafe {
                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                };
                let Some(kind) = crate::builtin_metadata::DirectInternalKind::from_id(
                    opline.extended_value,
                ) else {
                    return Err(VmError::Fatal(
                        "Invalid direct internal handler ID".into(),
                    ));
                };
                let result = crate::stdlib::invoke_direct_internal1(kind, argument)?;

                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe {
                        (*frame).get_op_mut(opline.result as u32, opline.result_type)
                    };
                    if kind.result_may_need_cleanup() && opline.result_type == OpType::Tmp {
                        unsafe { frame_tmp_set(frame, result_ptr, result) };
                    } else {
                        // Scalar direct kinds always overwrite their own unique
                        // TMP, so its previous value is Undef or scalar too.
                        unsafe { result_ptr.write(result) };
                    }
                }
            }

            OpCode::DirectInternalCall2 => {
                let first = unsafe {
                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                };
                let second = unsafe {
                    &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)
                };
                let Some(kind) = crate::builtin_metadata::DirectInternalKind::from_id(
                    opline.extended_value,
                ) else {
                    return Err(VmError::Fatal(
                        "Invalid direct internal handler ID".into(),
                    ));
                };
                let result = crate::stdlib::invoke_direct_internal2(kind, first, second)?;

                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe {
                        (*frame).get_op_mut(opline.result as u32, opline.result_type)
                    };
                    if kind.result_may_need_cleanup() && opline.result_type == OpType::Tmp {
                        unsafe { frame_tmp_set(frame, result_ptr, result) };
                    } else {
                        unsafe { result_ptr.write(result) };
                    }
                }
            }

            OpCode::Strlen => {
                let argument = unsafe {
                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                };
                let length = crate::stdlib::direct_strlen_len(argument);

                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe {
                        (*frame).get_op_mut(opline.result as u32, opline.result_type)
                    };
                    if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
                        unsafe { Value::write_long(result_ptr, length) };
                    } else {
                        unsafe { slot_set(result_ptr, Value::long(length)) };
                    }
                }
            }

            OpCode::Strlen_Cv => {
                let argument = unsafe { (*frame).cv(opline.op1 as u32) };
                let length = crate::stdlib::direct_strlen_len(argument);

                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe {
                        (*frame).get_op_mut(opline.result as u32, opline.result_type)
                    };
                    debug_assert!(matches!(opline.result_type, OpType::Tmp | OpType::Var));
                    unsafe { Value::write_long(result_ptr, length) };
                }
            }

            OpCode::Strlen_String => {
                let argument = unsafe {
                    &*proven_scalar_op_ptr(frame, op_array, opline.op1, opline.op1_type)
                };
                debug_assert_eq!(argument.value_type(), ValueType::String);
                let length = unsafe { argument.as_str().unwrap_unchecked().len() as i64 };

                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe {
                        (*frame).get_op_mut(opline.result as u32, opline.result_type)
                    };
                    if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
                        unsafe { Value::write_long(result_ptr, length) };
                    } else {
                        unsafe { slot_set(result_ptr, Value::long(length)) };
                    }
                }
            }

            OpCode::CallUserFuncArray => {
                match op_call_user_func_array(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(new_frame, new_op_array) => {
                        frame = new_frame;
                        op_array = new_op_array;
                        continue;
                    }
                    ColdResult::Unhandled(thrown) => {
                        eg.exception = Some(thrown);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::InitUserCall => {
                // A one-argument call_user_func()/call_user_func_array() with
                // a simple argument compiles to an adjacent
                // InitUserCall + SendUser + DoFcall sequence. Once its runtime
                // callback names a pure direct-ABI internal function,
                // invoke that handler on the caller's borrowed value and skip
                // the callback frame and the next two VM dispatches entirely.
                let next = unsafe { &*opline_ptr.add(1) };
                let direct_shape = direct_user_calls_enabled()
                    && opline.extended_value == 1
                    && next.opcode == OpCode::SendUser
                    && next.extended_value == 0
                    && unsafe { (*opline_ptr.add(2)).opcode == OpCode::DoFcall };
                let mut initialized = false;

                if direct_shape {
                    let next2 = unsafe { &*opline_ptr.add(2) };
                    let callback_raw = unsafe {
                        &*(*frame).get_op_ptr(
                            opline.op1 as u32,
                            opline.op1_type,
                            op_array,
                        )
                    };
                    let callback = if callback_raw.is_reference() {
                        unsafe { &*callback_raw.as_ref_ptr() }
                    } else {
                        callback_raw
                    };
                    let direct_kind = callback.as_str().and_then(|name| {
                        crate::builtin_metadata::direct_internal_spec(name)
                            .filter(|spec| spec.required_args <= 1 && spec.max_args >= 1)
                            .map(|spec| spec.kind)
                    });

                    if let Some(kind) = direct_kind {
                        let argument = unsafe {
                            &*(*frame).get_op_ptr(
                                next.op1 as u32,
                                next.op1_type,
                                op_array,
                            )
                        };
                        let result = crate::stdlib::invoke_direct_internal1(kind, argument)?;
                        if next2.result_type != OpType::Unused {
                            let result_ptr = unsafe {
                                (*frame).get_op_mut(
                                    next2.result as u32,
                                    next2.result_type,
                                )
                            };
                            if matches!(next2.result_type, OpType::Tmp | OpType::Var) {
                                unsafe { frame_tmp_set(frame, result_ptr, result) };
                            } else {
                                unsafe { frame_slot_set(frame, result_ptr, result) };
                            }
                        }
                        // Loop-bottom advance adds one more instruction.
                        opline_ptr = unsafe { opline_ptr.add(2) };
                        initialized = true;
                    } else if let Some(resolved) =
                        resolve_user_call_at_opline(eg, frame, op_array, opline)
                    {
                        init_resolved_user_call(
                            eg,
                            frame,
                            opline.extended_value,
                            resolved,
                        );
                        initialized = true;
                    }
                }

                if !initialized {
                    match op_init_user_call(eg, frame, op_array, opline)? {
                        ColdResult::NewFrame(new_frame, new_op_array) => {
                            frame = new_frame;
                            op_array = new_op_array;
                            continue;
                        }
                        ColdResult::Unhandled(thrown) => {
                            eg.exception = Some(thrown);
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }

            OpCode::InitFcall => {
                // op1 = num_args
                // op2 = CONST index pointing to function name string
                // extended_value = CONST index of fallback name (for unqualified calls in namespace), 0 = no fallback

                // Inline cache: if we resolved this function before, reuse the pointer
                let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
                let cached = op_array.cache[ip].func;
                let func_ptr = if !cached.is_null() {
                    cached
                } else {
                    let name_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                    let name = name_val.as_str().unwrap_or_else(|| {
                        panic!("INIT_FCALL: op2 must be a string");
                    });
                    let func_ptr_opt = eg.find_function(name).or_else(|| {
                        if opline.extended_value != 0 {
                            let fallback_val = unsafe { &*(*frame).get_op_ptr(opline.extended_value, OpType::Const, op_array) };
                            if let Some(fallback_name) = fallback_val.as_str() {
                                return eg.find_function(fallback_name);
                            }
                        }
                        None
                    });
                    match func_ptr_opt {
                        Some(ptr) => {
                            // Cache for next time (don't cache failures — function may be defined later via include)
                            unsafe { (*(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache)).func = ptr; }
                            ptr
                        }
                        None => {
                            let err = make_error_value("Error", &format!("Call to undefined function {}()", name_val.as_str().unwrap_or("?")));
                            match throw_in_frame(eg, frame, err) {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue;
                                }
                                ThrowResult::Unhandled(thrown) => {
                                    eg.exception = Some(thrown);
                                    return Ok(());
                                }
                            }
                        }
                    }
                };

                let num_args = opline.op1 as u32;
                let common = unsafe { &*func_ptr };
                let mut scalar_plan_eligible = false;
                if common.fn_type == FunctionType::User
                    && num_args == common.sig.public_arity()
                {
                    let user = unsafe { &*(func_ptr as *const UserFunction) };
                    scalar_plan_eligible = user.composed_scalar_long_plan.is_some()
                        || user.scalar_double_plan.is_some();
                    if let Some(plan) = user.scalar_long_plan.as_deref() {
                        scalar_plan_eligible = true;
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            try_execute_direct_scalar_long_call(
                                frame,
                                op_array,
                                opline_ptr.add(1),
                                common,
                                plan,
                            )
                        } {
                            stats::inc_do_fcall_fast();
                            stats::inc_return_fast();
                            let count = common.call_count.get();
                            if count < u32::MAX {
                                common.call_count.set(count + 1);
                            }
                            unsafe {
                                complete_direct_scalar_long_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            continue 'vm;
                        }
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            try_execute_composed_scalar_long_call(
                                frame,
                                op_array,
                                opline_ptr,
                                func_ptr,
                                plan,
                            )
                        } {
                            unsafe {
                                complete_direct_scalar_long_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            continue 'vm;
                        }
                    }
                    if let Some(plan) = user.scalar_double_plan.as_deref() {
                        scalar_plan_eligible = true;
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            try_execute_direct_scalar_double_call(
                                frame,
                                op_array,
                                opline_ptr.add(1),
                                common,
                                plan,
                            )
                        } {
                            stats::inc_do_fcall_fast();
                            stats::inc_return_fast();
                            let count = common.call_count.get();
                            if count < u32::MAX {
                                common.call_count.set(count + 1);
                            }
                            unsafe {
                                complete_direct_scalar_double_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            continue 'vm;
                        }
                    }
                    if let Some(plan) = user.composed_scalar_double_plan.as_deref() {
                        scalar_plan_eligible = true;
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            try_execute_direct_composed_scalar_double_call(
                                eg,
                                frame,
                                op_array,
                                opline_ptr.add(1),
                                common,
                                user,
                                plan,
                            )
                        } {
                            unsafe {
                                complete_direct_scalar_double_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            continue 'vm;
                        }
                    }
                    if let Some(plan) = user.composed_scalar_long_plan.as_deref() {
                        scalar_plan_eligible = true;
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            try_execute_direct_composed_scalar_body_call(
                                eg,
                                frame,
                                op_array,
                                opline_ptr,
                                func_ptr,
                                user,
                                plan,
                            )
                        } {
                            unsafe {
                                complete_direct_scalar_long_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            continue 'vm;
                        }
                    }
                }

                let pending_call = unsafe { (*frame).call };
                let deferred = should_defer_scalar_call(opline, scalar_plan_eligible);
                let call = if deferred {
                    eg.pending_call_stack.push_deferred_scalar_call(
                        func_ptr,
                        num_args,
                        num_args,
                        frame,
                        pending_call,
                    )
                } else {
                    eg.vm_stack.push_call_frame(
                        func_ptr,
                        num_args,
                        num_args,
                        frame,
                        pending_call,
                    )
                };
                unsafe {
                    (*frame).call = call;
                }

                // Peek ahead: if next is Sub_CvConst whose result feeds SendVal,
                // inline the subtraction + arg write, skip 2 instructions.
                let next = unsafe { &*opline_ptr.add(1) };
                if next.opcode == OpCode::Sub_CvConst {
                    let next2 = unsafe { &*opline_ptr.add(2) };
                    if next2.opcode == OpCode::SendVal
                        && next2.op1_type == OpType::Tmp
                        && next2.op1 == next.result
                    {
                        let op1_cv = unsafe { (*frame).cv(next.op1 as u32) };
                        let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                        let op2 = &op_array.literals()[next.op2 as usize];
                        if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                            let dst = unsafe {
                                (call as *mut Value).add(CALL_FRAME_SLOTS + next2.op2 as usize)
                            };
                            match l1.checked_sub(l2) {
                                Some(diff) => unsafe { Value::write_long(dst, diff) },
                                None => unsafe { dst.write(Value::double(l1 as f64 - l2 as f64)) },
                            }
                            // Skip Sub_CvConst + SendVal: advance local +2, loop bottom adds +1 → net +3
                            opline_ptr = unsafe { opline_ptr.add(2) };
                        }
                    }
                } else if next.opcode == OpCode::SendVal
                    && unsafe { try_send_scalar_arg(frame, call, op_array, next) }
                {
                    // InitFcall + scalar SendVal: argument is already in the
                    // callee frame. Loop-bottom advance skips the SendVal.
                    opline_ptr = unsafe { opline_ptr.add(1) };
                }
            }

            OpCode::SendVal => {
                // Send value to pending call frame
                // op1 = value to send, op2 = argument number (0-based)
                let call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                let dst = unsafe {
                    (call as *mut Value).add(CALL_FRAME_SLOTS + opline.op2 as usize)
                };
                let source = unsafe {
                    (*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                };
                let common = unsafe { &*(*call).func };
                let borrowed = opline.op2 as u32 >= common.sig.this_offset
                    && unsafe {
                        try_init_borrowed_heap_arg(
                            call,
                            opline.op2 as u32 - common.sig.this_offset,
                            source,
                            dst,
                        )
                    };
                // For TMP/Var operands that are provably scalar (Long, Double, Bool, Null),
                // use raw 16-byte bitwise copy — no clone/drop overhead.
                // TMP values are consumed (not read again), so move semantics are valid.
                // IMPORTANT: heap types (String, Array, Object, Closure) and References
                // MUST go through clone to maintain refcount / avoid double-free.
                if borrowed {
                    // The destination deliberately remains outside the owned
                    // heap bitmap; cleanup must not decrement the caller's Rc.
                } else if opline.op1_type == OpType::Tmp || opline.op1_type == OpType::Var {
                    let src = unsafe {
                        (frame as *const Value).add(CALL_FRAME_SLOTS + opline.op1 as usize)
                    };
                    let src_val = unsafe { &*src };
                    if !src_val.needs_cleanup() && !src_val.is_reference() {
                        // Scalar TMP/Var: safe bitwise move
                        unsafe { Value::raw_copy(src, dst) };
                    } else {
                        // Heap or reference TMP/Var: must clone + mark callee heap bits
                        let cloned = src_val.clone();
                        unsafe { dst.write(cloned) };
                        unsafe {
                            (*call).has_heap_slots = true;
                            let total = (*call).num_cvs + (*call).num_temps;
                            if total <= 64 {
                                (*call).heap_bitmap |= 1u64 << opline.op2;
                            }
                        }
                    }
                } else {
                    let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                    let cloned = val.clone();
                    unsafe { dst.write(cloned) };
                    // Mark heap bit if needed
                    if unsafe { (*dst).needs_cleanup() } {
                        unsafe {
                            (*call).has_heap_slots = true;
                            let total = (*call).num_cvs + (*call).num_temps;
                            if total <= 64 {
                                (*call).heap_bitmap |= 1u64 << opline.op2;
                            }
                        }
                    }
                }
            }

            OpCode::SendRef => {
                // Send reference to caller's CV into callee frame
                // op1 = CV index in caller, op1_type must be CV
                // op2 = argument number in callee (0-based)
                debug_assert!(opline.op1_type == OpType::Cv);
                let caller_cv_ptr = unsafe {
                    let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
                    let raw_ptr = base.add(opline.op1 as usize);
                    // If caller's CV is itself a reference, forward the target
                    if (*raw_ptr).is_reference() {
                        (*raw_ptr).as_ref_ptr()
                    } else {
                        materialize_borrowed_slot(frame, raw_ptr);
                        raw_ptr
                    }
                };
                let call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                let arg_slot = unsafe { (*call).cv_mut(opline.op2 as u32) };
                unsafe { frame_slot_init(call, arg_slot as *mut Value, Value::reference(caller_cv_ptr)) };
            }

            OpCode::SendVarEx => {
                // Runtime-checked send: by-ref if callee expects it AND op1 is CV, else by-val
                // op2 = CV slot in callee, extended_value = parameter index for ref_args check
                let call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                let param_idx = opline.extended_value;
                let func_common = unsafe { &*(*call).func };
                let is_ref = func_common.sig.is_param_by_ref(param_idx);

                if is_ref && opline.op1_type == OpType::Cv {
                    // Same logic as SendRef
                    let caller_cv_ptr = unsafe {
                        let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
                        let raw_ptr = base.add(opline.op1 as usize);
                        if (*raw_ptr).is_reference() {
                            (*raw_ptr).as_ref_ptr()
                        } else {
                            materialize_borrowed_slot(frame, raw_ptr);
                            raw_ptr
                        }
                    };
                    let arg_slot = unsafe { (*call).cv_mut(opline.op2 as u32) };
                    unsafe { frame_slot_init(call, arg_slot as *mut Value, Value::reference(caller_cv_ptr)) };
                } else {
                    // Same logic as SendVal
                    let source = unsafe {
                        (*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                    };
                    let arg_slot = unsafe { (*call).cv_mut(opline.op2 as u32) };
                    if !unsafe {
                        try_init_borrowed_heap_arg(
                            call,
                            param_idx,
                            source,
                            arg_slot as *mut Value,
                        )
                    } {
                        let cloned = unsafe { (&*source).clone() };
                        unsafe { frame_slot_init(call, arg_slot as *mut Value, cloned) };
                    }
                }
            }

            OpCode::SendUser => {
                let call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                let func_common = unsafe { &*(*call).func };
                let destination_index = func_common
                    .sig
                    .param_cv_index(opline.extended_value);
                let value = unsafe {
                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                };
                // call_user_func forwards ordinary arguments by value. Follow
                // an existing reference for the read, but do not create a new
                // reference merely because the callback parameter is by-ref.
                let value = if value.is_reference() {
                    unsafe { &*value.as_ref_ptr() }
                } else {
                    value
                };
                let destination = unsafe { (*call).cv_mut(destination_index) };
                unsafe { frame_slot_init(call, destination as *mut Value, value.clone()) };
            }

            OpCode::SendNamed => {
                match op_send_named(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::DoFcall => {
                // Execute the pending call
                let mut call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                // Restore previous pending call from the chain
                unsafe { (*frame).call = (*call).call };

                // A non-contiguous pure-scalar call captured its arguments in a
                // compact activation. On success it never acquires body CVs or
                // TMPs; on any guard failure it becomes the ordinary ABI frame
                // and continues through the unchanged DoFcall implementation.
                if unsafe { (*call).deferred_scalar_call } {
                    call = unsafe {
                        resolve_deferred_scalar_call(
                            eg,
                            frame,
                            call,
                            opline,
                            opline_ptr,
                        )
                    };
                    if call.is_null() {
                        continue 'vm;
                    }
                }

                // ── FastScalar path: tightest call protocol ──
                // Preconditions guaranteed at compile time: fixed arity, no by-ref,
                // no variadics, no generator, no globals, no type hints, no return type.
                // Runtime: only check fn_type + plan + no pending edge cases.
                let func_common_fast = unsafe { &*(*call).func };

                // A compiler-proven pure binary recurrence can preserve the
                // PHP depth-first evaluation order with compact integer
                // activations. The already-created root frame supplies the
                // canonical argument ABI; all recursive descendants avoid it.
                if func_common_fast.fn_type == FunctionType::User
                    && unsafe { (*call).num_args } == 1
                    && !unsafe { (*call).named_args_used }
                    && eg.pending_invoke_this.is_none()
                    && eg.pending_named_variadic.is_empty()
                    && matches!(opline.result_type, OpType::Tmp | OpType::Var | OpType::Unused)
                {
                    let user = unsafe { &*((*call).func as *const UserFunction) };
                    if let Some(plan) = user.binary_long_recursion_plan.as_ref() {
                        let method_dispatch_matches = if let Some(method_name) = &plan.method_name {
                            let receiver = unsafe { (*call).cv(0) };
                            if let Some(object) = receiver.as_object() {
                                let full_name = format!("{}::{}", object.class_name, method_name);
                                drop(object);
                                eg.find_function(&full_name)
                                    .is_some_and(|resolved| resolved == unsafe { (*call).func })
                            } else {
                                false
                            }
                        } else {
                            true
                        };
                        let argument_cv = func_common_fast.sig.param_cv_index(0);
                        let argument = unsafe { (*call).cv(argument_cv) };
                        if method_dispatch_matches
                            && argument.value_type() == ValueType::Long
                            && !argument.is_reference()
                        {
                            let evaluated = execute_binary_long_recursion(
                                eg,
                                plan,
                                unsafe { argument.raw_long() },
                            );
                            match evaluated {
                                Ok(Some(result)) => {
                                    stats::inc_do_fcall_fast();
                                    stats::inc_return_fast();
                                    let count = func_common_fast.call_count.get();
                                    if count < u32::MAX {
                                        func_common_fast.call_count.set(count + 1);
                                    }
                                    if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
                                        let result_ptr = unsafe {
                                            (frame as *mut Value)
                                                .add(CALL_FRAME_SLOTS + opline.result as usize)
                                        };
                                        unsafe { frame_tmp_set_long(frame, result_ptr, result) };
                                    }
                                    if unsafe { (*call).has_heap_slots } {
                                        unsafe { cleanup_frame_slots(call) };
                                    }
                                    eg.vm_stack.pop_call_frame(call);
                                    unsafe { (*frame).opline = opline_ptr.add(1) };
                                    continue 'vm;
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    if unsafe { (*call).has_heap_slots } {
                                        unsafe { cleanup_frame_slots(call) };
                                    }
                                    eg.vm_stack.pop_call_frame(call);
                                    return Err(error);
                                }
                            }
                        }
                    }
                }

                // ── Fast path for fixed-signature internal functions ──
                // Internal handlers still receive their ordinary ExecuteData
                // frame, so this changes no stdlib ABI or argument ownership.
                // It only avoids the generic type/variadic/class validation
                // path when the constructor proved those features absent.
                if func_common_fast.fn_type == FunctionType::Internal
                    && func_common_fast.plan.call == CallStrategy::Fast
                    && eg.pending_invoke_this.is_none()
                    && eg.pending_named_variadic.is_empty()
                {
                    let num_args_fast = unsafe { (*call).num_args };
                    let arity_ok = num_args_fast >= func_common_fast.sig.required_num_args
                        && num_args_fast <= func_common_fast.sig.public_arity();
                    let required_args_present = !unsafe { (*call).named_args_used } || {
                        let mut all_present = true;
                        for i in 0..func_common_fast.sig.required_num_args {
                            let cv_idx = func_common_fast.sig.param_cv_index(i);
                            if unsafe { (*(*call).cv(cv_idx)).is_undef() } {
                                all_present = false;
                                break;
                            }
                        }
                        all_present
                    };

                    if arity_ok && required_args_present {
                        stats::inc_do_fcall_fast();
                        let return_value_ptr = match opline.result_type {
                            OpType::Tmp | OpType::Var => unsafe {
                                (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                            },
                            OpType::Unused => std::ptr::null_mut(),
                            _ => unsafe {
                                (*frame).get_op_mut(opline.result as u32, opline.result_type)
                            },
                        };
                        unsafe { (*call).return_value = return_value_ptr };

                        let internal = unsafe {
                            &*((*call).func as *const super::function::InternalFunction)
                        };
                        if !return_value_ptr.is_null() {
                            unsafe { std::ptr::drop_in_place(return_value_ptr) };
                        }
                        let handler_result = (internal.handler)(call, return_value_ptr, eg);
                        unsafe { cleanup_frame_slots(call) };
                        eg.vm_stack.pop_call_frame(call);

                        if let Some(exc) = eg.exception.take() {
                            match throw_in_frame(eg, frame, exc) {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue;
                                }
                                ThrowResult::Unhandled(thrown) => {
                                    eg.exception = Some(thrown);
                                    return Ok(());
                                }
                            }
                        }
                        if let Err(e) = handler_result {
                            return Err(e);
                        }

                        unsafe { (*frame).opline = opline_ptr.add(1) };
                        continue 'vm;
                    }
                }

                if func_common_fast.fn_type == FunctionType::User
                    && (func_common_fast.plan.call == CallStrategy::FastScalar
                        || (func_common_fast.plan.call == CallStrategy::FastTypedScalar
                            && opline._pad & CALL_FLAG_EXACT_SCALAR_ARGS != 0))
                    // Both scalar ABIs are fixed-arity here; the typed variant
                    // enters only when the compiler proved every supplied
                    // argument. The required public count is therefore the
                    // exact arity, excluding the hidden method `$this`.
                    && unsafe { (*call).num_args } == func_common_fast.sig.required_num_args
                    && eg.pending_invoke_this.is_none()
                    && eg.pending_named_variadic.is_empty()
                {
                    let has_hole = unsafe { (*call).named_args_used } && {
                        let mut hole = false;
                        for i in 0..func_common_fast.sig.public_arity() {
                            let cv_idx = func_common_fast.sig.param_cv_index(i);
                            if unsafe { (*(*call).cv(cv_idx)).is_undef() } {
                                hole = true;
                                break;
                            }
                        }
                        hole
                    };
                    if !has_hole {
                    stats::inc_do_fcall_fast();

                    // Function-level hotness tracking.
                    // Promotion uses can_promote_to_hot() — single source of truth.
                    let cc = func_common_fast.call_count.get();
                    if cc < u32::MAX { func_common_fast.call_count.set(cc + 1); }
                    if cc == FUNC_HOT_THRESHOLD && func_common_fast.hot_status.get() == HotStatus::Cold {
                        if func_common_fast.can_promote_to_hot() {
                            func_common_fast.hot_status.set(HotStatus::Hot);
                        }
                    }

                    let user = unsafe { &*((*call).func as *const UserFunction) };
                    let return_value_ptr = match opline.result_type {
                        OpType::Tmp | OpType::Var => unsafe {
                            (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                        },
                        OpType::Unused => std::ptr::null_mut(),
                        _ => unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) },
                    };
                    unsafe { (*call).return_value = return_value_ptr };
                    unsafe {
                        (*call).opline = user.op_array.instructions.as_ptr();
                        (*frame).opline = opline_ptr.add(1);
                    }
                    eg.current_execute_data.set(call);

                    // Hot executor dispatch: if callee is hot, run in specialized executor.
                    // On Completed: callee returned, frame popped — restore caller state.
                    // On Bailout: callee still active — switch to it in baseline loop.
                    if func_common_fast.hot_status.get() == HotStatus::Hot {
                        match super::hot::execute_hot_frame(eg, call)? {
                            super::hot::HotResult::Completed => {
                                // Callee done. eg.current_execute_data is our caller (frame).
                                // (*frame).opline was already set to DoFcall+1 above.
                                // op_array unchanged (same caller function).
                                continue;
                            }
                            super::hot::HotResult::Bailout => {
                                match super::hot::resume_after_long_comparison(eg, call)? {
                                    super::hot::HotResult::Completed => continue,
                                    super::hot::HotResult::Bailout => {
                                        func_common_fast.hot_status.set(HotStatus::Cold);
                                        // Callee bailed. It's the active frame with opline at bailout point.
                                        frame = eg.current_execute_data.get();
                                        op_array = unsafe { (*frame).op_array() };
                                        continue;
                                    }
                                }
                            }
                        }
                    } else {
                        // Cold function — baseline interpreter
                        frame = call;
                        op_array = unsafe { (*frame).op_array() };
                        continue;
                    }
                    }
                }

                // ── Fast path for simple user function calls ──
                if func_common_fast.fn_type == FunctionType::User
                    && matches!(
                        func_common_fast.plan.call,
                        CallStrategy::Fast | CallStrategy::FastTypedScalar
                    )
                    && eg.pending_invoke_this.is_none()
                    && eg.pending_named_variadic.is_empty()
                {
                    let num_args_fast = unsafe { (*call).num_args };
                    let user = unsafe { &*((*call).func as *const UserFunction) };
                    let mut has_required_holes = false;
                    if func_common_fast.sig.required_num_args > 0 {
                        for i in 0..func_common_fast.sig.required_num_args {
                            let cv_idx = func_common_fast.sig.param_cv_index(i);
                            let val = unsafe { &*(*call).cv(cv_idx) };
                            if val.is_undef() {
                                has_required_holes = true;
                                break;
                            }
                        }
                    }
                    if !user.op_array.is_generator
                        && !has_required_holes
                        && num_args_fast >= func_common_fast.sig.required_num_args
                        && num_args_fast <= func_common_fast.sig.public_arity()
                    {
                        let caller_strict = op_array.strict_types;
                        let type_ok = opline._pad & CALL_FLAG_EXACT_SCALAR_ARGS != 0
                            || unsafe {
                                compact_scalar_call_types_match(
                                    eg,
                                    call,
                                    func_common_fast,
                                    caller_strict,
                                )
                            };
                        if !type_ok {
                            // Fall through to full path for proper TypeError
                        } else {
                        stats::inc_do_fcall_fast();
                        let return_value_ptr = match opline.result_type {
                            OpType::Tmp | OpType::Var => unsafe {
                                (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                            },
                            OpType::Unused => std::ptr::null_mut(),
                            _ => unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) },
                        };
                        unsafe { (*call).return_value = return_value_ptr };
                        if user.op_array.may_access_globals
                            && (!op_array.main_scope_vars.is_empty() || !op_array.global_vars.is_empty())
                        {
                            let vars_to_sync = if !op_array.main_scope_vars.is_empty() {
                                &op_array.main_scope_vars
                            } else {
                                &op_array.global_vars
                            };
                            for (cv_idx, var_name) in vars_to_sync {
                                let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                                let val = unsafe { (*cv_ptr).clone() };
                                globals_set(&mut eg.globals, var_name, val);
                            }
                        }
                        unsafe {
                            (*call).opline = user.op_array.instructions.as_ptr();
                            (*frame).opline = opline_ptr.add(1);
                        }
                        // Function-level hotness tracking.
                        // Promotion uses can_promote_to_hot() — single source of truth.
                        let cc = func_common_fast.call_count.get();
                        if cc < u32::MAX { func_common_fast.call_count.set(cc + 1); }
                        if cc == FUNC_HOT_THRESHOLD && func_common_fast.hot_status.get() == HotStatus::Cold {
                            if func_common_fast.can_promote_to_hot() {
                                func_common_fast.hot_status.set(HotStatus::Hot);
                            }
                        }

                        eg.current_execute_data.set(call);

                        // Hot executor dispatch: Hot status implies eligible (promotion guard above).
                        if func_common_fast.hot_status.get() == HotStatus::Hot {
                            match super::hot::execute_hot_frame(eg, call)? {
                                super::hot::HotResult::Completed => {
                                    continue;
                                }
                                super::hot::HotResult::Bailout => {
                                    match super::hot::resume_after_long_comparison(eg, call)? {
                                        super::hot::HotResult::Completed => continue,
                                        super::hot::HotResult::Bailout => {
                                            func_common_fast.hot_status.set(HotStatus::Cold);
                                            frame = eg.current_execute_data.get();
                                            op_array = unsafe { (*frame).op_array() };
                                            continue;
                                        }
                                    }
                                }
                            }
                        } else {
                            frame = call;
                            op_array = unsafe { (*frame).op_array() };
                            continue;
                        }
                    } // else: type_ok
                    } // if arity/generator ok
                }

                // ── Full path (handles all edge cases) ──
                match execute_full_call(eg, frame, op_array, opline, opline_ptr, call)? {
                    ColdResult::Done => {
                        unsafe { (*frame).opline = opline_ptr.add(1) };
                        continue 'vm;
                    }
                    ColdResult::NewFrame(nf, no) => {
                        frame = nf;
                        op_array = no;
                        continue 'vm;
                    }
                    ColdResult::Unhandled(exc) => {
                        eg.exception = Some(exc);
                        return Ok(());
                    }
                    ColdResult::Continue => continue 'vm,
                    ColdResult::Return => return Ok(()),
                }
            }

            OpCode::PreInc => {
                // ++$var: increment CV in place, result = new value
                let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };
                let old = unsafe { &*cv_ptr };
                let new_val = if let Some(n) = old.as_long() {
                    match n.checked_add(1) {
                        Some(v) => Value::long(v),
                        None => Value::double(n as f64 + 1.0),
                    }
                } else if let Some(d) = old.to_double() {
                    Value::double(d + 1.0)
                } else {
                    Value::long(1) // PHP: null++ = 1
                };
                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                    unsafe { slot_set(result_ptr, new_val.clone()) };
                }
                unsafe { slot_set(cv_ptr, new_val) };
            }

            OpCode::PreDec => {
                let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };
                let old = unsafe { &*cv_ptr };
                if let Some(n) = old.as_long() {
                    let new_val = match n.checked_sub(1) {
                        Some(v) => Value::long(v),
                        None => Value::double(n as f64 - 1.0),
                    };
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                        unsafe { slot_set(result_ptr, new_val.clone()) };
                    }
                    unsafe { slot_set(cv_ptr, new_val) };
                } else if let Some(d) = old.to_double() {
                    let new_val = Value::double(d - 1.0);
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                        unsafe { slot_set(result_ptr, new_val.clone()) };
                    }
                    unsafe { slot_set(cv_ptr, new_val) };
                } else {
                    // PHP: null-- has no effect, value stays null
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                        unsafe { slot_set(result_ptr, Value::null()) };
                    }
                }
            }

            OpCode::PostInc => {
                // $var++: increment CV in place, result = old value
                let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };
                let old = unsafe { &*cv_ptr };
                let new_val = if let Some(n) = old.as_long() {
                    match n.checked_add(1) {
                        Some(v) => Value::long(v),
                        None => Value::double(n as f64 + 1.0),
                    }
                } else if let Some(d) = old.to_double() {
                    Value::double(d + 1.0)
                } else {
                    Value::long(1)
                };
                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                    unsafe { slot_set(result_ptr, old.clone()) };
                }
                unsafe { slot_set(cv_ptr, new_val) };
            }

            OpCode::PostDec => {
                let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };
                let old = unsafe { &*cv_ptr };
                if let Some(n) = old.as_long() {
                    let new_val = match n.checked_sub(1) {
                        Some(v) => Value::long(v),
                        None => Value::double(n as f64 - 1.0),
                    };
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                        unsafe { slot_set(result_ptr, old.clone()) };
                    }
                    unsafe { slot_set(cv_ptr, new_val) };
                } else if let Some(d) = old.to_double() {
                    let new_val = Value::double(d - 1.0);
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                        unsafe { slot_set(result_ptr, old.clone()) };
                    }
                    unsafe { slot_set(cv_ptr, new_val) };
                } else {
                    // PHP: null-- has no effect, value stays null
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                        unsafe { slot_set(result_ptr, Value::null()) };
                    }
                }
            }

            OpCode::InitArray => {
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                let capacity = opline.extended_value as usize;
                let array = if opline._pad & ARRAY_INIT_HASH_HINT != 0 {
                    PhpArray::with_hash_capacity(capacity)
                } else {
                    PhpArray::with_packed_capacity(capacity)
                };
                unsafe { slot_set(result_ptr, Value::array(array)) };
            }

            OpCode::AddArrayElement => {
                // op1 = array TMP, op2 = value, result = key (or Unused for auto-key)
                let val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let cloned_val = val.clone();
                let arr_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
                let arr = unsafe { &mut *arr_ptr };
                let php_arr = arr.as_array_mut().ok_or_else(|| {
                    VmError::Fatal("AddArrayElement: operand is not an array".into())
                })?;
                if opline.result_type != OpType::Unused {
                    let key_val = unsafe { &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array) };
                    match value_to_array_key_ref(key_val)? {
                        ArrayKeyRef::Int(key) => php_arr.set_int(key, cloned_val),
                        ArrayKeyRef::String(key) => {
                            if key_val.value_type() == ValueType::String {
                                php_arr.set_str_value(key_val, cloned_val);
                            } else {
                                php_arr.set_str(key, cloned_val);
                            }
                        }
                    }
                } else {
                    php_arr.push(cloned_val);
                }
            }

            OpCode::FetchDimR => {
                #[cfg(feature = "quick-loops")]
                if opline.extended_value != 0
                    && unsafe {
                        execute_quick_region_entry(eg, frame, op_array, opline)?
                    }
                {
                    continue;
                }

                // result = op1[op2]
                let arr_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let idx_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let Some(arr) = arr_val.as_array() {
                    let fetched = match value_to_array_key_ref(idx_val)? {
                        ArrayKeyRef::Int(key) => arr.get_int(key),
                        ArrayKeyRef::String(key) => {
                            let cache_ip = unsafe {
                                (opline as *const Instruction)
                                    .offset_from(op_array.instructions.as_ptr())
                                    as usize
                            };
                            unsafe {
                                cached_string_array_value(
                                    op_array,
                                    cache_ip,
                                    arr,
                                    key,
                                )
                            }
                        }
                    };
                    let val = fetched.cloned().unwrap_or(Value::null());
                    unsafe { slot_set(result_ptr, val) };
                } else if let Some(s) = arr_val.as_str() {
                    // String offset access: $s[0] — PHP strings are byte-oriented
                    let bytes = s.as_bytes();
                    if let Some(idx) = idx_val.as_long() {
                        let pos = if idx >= 0 {
                            idx as usize
                        } else {
                            let len = bytes.len() as i64;
                            let p = len + idx;
                            if p >= 0 { p as usize } else { usize::MAX }
                        };
                        let val = if pos < bytes.len() {
                            // Single byte as a string
                            Value::string(String::from(bytes[pos] as char))
                        } else {
                            Value::string("")
                        };
                        unsafe { slot_set(result_ptr, val) };
                    } else {
                        unsafe { slot_set(result_ptr, Value::null()) };
                    }
                } else {
                    unsafe { slot_set(result_ptr, Value::null()) };
                }
            }

            OpCode::AssignDim => {
                // op1[op2] = result (value source encoded in result/result_type)
                let idx_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let key = value_to_array_key(idx_val)?;
                let val = unsafe { &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array) };
                let cloned_val = val.clone();
                let arr_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
                let arr = unsafe { &mut *arr_ptr };
                // Auto-create array if variable is null/undef
                if arr.value_type() == ValueType::Null || arr.value_type() == ValueType::Undef {
                    unsafe { slot_set(arr_ptr, Value::array(PhpArray::new())) };
                    let arr = unsafe { &mut *arr_ptr };
                    arr.as_array_mut().unwrap().set(key, cloned_val);
                } else if let Some(php_arr) = arr.as_array_mut() {
                    php_arr.set(key, cloned_val);
                } else {
                    return Err(VmError::Fatal("Cannot use a scalar value as an array".into()));
                }
            }

            OpCode::ArrayPushOp => {
                // op1[] = op2
                let val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let cloned_val = val.clone();
                let arr_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
                let arr = unsafe { &mut *arr_ptr };
                // Auto-create array if variable is null/undef
                if arr.value_type() == ValueType::Null || arr.value_type() == ValueType::Undef {
                    unsafe { slot_set(arr_ptr, Value::array(PhpArray::new())) };
                    let arr = unsafe { &mut *arr_ptr };
                    arr.as_array_mut().unwrap().push(cloned_val);
                } else if let Some(php_arr) = arr.as_array_mut() {
                    php_arr.push(cloned_val);
                } else {
                    return Err(VmError::Fatal("[] operator not supported for non-array".into()));
                }
            }

            OpCode::UnsetDim => {
                // Remove key op2 from array op1
                let idx_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let key = value_to_array_key(idx_val)?;
                let arr_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
                let arr = unsafe { &mut *arr_ptr };
                match arr.value_type() {
                    ValueType::Array => {
                        arr.as_array_mut().unwrap().remove(&key);
                    }
                    ValueType::Undef | ValueType::Null => {
                        // PHP silently ignores unset on undef/null
                    }
                    _ => {
                        return Err(VmError::Fatal(
                            "Cannot unset offset in a non-array variable".into(),
                        ));
                    }
                }
            }

            OpCode::ForeachInit => {
                if op_foreach_init(eg, frame, op_array, opline)? {
                    continue;
                }
            }

            OpCode::ForeachNext => {
                op_foreach_next(eg, frame, op_array, opline)?;
            }

            OpCode::Throw => {
                match op_throw(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::NewObj => {
                if opline._pad & NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE != 0
                    && unsafe {
                        try_execute_virtual_object_array_pipeline(
                            eg,
                            frame,
                            op_array,
                            opline_ptr,
                        )
                    }
                    .is_some()
                {
                    continue;
                }
                match op_new_obj(eg, frame, op_array, opline)? {
                    ColdResult::Continue => { continue; }
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::FetchObjR => {
                if !try_cached_fetch_obj_r(frame, op_array, opline) {
                    op_fetch_obj_r_slow(eg, frame, op_array, opline)?;
                }
            }

            OpCode::AssignObjProp => {
                // ── Cache-hit fast path for public, non-enum, non-readonly properties ──
                let obj_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                if obj_val.value_type() == ValueType::Object {
                    let obj_class_id = unsafe { obj_val.object_class_id_unchecked() };
                    let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
                    let ic = &op_array.cache[ip];
                    // flags == 3: read-safe + write-safe declared property slot.
                    if ic.property_flags() == 3 && ic.class_id == obj_class_id && obj_class_id != 0 {
                        let val = unsafe { &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array) };
                        let cloned = val.clone();
                        unsafe {
                            obj_val.object_set_property_slot_unchecked(ic.property_slot(), cloned);
                        };
                    } else {
                        match op_assign_obj_prop(eg, frame, op_array, opline)? {
                            ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                            ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                            _ => {}
                        }
                    }
                } else {
                    match op_assign_obj_prop(eg, frame, op_array, opline)? {
                        ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                        ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                        _ => {}
                    }
                }
            }

            OpCode::AssignObjDim => {
                // $obj->prop[$key] = val
                let obj_ptr = unsafe { (*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let key_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let val = unsafe { &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array) };
                let prop_name_val = &op_array.literals[opline.extended_value as usize];
                let prop_name = prop_name_val.as_str().unwrap_or("").to_string();
                let key = key_val.clone();
                let new_val = val.clone();

                let arr_key = value_to_array_key(&key)?;
                let obj = unsafe { &*obj_ptr };
                if let Some(mut php_obj) = obj.as_object_mut() {
                    let caller_class = get_caller_class(frame, eg);
                    let receiver_in_scope = caller_class.as_ref().map_or(false, |cc| {
                        eg.class_is_a(&php_obj.class_name, cc)
                    });
                    let effective_caller = if receiver_in_scope { caller_class.as_deref() } else { None };
                    let storage_key = crate::runtime::resolve_property_key(eg, &php_obj.class_name, &prop_name, effective_caller);

                    if let Some(arr_val) = php_obj.get_property_mut(&storage_key) {
                        // Property exists — mutate the array in place
                        if let Some(arr) = arr_val.as_array_mut() {
                            arr.set(arr_key, new_val);
                        } else {
                            return Err(VmError::Fatal(format!(
                                "Cannot use object of type {} as array", php_obj.class_name
                            )));
                        }
                    } else {
                        // Property doesn't exist — create new array
                        let mut new_arr = crate::value::PhpArray::new();
                        new_arr.set(arr_key, new_val);
                        php_obj.set_property(&storage_key, Value::array(new_arr));
                    }
                } else {
                    return Err(VmError::Fatal("Attempt to assign property on non-object".into()));
                }
            }

            OpCode::InitMethodCall => {
                // ── Cache-hit fast path (inlined) ──
                // Most method calls hit the monomorphic inline cache.
                // Bypass the #[inline(never)] helper entirely on cache hit.
                let obj_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                if obj_val.value_type() == ValueType::Object {
                    let obj_class_id = unsafe { obj_val.object_class_id_unchecked() };
                    let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
                    let ic = &op_array.cache[ip];
                    if !ic.func.is_null()
                        && ic.class_id == obj_class_id
                        && obj_class_id != 0
                        && method_return_dispatch_contract_matches(
                            opline,
                            unsafe { &*ic.func },
                        )
                    {
                        let func_ptr = ic.func;
                        let common = unsafe { &*func_ptr };
                        let num_args = opline.extended_value;
                        let mut scalar_plan_eligible = false;

                        // Public monomorphic methods whose bodies do not use
                        // `$this` can consume adjacent scalar arguments through
                        // the same frame-free ABI as ordinary functions. The
                        // receiver/class cache above still provides normal PHP
                        // virtual-dispatch semantics.
                        if common.fn_type == FunctionType::User
                            && num_args == common.sig.public_arity()
                        {
                            let user = unsafe { &*(func_ptr as *const UserFunction) };

                            // Method cache bits classify compiler-proven
                            // property bodies. Handle those before consulting
                            // the more general scalar planners so their common
                            // cache-hit path pays only the guards it needs.
                            if ic.method_has_long_property_plan() {
                                if let Some(plan) = user.long_property_plan.as_deref() {
                                    if opline._pad & CALL_FLAG_DEFERRED_SCALAR_CANDIDATE != 0
                                        && unsafe {
                                            try_execute_composed_long_property_call(
                                                frame,
                                                op_array,
                                                opline_ptr,
                                                obj_val,
                                                user,
                                                plan,
                                            )
                                        }
                                    {
                                        continue 'vm;
                                    }
                                    let sends = unsafe { opline_ptr.add(1) };
                                    let do_fcall_ptr = unsafe { sends.add(num_args as usize) };
                                    let do_fcall = unsafe { &*do_fcall_ptr };
                                    if plan.public_args as u32 == num_args
                                        && do_fcall.opcode == OpCode::DoFcall
                                        && do_fcall.result_type == OpType::Unused
                                        && unsafe {
                                            try_execute_long_property_method(
                                                frame,
                                                op_array,
                                                obj_val,
                                                sends,
                                                plan,
                                                user,
                                            )
                                        }
                                    {
                                        record_scalar_call(common);
                                        unsafe { (*frame).opline = do_fcall_ptr.add(1) };
                                        continue 'vm;
                                    }
                                }
                            }
                            if ic.method_has_property_getter_plan() {
                                if let Some(plan) = user.property_getter_plan.as_ref() {
                                    let do_fcall_ptr = unsafe { opline_ptr.add(1) };
                                    if unsafe {
                                        try_execute_direct_property_getter(
                                            frame,
                                            obj_val,
                                            do_fcall_ptr,
                                            user,
                                            plan,
                                        )
                                    } {
                                        continue 'vm;
                                    }
                                }
                            }

                            if let Some(plan) = user.object_array_plan.as_deref() {
                                if opline._pad & CALL_FLAG_OBJECT_ARRAY_CONSUMERS != 0
                                    && unsafe {
                                        try_execute_direct_object_array_consumers(
                                            eg,
                                            frame,
                                            op_array,
                                            opline_ptr,
                                            obj_val,
                                            user,
                                            plan,
                                        )
                                    }
                                    .is_some()
                                {
                                    record_scalar_call(common);
                                    continue 'vm;
                                }
                                if let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_direct_object_array_call(
                                        eg,
                                        frame,
                                        op_array,
                                        obj_val,
                                        opline_ptr.add(1),
                                        user,
                                        plan,
                                    )
                                } {
                                    record_scalar_call(common);
                                    unsafe {
                                        complete_direct_object_array_call(
                                            frame,
                                            do_fcall_ptr,
                                            result,
                                        );
                                    }
                                    continue 'vm;
                                }
                            }

                            if let Some(plan) = user.object_long_plan.as_deref() {
                                scalar_plan_eligible = true;
                                if let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_direct_object_long_call(
                                        eg,
                                        frame,
                                        op_array,
                                        obj_val,
                                        opline_ptr.add(1),
                                        user,
                                        plan,
                                    )
                                } {
                                    record_scalar_call(common);
                                    unsafe {
                                        complete_direct_scalar_long_call(
                                            frame,
                                            do_fcall_ptr,
                                            result,
                                        );
                                    }
                                    continue 'vm;
                                }
                            }

                            scalar_plan_eligible =
                                scalar_plan_eligible
                                    || user.composed_scalar_long_plan.is_some()
                                    || user.long_property_plan.is_some()
                                    || user.scalar_double_plan.is_some();
                            if let Some(plan) = user.scalar_long_plan.as_deref() {
                                scalar_plan_eligible = true;
                                if let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_direct_scalar_long_call(
                                        frame,
                                        op_array,
                                        opline_ptr.add(1),
                                        common,
                                        plan,
                                    )
                                } {
                                    stats::inc_do_fcall_fast();
                                    stats::inc_return_fast();
                                    let count = common.call_count.get();
                                    if count < u32::MAX {
                                        common.call_count.set(count + 1);
                                    }
                                    unsafe {
                                        complete_direct_scalar_long_call(
                                            frame,
                                            do_fcall_ptr,
                                            result,
                                        );
                                    }
                                    continue 'vm;
                                }
                                if let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_composed_scalar_long_call(
                                        frame,
                                        op_array,
                                        opline_ptr,
                                        func_ptr,
                                        plan,
                                    )
                                } {
                                    unsafe {
                                        complete_direct_scalar_long_call(
                                            frame,
                                            do_fcall_ptr,
                                            result,
                                        );
                                    }
                                    continue 'vm;
                                }
                            }
                            if let Some(plan) = user.scalar_double_plan.as_deref() {
                                scalar_plan_eligible = true;
                                if let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_direct_scalar_double_call(
                                        frame,
                                        op_array,
                                        opline_ptr.add(1),
                                        common,
                                        plan,
                                    )
                                } {
                                    stats::inc_do_fcall_fast();
                                    stats::inc_return_fast();
                                    let count = common.call_count.get();
                                    if count < u32::MAX {
                                        common.call_count.set(count + 1);
                                    }
                                    unsafe {
                                        complete_direct_scalar_double_call(
                                            frame,
                                            do_fcall_ptr,
                                            result,
                                        );
                                    }
                                    continue 'vm;
                                }
                            }
                            if let Some(plan) = user.composed_scalar_double_plan.as_deref() {
                                scalar_plan_eligible = true;
                                if let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_direct_composed_scalar_double_call(
                                        eg,
                                        frame,
                                        op_array,
                                        opline_ptr.add(1),
                                        common,
                                        user,
                                        plan,
                                    )
                                } {
                                    unsafe {
                                        complete_direct_scalar_double_call(
                                            frame,
                                            do_fcall_ptr,
                                            result,
                                        );
                                    }
                                    continue 'vm;
                                }
                            }
                            if let Some(plan) = user.composed_scalar_long_plan.as_deref() {
                                scalar_plan_eligible = true;
                                if let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_direct_composed_scalar_body_call(
                                        eg,
                                        frame,
                                        op_array,
                                        opline_ptr,
                                        func_ptr,
                                        user,
                                        plan,
                                    )
                                } {
                                    unsafe {
                                        complete_direct_scalar_long_call(
                                            frame,
                                            do_fcall_ptr,
                                            result,
                                        );
                                    }
                                    continue 'vm;
                                }
                            }
                        }

                        let pending_call = unsafe { (*frame).call };
                        let deferred = should_defer_scalar_call(
                            opline,
                            scalar_plan_eligible,
                        );
                        let call = if deferred {
                            eg.pending_call_stack.push_deferred_scalar_call(
                                func_ptr,
                                num_args + 1,
                                num_args,
                                frame,
                                pending_call,
                            )
                        } else {
                            eg.vm_stack.push_call_frame(
                                func_ptr,
                                num_args + 1,
                                num_args,
                                frame,
                                pending_call,
                            )
                        };
                        unsafe {
                            (*frame).call = call;
                            if common.plan.borrow_this {
                                frame_set_borrowed_this(call, obj_val as *const Value);
                            } else {
                                frame_set_this(call, obj_val.clone());
                            }
                        }

                        // Bind the contiguous scalar argument prefix while the
                        // new frame is hot in registers. Nested argument
                        // expressions naturally stop the fusion.
                        let bound = unsafe {
                            bind_contiguous_scalar_args(
                                frame,
                                call,
                                op_array,
                                opline_ptr.add(1),
                                num_args,
                                common.supports_scalar_long_plan(),
                            )
                        };

                        // When the whole scalar argument prefix was bound,
                        // fold the adjacent DoFcall into this cache-hit method
                        // setup. This removes one more baseline dispatch from
                        // the ordinary `$object->method(...)` protocol.
                        if ic.method_fusion_eligible()
                            && bound == num_args as usize
                        {
                            let do_fcall_ptr = unsafe { opline_ptr.add(1 + bound) };
                            let do_fcall = unsafe { &*do_fcall_ptr };
                            let argument_contract_satisfied =
                                common.plan.call == CallStrategy::FastScalar
                                    || do_fcall._pad & CALL_FLAG_EXACT_SCALAR_ARGS != 0
                                    || unsafe {
                                        compact_scalar_call_types_match(
                                            eg,
                                            call,
                                            common,
                                            op_array.strict_types,
                                        )
                                    };
                            if do_fcall.opcode == OpCode::DoFcall
                                && argument_contract_satisfied
                            {
                                match execute_fast_scalar_method_call(
                                    eg,
                                    frame,
                                    call,
                                    func_ptr,
                                    do_fcall,
                                    do_fcall_ptr,
                                )? {
                                    ColdResult::Continue => continue 'vm,
                                    ColdResult::NewFrame(nf, no) => {
                                        frame = nf;
                                        op_array = no;
                                        continue 'vm;
                                    }
                                    _ => unreachable!(),
                                }
                            }
                        }
                        if bound != 0 {
                            opline_ptr = unsafe { opline_ptr.add(bound) };
                        }
                    } else {
                        // Cache miss — full resolution in cold helper
                        match op_init_method_call(eg, frame, op_array, opline)? {
                            ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                            ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                            _ => {}
                        }
                    }
                } else {
                    // Non-object — cold path (error or __invoke)
                    match op_init_method_call(eg, frame, op_array, opline)? {
                        ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                        ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                        _ => {}
                    }
                }
            }

            OpCode::InitStaticCall => {
                match op_init_static_call(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::InitDynamicCall => {
                op_init_dynamic_call(eg, frame, op_array, opline)?;
            }

            OpCode::FetchStaticProp => {
                op_fetch_static_prop(eg, frame, op_array, opline);
            }

            OpCode::Instanceof => {
                op_instanceof(eg, frame, op_array, opline);
            }

            OpCode::FetchConst => {
                op_fetch_const(eg, frame, op_array, opline)?;
            }

            OpCode::BindDefaultParam => {
                if op_bind_default_param(frame, op_array, opline) {
                    continue;
                }
            }

            OpCode::BindGlobal => {
                op_bind_global(eg, frame, op_array, opline);
            }

            OpCode::BindStatic => {
                op_bind_static(eg, frame, op_array, opline);
            }

            OpCode::Return => {
                let func_common_ret = unsafe { &*(*frame).func };

                // ── FastScalar return: tightest path ──
                // No return type check, no globals sync, no dirty_globals propagation.
                // Guaranteed by FastScalar invariant: no globals, no statics, no try/finally,
                // no return type, no generator, may_access_globals == false.
                if func_common_ret.plan.call == CallStrategy::FastScalar
                    && func_common_ret.plan.ret == ReturnStrategy::Fast
                    && eg.exception.is_none()
                {
                    stats::inc_return_fast();
                    if opline.op1_type != OpType::Unused {
                        let return_target = unsafe { (*frame).return_value };
                        if !return_target.is_null() {
                            let frame_no_heap = !unsafe { (*frame).has_heap_slots };
                            if frame_no_heap && opline.op1_type != OpType::Const {
                                let retval_ptr = unsafe {
                                    (frame as *const Value).add(CALL_FRAME_SLOTS + opline.op1 as usize)
                                };
                                let src = if opline.op1_type == OpType::Cv {
                                    let cv_val = unsafe { &*retval_ptr };
                                    if cv_val.is_reference() {
                                        unsafe { cv_val.as_ref_ptr() as *const Value }
                                    } else {
                                        retval_ptr
                                    }
                                } else {
                                    retval_ptr
                                };
                                let prev = unsafe { (*frame).prev_execute_data };
                                if !prev.is_null() && unsafe { (*prev).has_heap_slots } {
                                    unsafe { std::ptr::drop_in_place(return_target) };
                                }
                                unsafe { Value::raw_copy(src, return_target) };
                            } else {
                                let retval = unsafe {
                                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                                };
                                unsafe { mark_caller_heap_return(frame, retval) };
                                unsafe { slot_set(return_target, retval.clone()) };
                            }
                        }
                    }
                    let prev = unsafe { (*frame).prev_execute_data };
                    if prev.is_null() {
                        return Ok(());
                    }
                    // Recursive execute_ex boundary: callee done → return to caller's macro loop
                    if frame == initial_frame {
                        eg.current_execute_data.set(prev);
                        unsafe { cleanup_frame_slots(frame) };
                        eg.vm_stack.pop_call_frame(frame);
                        return Ok(());
                    }
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    frame = prev;
                    op_array = unsafe { (*frame).op_array() };
                    // No dirty_globals check: FastScalar callee never touches globals,
                    // and may_access_globals == false means no deeper callee did either.
    
                    continue;
                }

                // ── Fast return path ──
                // Single precomputed flag check replaces 6 runtime conditions.
                // ReturnStrategy::Fast = no globals, no statics, no return type, no try/finally, not generator.
                if func_common_ret.plan.ret == ReturnStrategy::Fast
                    && eg.exception.is_none()
                {
                    // Inline exact scalar validation before transferring the
                    // value. Complex hints use ReturnStrategy::Full.
                    let ret_hint = &func_common_ret.sig.return_type_hint;
                    let has_return_type = !matches!(ret_hint, ParamTypeHint::None | ParamTypeHint::Mixed);
                    let return_type_proven = known_scalar_satisfies_type_hint(
                        opline.known_result_type(),
                        ret_hint,
                        op_array.strict_types,
                    );
                    if has_return_type
                        && !return_type_proven
                        && opline.op1_type != OpType::Unused
                    {
                        let retval = unsafe {
                            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                        };
                        let type_ok = check_fast_scalar_type_hint(
                            retval,
                            ret_hint,
                            op_array.strict_types,
                        ) == Some(true);
                        if !type_ok {
                            let err = make_error_value("TypeError", &format!(
                                "Return value must be of type {}, {} returned",
                                ret_hint.display_name(),
                                retval.type_name()
                            ));
                            match throw_in_frame(eg, frame, err) {
                                ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                            }
                        }
                    }
                    stats::inc_return_fast();
                    if opline.op1_type != OpType::Unused {
                        let return_target = unsafe { (*frame).return_value };
                        if !return_target.is_null() {
                            // Scalar-frame fast path: if frame has no heap slots and
                            // operand is a slot (not Const), ALL values are scalar.
                            // Skip clone/needs_cleanup entirely — raw 16-byte copy.
                            let frame_no_heap = !unsafe { (*frame).has_heap_slots };
                            if frame_no_heap && opline.op1_type != OpType::Const {
                                let retval_ptr = unsafe {
                                    (frame as *const Value).add(CALL_FRAME_SLOTS + opline.op1 as usize)
                                };
                                // CV ref check: even in scalar frame, CV could be a ref.
                                // But for Fast return path, function has no by-ref params
                                // and no globals, so refs are rare. Check anyway for safety.
                                let src = if opline.op1_type == OpType::Cv {
                                    let cv_val = unsafe { &*retval_ptr };
                                    if cv_val.is_reference() {
                                        unsafe { cv_val.as_ref_ptr() as *const Value }
                                    } else {
                                        retval_ptr
                                    }
                                } else {
                                    retval_ptr
                                };
                                // Caller's target: drop old only if caller has heap slots.
                                let prev = unsafe { (*frame).prev_execute_data };
                                if !prev.is_null() && unsafe { (*prev).has_heap_slots } {
                                    unsafe { std::ptr::drop_in_place(return_target) };
                                }
                                unsafe { Value::raw_copy(src, return_target) };
                            } else {
                                let retval = unsafe {
                                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                                };
                                unsafe { mark_caller_heap_return(frame, retval) };
                                unsafe { slot_set(return_target, retval.clone()) };
                            }
                        }
                    }
                    let prev = unsafe { (*frame).prev_execute_data };
                    if prev.is_null() {
                        return Ok(());
                    }
                    // Recursive execute_ex boundary: callee done → return to caller's macro loop
                    if frame == initial_frame {
                        eg.current_execute_data.set(prev);
                        unsafe { cleanup_frame_slots(frame) };
                        eg.vm_stack.pop_call_frame(frame);
                        return Ok(());
                    }
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    frame = prev;
                    op_array = unsafe { (*frame).op_array() };
                    // Fast-return functions don't sync globals themselves, but a deeper
                    // callee (via full return) may have left dirty entries that need to
                    // propagate up to the main scope or a function with `global` bindings.
                    if !eg.dirty_globals.is_empty()
                        && (!op_array.main_scope_vars.is_empty() || !op_array.global_vars.is_empty())
                    {
                        let vars_to_check = if !op_array.main_scope_vars.is_empty() {
                            &op_array.main_scope_vars
                        } else {
                            &op_array.global_vars
                        };
                        for (cv_idx, var_name) in vars_to_check {
                            if eg.dirty_globals.contains(var_name) {
                                if let Some(val) = eg.globals.get(var_name) {
                                    let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                                    unsafe { slot_set(cv_ptr, val.clone()) };
                                }
                            }
                        }
                        eg.dirty_globals.clear();
                    }
    
                    continue;
                }

                // ── Full return path ──
                stats::inc_return_full();
                // Note: don't clear dirty_globals here — deeper callees may have set entries
                // that need to propagate up to the main scope. Clearing happens in the
                // caller's "after return" handler when it actually consumes the dirty set.
                if !op_array.global_vars.is_empty() {
                    for (cv_idx, var_name) in &op_array.global_vars {
                        let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                        let val = unsafe { (*cv_ptr).clone() };
                        globals_set(&mut eg.globals, var_name, val);
                        eg.dirty_globals.insert(var_name.clone());
                    }
                }
                if !op_array.static_vars.is_empty() {
                    let func_name = op_array.name.clone();
                    for (cv_idx, var_name) in &op_array.static_vars {
                        let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                        let val = unsafe { (*cv_ptr).clone() };
                        eg.static_vars.entry(func_name.clone())
                            .or_insert_with(HashMap::new)
                            .insert(var_name.clone(), val);
                    }
                }

                // ── Return type validation ──
                let func_common = unsafe { &*(*frame).func };
                let return_hint = &func_common.sig.return_type_hint;
                if !matches!(return_hint, crate::vm::function::ParamTypeHint::None) {
                    let has_explicit_value = opline.extended_value == 1;
                    match return_hint {
                        crate::vm::function::ParamTypeHint::Void => {
                            if has_explicit_value {
                                // Any explicit `return expr;` in a void function is an error,
                                // including `return null;` (PHP rejects it).
                                // Only bare `return;` (extended_value=0) is allowed.
                                let err = make_error_value("TypeError",
                                    "A void function must not return a value");
                                match throw_in_frame(eg, frame, err) {
                                    ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                    ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                                }
                            }
                            // bare "return;" is OK for void
                        }
                        crate::vm::function::ParamTypeHint::Never => {
                            let err = make_error_value("TypeError",
                                "A never-returning function must not return");
                            match throw_in_frame(eg, frame, err) {
                                ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                            }
                        }
                        hint => {
                            if opline.op1_type != OpType::Unused {
                                let retval = unsafe {
                                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                                };
                                let ret_callee_class = eg.declaring_class_of(unsafe { (*frame).func });
                                if !check_type_hint(retval, hint, eg, op_array.strict_types, ret_callee_class) {
                                    let err = make_error_value("TypeError", &format!(
                                        "Return value must be of type {}, {} returned",
                                        hint.display_name(),
                                        retval.type_name()
                                    ));
                                    match throw_in_frame(eg, frame, err) {
                                        ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                        ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                                    }
                                }
                            }
                        }
                    }
                }

                // Check if we're inside a try region with a finally block
                let current_ip = unsafe {
                    (*frame).opline.offset_from(op_array.instructions.as_ptr()) as u32
                };
                let mut need_finally: Option<u32> = None;
                for entry in &op_array.try_entries {
                    if current_ip >= entry.try_start && current_ip < entry.finally_end
                        && entry.finally_start != 0xFFFFFFFF
                        // Don't re-enter finally if we're already inside it
                        && current_ip < entry.finally_start
                    {
                        need_finally = Some(entry.finally_start);
                        break;
                    }
                }

                if let Some(finally_ip) = need_finally {
                    // Write return value now (so it's available after finally)
                    if opline.op1_type != OpType::Unused {
                        let retval = unsafe {
                            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                        };
                        let return_target = unsafe { (*frame).return_value };
                        if !return_target.is_null() {
                            unsafe { mark_caller_heap_return(frame, retval) };
                            unsafe { slot_set(return_target, retval.clone()) };
                        }
                    }
                    // Jump to finally; after finally ends, the pending return
                    // will be detected by the finally_end check
                    eg.exception = None; // no exception, just deferred return
                    let base_ptr = op_array.instructions.as_ptr();
                    unsafe { (*frame).opline = base_ptr.add(finally_ip as usize) };
                    // Mark that we need to return after finally completes (per-frame)
                    unsafe { (*frame).pending_return_after_finally = true; }
                    continue;
                }

                // If returning from inside a finally block while an exception
                // is pending, the return suppresses the exception (PHP semantics).
                if eg.exception.is_some() {
                    eg.exception = None;
                }

                // Generator return — save return value and mark completed
                if op_array.is_generator {
                    if let Some(gen_ref) = eg.active_generator.take() {
                        let mut gen_data = gen_ref.borrow_mut();
                        if opline.op1_type != OpType::Unused {
                            let retval = unsafe {
                                &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                            };
                            gen_data.return_value = retval.clone();
                        }
                        gen_data.state = crate::vm::generator::GeneratorState::Completed;
                        gen_data.value = Value::null();
                        gen_data.key = Value::null();
                        drop(gen_data);
                        eg.active_generator = Some(gen_ref);
                    }

                    let prev = unsafe { (*frame).prev_execute_data };
                    if prev.is_null() {
                        return Ok(());
                    }
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    frame = prev;
                    op_array = unsafe { (*frame).op_array() };
                    continue;
                }

                if opline.op1_type != OpType::Unused {
                    let retval = unsafe {
                        &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                    };
                    let return_target = unsafe { (*frame).return_value };
                    if !return_target.is_null() {
                        unsafe { mark_caller_heap_return(frame, retval) };
                        unsafe { slot_set(return_target, retval.clone()) };
                    }
                }

                let prev = unsafe { (*frame).prev_execute_data };
                if prev.is_null() {
                    return Ok(());
                }
                // Recursive execute_ex boundary: callee done → return to caller's macro loop
                if frame == initial_frame {
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    return Ok(());
                }

                eg.current_execute_data.set(prev);
                unsafe { cleanup_frame_slots(frame) };
                eg.vm_stack.pop_call_frame(frame);
                frame = prev;
                op_array = unsafe { (*frame).op_array() };
                // After callee returns, selectively re-read globals that the callee modified.
                // Only update caller CVs for variables the callee wrote back via `global` keyword.
                // This avoids overwriting by-ref modifications to other variables.
                if !eg.dirty_globals.is_empty() {
                    let vars_to_check = if !op_array.main_scope_vars.is_empty() {
                        &op_array.main_scope_vars
                    } else {
                        &op_array.global_vars
                    };
                    for (cv_idx, var_name) in vars_to_check {
                        if eg.dirty_globals.contains(var_name) {
                            if let Some(val) = eg.globals.get(var_name) {
                                let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                                unsafe { slot_set(cv_ptr, val.clone()) };
                            }
                        }
                    }
                    // Clear dirty set once consumed by a scope that tracks globals.
                    // Intermediate frames without main_scope_vars/global_vars leave
                    // dirty_globals intact so changes propagate up to main scope.
                    if !op_array.main_scope_vars.is_empty() || !op_array.global_vars.is_empty() {
                        eg.dirty_globals.clear();
                    }
                }

                continue;
            }

            OpCode::Yield => {
                match op_yield(eg, frame, op_array, opline)? {
                    ColdResult::Return => { return Ok(()); }
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    _ => {}
                }
            }

            OpCode::YieldFrom => {
                match op_yield_from(eg, frame, op_array, opline)? {
                    ColdResult::Return => { return Ok(()); }
                    ColdResult::Continue => { continue; }
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    ColdResult::Done => {}
                }
            }

            OpCode::GeneratorReturn => {
                return Err(VmError::Fatal("GeneratorReturn outside generator context".into()));
            }

            OpCode::Include => {
                if op_include(eg, frame, op_array, opline)? {
                    continue;
                }
                // Refresh op_array — include may have changed frame context.
                op_array = unsafe { (*frame).op_array() };
            }

            OpCode::CloneObj => {
                match op_clone_obj(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::CreateClosure => {
                op_create_closure(eg, frame, op_array, opline);
            }

            OpCode::ClosureUseVar => {
                op_closure_use_var(frame, op_array, opline);
            }

            OpCode::NullSafeCheck => {
                if op_nullsafe_check(eg, frame, op_array, opline)? {
                    continue;
                }
            }

            // All opcodes handled — new opcodes must be added above
        }

        // Advance to next instruction.
        // Use local opline_ptr to avoid redundant memory load of (*frame).opline.
        unsafe { (*frame).opline = opline_ptr.add(1); }
    }
}
