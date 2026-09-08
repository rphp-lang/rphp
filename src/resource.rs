use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, hash_map::Entry};
use std::sync::atomic::{AtomicU32, Ordering};

#[cfg(feature = "resource-lifetime")]
use crate::resource_handle::ResourceHandle;
use crate::runtime::ExecutorGlobals;
#[cfg(feature = "resource-lifetime")]
use crate::value::Value;
#[cfg(feature = "resource-lifetime")]
use std::rc::{Rc, Weak};

static NEXT_RESOURCE_SCOPE: AtomicU32 = AtomicU32::new(1);

thread_local! {
    static REQUEST_RESOURCES: RefCell<HashMap<u32, ResourceRegistry>> =
        RefCell::new(HashMap::new());
}

struct ResourceEntry {
    resource_type: &'static str,
    payload: Box<dyn Any>,
    #[cfg(feature = "resource-lifetime")]
    owner: Weak<ResourceHandle>,
}

impl ResourceEntry {
    #[inline]
    fn retire_owner(&self) {
        #[cfg(feature = "resource-lifetime")]
        if let Some(owner) = self.owner.upgrade() {
            owner.retire();
        }
    }
}

/// Request-owned PHP resource registry.
///
/// Resource `Value`s contain a stable integer id. With `resource-lifetime`, a
/// shared handle also closes the backend after the final alias disappears.
/// Request shutdown remains the safety net in both configurations.
pub struct ResourceRegistry {
    next_id: i64,
    entries: HashMap<i64, ResourceEntry>,
}

impl ResourceRegistry {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            entries: HashMap::new(),
        }
    }

    #[cold]
    #[cfg(any(not(feature = "resource-lifetime"), test))]
    pub fn insert<T: 'static>(&mut self, resource_type: &'static str, payload: T) -> i64 {
        let id = self.allocate_id();
        let replaced = self.entries.insert(
            id,
            ResourceEntry {
                resource_type,
                payload: Box::new(payload),
                #[cfg(feature = "resource-lifetime")]
                owner: Weak::new(),
            },
        );
        debug_assert!(replaced.is_none());
        id
    }

    #[inline]
    fn allocate_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("PHP resource id overflow");
        id
    }

    #[cfg(feature = "resource-lifetime")]
    #[cold]
    fn insert_value<T: 'static>(
        &mut self,
        scope: u32,
        resource_type: &'static str,
        payload: T,
    ) -> Value {
        let id = self.allocate_id();
        let owner = Rc::new(ResourceHandle::new(scope, id, close_any));
        let replaced = self.entries.insert(
            id,
            ResourceEntry {
                resource_type,
                payload: Box::new(payload),
                owner: Rc::downgrade(&owner),
            },
        );
        debug_assert!(replaced.is_none());
        Value::resource_owner(owner)
    }

    #[inline]
    pub fn is_open(&self, id: i64) -> bool {
        self.entries.contains_key(&id)
    }

    #[inline]
    pub fn resource_type(&self, id: i64) -> &'static str {
        self.entries
            .get(&id)
            .map_or("Unknown", |entry| entry.resource_type)
    }

    // Payload projection is the steady-state native I/O path, not a cold
    // operation. Keep its type check next to the caller's backend operation.
    #[inline(always)]
    pub fn with_payload_mut<T: 'static, R>(
        &mut self,
        id: i64,
        operation: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
        // Bound by all IDs ever issued, not the current live count: a sparse
        // registry that grew large must never walk its retained table. A new
        // tiny registry can compare integer IDs without repeatedly hashing
        // them for native I/O. Type validation and the borrow stay identical.
        if self.next_id <= 8 {
            for (&key, entry) in &mut self.entries {
                if key == id {
                    return Some(operation(entry.payload.downcast_mut::<T>()?));
                }
            }
            return None;
        }
        let payload = self.entries.get_mut(&id)?.payload.downcast_mut::<T>()?;
        Some(operation(payload))
    }

    /// Close only a resource whose backend has the requested concrete type.
    /// A wrong kind or an id closed earlier leaves the registry unchanged.
    #[cfg_attr(
        feature = "resource-lifetime",
        allow(
            dead_code,
            reason = "request close removes the entry first so its destructor runs outside the TLS borrow"
        )
    )]
    #[cold]
    pub fn close<T: 'static>(&mut self, id: i64) -> bool {
        let Entry::Occupied(entry) = self.entries.entry(id) else {
            return false;
        };
        if !entry.get().payload.is::<T>() {
            return false;
        }
        // Validate and remove through one lookup; a wrong type leaves the
        // original entry and owner live.
        entry.remove().retire_owner();
        true
    }

    #[cfg(feature = "resource-lifetime")]
    #[cold]
    fn remove<T: 'static>(&mut self, id: i64) -> Option<ResourceEntry> {
        let Entry::Occupied(entry) = self.entries.entry(id) else {
            return None;
        };
        if !entry.get().payload.is::<T>() {
            return None;
        }
        let entry = entry.remove();
        entry.retire_owner();
        Some(entry)
    }

    #[cfg(feature = "resource-lifetime")]
    #[cold]
    fn remove_any(&mut self, id: i64) -> Option<ResourceEntry> {
        let entry = self.entries.remove(&id)?;
        entry.retire_owner();
        Some(entry)
    }
}

impl Drop for ResourceRegistry {
    fn drop(&mut self) {
        // Retire all handles before dropping any backend: backend teardown
        // may itself release another resource from this same registry.
        for entry in self.entries.values() {
            entry.retire_owner();
        }
    }
}

impl Default for ResourceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cold]
pub(crate) fn allocate_scope() -> u32 {
    NEXT_RESOURCE_SCOPE
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |scope| {
            scope.checked_add(1)
        })
        .expect("PHP resource scope id overflow")
}

#[cold]
#[cfg(any(not(feature = "resource-lifetime"), test))]
pub(crate) fn insert<T: 'static>(scope: u32, resource_type: &'static str, payload: T) -> i64 {
    debug_assert_ne!(scope, 0);
    REQUEST_RESOURCES.with(|registries| {
        let mut registries = registries.borrow_mut();
        if let Some(registry) = registry_for_scope_mut(&mut registries, scope) {
            return registry.insert(resource_type, payload);
        }
        registries
            .entry(scope)
            .or_default()
            .insert(resource_type, payload)
    })
}

#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn registry_for_scope_mut(
    registries: &mut HashMap<u32, ResourceRegistry>,
    scope: u32,
) -> Option<&mut ResourceRegistry> {
    // A thread commonly serves one request. In a tiny single-entry table
    // compare that key directly instead of hashing the same scope for every
    // I/O operation. Retained large tables and nested requests use the normal
    // lookup: iteration must never scale with capacity.
    if registries.len() == 1 && registries.capacity() <= 8 {
        let (registered_scope, registry) = registries.iter_mut().next()?;
        (*registered_scope == scope).then_some(registry)
    } else {
        registries.get_mut(&scope)
    }
}

#[inline(always)]
pub(crate) fn with_payload_mut<T: 'static, R>(
    scope: u32,
    id: i64,
    operation: impl FnOnce(&mut T) -> R,
) -> Option<R> {
    if scope == 0 {
        return None;
    }
    REQUEST_RESOURCES.with(|registries| {
        registry_for_scope_mut(&mut registries.borrow_mut(), scope)?
            .with_payload_mut::<T, _>(id, operation)
    })
}

#[cold]
#[cfg(not(feature = "resource-lifetime"))]
pub(crate) fn close<T: 'static>(scope: u32, id: i64) -> bool {
    if scope == 0 {
        return false;
    }
    REQUEST_RESOURCES.with(|registries| {
        registry_for_scope_mut(&mut registries.borrow_mut(), scope)
            .is_some_and(|registry| registry.close::<T>(id))
    })
}

#[cold]
#[cfg(feature = "resource-lifetime")]
pub(crate) fn close<T: 'static>(scope: u32, id: i64) -> bool {
    if scope == 0 {
        return false;
    }
    let entry = REQUEST_RESOURCES.with(|registries| {
        registry_for_scope_mut(&mut registries.borrow_mut(), scope)
            .and_then(|registry| registry.remove::<T>(id))
    });
    let closed = entry.is_some();
    drop(entry);
    closed
}

#[cfg(feature = "resource-lifetime")]
#[cold]
fn close_any(scope: u32, id: i64) {
    if scope == 0 {
        return;
    }
    let Ok(entry) = REQUEST_RESOURCES.try_with(|registries| {
        let Ok(mut registries) = registries.try_borrow_mut() else {
            // A backend operation currently owns the registry borrow. Request
            // shutdown remains the safety net for this exceptional re-entry.
            return None;
        };
        registry_for_scope_mut(&mut registries, scope).and_then(|registry| registry.remove_any(id))
    }) else {
        // Thread-local teardown already owns the registry and will drop it.
        return;
    };
    // Drop the backend after releasing the thread-local RefCell borrow. A
    // backend destructor may itself release another resource Value.
    drop(entry);
}

#[cold]
pub(crate) fn is_open(scope: u32, id: i64) -> bool {
    if scope == 0 {
        return false;
    }
    REQUEST_RESOURCES.with(|registries| {
        registries
            .borrow()
            .get(&scope)
            .is_some_and(|registry| registry.is_open(id))
    })
}

#[cold]
pub(crate) fn resource_type(scope: u32, id: i64) -> &'static str {
    if scope == 0 {
        return "Unknown";
    }
    REQUEST_RESOURCES.with(|registries| {
        registries
            .borrow()
            .get(&scope)
            .map_or("Unknown", |registry| registry.resource_type(id))
    })
}

#[cold]
pub(crate) fn close_scope(scope: u32) {
    if scope != 0 {
        REQUEST_RESOURCES.with(|registries| {
            registries.borrow_mut().remove(&scope);
        });
    }
}

#[inline]
fn request_scope(eg: &ExecutorGlobals) -> u32 {
    eg.resource_scope
}

#[inline]
fn ensure_request_scope(eg: &mut ExecutorGlobals) -> u32 {
    let mut scope = request_scope(eg);
    if scope == 0 {
        scope = allocate_scope();
        eg.resource_scope = scope;
    }
    scope
}

#[cold]
#[cfg(any(not(feature = "resource-lifetime"), test))]
pub(crate) fn insert_for_request<T: 'static>(
    eg: &mut ExecutorGlobals,
    resource_type: &'static str,
    payload: T,
) -> i64 {
    insert(ensure_request_scope(eg), resource_type, payload)
}

#[cfg(feature = "resource-lifetime")]
#[cold]
pub(crate) fn insert_value_for_request<T: 'static>(
    eg: &mut ExecutorGlobals,
    resource_type: &'static str,
    payload: T,
) -> Value {
    // Resolve the request once. The owner and registry entry must refer to the
    // same scope; a second constant-table lookup provides no new information.
    let scope = ensure_request_scope(eg);
    debug_assert_ne!(scope, 0);
    REQUEST_RESOURCES.with(|registries| {
        // Repeated opens belong to the same request as subsequent I/O. Reuse
        // its bounded lookup; only the first insertion creates a registry.
        let mut registries = registries.borrow_mut();
        if let Some(registry) = registry_for_scope_mut(&mut registries, scope) {
            return registry.insert_value(scope, resource_type, payload);
        }
        registries
            .entry(scope)
            .or_default()
            .insert_value(scope, resource_type, payload)
    })
}

#[inline(always)]
pub(crate) fn with_request_payload_mut<T: 'static, R>(
    eg: &mut ExecutorGlobals,
    id: i64,
    operation: impl FnOnce(&mut T) -> R,
) -> Option<R> {
    with_payload_mut(request_scope(eg), id, operation)
}

/// Install a protocol owner without changing the PHP resource identity. The
/// transformation is native-only: it must not execute PHP or reenter this
/// registry. Failed type checks leave the original entry untouched.
#[cold]
#[cfg(feature = "stream-registry")]
pub(crate) fn wrap_request_payload<T: 'static, U: 'static>(
    eg: &mut ExecutorGlobals,
    id: i64,
    wrap: impl FnOnce(T) -> U,
) -> bool {
    let scope = request_scope(eg);
    REQUEST_RESOURCES.with(|registries| {
        let mut registries = registries.borrow_mut();
        let Some(registry) = registry_for_scope_mut(&mut registries, scope) else {
            return false;
        };
        if !registry
            .entries
            .get(&id)
            .is_some_and(|entry| entry.payload.is::<T>())
        {
            return false;
        }
        let entry = registry
            .entries
            .remove(&id)
            .expect("checked resource entry");
        let payload = *entry
            .payload
            .downcast::<T>()
            .expect("checked resource payload");
        registry.entries.insert(
            id,
            ResourceEntry {
                resource_type: entry.resource_type,
                payload: Box::new(wrap(payload)),
                #[cfg(feature = "resource-lifetime")]
                owner: entry.owner,
            },
        );
        true
    })
}

#[cold]
pub(crate) fn close_for_request<T: 'static>(eg: &mut ExecutorGlobals, id: i64) -> bool {
    close::<T>(request_scope(eg), id)
}

#[cold]
pub(crate) fn is_open_for_request(eg: &ExecutorGlobals, id: i64) -> bool {
    is_open(request_scope(eg), id)
}

#[cold]
pub(crate) fn type_for_request(eg: &ExecutorGlobals, id: i64) -> &'static str {
    resource_type(request_scope(eg), id)
}

impl Drop for ExecutorGlobals {
    fn drop(&mut self) {
        super::directory::restore_initial_cwd(self);
        close_scope(request_scope(self));
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "resource-lifetime")]
    use super::insert_value_for_request;
    use super::{
        REQUEST_RESOURCES, ResourceRegistry, allocate_scope, close_for_request, close_scope,
        insert, insert_for_request, is_open, is_open_for_request, resource_type, with_payload_mut,
    };
    use crate::runtime::ExecutorGlobals;
    use std::cell::Cell;
    use std::rc::Rc;

    struct DropProbe(Rc<Cell<usize>>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn explicit_close_is_typed_and_drops_once() {
        let drops = Rc::new(Cell::new(0));
        let mut registry = ResourceRegistry::new();
        let id = registry.insert("probe", DropProbe(drops.clone()));
        assert!(registry.is_open(id));
        assert_eq!(registry.resource_type(id), "probe");
        assert!(!registry.close::<String>(id));
        assert!(registry.close::<DropProbe>(id));
        assert_eq!(drops.get(), 1);
        assert!(!registry.is_open(id));
        assert_eq!(registry.resource_type(id), "Unknown");
        assert!(!registry.close::<DropProbe>(id));
        assert_eq!(drops.get(), 1);
    }

    #[test]
    fn typed_close_misses_preserve_neighbors_and_monotonic_ids() {
        let drops = Rc::new(Cell::new(0));
        let mut registry = ResourceRegistry::new();
        let first = registry.insert("probe", DropProbe(drops.clone()));
        let second = registry.insert("text", String::from("neighbor"));
        assert!(!registry.close::<String>(first));
        assert!(!registry.close::<DropProbe>(second));
        assert!(!registry.close::<DropProbe>(i64::MAX));
        assert_eq!(drops.get(), 0);
        assert!(registry.is_open(first));
        assert_eq!(registry.resource_type(second), "text");
        assert!(registry.close::<DropProbe>(first));
        assert_eq!(drops.get(), 1);
        assert!(!registry.close::<DropProbe>(first));
        let third = registry.insert("probe", DropProbe(drops.clone()));
        assert_eq!(third, second + 1);
        assert_eq!(
            registry.with_payload_mut::<String, _>(second, |text| text.clone()),
            Some(String::from("neighbor"))
        );
        drop(registry);
        assert_eq!(drops.get(), 2);
    }

    #[test]
    fn registry_shutdown_drops_every_open_backend() {
        let drops = Rc::new(Cell::new(0));
        {
            let mut registry = ResourceRegistry::new();
            let first = registry.insert("probe", DropProbe(drops.clone()));
            let second = registry.insert("probe", DropProbe(drops.clone()));
            assert_ne!(first, second);
        }
        assert_eq!(drops.get(), 2);
    }

    #[test]
    fn lazy_scope_isolated_registry_and_cleanup_are_request_owned() {
        let drops = Rc::new(Cell::new(0));
        let first_scope = allocate_scope();
        let second_scope = allocate_scope();
        let first = insert(first_scope, "probe", DropProbe(drops.clone()));
        let second = insert(second_scope, "probe", DropProbe(drops.clone()));
        assert_ne!(first_scope, second_scope);
        assert_eq!(first, second, "resource ids are local to one request");
        assert!(is_open(first_scope, first));
        assert_eq!(resource_type(first_scope, first), "probe");
        close_scope(first_scope);
        assert_eq!(drops.get(), 1);
        assert!(!is_open(first_scope, first));
        assert!(is_open(second_scope, second));
        close_scope(second_scope);
        assert_eq!(drops.get(), 2);
    }

    #[test]
    fn single_scope_projection_checks_scope_resource_and_payload_type() {
        let scope = allocate_scope();
        let missing_scope = allocate_scope();
        let id = insert(scope, "number", 7u64);
        assert_eq!(with_payload_mut::<u64, _>(0, id, |_| ()), None);
        assert_eq!(with_payload_mut::<u64, _>(missing_scope, id, |_| ()), None);
        assert_eq!(with_payload_mut::<u64, _>(scope, id + 1, |_| ()), None);
        assert_eq!(with_payload_mut::<String, _>(scope, id, |_| ()), None);
        assert_eq!(
            with_payload_mut::<u64, _>(scope, id, |value| {
                *value += 5;
                *value
            }),
            Some(12)
        );
        close_scope(scope);
        assert_eq!(with_payload_mut::<u64, _>(scope, id, |_| ()), None);
    }

    #[test]
    fn nested_scope_projection_never_selects_another_requests_local_id() {
        let first = allocate_scope();
        let second = allocate_scope();
        let first_id = insert(first, "number", 3u64);
        let second_id = insert(second, "number", 11u64);
        assert_eq!(first_id, second_id);
        assert_eq!(with_payload_mut::<u64, _>(first, first_id, |v| *v), Some(3));
        assert_eq!(
            with_payload_mut::<u64, _>(second, second_id, |v| *v),
            Some(11)
        );
        close_scope(first);
        assert_eq!(with_payload_mut::<u64, _>(first, first_id, |_| ()), None);
        assert_eq!(
            with_payload_mut::<u64, _>(second, second_id, |v| *v),
            Some(11)
        );
        close_scope(second);
    }

    #[test]
    fn payload_projection_preserves_id_type_and_mutation_across_small_tables() {
        for width in [0, 1, 2, 4, 7, 8, 33] {
            let mut registry = ResourceRegistry::new();
            let ids: Vec<_> = (0..width)
                .map(|value| registry.insert("number", value as u64))
                .collect();
            let calls = Cell::new(0);
            for missing in [0, -1, i64::MAX] {
                assert_eq!(
                    registry.with_payload_mut::<u64, _>(missing, |_| calls.set(1)),
                    None,
                );
            }
            for (index, &id) in ids.iter().enumerate() {
                assert_eq!(
                    registry.with_payload_mut::<String, _>(id, |_| calls.set(1)),
                    None,
                );
                assert_eq!(
                    registry.with_payload_mut::<u64, _>(id, |value| {
                        assert_eq!(*value, index as u64);
                        *value += 100;
                        *value
                    }),
                    Some(index as u64 + 100),
                );
            }
            assert_eq!(calls.get(), 0, "misses must not invoke the operation");
            for (index, &id) in ids.iter().enumerate() {
                assert_eq!(
                    registry.with_payload_mut::<u64, _>(id, |value| *value),
                    Some(index as u64 + 100),
                );
            }
        }
    }

    #[test]
    fn payload_projection_preserves_retired_ids_and_large_sparse_registries() {
        let mut registry = ResourceRegistry::new();
        let ids: Vec<_> = (0..33)
            .map(|value| registry.insert("number", value as u64))
            .collect();
        for &id in &ids[..31] {
            assert!(registry.close::<u64>(id));
        }
        assert!(registry.next_id > 8);
        assert!(registry.entries.capacity() > 8);
        for &id in &ids[..31] {
            assert_eq!(
                registry.with_payload_mut::<u64, _>(id, |value| *value),
                None
            );
        }
        for (index, &id) in ids.iter().enumerate().skip(31) {
            assert_eq!(
                registry.with_payload_mut::<u64, _>(id, |value| *value),
                Some(index as u64),
            );
        }
        let next = registry.insert("number", 90u64);
        assert!(next > ids[32]);
        assert_eq!(
            registry.with_payload_mut::<u64, _>(next, |value| *value),
            Some(90)
        );
        assert_eq!(
            registry.with_payload_mut::<u64, _>(ids[0], |value| *value),
            None
        );
    }

    #[test]
    fn sparse_retained_scope_table_preserves_projection() {
        let scopes: Vec<_> = (0..32)
            .map(|value| {
                let scope = allocate_scope();
                let id = insert(scope, "number", value as u64);
                (scope, id)
            })
            .collect();
        for &(scope, _) in &scopes[..31] {
            close_scope(scope);
        }
        REQUEST_RESOURCES.with(|registries| assert!(registries.borrow().capacity() > 8));
        let (scope, id) = scopes[31];
        assert_eq!(with_payload_mut::<u64, _>(scope, id, |v| *v), Some(31));
        assert_eq!(with_payload_mut::<u64, _>(scopes[0].0, id, |_| ()), None);
        close_scope(scope);
    }

    #[test]
    fn executor_shutdown_drops_every_request_backend() {
        let drops = Rc::new(Cell::new(0));
        {
            let mut executor = ExecutorGlobals::with_output(Box::new(std::io::sink()));
            let id = insert_for_request(&mut executor, "probe", DropProbe(drops.clone()));
            assert!(is_open_for_request(&executor, id));
            assert_eq!(drops.get(), 0);
        }
        assert_eq!(drops.get(), 1);
    }

    #[test]
    fn public_constants_cannot_select_or_invalidate_resource_scope() {
        let drops = Rc::new(Cell::new(0));
        let mut first = ExecutorGlobals::with_output(Box::new(std::io::sink()));
        let mut second = ExecutorGlobals::with_output(Box::new(std::io::sink()));
        let key = "\0rphp-resource-scope";
        assert_eq!(first.resource_scope, 0);
        assert_eq!(second.resource_scope, 0);
        let first_id = insert_for_request(&mut first, "probe", DropProbe(drops.clone()));
        assert!(!first.constant_table.borrow().contains_key(key));
        second.constant_table.borrow_mut().insert(
            key.into(),
            crate::value::Value::long(first.resource_scope as i64),
        );
        let second_id = insert_for_request(&mut second, "probe", DropProbe(drops.clone()));
        assert_ne!(first.resource_scope, second.resource_scope);
        assert_eq!(first_id, second_id, "ids are local to their request");
        first.constant_table.borrow_mut().clear();
        second.constant_table.borrow_mut().clear();
        assert!(is_open_for_request(&first, first_id));
        assert!(is_open_for_request(&second, second_id));
        drop(first);
        assert_eq!(drops.get(), 1);
        assert!(is_open_for_request(&second, second_id));
        drop(second);
        assert_eq!(drops.get(), 2);
    }

    #[test]
    #[cfg(feature = "resource-lifetime")]
    fn final_value_alias_closes_backend_before_request_shutdown() {
        let drops = Rc::new(Cell::new(0));
        let mut executor = ExecutorGlobals::with_output(Box::new(std::io::sink()));
        let value = insert_value_for_request(&mut executor, "probe", DropProbe(drops.clone()));
        let id = value.as_resource_id().unwrap();
        let alias = value.clone();

        drop(value);
        assert_eq!(drops.get(), 0);
        assert!(is_open_for_request(&executor, id));

        drop(alias);
        assert_eq!(drops.get(), 1);
        assert!(!is_open_for_request(&executor, id));
    }

    #[test]
    #[cfg(feature = "resource-lifetime")]
    fn resource_handle_keeps_value_layout_compact() {
        assert_eq!(std::mem::size_of::<crate::value::Value>(), 16);
    }

    #[test]
    #[cfg(feature = "resource-lifetime")]
    fn repeated_registration_keeps_request_local_ids_and_alias_owners() {
        let first_drops = Rc::new(Cell::new(0));
        let second_drops = Rc::new(Cell::new(0));
        let mut first = ExecutorGlobals::with_output(Box::new(std::io::sink()));
        let mut second = ExecutorGlobals::with_output(Box::new(std::io::sink()));
        for index in 1..=17 {
            let a = insert_value_for_request(&mut first, "probe", DropProbe(first_drops.clone()));
            let b = insert_value_for_request(&mut second, "probe", DropProbe(second_drops.clone()));
            assert_eq!(a.as_resource_id(), Some(index));
            assert_eq!(b.as_resource_id(), Some(index));
            assert_ne!(first.resource_scope, second.resource_scope);
            let alias = a.clone();
            drop(a);
            assert_eq!(first_drops.get(), index as usize - 1);
            assert!(!close_for_request::<String>(&mut first, index));
            assert!(is_open_for_request(&first, index));
            assert!(is_open_for_request(&second, index));
            assert!(close_for_request::<DropProbe>(&mut first, index));
            assert_eq!(first_drops.get(), index as usize);
            assert_eq!(second_drops.get(), index as usize - 1);
            assert_eq!(alias.as_resource_id(), Some(index));
            drop(alias);
            assert_eq!(first_drops.get(), index as usize);
            drop(b);
            assert_eq!(second_drops.get(), index as usize);
        }
        drop(first);
        drop(second);
        assert_eq!(first_drops.get(), 17);
        assert_eq!(second_drops.get(), 17);
    }

    #[test]
    #[cfg(feature = "resource-lifetime")]
    fn rust_unwind_keeps_unconsumed_callback_backend_request_owned() {
        let drops = Rc::new(Cell::new(0));
        let mut executor = ExecutorGlobals::with_output(Box::new(std::io::sink()));
        let value = insert_value_for_request(&mut executor, "probe", DropProbe(drops.clone()));
        let id = value.as_resource_id().unwrap();
        value.set_vm_resource_release(|_, _| panic!("Rust drop must not enter PHP"));
        drop(value);
        assert!(is_open_for_request(&executor, id));
        assert_eq!(drops.get(), 0);
        assert!(close_for_request::<DropProbe>(&mut executor, id));
        assert_eq!(drops.get(), 1);
    }

    #[test]
    #[cfg(feature = "resource-lifetime")]
    fn deferred_release_and_caught_exception_suppression_are_distinct() {
        let drops = Rc::new(Cell::new(0));
        let mut executor = ExecutorGlobals::with_output(Box::new(std::io::sink()));
        let value = insert_value_for_request(&mut executor, "probe", DropProbe(drops.clone()));
        let id = value.as_resource_id().unwrap();
        value.set_vm_resource_release(|_, _| panic!("callback is not eligible"));
        {
            let _defer = crate::resource_handle::ResourceReleaseScope::defer();
            assert!(!value.run_vm_resource_release(&mut executor).unwrap());
            assert!(value.needs_vm_resource_release());
            {
                let _suppress = crate::resource_handle::ResourceReleaseScope::new(true);
                assert!(value.run_vm_resource_release(&mut executor).unwrap());
                assert!(!value.needs_vm_resource_release());
            }
            assert!(crate::resource_handle::php_release_is_deferred());
        }
        assert!(!crate::resource_handle::php_release_is_deferred());
        drop(value);
        assert!(!is_open_for_request(&executor, id));
        assert_eq!(drops.get(), 1);
    }

    #[test]
    #[cfg(feature = "resource-lifetime")]
    fn explicit_close_retires_lifetime_work_without_changing_alias_identity() {
        let drops = Rc::new(Cell::new(0));
        let mut executor = ExecutorGlobals::with_output(Box::new(std::io::sink()));
        let value = insert_value_for_request(&mut executor, "probe", DropProbe(drops.clone()));
        let id = value.as_resource_id().unwrap();
        let alias = value.clone();
        value.set_vm_resource_release(|_, _| panic!("retired callback must never execute"));
        assert!(alias.needs_vm_resource_release());
        assert!(!close_for_request::<String>(&mut executor, id));
        assert!(
            alias.needs_vm_resource_release(),
            "wrong type cannot retire owner"
        );
        assert!(close_for_request::<DropProbe>(&mut executor, id));
        assert_eq!(drops.get(), 1);
        assert_eq!(alias.as_resource_id(), Some(id));
        assert!(!alias.needs_vm_resource_release());
        assert!(!alias.run_vm_resource_release(&mut executor).unwrap());
        assert!(!close_for_request::<DropProbe>(&mut executor, id));
        drop(value);
        drop(alias);
        assert_eq!(drops.get(), 1);
    }

    #[test]
    #[cfg(feature = "resource-lifetime")]
    fn request_shutdown_retires_surviving_resource_aliases() {
        let drops = Rc::new(Cell::new(0));
        let alias;
        {
            let mut executor = ExecutorGlobals::with_output(Box::new(std::io::sink()));
            alias = insert_value_for_request(&mut executor, "probe", DropProbe(drops.clone()));
            alias.set_vm_resource_release(|_, _| panic!("request ended"));
        }
        assert_eq!(drops.get(), 1);
        assert!(!alias.needs_vm_resource_release());
        drop(alias);
        assert_eq!(drops.get(), 1);
    }

    #[test]
    #[cfg(feature = "resource-lifetime")]
    fn backend_drop_can_release_another_resource_without_reentrant_borrow() {
        let drops = Rc::new(Cell::new(0));
        let mut executor = ExecutorGlobals::with_output(Box::new(std::io::sink()));
        let inner = insert_value_for_request(&mut executor, "probe", DropProbe(drops.clone()));
        let inner_id = inner.as_resource_id().unwrap();
        let outer = insert_value_for_request(&mut executor, "nested", inner);
        let outer_id = outer.as_resource_id().unwrap();

        drop(outer);

        assert_eq!(drops.get(), 1);
        assert!(!is_open_for_request(&executor, outer_id));
        assert!(!is_open_for_request(&executor, inner_id));
    }

    #[test]
    #[cfg(feature = "resource-lifetime")]
    fn explicit_backend_close_can_release_a_nested_resource() {
        let drops = Rc::new(Cell::new(0));
        let mut executor = ExecutorGlobals::with_output(Box::new(std::io::sink()));
        let inner = insert_value_for_request(&mut executor, "probe", DropProbe(drops.clone()));
        let inner_id = inner.as_resource_id().unwrap();
        let outer = insert_value_for_request(&mut executor, "nested", inner);
        let outer_id = outer.as_resource_id().unwrap();

        assert!(close_for_request::<crate::value::Value>(
            &mut executor,
            outer_id
        ));
        assert_eq!(drops.get(), 1);
        assert!(!is_open_for_request(&executor, outer_id));
        assert!(!is_open_for_request(&executor, inner_id));
        drop(outer);
        assert_eq!(drops.get(), 1);
    }

    #[test]
    #[cfg(feature = "resource-lifetime")]
    fn explicit_close_then_alias_drops_do_not_drop_backend_twice() {
        let drops = Rc::new(Cell::new(0));
        let mut executor = ExecutorGlobals::with_output(Box::new(std::io::sink()));
        let value = insert_value_for_request(&mut executor, "probe", DropProbe(drops.clone()));
        let id = value.as_resource_id().unwrap();
        let alias = value.clone();

        assert!(close_for_request::<DropProbe>(&mut executor, id));
        assert_eq!(drops.get(), 1);
        drop(value);
        drop(alias);
        assert_eq!(drops.get(), 1);
    }
}
