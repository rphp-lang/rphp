use std::any::{Any, TypeId};
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
    static REQUEST_RESOURCES: RefCell<RequestRegistries> =
        const { RefCell::new(RequestRegistries::Empty) };
}

// Most native I/O belongs to one request on this thread. Store that scope
// directly instead of locating the only occupied hash bucket on every I/O.
// Nested requests promote once to the general table, which never scans its
// retained capacity and does not demote while requests are being removed.
enum RequestRegistries {
    Empty,
    Single(u32, ResourceRegistry),
    Multiple(HashMap<u32, ResourceRegistry>),
}

impl RequestRegistries {
    #[cold]
    fn get(&self, scope: &u32) -> Option<&ResourceRegistry> {
        match self {
            Self::Empty => None,
            Self::Single(registered_scope, registry) => {
                (registered_scope == scope).then_some(registry)
            }
            Self::Multiple(registries) => registries.get(scope),
        }
    }

    #[cold]
    fn get_or_insert(&mut self, scope: u32) -> &mut ResourceRegistry {
        match self {
            Self::Empty => *self = Self::Single(scope, ResourceRegistry::new()),
            Self::Single(registered_scope, _) if *registered_scope != scope => {
                let Self::Single(previous_scope, previous) = std::mem::replace(self, Self::Empty)
                else {
                    unreachable!("single request promotion");
                };
                let mut registries = HashMap::with_capacity(2);
                registries.insert(previous_scope, previous);
                *self = Self::Multiple(registries);
            }
            _ => {}
        }
        match self {
            Self::Single(_, registry) => registry,
            Self::Multiple(registries) => registries.entry(scope).or_default(),
            Self::Empty => unreachable!("request insertion initializes the registry"),
        }
    }

    #[cold]
    fn remove(&mut self, scope: &u32) -> Option<ResourceRegistry> {
        match self {
            Self::Single(registered_scope, _) if registered_scope == scope => {
                let Self::Single(_, registry) = std::mem::replace(self, Self::Empty) else {
                    unreachable!("single request removal");
                };
                Some(registry)
            }
            Self::Multiple(registries) => registries.remove(scope),
            _ => None,
        }
    }
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

// IDs are monotonic and never reused. Index a bounded prefix and retain one
// sparse overflow slot. Overlapping overflow IDs promote to a bounded sorted
// live set, then permanently to hashing. No lookup scans the lifetime ID space
// or a retained hash capacity.
const SMALL_RESOURCE_LIMIT: usize = 8;
// Keep the contiguous tagged entry storage below one 4-KiB page. Above this
// bound retain general hash lookup rather than an unbounded shifting table.
const COMPACT_RESOURCE_LIMIT: usize = 64;

// Allocate this slot once on first overflow, not on every open/close. Boxing
// keeps the common registry enum bounded by its existing HashMap variant.
type SparseResourceSlot = Option<Box<Option<(i64, ResourceEntry)>>>;

enum ResourceEntries {
    Small {
        entries: Vec<Option<ResourceEntry>>,
        overflow: SparseResourceSlot,
    },
    Compact(Vec<(i64, ResourceEntry)>),
    Large(HashMap<i64, ResourceEntry>),
}

impl ResourceEntries {
    #[inline]
    fn get(&self, id: &i64) -> Option<&ResourceEntry> {
        match self {
            Self::Small { entries, overflow } => {
                let index = usize::try_from(*id).ok()?.wrapping_sub(1);
                if index < entries.len() {
                    return entries[index].as_ref();
                }
                let (stored_id, entry) = overflow.as_deref()?.as_ref()?;
                (*stored_id == *id).then_some(entry)
            }
            Self::Compact(entries) => entries
                .binary_search_by_key(id, |(stored_id, _)| *stored_id)
                .ok()
                .map(|index| &entries[index].1),
            Self::Large(entries) => entries.get(id),
        }
    }

    #[inline]
    fn get_mut(&mut self, id: &i64) -> Option<&mut ResourceEntry> {
        match self {
            Self::Small { entries, overflow } => {
                let index = usize::try_from(*id).ok()?.wrapping_sub(1);
                if index < entries.len() {
                    return entries[index].as_mut();
                }
                let (stored_id, entry) = overflow.as_deref_mut()?.as_mut()?;
                (*stored_id == *id).then_some(entry)
            }
            Self::Compact(entries) => entries
                .binary_search_by_key(id, |(stored_id, _)| *stored_id)
                .ok()
                .map(|index| &mut entries[index].1),
            Self::Large(entries) => entries.get_mut(id),
        }
    }

    #[cold]
    fn insert(&mut self, id: i64, entry: ResourceEntry) -> Option<ResourceEntry> {
        debug_assert!(id > 0);
        if let Self::Small { entries, overflow } = self {
            if id <= SMALL_RESOURCE_LIMIT as i64 {
                let index = id as usize - 1;
                if index >= entries.len() {
                    entries.resize_with(index + 1, || None);
                }
                return entries[index].replace(entry);
            }
            let spare = overflow.get_or_insert_with(|| Box::new(None));
            if spare
                .as_ref()
                .as_ref()
                .is_none_or(|(stored_id, _)| *stored_id == id)
            {
                return spare.replace((id, entry)).map(|(_, entry)| entry);
            }
            let mut compact = Vec::with_capacity((entries.len() + 2).next_power_of_two());
            for (index, entry) in entries.drain(..).enumerate() {
                if let Some(entry) = entry {
                    compact.push((index as i64 + 1, entry));
                }
            }
            if let Some((stored_id, entry)) = spare.take() {
                compact.push((stored_id, entry));
            }
            *self = Self::Compact(compact);
        }
        if let Self::Compact(entries) = self {
            // New public IDs are monotonic. Retain replacement/out-of-order
            // insertion for internal entry transfers without a second lookup.
            let index = if entries.last().is_none_or(|(stored_id, _)| *stored_id < id) {
                entries.len()
            } else {
                match entries.binary_search_by_key(&id, |(stored_id, _)| *stored_id) {
                    Ok(index) => return Some(std::mem::replace(&mut entries[index].1, entry)),
                    Err(index) => index,
                }
            };
            if entries.len() < COMPACT_RESOURCE_LIMIT {
                if index == entries.len() {
                    entries.push((id, entry));
                } else {
                    entries.insert(index, (id, entry));
                }
                return None;
            }
            let mut large = HashMap::with_capacity(entries.len() + 1);
            for (stored_id, entry) in entries.drain(..) {
                large.insert(stored_id, entry);
            }
            *self = Self::Large(large);
        }
        let Self::Large(entries) = self else {
            unreachable!("bounded insertion returns before hash migration");
        };
        entries.insert(id, entry)
    }

    #[cold]
    #[inline(never)]
    fn remove_matching(&mut self, id: i64, expected_type: Option<TypeId>) -> Option<ResourceEntry> {
        // The table walk is identical for all concrete backends. Carry only
        // their exact type identity rather than cloning this whole walk into
        // every typed-close caller. None is the native untyped release path.
        let matches = |entry: &ResourceEntry| {
            expected_type.is_none_or(|expected| entry.payload.as_ref().type_id() == expected)
        };
        match self {
            Self::Small { entries, overflow } => {
                let index = usize::try_from(id).ok()?.wrapping_sub(1);
                if index < entries.len() {
                    let slot = &mut entries[index];
                    return if matches(slot.as_ref()?) {
                        slot.take()
                    } else {
                        None
                    };
                }
                let spare = overflow.as_deref_mut()?;
                let (stored_id, entry) = spare.as_ref()?;
                if *stored_id == id && matches(entry) {
                    spare.take().map(|(_, entry)| entry)
                } else {
                    None
                }
            }
            Self::Compact(entries) => {
                let index = entries
                    .binary_search_by_key(&id, |(stored_id, _)| *stored_id)
                    .ok()?;
                matches(&entries[index].1).then(|| entries.remove(index).1)
            }
            Self::Large(entries) => {
                let Entry::Occupied(entry) = entries.entry(id) else {
                    return None;
                };
                matches(entry.get()).then(|| entry.remove())
            }
        }
    }

    #[cold]
    #[cfg(any(feature = "resource-lifetime", feature = "stream-registry"))]
    fn remove(&mut self, id: &i64) -> Option<ResourceEntry> {
        self.remove_matching(*id, None)
    }
}

/// Request-owned PHP resource registry.
///
/// Resource `Value`s contain a stable integer id. With `resource-lifetime`, a
/// shared handle also closes the backend after the final alias disappears.
/// Request shutdown remains the safety net in both configurations.
pub struct ResourceRegistry {
    next_id: i64,
    entries: ResourceEntries,
}

impl ResourceRegistry {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            entries: ResourceEntries::Small {
                entries: Vec::new(),
                overflow: None,
            },
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
        if let Some(replaced) = replaced {
            drop(replaced);
        }
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
        let value = Value::resource_owner(owner);
        if let Some(replaced) = replaced {
            drop(replaced);
        }
        value
    }

    #[inline]
    pub fn is_open(&self, id: i64) -> bool {
        self.entries.get(&id).is_some()
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
        let Some(entry) = self.entries.remove_matching(id, Some(TypeId::of::<T>())) else {
            return false;
        };
        // Validate and remove through one lookup; a wrong type leaves the
        // original entry and owner live.
        entry.retire_owner();
        true
    }

    #[cfg(feature = "resource-lifetime")]
    #[cold]
    fn remove<T: 'static>(&mut self, id: i64) -> Option<ResourceEntry> {
        let entry = self.entries.remove_matching(id, Some(TypeId::of::<T>()))?;
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
        match &self.entries {
            ResourceEntries::Small { entries, overflow } => {
                for entry in entries.iter().flatten() {
                    entry.retire_owner();
                }
                if let Some(Some((_, entry))) = overflow.as_deref() {
                    entry.retire_owner();
                }
            }
            ResourceEntries::Compact(entries) => {
                for (_, entry) in entries {
                    entry.retire_owner();
                }
            }
            ResourceEntries::Large(entries) => {
                for entry in entries.values() {
                    entry.retire_owner();
                }
            }
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
            .get_or_insert(scope)
            .insert(resource_type, payload)
    })
}

#[inline(never)]
// SAFETY: compiler-generated executable code; placement does not change ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn registry_for_scope_mut(
    registries: &mut RequestRegistries,
    scope: u32,
) -> Option<&mut ResourceRegistry> {
    match registries {
        RequestRegistries::Empty => None,
        RequestRegistries::Single(registered_scope, registry) => {
            (*registered_scope == scope).then_some(registry)
        }
        RequestRegistries::Multiple(registries) => registries.get_mut(&scope),
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
    let Some(entry) = entry else {
        return false;
    };
    drop(entry);
    true
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
    if let Some(entry) = entry {
        drop(entry);
    }
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
            .get_or_insert(scope)
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
    fn request_payload_unwind_releases_the_borrow_in_single_and_multiple_scopes() {
        for nested in [false, true] {
            let scope = allocate_scope();
            let id = insert(scope, "number", 7u64);
            let neighbor = nested.then(|| {
                let scope = allocate_scope();
                (scope, insert(scope, "number", 19u64))
            });
            let outcome = std::panic::catch_unwind(|| {
                with_payload_mut::<u64, _>(scope, id, |value| {
                    *value = 11;
                    panic!("native operation interrupted");
                });
            });
            assert!(outcome.is_err());
            assert_eq!(
                with_payload_mut::<u64, _>(scope, id, |value| *value),
                Some(11)
            );
            assert_eq!(with_payload_mut::<String, _>(scope, id, |_| ()), None);
            if let Some((neighbor_scope, neighbor_id)) = neighbor {
                assert_eq!(neighbor_id, id);
                assert_eq!(
                    with_payload_mut::<u64, _>(neighbor_scope, neighbor_id, |value| *value),
                    Some(19)
                );
                close_scope(neighbor_scope);
                assert_eq!(with_payload_mut::<u64, _>(neighbor_scope, id, |_| ()), None);
            }
            close_scope(scope);
            assert_eq!(with_payload_mut::<u64, _>(scope, id, |_| ()), None);
        }
    }

    #[test]
    fn request_registry_promotion_preserves_payloads_and_release_order() {
        let drops = Rc::new(Cell::new(0));
        let mut requests = super::RequestRegistries::Empty;
        let first = requests
            .get_or_insert(11)
            .insert("probe", DropProbe(drops.clone()));
        assert!(matches!(requests, super::RequestRegistries::Single(11, _)));
        let address = super::registry_for_scope_mut(&mut requests, 11)
            .unwrap()
            .with_payload_mut::<DropProbe, _>(first, |probe| probe as *const _ as usize);
        assert!(super::registry_for_scope_mut(&mut requests, 12).is_none());
        assert!(requests.remove(&12).is_none());
        let second = requests.get_or_insert(12).insert("number", 23u64);
        assert_eq!(first, second, "ids stay local to the same scope");
        assert!(matches!(requests, super::RequestRegistries::Multiple(_)));
        assert_eq!(drops.get(), 0, "promotion must not release a backend");
        assert_eq!(
            super::registry_for_scope_mut(&mut requests, 11)
                .unwrap()
                .with_payload_mut::<DropProbe, _>(first, |probe| probe as *const _ as usize),
            address
        );
        let retired = requests.remove(&11).unwrap();
        assert_eq!(drops.get(), 0, "removal transfers ownership to its caller");
        assert!(super::registry_for_scope_mut(&mut requests, 11).is_none());
        assert_eq!(
            super::registry_for_scope_mut(&mut requests, 12)
                .unwrap()
                .with_payload_mut::<u64, _>(second, |number| *number),
            Some(23)
        );
        drop(retired);
        assert_eq!(drops.get(), 1);
        let replacement = requests.get_or_insert(13).insert("number", 51u64);
        assert_eq!(replacement, first);
        assert!(super::registry_for_scope_mut(&mut requests, 11).is_none());
        drop(requests);
        assert_eq!(drops.get(), 1);
    }

    #[test]
    fn single_request_removal_returns_to_empty_without_reusing_scope() {
        let drops = Rc::new(Cell::new(0));
        let mut requests = super::RequestRegistries::Empty;
        let first = requests
            .get_or_insert(17)
            .insert("probe", DropProbe(drops.clone()));
        drop(requests.remove(&17));
        assert_eq!(drops.get(), 1);
        assert!(matches!(requests, super::RequestRegistries::Empty));
        let second = requests
            .get_or_insert(18)
            .insert("probe", DropProbe(drops.clone()));
        assert_eq!(
            second, first,
            "a different request starts its own resource sequence"
        );
        assert!(super::registry_for_scope_mut(&mut requests, 17).is_none());
        assert!(requests.get(&18).unwrap().is_open(second));
        drop(requests);
        assert_eq!(drops.get(), 2);
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
        for width in [0, 1, 2, 4, 7, 8, 33, 64, 65, 129] {
            let mut registry = ResourceRegistry::new();
            let ids: Vec<_> = (0..width)
                .map(|value| registry.insert("number", value as u64))
                .collect();
            let calls = Cell::new(0);
            for missing in [0, -1, i64::MIN, i64::MAX] {
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
        for width in [33, 65, 129] {
            let mut registry = ResourceRegistry::new();
            let ids: Vec<_> = (0..width)
                .map(|value| registry.insert("number", value as u64))
                .collect();
            for &id in &ids[..width - 2] {
                assert!(registry.close::<u64>(id));
            }
            assert!(registry.next_id > 8);
            if width <= super::COMPACT_RESOURCE_LIMIT {
                assert!(
                    matches!(&registry.entries, super::ResourceEntries::Compact(entries) if entries.len() == 2)
                );
            } else {
                assert!(
                    matches!(&registry.entries, super::ResourceEntries::Large(entries) if entries.capacity() > 8)
                );
            }
            for &id in &ids[..width - 2] {
                assert_eq!(
                    registry.with_payload_mut::<u64, _>(id, |value| *value),
                    None
                );
            }
            for (index, &id) in ids.iter().enumerate().skip(width - 2) {
                assert_eq!(
                    registry.with_payload_mut::<u64, _>(id, |value| *value),
                    Some(index as u64),
                );
            }
            let next = registry.insert("number", 90u64);
            assert!(next > ids[width - 1]);
            assert_eq!(
                registry.with_payload_mut::<u64, _>(next, |value| *value),
                Some(90)
            );
            assert_eq!(
                registry.with_payload_mut::<u64, _>(ids[0], |value| *value),
                None
            );
        }
    }

    #[test]
    fn small_registry_migration_moves_backends_without_release_or_id_reuse() {
        let drops = Rc::new(Cell::new(0));
        let mut registry = ResourceRegistry::new();
        let ids: Vec<_> = (0..super::SMALL_RESOURCE_LIMIT)
            .map(|_| registry.insert("probe", DropProbe(drops.clone())))
            .collect();
        assert!(matches!(
            registry.entries,
            super::ResourceEntries::Small { .. }
        ));
        let address = registry
            .with_payload_mut::<DropProbe, _>(ids[0], |probe| probe as *const DropProbe as usize)
            .unwrap();
        assert!(registry.close::<DropProbe>(ids[2]));
        assert_eq!(drops.get(), 1);
        let next = registry.insert("probe", DropProbe(drops.clone()));
        assert_eq!(next, ids.last().unwrap() + 1);
        assert!(matches!(
            registry.entries,
            super::ResourceEntries::Small { .. }
        ));
        let overflow_address =
            registry.with_payload_mut::<DropProbe, _>(next, |probe| probe as *const _ as usize);
        let second_overflow = registry.insert("probe", DropProbe(drops.clone()));
        assert_eq!(second_overflow, next + 1);
        assert!(matches!(
            registry.entries,
            super::ResourceEntries::Compact(_)
        ));
        assert_eq!(drops.get(), 1, "migration must only move entries");
        assert_eq!(
            registry.with_payload_mut::<DropProbe, _>(ids[0], |probe| probe as *const DropProbe
                as usize),
            Some(address)
        );
        assert_eq!(
            registry.with_payload_mut::<DropProbe, _>(next, |probe| probe as *const _ as usize),
            overflow_address
        );
        assert!(!registry.is_open(ids[2]));
        assert_eq!(registry.resource_type(ids[2]), "Unknown");
        assert!(!registry.close::<String>(next));
        for id in ids.into_iter().chain([next, second_overflow]) {
            if registry.is_open(id) {
                assert_eq!(registry.resource_type(id), "probe");
            }
        }
        drop(registry);
        assert_eq!(drops.get(), super::SMALL_RESOURCE_LIMIT + 2);
    }

    #[test]
    fn sparse_overflow_reuses_storage_without_reusing_ids_or_disturbing_live_prefix() {
        for kept in [0, 1, 3, 7, 8] {
            let drops = Rc::new(Cell::new(0));
            let mut registry = ResourceRegistry::new();
            let prefix: Vec<_> = (0..kept)
                .map(|_| registry.insert("probe", DropProbe(drops.clone())))
                .collect();
            let mut previous = 0;
            let mut slot_address = None;
            for turn in 1..=257 {
                let id = registry.insert("probe", DropProbe(drops.clone()));
                assert_eq!(id, kept + turn);
                assert!(!registry.is_open(previous));
                assert_eq!(registry.resource_type(previous), "Unknown");
                assert!(!registry.close::<DropProbe>(previous));
                let super::ResourceEntries::Small { entries, overflow } = &registry.entries else {
                    panic!("one sparse overflow must not require a hash table");
                };
                assert!(
                    entries.len() <= super::SMALL_RESOURCE_LIMIT
                        && entries.capacity() <= super::SMALL_RESOURCE_LIMIT
                );
                if let Some(slot) = overflow {
                    let address = slot.as_ref() as *const _ as usize;
                    if let Some(previous_address) = slot_address {
                        assert_eq!(address, previous_address);
                    }
                    slot_address = Some(address);
                }
                assert!(!registry.close::<String>(id));
                assert!(registry.close::<DropProbe>(id));
                assert_eq!(drops.get(), turn as usize);
                for &pinned in &prefix {
                    assert!(registry.is_open(pinned));
                }
                previous = id;
            }
            drop(registry);
            assert_eq!(drops.get(), kept as usize + 257);
        }
    }

    #[test]
    fn sparse_overflow_promotion_preserves_payloads_and_retired_ids() {
        let mut registry = ResourceRegistry::new();
        for _ in 0..24 {
            let id = registry.insert("number", 0u64);
            assert!(registry.close::<u64>(id));
        }
        let first = registry.insert("number", 77u64);
        assert_eq!(first, 25);
        let address =
            registry.with_payload_mut::<u64, _>(first, |value| value as *const _ as usize);
        for _ in 0..7 {
            let id = registry.insert("number", 1u64);
            assert!(registry.close::<u64>(id));
        }
        let next = registry.insert("number", 99u64);
        assert_eq!(next, 33);
        assert!(matches!(
            &registry.entries,
            super::ResourceEntries::Compact(_)
        ));
        assert_eq!(
            registry.with_payload_mut::<u64, _>(first, |value| value as *const _ as usize),
            address
        );
        assert_eq!(
            registry.with_payload_mut::<u64, _>(first, |value| *value),
            Some(77)
        );
        for retired in (1..first).chain(first + 1..next) {
            assert!(!registry.is_open(retired));
            assert!(!registry.close::<u64>(retired));
        }
        assert!(registry.close::<u64>(first));
        assert!(registry.close::<u64>(next));
        let last = registry.insert("number", 101u64);
        assert_eq!(last, 34);
        assert!(matches!(
            &registry.entries,
            super::ResourceEntries::Compact(_)
        ));
    }

    #[test]
    fn compact_registry_reuses_live_storage_without_reviving_shifted_ids() {
        for width in [2, 5, 9, 17, 61] {
            let drops = Rc::new(Cell::new(0));
            let mut registry = ResourceRegistry::new();
            let pinned: Vec<_> = (0..3)
                .map(|_| registry.insert("probe", DropProbe(drops.clone())))
                .collect();
            let mut previous = Vec::new();
            for turn in 0..32 {
                let current: Vec<_> = (0..width)
                    .map(|_| registry.insert("probe", DropProbe(drops.clone())))
                    .collect();
                assert_eq!(current[0], (4 + turn * width) as i64);
                for &id in &previous {
                    assert!(!registry.is_open(id));
                    assert!(!registry.close::<DropProbe>(id));
                }
                if let super::ResourceEntries::Compact(entries) = &registry.entries {
                    assert!(entries.capacity() <= super::COMPACT_RESOURCE_LIMIT);
                    assert!(entries.windows(2).all(|pair| pair[0].0 < pair[1].0));
                } else {
                    assert!(matches!(
                        registry.entries,
                        super::ResourceEntries::Small { .. }
                    ));
                }
                // Remove from both ends and the middle as positions shift.
                for &id in current
                    .iter()
                    .step_by(2)
                    .chain(current.iter().skip(1).step_by(2))
                {
                    assert!(!registry.close::<String>(id));
                    assert!(registry.close::<DropProbe>(id));
                    assert!(!registry.is_open(id));
                    for &pinned_id in &pinned {
                        assert!(registry.is_open(pinned_id));
                    }
                }
                assert_eq!(drops.get(), (turn + 1) * width);
                previous = current;
            }
            drop(registry);
            assert_eq!(drops.get(), 3 + 32 * width);
        }
    }

    #[test]
    fn compact_hash_promotion_and_entry_transfers_preserve_backend_addresses() {
        let drops = Rc::new(Cell::new(0));
        let mut registry = ResourceRegistry::new();
        let mut addresses = Vec::new();
        for _ in 0..super::COMPACT_RESOURCE_LIMIT {
            let id = registry.insert("probe", DropProbe(drops.clone()));
            let address = registry.with_payload_mut::<DropProbe, _>(id, |v| v as *const _ as usize);
            addresses.push((id, address));
        }
        assert!(
            matches!(&registry.entries, super::ResourceEntries::Compact(entries)
            if entries.len() == super::COMPACT_RESOURCE_LIMIT
                && entries.capacity() <= super::COMPACT_RESOURCE_LIMIT)
        );
        let moved = registry.entries.remove_matching(17, None).unwrap();
        assert!(!registry.is_open(17));
        assert!(registry.entries.insert(17, moved).is_none());
        let replacement = super::ResourceEntry {
            resource_type: "probe",
            payload: Box::new(DropProbe(drops.clone())),
            #[cfg(feature = "resource-lifetime")]
            owner: std::rc::Weak::new(),
        };
        // Replacement at full capacity must not promote or release the old
        // backend before the caller has consumed the returned entry.
        let old = registry.entries.insert(17, replacement).unwrap();
        assert!(matches!(
            registry.entries,
            super::ResourceEntries::Compact(_)
        ));
        assert_eq!(drops.get(), 0);
        let replacement = registry.entries.insert(17, old).unwrap();
        drop(replacement);
        assert_eq!(drops.get(), 1);
        let next = registry.insert("probe", DropProbe(drops.clone()));
        assert_eq!(next, super::COMPACT_RESOURCE_LIMIT as i64 + 1);
        assert!(matches!(registry.entries, super::ResourceEntries::Large(_)));
        assert_eq!(drops.get(), 1);
        for (id, address) in addresses {
            assert_eq!(
                registry.with_payload_mut::<DropProbe, _>(id, |v| v as *const _ as usize),
                address
            );
        }
        drop(registry);
        assert_eq!(drops.get(), super::COMPACT_RESOURCE_LIMIT + 2);
    }

    #[test]
    fn backend_type_identity_is_not_the_public_resource_label() {
        for width in [1, 8, 9, 64, 65, 129] {
            let mut registry = ResourceRegistry::new();
            for index in 0..width {
                let id = if index % 2 == 0 {
                    registry.insert("shared-label", index as u32)
                } else {
                    registry.insert("shared-label", index as u64)
                };
                let entry = registry.entries.get(&id).unwrap();
                let expected = if index % 2 == 0 {
                    std::any::TypeId::of::<u32>()
                } else {
                    std::any::TypeId::of::<u64>()
                };
                assert_eq!(entry.payload.as_ref().type_id(), expected);
            }
            for index in 0..width {
                let id = index as i64 + 1;
                assert_eq!(registry.resource_type(id), "shared-label");
                if index % 2 == 0 {
                    assert!(!registry.close::<u64>(id));
                    assert!(registry.close::<u32>(id));
                } else {
                    assert!(!registry.close::<u32>(id));
                    assert!(registry.close::<u64>(id));
                }
                assert!(!registry.is_open(id));
            }
        }
    }

    #[test]
    fn bounded_registry_storage_retains_the_hash_variant_size_envelope() {
        assert!(
            std::mem::size_of::<super::ResourceEntries>()
                <= std::mem::size_of::<std::collections::HashMap<i64, super::ResourceEntry>>()
                    + std::mem::size_of::<usize>()
        );
        assert!(
            std::mem::size_of::<(i64, super::ResourceEntry)>() * super::COMPACT_RESOURCE_LIMIT
                <= 4096
        );
    }

    #[test]
    fn migrated_payload_unwind_releases_borrow_and_keeps_mutation() {
        let scope = allocate_scope();
        let ids: Vec<_> = (0..12).map(|n| insert(scope, "number", n as u64)).collect();
        let id = ids[1];
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_payload_mut::<u64, _>(scope, id, |value| {
                *value = 91;
                panic!("native operation stopped");
            });
        }));
        assert!(result.is_err());
        assert_eq!(
            with_payload_mut::<u64, _>(scope, id, |value| *value),
            Some(91)
        );
        assert_eq!(with_payload_mut::<String, _>(scope, id, |_| ()), None);
        close_scope(scope);
        assert_eq!(with_payload_mut::<u64, _>(scope, id, |_| ()), None);
    }

    #[test]
    #[cfg(feature = "stream-registry")]
    fn wrapping_preserves_resource_ids_on_both_sides_of_migration() {
        for width in [1, 12, 65] {
            let mut executor = ExecutorGlobals::with_output(Box::new(std::io::sink()));
            let ids: Vec<_> = (0..width)
                .map(|n| insert_for_request(&mut executor, "probe", n as u64))
                .collect();
            let id = ids[0];
            assert!(!super::wrap_request_payload::<String, u64>(
                &mut executor,
                id,
                |_| { panic!("wrong type must not invoke the wrapper") }
            ));
            assert!(super::wrap_request_payload::<u64, String>(
                &mut executor,
                id,
                |n| { format!("wrapped:{n}") }
            ));
            assert!(is_open_for_request(&executor, id));
            assert_eq!(super::type_for_request(&executor, id), "probe");
            assert_eq!(
                super::with_request_payload_mut::<String, _>(&mut executor, id, |s| s.clone()),
                Some("wrapped:0".to_string())
            );
            assert!(!close_for_request::<u64>(&mut executor, id));
            assert!(close_for_request::<String>(&mut executor, id));
            assert!(!is_open_for_request(&executor, id));
            if width > 1 {
                assert_eq!(
                    super::with_request_payload_mut::<u64, _>(&mut executor, ids[1], |n| *n),
                    Some(1)
                );
            }
        }
    }

    #[test]
    #[cfg(feature = "resource-lifetime")]
    fn shutdown_retires_all_aliases_before_any_small_or_large_backend_drop() {
        struct InspectOwners {
            aliases: Rc<std::cell::RefCell<Vec<crate::value::Value>>>,
            drops: Rc<Cell<usize>>,
        }
        impl Drop for InspectOwners {
            fn drop(&mut self) {
                assert!(
                    self.aliases
                        .borrow()
                        .iter()
                        .all(|v| !v.needs_vm_resource_release())
                );
                self.drops.set(self.drops.get() + 1);
            }
        }
        for width in [3, 8, 9, 17, 64, 65, 129] {
            let aliases = Rc::new(std::cell::RefCell::new(Vec::new()));
            let drops = Rc::new(Cell::new(0));
            let mut executor = ExecutorGlobals::with_output(Box::new(std::io::sink()));
            for _ in 0..width {
                let value = insert_value_for_request(
                    &mut executor,
                    "probe",
                    InspectOwners {
                        aliases: aliases.clone(),
                        drops: drops.clone(),
                    },
                );
                value.set_vm_resource_release(|_, _| panic!("retired callback cannot run"));
                aliases.borrow_mut().push(value);
            }
            assert_eq!(drops.get(), 0);
            drop(executor);
            assert_eq!(drops.get(), width);
            aliases.borrow_mut().clear();
            assert_eq!(drops.get(), width);
        }
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
        REQUEST_RESOURCES.with(|registries| {
            assert!(matches!(&*registries.borrow(), super::RequestRegistries::Multiple(table) if table.capacity() > 8));
        });
        let (scope, id) = scopes[31];
        assert_eq!(with_payload_mut::<u64, _>(scope, id, |v| *v), Some(31));
        assert_eq!(with_payload_mut::<u64, _>(scopes[0].0, id, |_| ()), None);
        close_scope(scope);
    }

    #[test]
    fn payload_operation_capture_release_boundaries() {
        struct Capture(Rc<Cell<Option<bool>>>);
        impl Drop for Capture {
            fn drop(&mut self) {
                self.0.set(Some(
                    REQUEST_RESOURCES.with(|registries| registries.try_borrow_mut().is_err()),
                ));
            }
        }
        let scope = allocate_scope();
        let id = insert(scope, "number", 7u64);
        for (case, requested_scope, requested_id, wrong_type, panic) in [
            ("zero scope", 0, id, false, false),
            ("absent scope", allocate_scope(), id, false, false),
            ("absent id", scope, id + 1, false, false),
            ("wrong type", scope, id, true, false),
            ("success", scope, id, false, false),
            ("unwind", scope, id, false, true),
        ] {
            let state = Rc::new(Cell::new(None));
            let capture = Capture(state.clone());
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if wrong_type {
                    with_payload_mut::<String, _>(requested_scope, requested_id, |_| {
                        drop(capture);
                    })
                } else {
                    with_payload_mut::<u64, _>(requested_scope, requested_id, |_| {
                        if panic {
                            panic!("operation unwind");
                        }
                        drop(capture);
                    })
                }
            }));
            assert_eq!(result.is_err(), panic, "{case}");
            assert_eq!(
                state.get(),
                Some(requested_scope == scope),
                "capture release borrow boundary: {case}"
            );
            assert_eq!(with_payload_mut::<u64, _>(scope, id, |v| *v), Some(7));
        }
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
