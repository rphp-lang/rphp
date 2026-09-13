use rphp::compiler::{make_internal_function, make_user_function};
use rphp::runtime::ExecutorGlobals;
use rphp::value::Value;
use rphp::vm::execute::{VmError, execute};
use rphp::vm::frame::ExecuteData;
use rphp::vm::instruction::{Instruction, OpType};
use rphp::vm::opcode::OpCode;
use std::sync::{Arc, Mutex, atomic::Ordering};

struct Capture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn arm_timeout(
    call: *mut ExecuteData,
    _result: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // SAFETY: an active internal handler retains its suspended user caller.
    // The caller's current instruction must remain published during callbacks.
    unsafe {
        let caller = (*call).prev_execute_data;
        assert!(!caller.is_null());
        assert_eq!((*(*caller).opline).opcode, OpCode::DoFcall);
    }
    eg.vm_interrupt.store(true, Ordering::Relaxed);
    eg.timed_out.store(true, Ordering::Relaxed);
    Ok(())
}

fn timeout_after_callback(backedge: bool) {
    let mut code = rphp::compiler::compile::Compiler::new()
        .compile(&[])
        .unwrap()
        .main;
    let mut init = Instruction::new(OpCode::InitFcall);
    init.op2_type = OpType::Const;
    let call = Instruction::new(OpCode::DoFcall);
    let mut echo = Instruction::new(OpCode::Echo);
    echo.op1_type = OpType::Const;
    echo.op1 = 1;
    code.instructions = vec![init, call];
    if backedge {
        code.instructions.push(echo);
        let mut jump = Instruction::new(OpCode::Jmp);
        jump.op1 = 2;
        code.instructions.push(jump);
    } else {
        code.instructions.extend(std::iter::repeat_n(echo, 300));
        let mut ret = Instruction::new(OpCode::Return);
        ret.op1_type = OpType::Const;
        ret.op1 = 2;
        code.instructions.push(ret);
    }
    code.literals = vec![
        Value::string("arm_timeout"),
        Value::string("."),
        Value::null(),
    ];
    code.cache = vec![rphp::vm::instruction::InlineCache::empty(); code.instructions.len()];
    let main = make_user_function(code);
    let callback = make_internal_function(arm_timeout, 0, 0, vec![]);
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let mut eg = ExecutorGlobals::with_output(Box::new(Capture(bytes.clone())));
    eg.register_function("arm_timeout", &callback.common)
        .unwrap();
    let error = execute(&mut eg, &main).unwrap_err();
    assert!(matches!(error, VmError::Fatal(ref text) if text == "Maximum execution time exceeded"));
    assert_eq!(
        bytes.lock().unwrap().len(),
        if backedge { 127 } else { 254 }
    );
    assert!(!eg.vm_interrupt.load(Ordering::Relaxed));
    assert!(!eg.timed_out.load(Ordering::Relaxed));
}

#[test]
fn fallthrough_still_polls_exactly_every_256_instructions() {
    timeout_after_callback(false);
}

#[test]
fn taken_backedges_still_share_the_interrupt_counter() {
    timeout_after_callback(true);
}
