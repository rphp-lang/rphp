/// PHP Generator — suspendable execution context.
/// Created when a generator function is called. Holds all state needed to
/// suspend at yield and resume later.
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::value::{ArrayKey, PhpObject, Value};
use crate::vm::function::{FunctionCommon, UserFunction};

/// Delegate for `yield from` — a sub-generator, array or user Iterator.
pub enum YieldFromDelegate {
    /// Delegating to another Generator. IteratorAggregate-produced generators
    /// share the iterative engine while retaining Traversable send/throw and
    /// return-value semantics.
    Generator(GeneratorRef, YieldFromGeneratorMode),
    /// Delegating to an array (entries + current position)
    Array(Vec<(ArrayKey, Value)>, usize, bool),
    /// Delegating lazily to a userland Iterator object.
    Iterator(Value),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum YieldFromGeneratorMode {
    Direct,
    Traversable,
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

/// Value injected when the owning Fiber resumes a generator activation that
/// stopped inside `Fiber::suspend()`. This is kept on the detached Generator
/// sidecar instead of retaining a pointer into the materialized VM frame.
pub(crate) enum GeneratorFiberInput {
    Resume(Value),
    Throw(Value),
    ForceClose(Value),
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
    /// Last published pair retained after completion so a separate generator
    /// delegating to this one can synchronize its still-visible current item.
    pub last_yielded_value: Value,
    pub last_yielded_key: Value,
    /// Number of public arguments supplied at creation, retained in the
    /// existing Value-sized cold metadata slot for exception traces.
    pub trace_num_args: Value,
    /// Positional arguments beyond the declared non-variadic parameter list.
    /// Their creation frame is retired before the generator body starts, so
    /// func_get_arg(s) and trace collection need an activation-owned copy.
    pub extra_args: Vec<Value>,
    /// Return value (set when generator returns)
    pub return_value: Value,
    /// True only after the generator reaches a normal explicit or implicit
    /// return. Exceptional closure also uses Completed but has no return value.
    pub has_returned: bool,
    /// The last userland owner is force-closing this suspended activation.
    /// Finally blocks still execute, but another yield/yield-from is rejected.
    pub force_closing: bool,
    /// A `return` crossed into a finally block before the last suspension.
    /// The frame-local bit must survive yield just like CV/TMP state.
    pub pending_return_after_finally: bool,
    /// Exceptions displaced while a suspended activation executes a finally
    /// block. The request sidecar is keyed by the transient frame address, so
    /// its stack must travel with the generator snapshot across every yield.
    pub pending_finally_exceptions: Vec<Value>,
    /// Result slot of the `Fiber::suspend()` call in the detached generator
    /// snapshot. `None` means this generator is not the active leaf of a
    /// Fiber-owned suspended delegation chain.
    pub(crate) fiber_suspend_result_slot: Option<u32>,
    pub(crate) fiber_suspended: bool,
    /// Resume input is published only on the root Generator whose internal
    /// method call was interrupted. External attempts to advance the same
    /// Running generator therefore continue to receive the canonical error.
    pub(crate) fiber_resume_input: Option<GeneratorFiberInput>,
    /// Rewind remains legal until execution advances beyond the first
    /// suspension point. An empty generator also completes while rewindable.
    pub rewindable: bool,
    /// The first suspension was reached only while an outer yield-from chain
    /// primed this generator. PHP lets the first direct next() claim that
    /// already-published item without advancing the shared delegate again.
    pub indirectly_primed: bool,
    /// Auto-incrementing key for yield without explicit key
    pub implicit_key: i64,
    /// Class scope captured when a generator closure/method is invoked.
    /// Resume frames republish it for visibility and late-static semantics.
    pub called_scope_class_id: u32,
    /// Anonymous Closure-owned function statics retained across the short
    /// creation frame and every suspended generator activation.
    pub closure_static_vars: Option<crate::value::ClosureStaticVars>,
    /// Anonymous generator functions keep their Closure object observable
    /// until the Generator itself is released, even after execution completes.
    pub closure_owner: Option<Value>,
    /// Weak back-reference to the PHP Generator object. Force-close uses its
    /// visible owner count to avoid aborting a delegate that is also retained
    /// independently by userland.
    pub owner_object: Option<Weak<RefCell<PhpObject>>>,
    /// Active `yield from` delegate (sub-generator or array)
    pub delegate: Option<YieldFromDelegate>,
    /// TMP slot index for writing `yield from` result when delegate completes
    pub yield_from_result_slot: u32,
    /// Relative TMP index retaining the active yield-from source. A sentinel
    /// marks CV/constant sources that stay live through ordinary frame rules.
    pub(crate) yield_from_source_tmp: u32,
    /// Reified call context detached from the short-lived creation frame.
    /// A fresh execution frame receives this context on every resume.
    #[cfg(feature = "php-generics-reified")]
    pub reified_context: Option<GeneratorReifiedContext>,
    /// Substituted instance-member contract preserved across suspensions.
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    pub generic_member_contract: Option<Rc<crate::generics::GenericMethodContract>>,
}

#[cfg(feature = "php-generics-reified")]
#[derive(Clone, Copy)]
pub struct GeneratorReifiedContext {
    pub binding: crate::generics::ReifiedBinding,
    pub class_id: u32,
}

impl Generator {
    /// Create a new generator from a generator function.
    /// `args` are the pre-bound argument values (already validated by DoFcall).
    pub fn new(
        func: *const FunctionCommon,
        args: Vec<Value>,
        num_cvs: u32,
        num_temps: u32,
    ) -> Self {
        let trace_num_args = args.len();
        let mut args = args.into_iter();
        let mut cv_values = Vec::with_capacity(num_cvs as usize);
        // Move the already-prepared parameter and capture values into their
        // detached slots. Re-cloning an owned reference here would
        // dereference a `use (&$value)` capture and lose its lexical cell.
        for _ in 0..num_cvs {
            cv_values.push(args.next().unwrap_or_else(Value::undef));
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
            last_yielded_value: Value::null(),
            last_yielded_key: Value::long(-1),
            trace_num_args: Value::long(i64::try_from(trace_num_args).unwrap_or(i64::MAX)),
            extra_args: Vec::new(),
            return_value: Value::null(),
            has_returned: false,
            force_closing: false,
            pending_return_after_finally: false,
            pending_finally_exceptions: Vec::new(),
            fiber_suspend_result_slot: None,
            fiber_suspended: false,
            fiber_resume_input: None,
            rewindable: true,
            indirectly_primed: false,
            implicit_key: 0,
            called_scope_class_id: 0,
            closure_static_vars: None,
            closure_owner: None,
            owner_object: None,
            delegate: None,
            yield_from_result_slot: 0,
            yield_from_source_tmp: u32::MAX,
            #[cfg(feature = "php-generics-reified")]
            reified_context: None,
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            generic_member_contract: None,
        }
    }

    /// Get the UserFunction from the stored func pointer
    pub unsafe fn user_function(&self) -> &UserFunction {
        &*(self.func as *const UserFunction)
    }

    /// Whether this generator's declaration permits by-reference iteration.
    #[inline]
    pub(crate) fn yields_by_reference(&self) -> bool {
        // SAFETY: generator construction stores a stable FunctionCommon pointer
        // owned by the request function table for the generator's lifetime.
        unsafe { (*self.func).sig.returns_reference }
    }

    /// Visit every PHP value owned by a detached generator activation.
    ///
    /// The request-local cycle collector cannot infer these edges from the
    /// public Generator object's ordinary properties. In particular, a
    /// suspended frame may retain a reference back to its own Generator
    /// object plus otherwise acyclic argument values. Exposing the complete
    /// saved-value set lets the collector subtract those internal owners and
    /// release the activation when its last userland root disappears.
    pub(crate) fn try_for_each_cycle_child(&self, mut visitor: impl FnMut(&Value)) -> bool {
        // Zend retires operands already transferred to an interrupted call
        // before the generator's local CVs. Preserve that observable order
        // when those values own user destructors.
        for value in &self.tmp_values {
            visitor(value);
        }
        for value in &self.cv_values {
            visitor(value);
        }
        visitor(&self.value);
        visitor(&self.key);
        visitor(&self.last_yielded_value);
        visitor(&self.last_yielded_key);
        visitor(&self.trace_num_args);
        for value in &self.extra_args {
            visitor(value);
        }
        for value in &self.pending_finally_exceptions {
            visitor(value);
        }
        if let Some(input) = &self.fiber_resume_input {
            match input {
                GeneratorFiberInput::Resume(value)
                | GeneratorFiberInput::Throw(value)
                | GeneratorFiberInput::ForceClose(value) => visitor(value),
            }
        }
        visitor(&self.return_value);
        if let Some(static_vars) = &self.closure_static_vars {
            let Ok(static_vars) = static_vars.as_ref().try_borrow() else {
                return false;
            };
            for value in static_vars.values() {
                visitor(value);
            }
        }
        // An acyclic Closure is an ordinary strong child, not a possible
        // cycle edge. Publishing it to the request collector would let the
        // discarded invocation handle nominate the still-observable Closure
        // independently of its live Generator owner. Keep only closures that
        // can actually lead back into a cycle in the collector graph; every
        // Closure remains strongly retained by `closure_owner` either way.
        if self
            .closure_owner
            .as_ref()
            .is_some_and(|owner| owner.try_has_cycle_children().unwrap_or(true))
        {
            visitor(
                self.closure_owner
                    .as_ref()
                    .expect("checked generator closure owner"),
            );
        }
        if let Some(YieldFromDelegate::Array(entries, _, _)) = &self.delegate {
            for (_, value) in entries {
                visitor(value);
            }
        }
        if let Some(YieldFromDelegate::Iterator(iterator)) = &self.delegate {
            visitor(iterator);
        }
        true
    }

    /// Strict collector traversal. Drop-time marker discovery uses the
    /// fallible variant above because it may run inside an active VM borrow.
    pub(crate) fn for_each_cycle_child(&self, visitor: impl FnMut(&Value)) {
        assert!(
            self.try_for_each_cycle_child(visitor),
            "generator child traversal requires unborrowed closure statics"
        );
    }
}

impl Drop for Generator {
    fn drop(&mut self) {
        let mut next = match self.delegate.take() {
            Some(YieldFromDelegate::Generator(delegate, _)) => Some(delegate),
            Some(YieldFromDelegate::Array(_, _, _))
            | Some(YieldFromDelegate::Iterator(_))
            | None => None,
        };

        // A suspended `yield from` frame retains its delegate both explicitly
        // and in the saved TMP values. Keep the explicit reference alive while
        // clearing the snapshot, then peel uniquely-owned delegates one by one.
        // Letting the fields drop structurally would recurse once per delegated
        // generator and overflow the native stack for valid, deep PHP programs.
        self.cv_values.clear();
        self.tmp_values.clear();
        self.extra_args.clear();
        self.pending_finally_exceptions.clear();
        self.fiber_resume_input = None;

        while let Some(generator) = next {
            if Rc::strong_count(&generator) != 1 {
                break;
            }

            let mut generator_data = generator.borrow_mut();
            next = match generator_data.delegate.take() {
                Some(YieldFromDelegate::Generator(delegate, _)) => Some(delegate),
                Some(YieldFromDelegate::Array(_, _, _))
                | Some(YieldFromDelegate::Iterator(_))
                | None => None,
            };
            generator_data.cv_values.clear();
            generator_data.tmp_values.clear();
            generator_data.extra_args.clear();
            generator_data.pending_finally_exceptions.clear();
            generator_data.fiber_resume_input = None;
        }
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
