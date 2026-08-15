/// PHP opcodes — subset for vertical slice.
/// Full set will have ~200 opcodes.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum OpCode {
    JmpFinally = 0,
    // Arithmetic
    Add = 1,
    Sub = 2,
    Mul = 3,
    Div = 4,
    Mod = 5,
    Concat = 8,
    /// In-place string concat: op1(CV) .= op2 — mutates string in place, no TMP alloc.
    AssignConcat = 9,

    // Variable access
    AssignCv = 10,
    /// Snapshot an ordinary CV rvalue. Undef becomes null after routing one
    /// PHP warning through the request error handler.
    FetchCvR = 11,

    // Output
    Echo = 40,

    // Function calls
    InitFcall = 61,
    DoFcall = 60,
    SendVal = 63,
    SendRef = 64,
    SendVarEx = 65, // Runtime check: send by-ref if callee expects it, else by-val
    SendNamed = 66, // Named argument: op1=value, op2=CONST name string; resolved to CV slot at runtime
    /// Compiler-lowered call_user_func_array(): op1=callback, op2=args array.
    /// Resolves and invokes the callback without entering the stdlib wrapper.
    CallUserFuncArray = 67,
    /// Compiler-lowered call_user_func(): resolve callback and create its real
    /// call frame directly, without the variadic stdlib wrapper frame.
    InitUserCall = 68,
    /// Send a call_user_func argument by value. The target is known only at
    /// runtime, so a hidden method `$this` offset comes from its signature.
    SendUser = 69,

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
    InitArray = 70,       // Create empty array in result TMP
    AddArrayElement = 71, // Add element to array: op1=array CV/TMP, op2=value, result=key (or Unused)
    FetchDimR = 72,       // Read: result = op1[op2]
    AssignDim = 73,       // Write: op1[op2] = result (value source in extended_value)
    ArrayPushOp = 74,     // Append: op1[] = op2
    UnsetDim = 75,        // Remove key op2 from array op1
    AddArrayUnpack = 76,  // Merge op2 array into op1 literal, reindexing integer keys
    /// Append one ordinary source argument to a compiler-owned unpack call list.
    AddCallArgument = 77,
    /// Expand one array or Traversable into a compiler-owned unpack call list.
    AddCallUnpack = 78,

    // Foreach
    ForeachInit = 80, // Copy array op1 to result TMP, set position to 0; jump op2 if empty
    ForeachNext = 81, // Fetch next from array op1 at position op2; result=value TMP; jump extended_value if done
    ForeachNextRef = 82, // Fetch by-reference loop value, flushing the previous value first
    ForeachWriteback = 83, // Flush the current by-reference loop value without advancing
    BindArrayAppendRef = 84, // Append null and bind result(CV) to the new element
    /// By-value foreach whose destination CV is proven never to become a PHP
    /// reference. This keeps the common frame-local write branch-free.
    ForeachNextPlain = 85,

    // Exceptions
    Throw = 90, // Throw exception: op1 = value to throw

    // Objects
    NewObj = 100,          // Create new object: op1=class_name_const, result=TMP(object)
    FetchObjR = 101,       // Read property: result = op1->op2 (op2=CONST prop name)
    AssignObjProp = 102,   // Write property: op1->op2 = result
    InitMethodCall = 103,  // Like InitFcall but for method: op1=object, op2=method name
    FetchStaticProp = 104, // Read static property: result = ClassName::$prop
    InitStaticCall = 105,  // Init static method call: op1=class_const, op2=method_const
    InitDynamicCall = 106, // Dynamic call: op1=CV holding function name, op2=num_args
    Instanceof = 107,      // result = op1 instanceof op2 (op2=class_name_const)
    FetchConst = 108,      // result = constant by name (op1=CONST name string)
    BindDefaultParam = 109, // If CV op1 is NOT undef (arg was passed), jump to op2 (skip default init)

    // Generator
    Yield = 110, // Yield value: op1=value, op2=key (Unused if no key), result=received value from send()
    YieldFrom = 111, // Yield from: op1=sub-generator/iterable, result=return value of sub-generator
    GeneratorReturn = 112, // Return from generator: op1=return value (like Return but for generators)

    // New operators
    Spaceship = 113,  // <=>: compare two values, result is -1, 0, or 1
    Pow = 114,        // **: numeric power
    BitwiseAnd = 115, // &: integer bitwise AND
    BitwiseOr = 116,  // |: integer bitwise OR
    BitwiseXor = 117, // ^: integer bitwise XOR
    ShiftLeft = 118,  // <<: integer left shift
    ShiftRight = 119, // >>: integer right shift
    BitwiseNot = 120, // ~: unary integer bitwise NOT

    // Global/static variable binding
    BindGlobal = 121,    // Bind CV op1 to global variable named op2 (CONST string)
    BindStatic = 123, // Bind CV op1 to static variable named op2 (CONST string), func name in extended_value (CONST)
    AssignObjDim = 124, // $obj->prop[$key] = val; op1=obj, op2=key, result=val, extended_value=prop literal idx
    Include = 125, // Include/require file: op1=path (CONST/TMP/CV), extended_value flags: bit0=require, bit1=once
    NullSafeCheck = 126, // If op1 is null, store null in result and jump to op2; otherwise no-op
    CloneObj = 127, // Clone object: op1=source object, result=new cloned object
    /// Create closure value: op1=CONST function name, result=TMP(closure).
    /// Resolves function pointer via inline cache. Captures added by ClosureUseVar.
    CreateClosure = 128,
    /// Push captured value into closure: op1=TMP(closure), op2=CV(captured var).
    ClosureUseVar = 129,
    /// Frame-free call to a known pure internal function with one positional
    /// argument: op1=argument, extended_value=handler ID, result=return value.
    DirectInternalCall1 = 130,
    /// Dedicated strlen(): op1=argument, result=byte length.
    Strlen = 131,
    /// Frame-free call to a known pure internal function with two positional
    /// arguments: op1/op2=arguments, extended_value=handler ID.
    DirectInternalCall2 = 132,
    /// Validate an explicit generic `::<...>` use. `extended_value` addresses
    /// immutable interned metadata; the ordinary per-opline cache records a
    /// successful erased validation so subsequent executions are one branch.
    CheckGenericArgs = 133,
    /// Reified-only boundary check of values already written into the pending
    /// call frame. Erased-only binaries never emit this opcode.
    CheckReifiedArgs = 134,
    /// Reified-only return check. Pops the matching LIFO sidecar binding.
    CheckReifiedReturn = 135,
    /// Validate one generic parameter after its omitted default expression has
    /// materialized inside the callee. Explicit arguments jump over this op.
    CheckGenericDefault = 136,
    /// Late-static method call. Unlike InitStaticCall, its one-entry cache is
    /// keyed by the runtime called class and cannot affect ordinary dispatch.
    InitLateStaticCall = 137,
    /// Generic use-site validation for a late-bound static/trait owner. Its
    /// declaration cache is likewise keyed by the runtime called class.
    CheckLateStaticGenericArgs = 138,
    /// Read a static property through PHP's runtime called class. The cache is
    /// keyed separately from explicit/self/parent property reads.
    FetchLateStaticProp = 139,
    /// Assign a declared static property through an explicit lexical owner.
    AssignStaticProp = 140,
    /// Assign a declared static property through PHP's runtime called class.
    AssignLateStaticProp = 141,
    /// Read an immutable class-like constant through an explicit lexical owner.
    FetchClassConst = 142,
    /// Read an immutable class-like constant through PHP's runtime called class.
    FetchLateClassConst = 143,
    /// Read a class-like constant from a runtime object/string owner or name.
    FetchDynamicClassConst = 144,
    /// Read a dynamically named constant through PHP's runtime called class.
    FetchLateDynamicClassConst = 145,
    /// Test one object property without materializing it or emitting ordinary
    /// missing/non-object read diagnostics.
    IssetObj = 146,
    /// Unset one object property: op1=object, op2=CONST property name.
    UnsetObj = 147,
    /// Resolve a PHP callable and materialize a real Closure value. Unlike an
    /// array callable, the result satisfies Closure type declarations while
    /// retaining the receiver and lexical called-class scope.
    CreateFirstClassCallable = 148,
    /// End-of-expression TMP release range. op1/op2 are absolute slot bounds;
    /// object destructors run before the owned values are dropped.
    ReleaseTemps = 149,
    /// Bind result(CV) by reference to op1->op2. The property is promoted to
    /// an owned reference cell so object storage and the frame remain aliases.
    BindObjPropRef = 150,
    /// Bind result(CV) by reference to op1[op2], promoting the array element
    /// to the same stable owned reference-cell representation.
    BindArrayDimRef = 151,
    /// Read one entry from PHP's request-global symbol table. op1 is the
    /// dimension key and result is the fetched value.
    FetchGlobal = 152,
    /// Assign one entry in PHP's request-global symbol table. op1 is the key
    /// and op2 is the assigned value.
    AssignGlobal = 153,
    /// Remove one entry from PHP's request-global symbol table. op1 is the key.
    UnsetGlobal = 154,
    /// Bind result(CV) by reference to one request-global entry.
    BindGlobalRef = 155,
    /// Bind one request-global entry by reference to op2(CV).
    AssignGlobalRef = 156,
    /// Materialize a value snapshot of the request-global symbol table.
    /// The result excludes the GLOBALS pseudo-variable itself.
    FetchGlobals = 157,
    /// Read a runtime-named entry from the active PHP symbol table.
    FetchDynamicVar = 158,
    /// Assign a runtime-named entry in the active PHP symbol table.
    AssignDynamicVar = 159,
    /// Remove a runtime-named entry from the active PHP symbol table.
    UnsetDynamicVar = 160,
    /// Bind result(CV) to a runtime-named entry by reference.
    BindDynamicVarRef = 161,
    /// Bind a runtime-named entry by reference to op2(CV).
    AssignDynamicVarRef = 162,
    /// `global ${expr}`: bind the runtime-named local entry to the request
    /// global entry with the same name.
    BindDynamicGlobal = 163,
    /// Resolve a static-property owner/name and raise PHP's runtime Error;
    /// static properties cannot be unset even when selected dynamically.
    UnsetStaticProp = 164,
    /// Rebind result(CV) to the same stable PHP reference cell as op1(CV).
    /// Both variable names remain aliases until one is explicitly rebound or
    /// unset; ordinary assignments through either CV update the shared cell.
    BindCvRef = 165,
    /// Compile and execute op1 as PHP source in the active lexical scope.
    Eval = 166,

    // ── Specialized opcodes ──────────────────────────────────────────
    // Compiler emits these for common operand-type patterns.
    // Each inlines operand fetch — no runtime OpType match needed.
    // Falls back to general handler on type mismatch (overflow, float, etc).
    /// Add Tmp + Tmp → Tmp (Long fast path)
    Add_TmpTmp = 200,
    /// Sub CV - Const → Tmp (Long fast path)
    Sub_CvConst = 201,
    /// IsSmaller CV < Const → Tmp (Long fast path)
    IsSmaller_CvConst = 202,
    /// IsSmallerOrEqual CV <= Const → Tmp (Long fast path)
    IsSmallerOrEqual_CvConst = 203,
    /// Add CV + Tmp → Tmp (Long fast path)
    Add_CvTmp = 204,
    /// Sub Tmp - Tmp → Tmp (Long fast path)
    Sub_TmpTmp = 205,
    /// IsEqual CV == Const → Tmp (Long fast path)
    IsEqual_CvConst = 210,

    // ── Superinstructions (fused opcode pairs) ──────────────────────
    // Fuse comparison + conditional jump into a single dispatch.
    // Eliminates TMP write/read and one dispatch cycle.
    // result field stores jump target IP (replaces JmpZ/JmpNZ.op2).
    // On fall-through, opline advances by 2 (skipping the dead JmpZ/JmpNZ).
    /// Fused: IsSmallerOrEqual CV <= Const; JmpZ → target
    /// If !(CV <= Const), jump to result. Else fall through (+2).
    JmpZ_Le_CvConst = 206,
    /// Fused: IsSmallerOrEqual CV <= Const; JmpNZ → target
    /// If CV <= Const, jump to result. Else fall through (+2).
    JmpNZ_Le_CvConst = 207,
    /// Fused: IsSmaller CV < Const; JmpZ → target
    /// If !(CV < Const), jump to result. Else fall through (+2).
    JmpZ_Lt_CvConst = 208,
    /// Fused: IsSmaller CV < Const; JmpNZ → target
    /// If CV < Const, jump to result. Else fall through (+2).
    JmpNZ_Lt_CvConst = 209,

    /// Fused: IsEqual CV == Const; JmpZ → target
    /// If !(CV == Const), jump to result. Else fall through (+2).
    JmpZ_Eq_CvConst = 211,
    /// Fused: IsEqual CV == Const; JmpNZ → target
    /// If CV == Const, jump to result. Else fall through (+2).
    JmpNZ_Eq_CvConst = 212,

    /// Backward jump for a precomputed guarded scalar loop region.
    /// `op1` remains the baseline target; `extended_value` is block index + 1.
    QuickLongLoopJmp = 213,

    /// strlen(CV) → TMP/Unused without generic operand dispatch.
    Strlen_Cv = 214,

    // Declaration-derived scalar specializations. Their inputs are proven by
    // parameter boundaries or exact return hints, so handlers skip repeated
    // Value tag/coercion probes while preserving overflow and error behavior.
    Add_LongLong = 215,
    Sub_LongLong = 216,
    Mul_LongLong = 217,
    Mod_LongLong = 218,
    Concat_StringString = 219,
    Strlen_String = 220,
    Echo_String = 221,
    Echo_Long = 222,
    BitwiseXor_LongLong = 223,
    BitwiseAnd_LongLong = 224,
    BitwiseOr_LongLong = 225,
}
