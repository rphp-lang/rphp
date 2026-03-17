/// PHP Generator — suspendable execution context.
/// Created when a generator function is called. Holds all state needed to
/// suspend at yield and resume later.

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{Value, ArrayKey};
use crate::vm::function::{FunctionCommon, UserFunction};

/// Delegate for `yield from` — either a sub-generator or an array being iterated.
pub enum YieldFromDelegate {
    /// Delegating to another Generator
    Generator(GeneratorRef),
    /// Delegating to an array (entries + current position)
    Array(Vec<(ArrayKey, Value)>, usize),
}

/// Generator execution state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GeneratorState {
    /// Created but not yet started (before first next()/send())
    Created,
    /// Suspended at a yield point
    Suspended,
    /// Currently executing (re-entrancy guard)
    Running,
    /// Completed (returned or threw)
    Completed,
}

/// A PHP Generator object.
/// Stores a snapshot of the execution context so execution can be
/// suspended at yield points and resumed later.
///
/// Debug is manually implemented because Value doesn't derive Debug.
pub struct Generator {
    /// The generator function pointer (stable — lives in function_table)
    pub func: *const FunctionCommon,
    /// Saved compiled variable values (snapshot of CV slots)
    pub cv_values: Vec<Value>,
    /// Saved temporary variable values (snapshot of TMP slots)
    pub tmp_values: Vec<Value>,
    /// Saved instruction offset (relative to op_array.instructions start)
    pub ip_offset: usize,
    /// Current state
    pub state: GeneratorState,
    /// Last yielded value (available via current())
    pub value: Value,
    /// Last yielded key (available via key())
    pub key: Value,
    /// Value to send into generator on next resume
    pub send_value: Value,
    /// Return value (set when generator returns)
    pub return_value: Value,
    /// Auto-incrementing key for yield without explicit key
    pub implicit_key: i64,
    /// Active `yield from` delegate (sub-generator or array)
    pub delegate: Option<YieldFromDelegate>,
    /// TMP slot index for writing `yield from` result when delegate completes
    pub yield_from_result_slot: u32,
}

impl Generator {
    /// Create a new generator from a generator function.
    /// `args` are the pre-bound argument values (already validated by DoFcall).
    pub fn new(func: *const FunctionCommon, args: Vec<Value>, num_cvs: u32, num_temps: u32) -> Self {
        let mut cv_values = Vec::with_capacity(num_cvs as usize);
        // Copy arguments into CV slots
        for i in 0..num_cvs as usize {
            if i < args.len() {
                cv_values.push(args[i].clone());
            } else {
                cv_values.push(Value::undef());
            }
        }
        let mut tmp_values = Vec::with_capacity(num_temps as usize);
        for _ in 0..num_temps {
            tmp_values.push(Value::undef());
        }
        Self {
            func,
            cv_values,
            tmp_values,
            ip_offset: 0,
            state: GeneratorState::Created,
            value: Value::null(),
            key: Value::long(-1), // will become 0 on first yield
            send_value: Value::null(),
            return_value: Value::null(),
            implicit_key: 0,
            delegate: None,
            yield_from_result_slot: 0,
        }
    }

    /// Get the UserFunction from the stored func pointer
    pub unsafe fn user_function(&self) -> &UserFunction {
        &*(self.func as *const UserFunction)
    }
}

impl std::fmt::Debug for Generator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Generator")
            .field("state", &self.state)
            .field("ip_offset", &self.ip_offset)
            .field("implicit_key", &self.implicit_key)
            .finish()
    }
}

/// Shared reference to a Generator (used inside Value::Object)
pub type GeneratorRef = Rc<RefCell<Generator>>;

/// Create a new GeneratorRef
pub fn new_generator_ref(generator: Generator) -> GeneratorRef {
    Rc::new(RefCell::new(generator))
}
