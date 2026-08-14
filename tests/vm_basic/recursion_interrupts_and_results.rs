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
        source_lines: vec![],
        instructions: vec![f1_ret],
        literals: vec![Value::long(42)],
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
        source_lines: vec![],
        instructions: vec![f2_init, f2_do, f2_ret],
        literals: vec![Value::string("f1")],
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
        source_lines: vec![],
        instructions: vec![f3_init, f3_do, f3_ret],
        literals: vec![Value::string("f2")],
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
        source_lines: vec![],
        instructions: vec![main_init, main_do, main_echo, main_ret],
        literals: vec![Value::string("f3"), Value::null()],
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
    });

    let (mut eg, buf) = make_eg_with_capture();
    eg.register_function("f1", &f1_func.common as *const FunctionCommon)
        .unwrap();
    eg.register_function("f2", &f2_func.common as *const FunctionCommon)
        .unwrap();
    eg.register_function("f3", &f3_func.common as *const FunctionCommon)
        .unwrap();
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
        source_lines: vec![],
        instructions: vec![f1_echo, f1_ret],
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
        source_lines: vec![],
        instructions: vec![init, do_fcall, echo2, ret],
        literals: vec![Value::string("f1"), Value::long(2), Value::null()],
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
    });

    let (mut eg, _buf) = make_eg_with_capture();
    eg.register_function("f1", &f1_func.common as *const FunctionCommon)
        .unwrap();

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
        source_lines: vec![],
        instructions: vec![assign_b, assign_a, echo, ret],
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
    });

    let (mut eg, buf) = make_eg_with_capture();
    execute::execute(&mut eg, &main_func).unwrap();
    assert_eq!(captured_output(&buf), "42");
}
