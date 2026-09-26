//! Request-owned PHP storage accounting and a checked allocation boundary.
//!
//! This is deliberately not a GlobalAlloc implementation. A reservation is
//! made in ordinary Rust code, before modifying PHP storage. The private
//! unwind payload crosses only Rust VM frames and is caught at the executor
//! boundary; it is never a PHP Throwable or an allocator callback. Native
//! mutating regions must side-exit while a finite budget is active.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

// Reporting can temporarily retain the failed array plus a callback's COW
// replacement and trace. This is bounded even if a hostile handler loops.
const FINALIZATION_RESERVE: usize = 4 * 1024 * 1024;
const EMERGENCY_RESERVE: usize = 64 * 1024;

#[derive(Debug, Default)]
struct State {
    used: Cell<usize>,
    collector_bytes: Cell<usize>,
    peak: Cell<usize>,
    limit: Cell<Option<usize>>,
    exhausted: Cell<bool>,
    exhausted_limit: Cell<usize>,
    final_limit: Cell<usize>,
    reporting: Cell<bool>,
    strings: RefCell<HashMap<usize, (std::rc::Weak<String>, usize)>>,
    string_sweep: Cell<usize>,
    emergency: RefCell<Option<Box<[u8]>>>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Budget(Rc<State>);

thread_local! {
    static ACTIVE: RefCell<Option<Budget>> = const { RefCell::new(None) };
}

#[derive(Debug)]
pub(crate) struct Exhausted {
    pub(crate) limit: usize,
    pub(crate) attempted: usize,
}

pub(crate) struct Scope(Option<Budget>);

pub(crate) struct DiagnosticScope(Budget, bool);

impl Drop for DiagnosticScope {
    fn drop(&mut self) {
        self.0.0.reporting.set(self.1);
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|active| *active.borrow_mut() = self.0.take());
    }
}

impl Budget {
    /// Keep real, accounted storage available for the small allocations made
    /// by fatal shutdown callbacks. Release it once, before publishing OOM;
    /// user code still obeys its original limit after diagnostic finalization.
    pub(crate) fn prepare_request(&self) {
        self.0.exhausted.set(false);
        self.0.reporting.set(false);
        if self.0.emergency.borrow().is_none() {
            self.charge(EMERGENCY_RESERVE);
            *self.0.emergency.borrow_mut() = Some(vec![0; EMERGENCY_RESERVE].into_boxed_slice());
        }
    }

    pub(crate) fn diagnostic_scope(&self) -> DiagnosticScope {
        DiagnosticScope(self.clone(), self.0.reporting.replace(true))
    }
    pub(crate) fn is_reporting(&self) -> bool {
        self.0.reporting.get()
    }
    pub(crate) fn enter(&self) -> Scope {
        Scope(ACTIVE.with(|active| active.replace(Some(self.clone()))))
    }

    pub(crate) fn usage(&self) -> usize {
        // Collector bookkeeping is allocated while Values are dropped. Do
        // not unwind through a half-retired slot: sample its actual capacity
        // at the next checked allocation/VM boundary instead. Old owners must
        // never adopt a different request's thread-local collector storage.
        let active = ACTIVE.with(|active| {
            active
                .borrow()
                .as_ref()
                .is_some_and(|active| Rc::ptr_eq(&active.0, &self.0))
        });
        if active && let Some(bytes) = crate::value::request_cycle_storage_bytes() {
            self.0.collector_bytes.set(bytes);
        }
        let used = self
            .0
            .used
            .get()
            .saturating_add(self.0.collector_bytes.get());
        self.0.peak.set(self.0.peak.get().max(used));
        used
    }

    pub(crate) fn collect_strings(&self) {
        let mut released = 0usize;
        self.0.strings.borrow_mut().retain(|_, (owner, bytes)| {
            if owner.strong_count() != 0 {
                true
            } else {
                released = released.saturating_add(*bytes);
                false
            }
        });
        self.release(released);
    }

    pub(crate) fn peak(&self) -> usize {
        self.0.peak.get()
    }

    pub(crate) fn reset_peak(&self) {
        self.0.peak.set(self.usage());
    }

    pub(crate) fn is_limited(&self) -> bool {
        self.0.limit.get().is_some() || (self.exhausted() && self.is_reporting())
    }

    pub(crate) fn exhausted(&self) -> bool {
        self.0.exhausted.get()
    }

    pub(crate) fn set_limit(&self, limit: i64) -> bool {
        self.collect_strings();
        let limit = (limit >= 0).then_some(limit as usize);
        if limit.is_some_and(|limit| limit < self.usage()) {
            return false;
        }
        self.0.limit.set(limit);
        true
    }

    fn check(&self, attempted: usize) {
        let (limit, ceiling) = if self.exhausted() && self.0.reporting.get() {
            // Even ini_set(-1) inside an output error callback must not turn
            // the engine's emergency reporting phase into unbounded growth.
            (self.0.exhausted_limit.get(), self.0.final_limit.get())
        } else {
            let Some(limit) = self.0.limit.get() else {
                return;
            };
            (limit, limit)
        };
        if self
            .usage()
            .checked_add(attempted)
            .is_some_and(|used| used <= ceiling)
        {
            return;
        }
        self.collect_strings();
        if self
            .usage()
            .checked_add(attempted)
            .is_some_and(|used| used <= ceiling)
        {
            return;
        }
        // Dropping partially built Rust temporaries must never double-panic.
        // Their PHP children still release their original owning budget.
        if std::thread::panicking() {
            return;
        }
        if !self.0.exhausted.replace(true) {
            self.0.exhausted_limit.set(limit);
            if let Some(reserve) = self.0.emergency.borrow_mut().take() {
                self.release(reserve.len());
                drop(reserve);
            }
            self.0
                .final_limit
                .set(self.usage().saturating_add(FINALIZATION_RESERVE));
        }
        std::panic::resume_unwind(Box::new(Exhausted {
            limit,
            attempted: attempted.max(1),
        }));
    }

    pub(crate) fn enforce(&self) {
        self.check(0);
    }

    fn charge(&self, bytes: usize) {
        self.check(bytes);
        let used = self.0.used.get().saturating_add(bytes);
        self.0.used.set(used);
        self.0.peak.set(self.peak().max(self.usage()));
    }

    fn release(&self, bytes: usize) {
        self.0.used.set(self.0.used.get().saturating_sub(bytes));
    }
}

/// The charge follows the physical storage owner, not a Value alias or the
/// currently executing request. Late drops and nested requests cannot debit
/// a different request. Immutable compiler storage has no request owner.
#[derive(Debug, Default)]
pub(crate) struct Allocation {
    budget: Option<Budget>,
    bytes: usize,
}

impl Allocation {
    pub(crate) fn new(bytes: usize) -> Self {
        let budget = ACTIVE.with(|active| active.borrow().clone());
        if let Some(budget) = &budget {
            budget.charge(bytes);
        }
        Self { budget, bytes }
    }

    pub(crate) fn grow_to(&mut self, bytes: usize) {
        if self.budget.is_none() {
            *self = Self::new(bytes.max(self.bytes));
        } else if bytes > self.bytes {
            self.budget.as_ref().unwrap().charge(bytes - self.bytes);
            self.bytes = bytes;
        }
    }

    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) fn release_bytes(&mut self, bytes: usize) {
        let bytes = bytes.min(self.bytes);
        if let Some(budget) = &self.budget {
            budget.release(bytes);
        }
        self.bytes -= bytes;
    }
}

impl Clone for Allocation {
    fn clone(&self) -> Self {
        Self::new(self.bytes)
    }
}

impl Drop for Allocation {
    fn drop(&mut self) {
        if let Some(budget) = &self.budget {
            budget.release(self.bytes);
        }
    }
}

/// Check transient replacement storage before a String/Vec builder runs.
pub(crate) fn check(bytes: usize) {
    let budget = ACTIVE.with(|active| active.borrow().clone());
    if let Some(budget) = budget {
        budget.check(bytes);
    }
}

pub(crate) fn is_limited() -> bool {
    ACTIVE.with(|active| active.borrow().as_ref().is_some_and(Budget::is_limited))
}

/// Strings keep their established Rc<String> ABI (also shared by array keys
/// and call caches). Weak records do not keep the payload alive; bounded
/// sweeps reclaim charges before the limit is tested, including cached owners.
pub(crate) fn reserve_string(owner: &Rc<String>, capacity: usize) {
    let Some(budget) = ACTIVE.with(|active| active.borrow().clone()) else {
        return;
    };
    let tick = budget.0.string_sweep.get().wrapping_add(1);
    budget.0.string_sweep.set(tick);
    if tick.is_multiple_of(256) {
        budget.collect_strings();
    }
    let identity = Rc::as_ptr(owner) as usize;
    let bytes = capacity.saturating_add(std::mem::size_of::<String>() + 16);
    let previous = budget
        .0
        .strings
        .borrow()
        .get(&identity)
        .map_or(0, |(_, bytes)| *bytes);
    if bytes > previous {
        budget.charge(bytes - previous);
        budget
            .0
            .strings
            .borrow_mut()
            .insert(identity, (Rc::downgrade(owner), bytes));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_memory_reservation_is_atomic_and_release_uses_its_owner() {
        let first = Budget::default();
        first.set_limit(128);
        let scope = first.enter();
        let mut allocation = Allocation::new(96);
        let failed =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| allocation.grow_to(160)));
        assert!(failed.unwrap_err().is::<Exhausted>());
        assert_eq!(allocation.bytes(), 96);
        assert_eq!(first.usage(), 96);
        drop(scope);
        let second = Budget::default();
        let _scope = second.enter();
        drop(allocation);
        assert_eq!(first.usage(), 0);
        assert_eq!(second.usage(), 0);
    }

    #[test]
    fn request_memory_clones_charge_storage_not_aliases_and_reserve_is_bounded() {
        let budget = Budget::default();
        let _scope = budget.enter();
        let allocation = Rc::new(Allocation::new(64));
        let alias = allocation.clone();
        assert_eq!(budget.usage(), 64);
        let separated = allocation.as_ref().clone();
        assert_eq!(budget.usage(), 128);
        assert!(!budget.set_limit(100));
        assert!(budget.set_limit(128));
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(1))).is_err());
        let _diagnostic = budget.diagnostic_scope();
        let reserve = Allocation::new(FINALIZATION_RESERVE);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(1))).is_err());
        budget.set_limit(-1);
        assert!(budget.is_limited());
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(1))).is_err());
        drop((allocation, alias, separated, reserve));
        assert_eq!(budget.usage(), 0);
    }

    #[test]
    fn request_memory_array_key_projection_failure_keeps_reference_cells() {
        use crate::value::{PhpArray, Value};
        let budget = Budget::default();
        let _scope = budget.enter();
        let mut array = PhpArray::new();
        array.set_str("original", Value::owned_reference(Value::long(23)));
        let identity = array.get_str("original").unwrap().reference_identity();
        assert!(budget.set_limit(budget.usage() as i64));
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            array.promote_keys_to_external_storage();
        }));
        assert!(failure.unwrap_err().is::<Exhausted>());
        assert_eq!(array.len(), 1);
        assert_eq!(
            array.get_str("original").unwrap().reference_identity(),
            identity
        );
        assert!(!array.has_external_byte_keys());
    }

    #[test]
    fn request_memory_cow_failure_does_not_publish_a_replacement() {
        use crate::value::{PhpArray, Value};
        let budget = Budget::default();
        let _scope = budget.enter();
        let mut array = PhpArray::new();
        array.push(Value::long(19));
        let original = Value::array(array);
        let mut alias = original.clone();
        let identity = original.array_identity();
        let before = budget.usage();
        assert!(budget.set_limit(before as i64));
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            alias.as_array_mut().unwrap().push(Value::long(21));
        }));
        assert!(failure.unwrap_err().is::<Exhausted>());
        assert_eq!(alias.array_identity(), identity);
        assert_eq!(original.as_array().unwrap().len(), 1);
        assert_eq!(budget.usage(), before);
    }

    #[test]
    fn request_memory_collector_capacity_is_checked_outside_value_drop() {
        use crate::value::{PhpArray, Value};
        let budget = Budget::default();
        let _scope = budget.enter();
        crate::value::begin_object_handle_request();
        let mut array = PhpArray::new();
        array.push(Value::long(41));
        let original = Value::array(array);
        let alias = original.clone();
        assert!(budget.set_limit(budget.usage() as i64));
        // Publishing the weak candidate must finish retiring its Value slot.
        drop(alias);
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| budget.enforce()));
        assert!(failed.unwrap_err().is::<Exhausted>());
        assert_eq!(
            original
                .as_array()
                .unwrap()
                .get_int(0)
                .and_then(Value::as_long),
            Some(41)
        );
        crate::value::end_object_handle_request();
    }
}
