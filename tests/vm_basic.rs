use rphp::value::Value;
use rphp::vm::opcode::OpCode;
use rphp::vm::instruction::{Instruction, OpType};
use rphp::vm::execute;
use rphp::vm::frame::ExecuteData;
use rphp::vm::function::FunctionCommon;
use rphp::compiler::{OpArray, make_user_function, make_user_function_with_args, make_internal_function};
use rphp::runtime::ExecutorGlobals;

fn make_eg_with_capture() -> (ExecutorGlobals, std::sync::Arc<std::sync::Mutex<Vec<u8>>>) {
    let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let buf_clone = buf.clone();
    let writer = WriterCapture(buf_clone);
    let eg = ExecutorGlobals::with_output(Box::new(writer));
    (eg, buf)
}

struct WriterCapture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for WriterCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn captured_output(buf: &std::sync::Arc<std::sync::Mutex<Vec<u8>>>) -> String {
    String::from_utf8(buf.lock().unwrap().clone()).unwrap()
}

#[test]
fn test_echo_int() {
    let mut echo = Instruction::new(OpCode::Echo);
    echo.op1_type = OpType::Const;
    echo.op1 = 0;

    let mut ret = Instruction::new(OpCode::Return);
    ret.op1_type = OpType::Const;
    ret.op1 = 1;

    let op_array = OpArray {
        num_cvs: 0,
        num_temps: 0,
        instructions: vec![echo, ret],
        literals: vec![Value::long(42), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    };

    let main_func = make_user_function(op_array);
    let (mut eg, buf) = make_eg_with_capture();
    let result = execute::execute(&mut eg, &main_func);

    assert!(result.is_ok());
    assert_eq!(captured_output(&buf), "42");
}

#[test]
fn test_echo_negative() {
    let mut echo = Instruction::new(OpCode::Echo);
    echo.op1_type = OpType::Const;
    echo.op1 = 0;

    let mut ret = Instruction::new(OpCode::Return);
    ret.op1_type = OpType::Const;
    ret.op1 = 1;

    let op_array = OpArray {
        num_cvs: 0,
        num_temps: 0,
        instructions: vec![echo, ret],
        literals: vec![Value::long(-1), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    };

    let main_func = make_user_function(op_array);
    let (mut eg, buf) = make_eg_with_capture();
    execute::execute(&mut eg, &main_func).unwrap();
    assert_eq!(captured_output(&buf), "-1");
}

#[test]
fn test_add_and_echo() {
    // echo 20 + 22;
    // CONST(0)=20, CONST(1)=22, TMP(0) = result

    let mut add = Instruction::new(OpCode::Add);
    add.op1_type = OpType::Const;
    add.op1 = 0;
    add.op2_type = OpType::Const;
    add.op2 = 1;
    add.result_type = OpType::Tmp;
    add.result = 0;

    let mut echo = Instruction::new(OpCode::Echo);
    echo.op1_type = OpType::Tmp;
    echo.op1 = 0;

    let mut ret = Instruction::new(OpCode::Return);
    ret.op1_type = OpType::Const;
    ret.op1 = 2;

    let op_array = OpArray {
        num_cvs: 0,
        num_temps: 1,
        instructions: vec![add, echo, ret],
        literals: vec![Value::long(20), Value::long(22), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    };

    let main_func = make_user_function(op_array);
    let (mut eg, buf) = make_eg_with_capture();
    execute::execute(&mut eg, &main_func).unwrap();
    assert_eq!(captured_output(&buf), "42");
}

#[test]
fn test_overflow_to_float() {
    // i64::MAX + 1 → float
    let mut add = Instruction::new(OpCode::Add);
    add.op1_type = OpType::Const;
    add.op1 = 0;
    add.op2_type = OpType::Const;
    add.op2 = 1;
    add.result_type = OpType::Tmp;
    add.result = 0;

    let mut ret = Instruction::new(OpCode::Return);
    ret.op1_type = OpType::Tmp;
    ret.op1 = 0;

    let op_array = OpArray {
        num_cvs: 0,
        num_temps: 1,
        instructions: vec![add, ret],
        literals: vec![Value::long(i64::MAX), Value::long(1)],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    };

    let main_func = make_user_function(op_array);
    let (mut eg, _buf) = make_eg_with_capture();
    let result = execute::execute(&mut eg, &main_func).unwrap();
    assert!(result.as_double().is_some());
}

#[test]
fn test_timeout_interrupt() {
    use std::sync::atomic::Ordering;

    let mut echo = Instruction::new(OpCode::Echo);
    echo.op1_type = OpType::Const;
    echo.op1 = 0;

    let mut ret = Instruction::new(OpCode::Return);
    ret.op1_type = OpType::Const;
    ret.op1 = 1;

    let op_array = OpArray {
        num_cvs: 0,
        num_temps: 0,
        instructions: vec![echo, ret],
        literals: vec![Value::long(1), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    };

    let main_func = make_user_function(op_array);
    let (mut eg, _buf) = make_eg_with_capture();

    // Simulate timeout
    eg.vm_interrupt.store(true, Ordering::Relaxed);
    eg.timed_out.store(true, Ordering::Relaxed);

    let result = execute::execute(&mut eg, &main_func);
    assert!(result.is_err());
    match result.unwrap_err() {
        execute::VmError::Fatal(msg) => {
            assert!(msg.contains("execution time"));
        }
        _ => panic!("Expected Fatal error"),
    }
}

// === CV (variable) tests ===

#[test]
fn test_assign_and_echo_cv() {
    // $a = 42; echo $a;
    // ASSIGN_CV  CV(0), CONST(0)    ; $a = 42
    // ECHO       CV(0)              ; echo $a
    // RETURN     CONST(1)           ; return null

    let mut assign = Instruction::new(OpCode::AssignCv);
    assign.op1_type = OpType::Cv;
    assign.op1 = 0;
    assign.op2_type = OpType::Const;
    assign.op2 = 0;

    let mut echo = Instruction::new(OpCode::Echo);
    echo.op1_type = OpType::Cv;
    echo.op1 = 0;

    let mut ret = Instruction::new(OpCode::Return);
    ret.op1_type = OpType::Const;
    ret.op1 = 1;

    let op_array = OpArray {
        num_cvs: 1,
        num_temps: 0,
        instructions: vec![assign, echo, ret],
        literals: vec![Value::long(42), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    };

    let main_func = make_user_function(op_array);
    let (mut eg, buf) = make_eg_with_capture();
    execute::execute(&mut eg, &main_func).unwrap();
    assert_eq!(captured_output(&buf), "42");
}

#[test]
fn test_assign_add_echo() {
    // $a = 20; $b = 22; echo $a + $b;
    // ASSIGN_CV  CV(0), CONST(0)    ; $a = 20
    // ASSIGN_CV  CV(1), CONST(1)    ; $b = 22
    // ADD        CV(0), CV(1) -> TMP(0)
    // ECHO       TMP(0)
    // RETURN     CONST(2)

    let mut assign_a = Instruction::new(OpCode::AssignCv);
    assign_a.op1_type = OpType::Cv;
    assign_a.op1 = 0;
    assign_a.op2_type = OpType::Const;
    assign_a.op2 = 0;

    let mut assign_b = Instruction::new(OpCode::AssignCv);
    assign_b.op1_type = OpType::Cv;
    assign_b.op1 = 1;
    assign_b.op2_type = OpType::Const;
    assign_b.op2 = 1;

    let mut add = Instruction::new(OpCode::Add);
    add.op1_type = OpType::Cv;
    add.op1 = 0;
    add.op2_type = OpType::Cv;
    add.op2 = 1;
    add.result_type = OpType::Tmp;
    add.result = 0;

    let mut echo = Instruction::new(OpCode::Echo);
    echo.op1_type = OpType::Tmp;
    echo.op1 = 0;

    let mut ret = Instruction::new(OpCode::Return);
    ret.op1_type = OpType::Const;
    ret.op1 = 2;

    let op_array = OpArray {
        num_cvs: 2,
        num_temps: 1,
        instructions: vec![assign_a, assign_b, add, echo, ret],
        literals: vec![Value::long(20), Value::long(22), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    };

    let main_func = make_user_function(op_array);
    let (mut eg, buf) = make_eg_with_capture();
    execute::execute(&mut eg, &main_func).unwrap();
    assert_eq!(captured_output(&buf), "42");
}

// === Function call tests ===

#[test]
fn test_internal_function_call() {
    // Simulate: echo my_double(21);
    // my_double is an internal function that returns arg * 2

    fn my_double_handler(
        execute_data: *mut ExecuteData,
        return_value: *mut Value,
        _eg: &mut ExecutorGlobals,
    ) -> Result<(), rphp::vm::execute::VmError> {
        let arg = unsafe { (*execute_data).cv(0) };
        let val = arg.as_long().unwrap();
        if !return_value.is_null() {
            unsafe { return_value.write(Value::long(val * 2)) };
        }
        Ok(())
    }

    let my_double_func = make_internal_function(my_double_handler, 1, 1, vec!["value".to_string()]);

    // Main script:
    // INIT_FCALL 1, CONST(0)="my_double"
    // SEND_VAL   CONST(1)=21, arg_num=0
    // DO_FCALL   -> TMP(0)
    // ECHO       TMP(0)
    // RETURN     CONST(2)=null

    let mut init = Instruction::new(OpCode::InitFcall);
    init.op1 = 1; // num_args
    init.op2_type = OpType::Const;
    init.op2 = 0; // function name

    let mut send = Instruction::new(OpCode::SendVal);
    send.op1_type = OpType::Const;
    send.op1 = 1; // value = 21
    send.op2 = 0; // arg number

    let mut do_fcall = Instruction::new(OpCode::DoFcall);
    do_fcall.result_type = OpType::Tmp;
    do_fcall.result = 0;

    let mut echo = Instruction::new(OpCode::Echo);
    echo.op1_type = OpType::Tmp;
    echo.op1 = 0;

    let mut ret = Instruction::new(OpCode::Return);
    ret.op1_type = OpType::Const;
    ret.op1 = 2;

    let op_array = OpArray {
        num_cvs: 0,
        num_temps: 1,
        instructions: vec![init, send, do_fcall, echo, ret],
        literals: vec![Value::string("my_double"), Value::long(21), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    };

    let main_func = make_user_function(op_array);
    let (mut eg, buf) = make_eg_with_capture();
    eg.register_function("my_double", &my_double_func.common as *const FunctionCommon).unwrap();
    execute::execute(&mut eg, &main_func).unwrap();
    assert_eq!(captured_output(&buf), "42");
}

#[test]
fn test_user_function_call() {
    // User function: function add_one($x) { return $x + 1; }
    // Main: echo add_one(41);

    // --- add_one function ---
    // ADD CV(0), CONST(0)=1 -> TMP(0)
    // RETURN TMP(0)
    let mut add = Instruction::new(OpCode::Add);
    add.op1_type = OpType::Cv;
    add.op1 = 0;
    add.op2_type = OpType::Const;
    add.op2 = 0;
    add.result_type = OpType::Tmp;
    add.result = 0;

    let mut fn_ret = Instruction::new(OpCode::Return);
    fn_ret.op1_type = OpType::Tmp;
    fn_ret.op1 = 0;

    let fn_op_array = OpArray {
        num_cvs: 1,  // $x
        num_temps: 1,
        instructions: vec![add, fn_ret],
        literals: vec![Value::long(1)],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    };

    let add_one_func = make_user_function_with_args(fn_op_array, 1);

    // --- main script ---
    // INIT_FCALL 1, CONST(0)="add_one"
    // SEND_VAL CONST(1)=41, arg=0
    // DO_FCALL -> TMP(0)
    // ECHO TMP(0)
    // RETURN CONST(2)=null

    let mut init = Instruction::new(OpCode::InitFcall);
    init.op1 = 1;
    init.op2_type = OpType::Const;
    init.op2 = 0;

    let mut send = Instruction::new(OpCode::SendVal);
    send.op1_type = OpType::Const;
    send.op1 = 1;
    send.op2 = 0;

    let mut do_fcall = Instruction::new(OpCode::DoFcall);
    do_fcall.result_type = OpType::Tmp;
    do_fcall.result = 0;

    let mut echo = Instruction::new(OpCode::Echo);
    echo.op1_type = OpType::Tmp;
    echo.op1 = 0;

    let mut ret = Instruction::new(OpCode::Return);
    ret.op1_type = OpType::Const;
    ret.op1 = 2;

    let main_op_array = OpArray {
        num_cvs: 0,
        num_temps: 1,
        instructions: vec![init, send, do_fcall, echo, ret],
        literals: vec![Value::string("add_one"), Value::long(41), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    };

    let main_func = make_user_function(main_op_array);
    let (mut eg, buf) = make_eg_with_capture();
    eg.register_function("add_one", &add_one_func.common as *const FunctionCommon).unwrap();
    execute::execute(&mut eg, &main_func).unwrap();
    assert_eq!(captured_output(&buf), "42");
}

#[test]
fn test_undefined_function_error() {
    // Call to undefined function → VmError::Fatal
    let mut init = Instruction::new(OpCode::InitFcall);
    init.op1 = 0;
    init.op2_type = OpType::Const;
    init.op2 = 0;

    let mut do_fcall = Instruction::new(OpCode::DoFcall);
    do_fcall.result_type = OpType::Unused;

    let mut ret = Instruction::new(OpCode::Return);
    ret.op1_type = OpType::Const;
    ret.op1 = 1;

    let op_array = OpArray {
        num_cvs: 0,
        num_temps: 0,
        instructions: vec![init, do_fcall, ret],
        literals: vec![Value::string("nonexistent"), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    };

    let main_func = make_user_function(op_array);
    let (mut eg, _buf) = make_eg_with_capture();
    let result = execute::execute(&mut eg, &main_func);
    assert!(result.is_err());
    match result.unwrap_err() {
        execute::VmError::Fatal(msg) => {
            assert!(msg.contains("undefined function"));
        }
        _ => panic!("Expected Fatal error"),
    }
}

#[test]
fn test_nested_calls() {
    // $x = add_one(my_double(20)); echo $x;
    // my_double(20) → 40, add_one(40) → 41
    // Expected output: "41"

    // --- my_double: internal, returns arg * 2 ---
    fn my_double_handler(
        execute_data: *mut ExecuteData,
        return_value: *mut Value,
        _eg: &mut ExecutorGlobals,
    ) -> Result<(), rphp::vm::execute::VmError> {
        let arg = unsafe { (*execute_data).cv(0) };
        let val = arg.as_long().unwrap();
        if !return_value.is_null() {
            unsafe { return_value.write(Value::long(val * 2)) };
        }
        Ok(())
    }
    let my_double_func = make_internal_function(my_double_handler, 1, 1, vec!["value".to_string()]);

    // --- add_one: user function, return $x + 1 ---
    let mut add = Instruction::new(OpCode::Add);
    add.op1_type = OpType::Cv;
    add.op1 = 0;
    add.op2_type = OpType::Const;
    add.op2 = 0;
    add.result_type = OpType::Tmp;
    add.result = 0;

    let mut fn_ret = Instruction::new(OpCode::Return);
    fn_ret.op1_type = OpType::Tmp;
    fn_ret.op1 = 0;

    let add_one_func = make_user_function_with_args(OpArray {
        num_cvs: 1,
        num_temps: 1,
        instructions: vec![add, fn_ret],
        literals: vec![Value::long(1)],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    }, 1);

    // --- main script ---
    // INIT_FCALL 1, "my_double"       ; prepare inner call
    // SEND_VAL   CONST(1)=20, arg=0
    // DO_FCALL   -> TMP(0)            ; TMP(0) = 40
    // INIT_FCALL 1, "add_one"         ; prepare outer call
    // SEND_VAL   TMP(0), arg=0
    // DO_FCALL   -> TMP(1)            ; TMP(1) = 41
    // ASSIGN_CV  CV(0), TMP(1)        ; $x = 41
    // ECHO       CV(0)
    // RETURN     CONST(3)

    let mut init1 = Instruction::new(OpCode::InitFcall);
    init1.op1 = 1;
    init1.op2_type = OpType::Const;
    init1.op2 = 0; // "my_double"

    let mut send1 = Instruction::new(OpCode::SendVal);
    send1.op1_type = OpType::Const;
    send1.op1 = 1; // 20
    send1.op2 = 0;

    let mut do1 = Instruction::new(OpCode::DoFcall);
    do1.result_type = OpType::Tmp;
    do1.result = 0;

    let mut init2 = Instruction::new(OpCode::InitFcall);
    init2.op1 = 1;
    init2.op2_type = OpType::Const;
    init2.op2 = 2; // "add_one"

    let mut send2 = Instruction::new(OpCode::SendVal);
    send2.op1_type = OpType::Tmp;
    send2.op1 = 0; // TMP(0) = result of my_double
    send2.op2 = 0;

    let mut do2 = Instruction::new(OpCode::DoFcall);
    do2.result_type = OpType::Tmp;
    do2.result = 1;

    let mut assign = Instruction::new(OpCode::AssignCv);
    assign.op1_type = OpType::Cv;
    assign.op1 = 0;
    assign.op2_type = OpType::Tmp;
    assign.op2 = 1;

    let mut echo = Instruction::new(OpCode::Echo);
    echo.op1_type = OpType::Cv;
    echo.op1 = 0;

    let mut ret = Instruction::new(OpCode::Return);
    ret.op1_type = OpType::Const;
    ret.op1 = 3;

    let main_func = make_user_function(OpArray {
        num_cvs: 1,
        num_temps: 2,
        instructions: vec![init1, send1, do1, init2, send2, do2, assign, echo, ret],
        literals: vec![
            Value::string("my_double"),
            Value::long(20),
            Value::string("add_one"),
            Value::null(),
        ],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    });

    let (mut eg, buf) = make_eg_with_capture();
    eg.register_function("my_double", &my_double_func.common as *const FunctionCommon).unwrap();
    eg.register_function("add_one", &add_one_func.common as *const FunctionCommon).unwrap();
    execute::execute(&mut eg, &main_func).unwrap();
    assert_eq!(captured_output(&buf), "41");
}

#[test]
fn test_recursive_countdown() {
    // function countdown($n) {
    //     if ($n <= 0) { return 0; }
    //     echo $n;
    //     return countdown($n - 1);
    // }
    // echo countdown(3);  → prints "321" then "0"
    //
    // We don't have JmpZ yet, so we use an internal function instead:
    // Internal "countdown" that does the recursion in Rust but calls back
    // into the VM for each level via print.
    //
    // Simpler approach: user function that just calls itself with $n-1,
    // but we need conditional branching for that. So let's test with
    // an internal recursive helper.

    // Instead, let's just test 3-level deep user function calls:
    // f3 calls f2, f2 calls f1, f1 returns 42
    // echo f3() → "42"

    // f1: return 42
    let mut f1_ret = Instruction::new(OpCode::Return);
    f1_ret.op1_type = OpType::Const;
    f1_ret.op1 = 0;

    let f1_func = make_user_function(OpArray {
        num_cvs: 0,
        num_temps: 0,
        instructions: vec![f1_ret],
        literals: vec![Value::long(42)],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    });

    // f2: return f1()
    let mut f2_init = Instruction::new(OpCode::InitFcall);
    f2_init.op1 = 0;
    f2_init.op2_type = OpType::Const;
    f2_init.op2 = 0; // "f1"

    let mut f2_do = Instruction::new(OpCode::DoFcall);
    f2_do.result_type = OpType::Tmp;
    f2_do.result = 0;

    let mut f2_ret = Instruction::new(OpCode::Return);
    f2_ret.op1_type = OpType::Tmp;
    f2_ret.op1 = 0;

    let f2_func = make_user_function(OpArray {
        num_cvs: 0,
        num_temps: 1,
        instructions: vec![f2_init, f2_do, f2_ret],
        literals: vec![Value::string("f1")],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    });

    // f3: return f2()
    let mut f3_init = Instruction::new(OpCode::InitFcall);
    f3_init.op1 = 0;
    f3_init.op2_type = OpType::Const;
    f3_init.op2 = 0; // "f2"

    let mut f3_do = Instruction::new(OpCode::DoFcall);
    f3_do.result_type = OpType::Tmp;
    f3_do.result = 0;

    let mut f3_ret = Instruction::new(OpCode::Return);
    f3_ret.op1_type = OpType::Tmp;
    f3_ret.op1 = 0;

    let f3_func = make_user_function(OpArray {
        num_cvs: 0,
        num_temps: 1,
        instructions: vec![f3_init, f3_do, f3_ret],
        literals: vec![Value::string("f2")],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    });

    // main: echo f3()
    let mut main_init = Instruction::new(OpCode::InitFcall);
    main_init.op1 = 0;
    main_init.op2_type = OpType::Const;
    main_init.op2 = 0;

    let mut main_do = Instruction::new(OpCode::DoFcall);
    main_do.result_type = OpType::Tmp;
    main_do.result = 0;

    let mut main_echo = Instruction::new(OpCode::Echo);
    main_echo.op1_type = OpType::Tmp;
    main_echo.op1 = 0;

    let mut main_ret = Instruction::new(OpCode::Return);
    main_ret.op1_type = OpType::Const;
    main_ret.op1 = 1;

    let main_func = make_user_function(OpArray {
        num_cvs: 0,
        num_temps: 1,
        instructions: vec![main_init, main_do, main_echo, main_ret],
        literals: vec![Value::string("f3"), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    });

    let (mut eg, buf) = make_eg_with_capture();
    eg.register_function("f1", &f1_func.common as *const FunctionCommon).unwrap();
    eg.register_function("f2", &f2_func.common as *const FunctionCommon).unwrap();
    eg.register_function("f3", &f3_func.common as *const FunctionCommon).unwrap();
    execute::execute(&mut eg, &main_func).unwrap();
    assert_eq!(captured_output(&buf), "42");
}

#[test]
fn test_interrupt_during_deep_call() {
    // f1 echoes "1", f2 calls f1 then echoes "2"
    // Interrupt set before execution → should fail at first instruction of f2

    use std::sync::atomic::Ordering;

    // f1: echo CONST(0)=1; return null
    let mut f1_echo = Instruction::new(OpCode::Echo);
    f1_echo.op1_type = OpType::Const;
    f1_echo.op1 = 0;

    let mut f1_ret = Instruction::new(OpCode::Return);
    f1_ret.op1_type = OpType::Const;
    f1_ret.op1 = 1;

    let f1_func = make_user_function(OpArray {
        num_cvs: 0,
        num_temps: 0,
        instructions: vec![f1_echo, f1_ret],
        literals: vec![Value::long(1), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    });

    // main: init_fcall f1, do_fcall, echo "2", return
    let mut init = Instruction::new(OpCode::InitFcall);
    init.op1 = 0;
    init.op2_type = OpType::Const;
    init.op2 = 0;

    let mut do_fcall = Instruction::new(OpCode::DoFcall);
    do_fcall.result_type = OpType::Unused;

    let mut echo2 = Instruction::new(OpCode::Echo);
    echo2.op1_type = OpType::Const;
    echo2.op1 = 1;

    let mut ret = Instruction::new(OpCode::Return);
    ret.op1_type = OpType::Const;
    ret.op1 = 2;

    let main_func = make_user_function(OpArray {
        num_cvs: 0,
        num_temps: 0,
        instructions: vec![init, do_fcall, echo2, ret],
        literals: vec![Value::string("f1"), Value::long(2), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    });

    let (mut eg, _buf) = make_eg_with_capture();
    eg.register_function("f1", &f1_func.common as *const FunctionCommon).unwrap();

    // Set interrupt before execution
    eg.vm_interrupt.store(true, Ordering::Relaxed);
    eg.timed_out.store(true, Ordering::Relaxed);

    let result = execute::execute(&mut eg, &main_func);
    assert!(result.is_err());
    match result.unwrap_err() {
        execute::VmError::Fatal(msg) => {
            assert!(msg.contains("execution time"));
        }
        _ => panic!("Expected Fatal timeout error"),
    }
}

#[test]
fn test_assign_result_used() {
    // $a = $b = 42; echo $a;
    // ASSIGN_CV CV(1), CONST(0)=42 -> TMP(0)   ; $b = 42, TMP(0) = 42
    // ASSIGN_CV CV(0), TMP(0)                   ; $a = 42
    // ECHO      CV(0)
    // RETURN    CONST(1)

    let mut assign_b = Instruction::new(OpCode::AssignCv);
    assign_b.op1_type = OpType::Cv;
    assign_b.op1 = 1; // $b
    assign_b.op2_type = OpType::Const;
    assign_b.op2 = 0; // 42
    assign_b.result_type = OpType::Tmp;
    assign_b.result = 0;

    let mut assign_a = Instruction::new(OpCode::AssignCv);
    assign_a.op1_type = OpType::Cv;
    assign_a.op1 = 0; // $a
    assign_a.op2_type = OpType::Tmp;
    assign_a.op2 = 0; // TMP(0)

    let mut echo = Instruction::new(OpCode::Echo);
    echo.op1_type = OpType::Cv;
    echo.op1 = 0;

    let mut ret = Instruction::new(OpCode::Return);
    ret.op1_type = OpType::Const;
    ret.op1 = 1;

    let main_func = make_user_function(OpArray {
        num_cvs: 2,
        num_temps: 1,
        instructions: vec![assign_b, assign_a, echo, ret],
        literals: vec![Value::long(42), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        main_scope_vars: vec![],
        all_cvs: vec![],
    });

    let (mut eg, buf) = make_eg_with_capture();
    execute::execute(&mut eg, &main_func).unwrap();
    assert_eq!(captured_output(&buf), "42");
}
