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
        source_lines: vec![],
        instructions: vec![echo, ret],
        literals: vec![Value::long(42), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        source_file: std::rc::Rc::new(String::new()),
        main_scope_vars: vec![],
        all_cvs: vec![],
        cache: vec![],
        may_access_globals: false,
        block_info: Vec::new(),
        block_counters: Vec::new(),
        block_plans: Vec::new(),
        ip_to_block: Vec::new(),
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
        source_lines: vec![],
        instructions: vec![echo, ret],
        literals: vec![Value::long(-1), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        source_file: std::rc::Rc::new(String::new()),
        main_scope_vars: vec![],
        all_cvs: vec![],
        cache: vec![],
        may_access_globals: false,
        block_info: Vec::new(),
        block_counters: Vec::new(),
        block_plans: Vec::new(),
        ip_to_block: Vec::new(),
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
        source_lines: vec![],
        instructions: vec![add, echo, ret],
        literals: vec![Value::long(20), Value::long(22), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        source_file: std::rc::Rc::new(String::new()),
        main_scope_vars: vec![],
        all_cvs: vec![],
        cache: vec![],
        may_access_globals: false,
        block_info: Vec::new(),
        block_counters: Vec::new(),
        block_plans: Vec::new(),
        ip_to_block: Vec::new(),
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
        source_lines: vec![],
        instructions: vec![add, ret],
        literals: vec![Value::long(i64::MAX), Value::long(1)],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        source_file: std::rc::Rc::new(String::new()),
        main_scope_vars: vec![],
        all_cvs: vec![],
        cache: vec![],
        may_access_globals: false,
        block_info: Vec::new(),
        block_counters: Vec::new(),
        block_plans: Vec::new(),
        ip_to_block: Vec::new(),
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
        source_lines: vec![],
        instructions: vec![echo, ret],
        literals: vec![Value::long(1), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        source_file: std::rc::Rc::new(String::new()),
        main_scope_vars: vec![],
        all_cvs: vec![],
        cache: vec![],
        may_access_globals: false,
        block_info: Vec::new(),
        block_counters: Vec::new(),
        block_plans: Vec::new(),
        ip_to_block: Vec::new(),
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
        source_lines: vec![],
        instructions: vec![assign, echo, ret],
        literals: vec![Value::long(42), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        source_file: std::rc::Rc::new(String::new()),
        main_scope_vars: vec![],
        all_cvs: vec![],
        cache: vec![],
        may_access_globals: false,
        block_info: Vec::new(),
        block_counters: Vec::new(),
        block_plans: Vec::new(),
        ip_to_block: Vec::new(),
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
        source_lines: vec![],
        instructions: vec![assign_a, assign_b, add, echo, ret],
        literals: vec![Value::long(20), Value::long(22), Value::null()],
        try_entries: vec![],
        strict_types: false,
        is_generator: false,
        global_vars: vec![],
        static_vars: vec![],
        name: String::new(),
        source_file: std::rc::Rc::new(String::new()),
        main_scope_vars: vec![],
        all_cvs: vec![],
        cache: vec![],
        may_access_globals: false,
        block_info: Vec::new(),
        block_counters: Vec::new(),
        block_plans: Vec::new(),
        ip_to_block: Vec::new(),
    };

    let main_func = make_user_function(op_array);
    let (mut eg, buf) = make_eg_with_capture();
    execute::execute(&mut eg, &main_func).unwrap();
    assert_eq!(captured_output(&buf), "42");
}
