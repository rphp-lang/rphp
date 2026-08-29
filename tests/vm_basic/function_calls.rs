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
        trait_class_scope_tmp: None,
        source_lines: vec![],
        instructions: vec![init, send, do_fcall, echo, ret],
        literals: vec![Value::string("my_double"), Value::long(21), Value::null()],
        try_entries: vec![],

        has_finally: false,
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
    eg.register_function("my_double", &my_double_func.common as *const FunctionCommon)
        .unwrap();
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
        num_cvs: 1, // $x
        num_temps: 1,
        trait_class_scope_tmp: None,
        source_lines: vec![],
        instructions: vec![add, fn_ret],
        literals: vec![Value::long(1)],
        try_entries: vec![],

        has_finally: false,
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
        trait_class_scope_tmp: None,
        source_lines: vec![],
        instructions: vec![init, send, do_fcall, echo, ret],
        literals: vec![Value::string("add_one"), Value::long(41), Value::null()],
        try_entries: vec![],

        has_finally: false,
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

    let main_func = make_user_function(main_op_array);
    let (mut eg, buf) = make_eg_with_capture();
    eg.register_function("add_one", &add_one_func.common as *const FunctionCommon)
        .unwrap();
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
        trait_class_scope_tmp: None,
        source_lines: vec![],
        instructions: vec![init, do_fcall, ret],
        literals: vec![Value::string("nonexistent"), Value::null()],
        try_entries: vec![],

        has_finally: false,
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

    let add_one_func = make_user_function_with_args(
        OpArray {
            num_cvs: 1,
            num_temps: 1,
            trait_class_scope_tmp: None,
            source_lines: vec![],
            instructions: vec![add, fn_ret],
            literals: vec![Value::long(1)],
            try_entries: vec![],

            has_finally: false,
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
        },
        1,
    );

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
        trait_class_scope_tmp: None,
        source_lines: vec![],
        instructions: vec![init1, send1, do1, init2, send2, do2, assign, echo, ret],
        literals: vec![
            Value::string("my_double"),
            Value::long(20),
            Value::string("add_one"),
            Value::null(),
        ],
        try_entries: vec![],

        has_finally: false,
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
    eg.register_function("my_double", &my_double_func.common as *const FunctionCommon)
        .unwrap();
    eg.register_function("add_one", &add_one_func.common as *const FunctionCommon)
        .unwrap();
    execute::execute(&mut eg, &main_func).unwrap();
    assert_eq!(captured_output(&buf), "41");
}
