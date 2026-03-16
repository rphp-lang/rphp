/// PHP opcodes — subset for vertical slice.
/// Full set will have ~200 opcodes.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpCode {
    // Arithmetic
    Add = 1,
    Sub = 2,
    Mul = 3,
    Div = 4,
    Mod = 5,
    Concat = 8,

    // Variable access
    AssignCv = 10,

    // Output
    Echo = 40,

    // Function calls
    InitFcall = 61,
    DoFcall = 60,
    SendVal = 63,
    SendRef = 64,
    SendVarEx = 65,  // Runtime check: send by-ref if callee expects it, else by-val
    SendNamed = 66,  // Named argument: op1=value, op2=CONST name string; resolved to CV slot at runtime

    // Comparison
    IsEqual = 15,
    IsNotEqual = 16,
    IsSmaller = 17,
    IsSmallerOrEqual = 18,
    IsIdentical = 19,
    IsNotIdentical = 20,

    // Type checks
    Isset = 21,
    Cast = 22,

    // Increment/decrement
    PreInc = 34,
    PreDec = 35,
    PostInc = 36,
    PostDec = 37,

    // Logical
    BoolNot = 13,

    // Control flow
    Jmp = 42,
    JmpZ = 43,
    JmpNZ = 44,
    Return = 62,

    // Arrays
    InitArray = 70,      // Create empty array in result TMP
    AddArrayElement = 71, // Add element to array: op1=array CV/TMP, op2=value, result=key (or Unused)
    FetchDimR = 72,      // Read: result = op1[op2]
    AssignDim = 73,      // Write: op1[op2] = result (value source in extended_value)
    ArrayPushOp = 74,    // Append: op1[] = op2
    UnsetDim = 75,       // Remove key op2 from array op1

    // Foreach
    ForeachInit = 80,    // Copy array op1 to result TMP, set position to 0; jump op2 if empty
    ForeachNext = 81,    // Fetch next from array op1 at position op2; result=value TMP; jump extended_value if done

    // Exceptions
    Throw = 90,          // Throw exception: op1 = value to throw

    // Objects
    NewObj = 100,        // Create new object: op1=class_name_const, result=TMP(object)
    FetchObjR = 101,     // Read property: result = op1->op2 (op2=CONST prop name)
    AssignObjProp = 102, // Write property: op1->op2 = result
    InitMethodCall = 103, // Like InitFcall but for method: op1=object, op2=method name
    FetchStaticProp = 104, // Read static property: result = ClassName::$prop
    InitStaticCall = 105, // Init static method call: op1=class_const, op2=method_const
    InitDynamicCall = 106, // Dynamic call: op1=CV holding function name, op2=num_args
    Instanceof = 107,      // result = op1 instanceof op2 (op2=class_name_const)
    FetchConst = 108,      // result = constant by name (op1=CONST name string)
    BindDefaultParam = 109, // If CV op1 is NOT undef (arg was passed), jump to op2 (skip default init)
}
