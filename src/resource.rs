use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

#[cfg(feature = "resource-lifetime")]
use crate::resource_handle::ResourceHandle;
use crate::runtime::ExecutorGlobals;
use crate::value::Value;

const RESOURCE_SCOPE_CONSTANT: &str = "\0rphp-resource-scope";

static NEXT_RESOURCE_SCOPE: AtomicU32 = AtomicU32::new(1);

thread_local! {
    static REQUEST_RESOURCES: RefCell<HashMap<u32, ResourceRegistry>> =
        RefCell::new(HashMap::new());
}

struct ResourceEntry {
    resource_type: &'static str,
    payload: Box<dyn Any>,
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
    pub fn insert<T: 'static>(&mut self, resource_type: &'static str, payload: T) -> i64 {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("PHP resource id overflow");
        let replaced = self.entries.insert(
            id,
            ResourceEntry {
                resource_type,
                payload: Box::new(payload),
            },
        );
        debug_assert!(replaced.is_none());
        id
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

    #[cold]
    pub fn with_payload_mut<T: 'static, R>(
        &mut self,
        id: i64,
        operation: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
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
        if !self
            .entries
            .get(&id)
            .is_some_and(|entry| entry.payload.is::<T>())
        {
            return false;
        }
        self.entries.remove(&id);
        true
    }

    #[cfg(feature = "resource-lifetime")]
    #[cold]
    fn remove<T: 'static>(&mut self, id: i64) -> Option<ResourceEntry> {
        if !self
            .entries
            .get(&id)
            .is_some_and(|entry| entry.payload.is::<T>())
        {
            return None;
        }
        self.entries.remove(&id)
    }

    #[cfg(feature = "resource-lifetime")]
    #[cold]
    fn remove_any(&mut self, id: i64) -> Option<ResourceEntry> {
        self.entries.remove(&id)
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
pub(crate) fn insert<T: 'static>(scope: u32, resource_type: &'static str, payload: T) -> i64 {
    debug_assert_ne!(scope, 0);
    REQUEST_RESOURCES.with(|registries| {
        registries
            .borrow_mut()
            .entry(scope)
            .or_default()
            .insert(resource_type, payload)
    })
}

#[cold]
pub(crate) fn with_payload_mut<T: 'static, R>(
    scope: u32,
    id: i64,
    operation: impl FnOnce(&mut T) -> R,
) -> Option<R> {
    if scope == 0 {
        return None;
    }
    REQUEST_RESOURCES.with(|registries| {
        registries
            .borrow_mut()
            .get_mut(&scope)?
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
        registries
            .borrow_mut()
            .get_mut(&scope)
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
        registries
            .borrow_mut()
            .get_mut(&scope)
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
        registries
            .get_mut(&scope)
            .and_then(|registry| registry.remove_any(id))
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

fn request_scope(eg: &ExecutorGlobals) -> u32 {
    eg.constant_table
        .borrow()
        .get(RESOURCE_SCOPE_CONSTANT)
        .and_then(crate::value::Value::as_long)
        .unwrap_or(0) as u32
}

#[cold]
pub(crate) fn insert_for_request<T: 'static>(
    eg: &mut ExecutorGlobals,
    resource_type: &'static str,
    payload: T,
) -> i64 {
    let mut scope = request_scope(eg);
    if scope == 0 {
        scope = allocate_scope();
        eg.constant_table
            .borrow_mut()
            .insert(RESOURCE_SCOPE_CONSTANT.into(), Value::long(scope as i64));
    }
    insert(scope, resource_type, payload)
}

#[cfg(feature = "resource-lifetime")]
#[cold]
pub(crate) fn insert_value_for_request<T: 'static>(
    eg: &mut ExecutorGlobals,
    resource_type: &'static str,
    payload: T,
) -> Value {
    let id = insert_for_request(eg, resource_type, payload);
    let scope = request_scope(eg);
    debug_assert_ne!(scope, 0);
    Value::resource(ResourceHandle::new(scope, id, close_any))
}

#[cold]
pub(crate) fn with_request_payload_mut<T: 'static, R>(
    eg: &mut ExecutorGlobals,
    id: i64,
    operation: impl FnOnce(&mut T) -> R,
) -> Option<R> {
    with_payload_mut(request_scope(eg), id, operation)
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
        ResourceRegistry, allocate_scope, close_for_request, close_scope, insert,
        insert_for_request, is_open, is_open_for_request, resource_type,
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
