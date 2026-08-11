use std::cell::Cell;
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::compiler::compile::{ClassDef, PropertyDefinition};
#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
use crate::generics::GenericType;
use crate::generics::{GenericMetadata, GenericMethodContract, ReifiedBinding};
use crate::parser::Visibility;
use crate::value::{ObjectLayout, PhpArray, Value};
use crate::vm::frame::ExecuteData;
use crate::vm::function::FunctionCommon;
use crate::vm::stack::VmStack;
use crate::vm::stats;

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[path = "generic_contracts.rs"]
mod generic_contracts;
#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[path = "generic_properties.rs"]
mod generic_properties;
#[cfg(feature = "php-generics-reified")]
#[path = "generic_reified_values.rs"]
mod generic_reified_values;
#[cfg(feature = "php-generics-reified")]
#[path = "generic_scopes.rs"]
mod generic_scopes;
#[cfg(feature = "php-generics-reified")]
use generic_scopes::{ActiveReifiedBindingScope, PendingReifiedBindingScope};

#[cfg(feature = "php-generics-reified")]
#[derive(Clone)]
struct ReifiedObjectBinding {
    identity: usize,
    object: std::rc::Weak<std::cell::RefCell<crate::value::PhpObject>>,
    binding: ReifiedBinding,
}

#[cfg(feature = "php-generics-reified")]
struct ReifiedNestedArgumentsBinding {
    identity: usize,
    object: std::rc::Weak<std::cell::RefCell<crate::value::PhpObject>>,
    owner_name_identity: usize,
    owner_name_len: usize,
    arguments: Box<[GenericType]>,
    binding_expected_arguments: usize,
    binding_expected_len: usize,
    binding_site: usize,
    binding_matches: bool,
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[derive(Clone)]
struct GenericMethodContractBinding {
    class_id: u32,
    declaration: u32,
    use_site: Option<u32>,
    receiver_can_reify: bool,
    contract: std::rc::Rc<GenericMethodContract>,
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
struct GenericPropertyContractBinding {
    declaration: u32,
    use_site: Option<u32>,
    property: Box<str>,
    scope: Box<str>,
    expected: GenericType,
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
fn take_generic_member_call(
    entries: &mut Vec<(usize, std::rc::Rc<GenericMethodContract>)>,
    call: usize,
) -> Option<std::rc::Rc<GenericMethodContract>> {
    if entries
        .last()
        .is_some_and(|(candidate, _)| *candidate == call)
    {
        return entries.pop().map(|(_, contract)| contract);
    }
    let position = entries
        .iter()
        .rposition(|(candidate, _)| *candidate == call)?;
    Some(entries.remove(position).1)
}

/// Mangle a private property name: `ClassName\0propname`.
/// Public/protected properties are stored under their plain name.
#[inline]
pub fn mangle_private_prop(class_name: &str, prop_name: &str) -> String {
    format!("{}\0{}", class_name, prop_name)
}

/// Resolve the storage key for a property access.
/// - Private: use caller class scope to build mangled key
/// - Public/protected: plain name
pub fn resolve_property_key(
    eg: &ExecutorGlobals,
    obj_class: &str,
    prop_name: &str,
    caller_class: Option<&str>,
) -> String {
    // If caller is in a class scope, check if that class declares this property as private
    if let Some(caller) = caller_class {
        if let Some((Visibility::Private, defining_class)) =
            eg.find_property_visibility(caller, prop_name)
        {
            if defining_class.eq_ignore_ascii_case(caller) {
                return mangle_private_prop(&defining_class, prop_name);
            }
        }
    }
    // Otherwise, check if the property is private in the target class hierarchy
    if let Some((Visibility::Private, defining_class)) =
        eg.find_property_visibility(obj_class, prop_name)
    {
        return mangle_private_prop(&defining_class, prop_name);
    }
    prop_name.to_string()
}

/// Merge inherited declarations while preserving PHP's private-slot rule.
/// The same rule applies to instance and static properties; keeping it here
/// prevents their registration paths from drifting.
fn inherit_property_definitions(
    child: &mut Vec<PropertyDefinition>,
    parent: &[PropertyDefinition],
) {
    let child_names: std::collections::HashSet<&str> = child
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    let mut inherited = Vec::new();
    for property in parent {
        if child_names.contains(property.name.as_str()) {
            if property.visibility == Visibility::Private
                && child.iter().any(|child_property| {
                    child_property.name == property.name
                        && child_property.visibility == Visibility::Private
                })
            {
                inherited.push(property.clone());
            }
        } else {
            inherited.push(property.clone());
        }
    }
    child.extend(inherited);
}

/// Static declarations additionally carry a storage identity. An inherited
/// declaration reuses its parent's slot; a redeclaration keeps the child's
/// independently allocated slot.
fn inherit_static_property_definitions(
    child: &mut Vec<PropertyDefinition>,
    child_slots: &mut Vec<Option<u32>>,
    parent: &[PropertyDefinition],
    parent_slots: &[u32],
) {
    debug_assert_eq!(child.len(), child_slots.len());
    debug_assert_eq!(parent.len(), parent_slots.len());
    let child_names: std::collections::HashSet<&str> = child
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    let mut inherited = Vec::new();
    for (index, property) in parent.iter().enumerate() {
        let keep = if child_names.contains(property.name.as_str()) {
            property.visibility == Visibility::Private
                && child.iter().any(|child_property| {
                    child_property.name == property.name
                        && child_property.visibility == Visibility::Private
                })
        } else {
            true
        };
        if keep {
            inherited.push((property.clone(), parent_slots[index]));
        }
    }
    for (definition, slot) in inherited {
        child.push(definition);
        child_slots.push(Some(slot));
    }
}

#[inline]
fn property_definitions_are_compatible(
    left_default: &Option<Value>,
    left_visibility: Visibility,
    right_default: &Option<Value>,
    right_visibility: Visibility,
) -> bool {
    left_visibility == right_visibility
        && match (left_default, right_default) {
            (None, None) => true,
            (Some(left), Some(right)) => left.structurally_equal(right),
            _ => false,
        }
}

/// A trait static property is composed into the consuming class, not shared
/// with the trait or unrelated consumers. Since PHP 8.3, using the same trait
/// again in a child also creates storage distinct from the parent's inherited
/// property. Class/trait and trait/trait declarations still have to be
/// compatible; a trait declaration simply replaces an inherited declaration.
fn merge_trait_static_property_definitions(
    target: &mut Vec<PropertyDefinition>,
    target_slots: &mut Vec<Option<u32>>,
    source: &[PropertyDefinition],
    class_name: &str,
    trait_name: &str,
    own_names: &std::collections::HashSet<String>,
    composed_names: &mut std::collections::HashSet<String>,
) -> Result<(), String> {
    debug_assert_eq!(target.len(), target_slots.len());
    for property in source {
        let existing = target
            .iter()
            .position(|candidate| candidate.name == property.name);
        if own_names.contains(&property.name) || composed_names.contains(&property.name) {
            let index = existing.expect("own/composed static property definition");
            let existing_property = &target[index];
            if !property_definitions_are_compatible(
                &existing_property.default,
                existing_property.visibility,
                &property.default,
                property.visibility,
            ) {
                return Err(format!(
                    "{} and {} define the same property (${}) in the composition of {}. \
                     However, the definition differs and is considered incompatible",
                    existing_property.declaring_class, trait_name, property.name, class_name
                ));
            }
            continue;
        }

        let mut definition = property.clone();
        definition.declaring_class = trait_name.to_string();
        if let Some(index) = existing {
            // A first trait declaration in this class overrides inherited
            // metadata and receives a fresh storage slot.
            target[index] = definition;
            target_slots[index] = None;
        } else {
            target.push(definition);
            target_slots.push(None);
        }
        composed_names.insert(property.name.clone());
    }
    Ok(())
}

/// Merge one trait's declarations into a consuming class. Instance and static
/// tables both use this exact collision contract, but remain separate storage.
fn merge_trait_property_definitions(
    target: &mut Vec<PropertyDefinition>,
    source: &[PropertyDefinition],
    class_name: &str,
    trait_name: &str,
) -> Result<(), String> {
    let mut additions = Vec::new();
    for property in source {
        if let Some(existing_property) = target
            .iter()
            .find(|candidate| candidate.name == property.name)
        {
            if existing_property.declaring_class == class_name {
                continue;
            }
            let compatible = existing_property.visibility == property.visibility
                && property_definitions_are_compatible(
                    &property.default,
                    property.visibility,
                    &existing_property.default,
                    existing_property.visibility,
                );
            if !compatible {
                return Err(format!(
                    "{} and {} define the same property (${}) in the composition of {}. \
                     However, the definition differs and is considered incompatible",
                    existing_property.declaring_class, trait_name, property.name, class_name
                ));
            }
            continue;
        }
        let mut addition = property.clone();
        addition.declaring_class = trait_name.to_string();
        additions.push(addition);
    }
    target.extend(additions);
    Ok(())
}

/// Minimal ExecutorGlobals for vertical slice.
/// Will grow as we implement more features.
pub struct ExecutorGlobals {
    pub vm_stack: VmStack,
    /// Compact argument-only activations for deferred pure-scalar calls.
    pub pending_call_stack: VmStack,
    pub current_execute_data: Cell<*mut ExecuteData>,
    pub vm_interrupt: AtomicBool,
    pub timed_out: AtomicBool,
    /// Function table — name → pointer to FunctionCommon
    pub function_table: HashMap<String, *const FunctionCommon>,
    /// Class table — name → ClassDef (Boxed for stable pointer addresses)
    pub class_table: HashMap<String, Box<ClassDef>>,
    /// Cold generic declaration side table. Ordinary dispatch never reads it.
    pub generic_metadata: GenericMetadata,
    /// Constant table — name → Value (case-sensitive, like PHP)
    /// Uses RefCell to allow define() from internal functions (which receive &self).
    pub constant_table: std::cell::RefCell<HashMap<String, crate::value::Value>>,
    /// Parsed and compiled regular expressions shared by all preg_* calls for
    /// the lifetime of this executor.
    pub regex_cache: crate::regex::RegexCache,
    /// Exception being thrown — None = no exception
    pub exception: Option<crate::value::Value>,
    /// Reverse map: func_ptr → declaring class name (for visibility scope resolution)
    pub method_declaring_class: HashMap<*const FunctionCommon, String>,
    /// Output buffer — collected output for testing, or stdout
    output: std::cell::RefCell<Box<dyn Write>>,
    /// Temporary buffer for named variadic arguments.
    /// Key = call frame pointer as usize, value = vec of (name, value) pairs.
    /// Populated by SendNamed when target function is variadic and name isn't a declared param.
    /// Consumed by DoFcall during variadic packing.
    pub pending_named_variadic: HashMap<usize, Vec<(String, crate::value::Value)>>,
    /// Active generator being executed (set during resume, used by Yield opcode)
    pub active_generator: Option<crate::vm::generator::GeneratorRef>,
    /// Global variables — shared across function calls via `global $x;`
    pub globals: HashMap<String, crate::value::Value>,
    /// Globals modified by the last callee Return (for selective re-read by caller)
    pub dirty_globals: std::collections::HashSet<String>,
    /// Static variables — persisted across function calls: func_name → (var_name → value)
    pub static_vars: HashMap<String, HashMap<String, crate::value::Value>>,
    /// Packed internal `(call frame, $this)` pairs for dynamically resolved
    /// `__invoke` calls. The existing Option remains the cheap hot-path marker.
    pub pending_invoke_this: Option<crate::value::Value>,
    /// Set of absolute file paths already included via include_once/require_once
    pub included_files: std::collections::HashSet<String>,
    /// Owned storage for functions/data from included files (prevents dangling pointers)
    pub included_functions: Vec<Box<crate::vm::function::UserFunction>>,
    /// Monotonically increasing counter for class IDs
    next_class_id: u32,
    /// Stable boxed ClassDef pointers indexed by class ID. Slot zero is
    /// reserved for dynamic/unknown classes.
    class_by_id: Vec<*const ClassDef>,
    /// LIFO binding sidecar used only by explicit reified calls.
    #[cfg(feature = "php-generics-reified")]
    pub reified_bindings: Vec<ReifiedBinding>,
    /// Explicit generic bindings are born in a caller before its call frame is
    /// active. Scope sidecars make success, abandoned arguments and exception
    /// unwinding remove the exact binding instead of leaving stale LIFO state.
    #[cfg(feature = "php-generics-reified")]
    pending_reified_binding_scopes: Vec<PendingReifiedBindingScope>,
    #[cfg(feature = "php-generics-reified")]
    active_reified_binding_scopes: Vec<ActiveReifiedBindingScope>,
    /// Sparse called-class scope for explicit signatures containing relative
    /// class types such as `self`, `parent`, or late-bound `static`.
    /// Ordinary generic calls never allocate or push into this sidecar.
    #[cfg(feature = "php-generics-reified")]
    reified_binding_scope_classes: Vec<(usize, u32)>,
    /// Object identity → canonical type arguments. Weak ownership prevents a
    /// recycled allocation from inheriting a stale binding; periodic
    /// exponential sweeps keep construction amortized O(1).
    #[cfg(feature = "php-generics-reified")]
    reified_objects: HashMap<usize, ReifiedObjectBinding>,
    /// Weak one-entry L0 for the overwhelmingly common repeated-property
    /// receiver. RefCell is cold feature-only state and never enters an
    /// ordinary build or changes the object itself.
    #[cfg(feature = "php-generics-reified")]
    reified_object_cache: std::cell::RefCell<Option<ReifiedObjectBinding>>,
    /// One-entry cache of the concrete generic arguments that one object
    /// exposes through reification or a linked concrete ancestor. Nested checks are
    /// allocation-free after the first monomorphic hit.
    #[cfg(feature = "php-generics-reified")]
    reified_nested_arguments_cache: std::cell::RefCell<Option<ReifiedNestedArgumentsBinding>>,
    #[cfg(feature = "php-generics-reified")]
    reified_object_sweep_at: usize,
    /// Call-frame identity → substituted instance-method contract. Pending
    /// entries have not crossed DoFcall yet; active entries are consumed by
    /// the matching Return. Both remain feature-only cold sidecars.
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    pending_generic_member_calls: Vec<(usize, std::rc::Rc<GenericMethodContract>)>,
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    active_generic_member_calls: Vec<(usize, std::rc::Rc<GenericMethodContract>)>,
    /// One-entry binding+method cache makes repeated calls on the same
    /// monomorphic generic receiver allocation-free after warmup.
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    generic_method_contract_cache: std::cell::RefCell<Option<GenericMethodContractBinding>>,
    /// One-entry fully substituted property contract. Cold resolution may
    /// compose an arbitrary inheritance chain; warm writes only compare the
    /// receiver binding/name and validate against this owned type.
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    generic_property_contract_cache: std::cell::RefCell<Option<GenericPropertyContractBinding>>,
    /// Canonical mutable storage for declared static properties. Appending new
    /// runtime state here preserves the layout of every pre-existing field.
    /// Inline caches keep only an index into this vector, so reallocation
    /// cannot invalidate a warmed site and inherited declarations can share
    /// one slot exactly.
    static_property_values: Vec<Value>,
    /// Per-class property-index → canonical storage-slot mapping. Slot zero in
    /// this outer vector is reserved alongside `class_by_id`.
    static_property_slots_by_class: Vec<Box<[u32]>>,
}

impl ExecutorGlobals {
    /// Reserve the stable built-in registry envelope immediately before stdlib
    /// registration. Executors that never install stdlib stay allocation-lazy;
    /// normal executors avoid repeated hash-table growth while installing the
    /// fixed built-in class and function set.
    pub(crate) fn reserve_stdlib_capacity(&mut self) {
        self.function_table.reserve(256);
        self.class_table.reserve(64);
        self.method_declaring_class.reserve(96);
        self.class_by_id.reserve(64);
        self.static_property_slots_by_class.reserve(64);
        self.static_property_values.reserve(16);
    }

    pub fn new() -> Self {
        Self {
            vm_stack: VmStack::new(),
            pending_call_stack: VmStack::new_pending(),
            current_execute_data: Cell::new(std::ptr::null_mut()),
            vm_interrupt: AtomicBool::new(false),
            timed_out: AtomicBool::new(false),
            function_table: HashMap::new(),
            class_table: HashMap::new(),
            generic_metadata: GenericMetadata::default(),
            #[cfg(feature = "php-generics-reified")]
            reified_bindings: Vec::new(),
            #[cfg(feature = "php-generics-reified")]
            pending_reified_binding_scopes: Vec::new(),
            #[cfg(feature = "php-generics-reified")]
            active_reified_binding_scopes: Vec::new(),
            #[cfg(feature = "php-generics-reified")]
            reified_binding_scope_classes: Vec::new(),
            #[cfg(feature = "php-generics-reified")]
            reified_objects: HashMap::new(),
            #[cfg(feature = "php-generics-reified")]
            reified_object_cache: std::cell::RefCell::new(None),
            #[cfg(feature = "php-generics-reified")]
            reified_nested_arguments_cache: std::cell::RefCell::new(None),
            #[cfg(feature = "php-generics-reified")]
            reified_object_sweep_at: 256,
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            pending_generic_member_calls: Vec::new(),
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            active_generic_member_calls: Vec::new(),
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            generic_method_contract_cache: std::cell::RefCell::new(None),
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            generic_property_contract_cache: std::cell::RefCell::new(None),
            constant_table: std::cell::RefCell::new(HashMap::new()),
            regex_cache: crate::regex::RegexCache::default(),
            exception: None,
            method_declaring_class: HashMap::new(),

            output: std::cell::RefCell::new(Box::new(std::io::stdout())),
            pending_named_variadic: HashMap::new(),
            active_generator: None,
            globals: HashMap::new(),
            dirty_globals: std::collections::HashSet::new(),
            static_vars: HashMap::new(),
            pending_invoke_this: None,
            included_files: std::collections::HashSet::new(),
            included_functions: Vec::new(),
            next_class_id: 1,
            class_by_id: vec![std::ptr::null()],
            static_property_values: Vec::new(),
            static_property_slots_by_class: vec![Box::new([])],
        }
    }

    /// Create EG with captured output (for testing)
    pub fn with_output(output: Box<dyn Write>) -> Self {
        Self {
            vm_stack: VmStack::new(),
            pending_call_stack: VmStack::new_pending(),
            current_execute_data: Cell::new(std::ptr::null_mut()),
            vm_interrupt: AtomicBool::new(false),
            timed_out: AtomicBool::new(false),
            function_table: HashMap::new(),
            class_table: HashMap::new(),
            generic_metadata: GenericMetadata::default(),
            #[cfg(feature = "php-generics-reified")]
            reified_bindings: Vec::new(),
            #[cfg(feature = "php-generics-reified")]
            pending_reified_binding_scopes: Vec::new(),
            #[cfg(feature = "php-generics-reified")]
            active_reified_binding_scopes: Vec::new(),
            #[cfg(feature = "php-generics-reified")]
            reified_binding_scope_classes: Vec::new(),
            #[cfg(feature = "php-generics-reified")]
            reified_objects: HashMap::new(),
            #[cfg(feature = "php-generics-reified")]
            reified_object_cache: std::cell::RefCell::new(None),
            #[cfg(feature = "php-generics-reified")]
            reified_nested_arguments_cache: std::cell::RefCell::new(None),
            #[cfg(feature = "php-generics-reified")]
            reified_object_sweep_at: 256,
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            pending_generic_member_calls: Vec::new(),
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            active_generic_member_calls: Vec::new(),
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            generic_method_contract_cache: std::cell::RefCell::new(None),
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            generic_property_contract_cache: std::cell::RefCell::new(None),
            constant_table: std::cell::RefCell::new(HashMap::new()),
            regex_cache: crate::regex::RegexCache::default(),
            exception: None,
            method_declaring_class: HashMap::new(),

            output: std::cell::RefCell::new(output),
            pending_named_variadic: HashMap::new(),
            active_generator: None,
            globals: HashMap::new(),
            dirty_globals: std::collections::HashSet::new(),
            static_vars: HashMap::new(),
            pending_invoke_this: None,
            included_files: std::collections::HashSet::new(),
            included_functions: Vec::new(),
            next_class_id: 1,
            class_by_id: vec![std::ptr::null()],
            static_property_values: Vec::new(),
            static_property_slots_by_class: vec![Box::new([])],
        }
    }

    /// Reuse the existing cold packed call-side state so ordinary builds do
    /// not grow or reorder ExecutorGlobals. The high-bit tag cannot collide
    /// with a valid Rust allocation pointer on supported targets.
    #[cold]
    #[inline(never)]
    pub(crate) fn push_late_static_scope(&mut self, call: usize, class_id: u32) {
        if class_id == 0 {
            return;
        }
        const TAG: usize = 1usize << (usize::BITS - 1);
        debug_assert_eq!(call & TAG, 0);
        let pending = self
            .pending_invoke_this
            .get_or_insert_with(|| Value::array(PhpArray::with_packed_capacity(4)));
        let stack = pending
            .as_array_mut()
            .expect("pending call side state must remain a packed array");
        stack.push(Value::long((call | TAG) as i64));
        stack.push(Value::long(class_id as i64));
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn late_static_scope_class_id(&self, call: usize) -> u32 {
        const TAG: usize = 1usize << (usize::BITS - 1);
        let Some(stack) = self.pending_invoke_this.as_ref().and_then(Value::as_array) else {
            return 0;
        };
        let Some(key_index) = stack.len().checked_sub(2) else {
            return 0;
        };
        if stack
            .get_value_at(key_index)
            .and_then(Value::as_long)
            .map(|key| key as usize)
            != Some(call | TAG)
        {
            return 0;
        }
        stack
            .get_value_at(key_index + 1)
            .and_then(Value::as_long)
            .map_or(0, |class_id| class_id as u32)
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn discard_late_static_scope(&mut self, call: usize) {
        const TAG: usize = 1usize << (usize::BITS - 1);
        let Some(pending) = self.pending_invoke_this.as_mut() else {
            return;
        };
        let stack = pending
            .as_array_mut()
            .expect("pending call side state must remain a packed array");
        let Some(key_index) = stack.len().checked_sub(2) else {
            return;
        };
        if stack
            .get_value_at(key_index)
            .and_then(Value::as_long)
            .map(|key| key as usize)
            != Some(call | TAG)
        {
            return;
        }
        let _class_id = stack.pop();
        let _call = stack.pop();
        if stack.is_empty() {
            self.pending_invoke_this = None;
        }
    }

    #[cfg(feature = "php-generics-reified")]
    pub(crate) fn bind_reified_object(
        &mut self,
        value: &crate::value::Value,
        binding: ReifiedBinding,
    ) {
        let Some(object) = value.as_object_rc() else {
            return;
        };
        if self.reified_objects.len() >= self.reified_object_sweep_at {
            self.reified_objects
                .retain(|_, entry| entry.object.strong_count() != 0);
            self.reified_object_sweep_at = self
                .reified_objects
                .len()
                .saturating_add(1)
                .max(256)
                .checked_next_power_of_two()
                .unwrap_or(usize::MAX);
        }
        let identity = std::rc::Rc::as_ptr(&object) as usize;
        let entry = ReifiedObjectBinding {
            identity,
            object: std::rc::Rc::downgrade(&object),
            binding,
        };
        self.reified_object_cache.replace(Some(entry.clone()));
        self.reified_objects.insert(identity, entry);
    }

    #[cfg(feature = "php-generics-reified")]
    pub(crate) fn reified_object_binding(
        &self,
        value: &crate::value::Value,
    ) -> Option<ReifiedBinding> {
        let object = value.as_object_rc()?;
        let identity = std::rc::Rc::as_ptr(&object) as usize;
        if let Some(entry) = self.reified_object_cache.borrow().as_ref() {
            if entry.identity == identity && entry.object.strong_count() != 0 {
                return Some(entry.binding);
            }
        }
        let entry = self.reified_objects.get(&identity)?;
        if entry.object.strong_count() == 0 {
            return None;
        }
        let binding = entry.binding;
        self.reified_object_cache.replace(Some(entry.clone()));
        Some(binding)
    }

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    pub(crate) fn generic_instance_method_contract(
        &self,
        object: &crate::value::Value,
        method: &str,
    ) -> Option<std::rc::Rc<GenericMethodContract>> {
        let (class_id, class_name) = {
            let object = object.as_object()?;
            if let Some(cached) = self.generic_method_contract_cache.borrow().as_ref()
                && cached.class_id == object.class_id
                && !cached.receiver_can_reify
                && cached.contract.method.eq_ignore_ascii_case(method)
            {
                return Some(cached.contract.clone());
            }
            (object.class_id, object.class_name.clone())
        };
        let declaration = self.generic_metadata.find_class_like_index(&class_name)?;
        let receiver_can_reify = cfg!(feature = "php-generics-reified")
            && self
                .generic_metadata
                .declarations()
                .get(declaration as usize)
                .is_some_and(|declaration| !declaration.parameters.is_empty());

        #[cfg(feature = "php-generics-reified")]
        if receiver_can_reify && let Some(binding) = self.reified_object_binding(object) {
            if let Some(cached) = self.generic_method_contract_cache.borrow().as_ref() {
                if cached.declaration == binding.declaration
                    && cached.use_site == Some(binding.use_site)
                    && cached.contract.method.eq_ignore_ascii_case(method)
                {
                    return Some(cached.contract.clone());
                }
            }
            if let Some(mut contract) = self
                .generic_metadata
                .reified_instance_method_contract(binding, method)
            {
                let scope = self.generic_declaration_scope(&contract.scope, Some(&class_name));
                if scope != contract.scope.as_ref() {
                    contract.scope = scope.into();
                }
                if class_name.as_ref() != contract.called_scope.as_ref() {
                    contract.called_scope = class_name.as_ref().into();
                }
                let contract = std::rc::Rc::new(contract);
                self.generic_method_contract_cache
                    .replace(Some(GenericMethodContractBinding {
                        class_id,
                        declaration: binding.declaration,
                        use_site: Some(binding.use_site),
                        receiver_can_reify,
                        contract: contract.clone(),
                    }));
                return Some(contract);
            }
        }

        if let Some(cached) = self.generic_method_contract_cache.borrow().as_ref() {
            if cached.declaration == declaration
                && cached.use_site.is_none()
                && cached.contract.method.eq_ignore_ascii_case(method)
            {
                return Some(cached.contract.clone());
            }
        }
        let mut contract = self
            .generic_metadata
            .linked_instance_method_contract(declaration, method)?;
        let scope = self.generic_declaration_scope(&contract.scope, Some(&class_name));
        if scope != contract.scope.as_ref() {
            contract.scope = scope.into();
        }
        if class_name.as_ref() != contract.called_scope.as_ref() {
            contract.called_scope = class_name.as_ref().into();
        }
        let contract = std::rc::Rc::new(contract);
        self.generic_method_contract_cache
            .replace(Some(GenericMethodContractBinding {
                class_id,
                declaration,
                use_site: None,
                receiver_can_reify,
                contract: contract.clone(),
            }));
        Some(contract)
    }

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    pub(crate) fn push_pending_generic_member_call(
        &mut self,
        call: usize,
        contract: std::rc::Rc<GenericMethodContract>,
    ) {
        self.pending_generic_member_calls.push((call, contract));
    }

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    pub(crate) fn take_pending_generic_member_call(
        &mut self,
        call: usize,
    ) -> Option<std::rc::Rc<GenericMethodContract>> {
        take_generic_member_call(&mut self.pending_generic_member_calls, call)
    }

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    pub(crate) fn activate_generic_member_call(
        &mut self,
        call: usize,
        contract: std::rc::Rc<GenericMethodContract>,
    ) {
        self.active_generic_member_calls.push((call, contract));
    }

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    pub(crate) fn take_active_generic_member_call(
        &mut self,
        call: usize,
    ) -> Option<std::rc::Rc<GenericMethodContract>> {
        take_generic_member_call(&mut self.active_generic_member_calls, call)
    }

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    pub(crate) fn active_generic_member_call(&self, call: usize) -> Option<&GenericMethodContract> {
        self.active_generic_member_calls
            .iter()
            .rfind(|(candidate, _)| *candidate == call)
            .map(|(_, contract)| contract.as_ref())
    }

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    pub(crate) fn discard_generic_member_call(&mut self, call: usize) {
        let _ = take_generic_member_call(&mut self.pending_generic_member_calls, call);
        let _ = take_generic_member_call(&mut self.active_generic_member_calls, call);
    }

    /// Register a class definition and its methods in the function table.
    /// Resolves inheritance: merges parent properties/methods into child.
    /// For non-interface, non-abstract classes: validates interface contracts.
    pub fn register_class(&mut self, mut class_def: ClassDef) -> Result<(), String> {
        let class_name = class_def.name.clone();
        // PHP does not permit class redeclaration. Besides matching that rule,
        // this guarantees class_by_id pointers remain stable for inline caches.
        if self.class_table.contains_key(&class_name) {
            return Err(format!(
                "Cannot declare class {}, because the name is already in use",
                class_name
            ));
        }
        let class_table = &self.class_table;
        self.generic_metadata
            .validate_inheritance(&class_name, |actual, bound| {
                class_is_a_in_table(class_table, actual, bound)
            })?;
        self.generic_metadata.validate_variance_for(&class_name)?;
        let class_table = &self.class_table;
        self.generic_metadata
            .validate_parametric_lsp(&class_name, |actual, bound| {
                class_is_a_in_table(class_table, actual, bound)
            })?;
        // Assign stable class ID
        let id = self.next_class_id;
        self.next_class_id += 1;
        class_def.class_id = id;
        let own_static_names = class_def
            .static_properties
            .iter()
            .map(|property| property.name.clone())
            .collect::<std::collections::HashSet<_>>();
        // `None` denotes a declaration composed by this class and therefore a
        // fresh slot. Inherited entries carry the parent's canonical slot.
        let mut static_property_slots = vec![None; class_def.static_properties.len()];

        // Check if parent is final — cannot extend a final class
        if let Some(parent_name) = &class_def.parent {
            if let Some(parent) = self.class_table.get(parent_name.as_str()) {
                if parent.is_final {
                    return Err(format!(
                        "Class {} cannot extend final class {}",
                        class_name, parent_name
                    ));
                }
            }
        }

        // Resolve inheritance — merge parent's properties and methods
        if let Some(parent_name) = &class_def.parent {
            if let Some(parent) = self.class_table.get(parent_name.as_str()) {
                // Own declarations stay first, so late-static lookup sees a
                // redeclaration before inherited storage.
                inherit_property_definitions(&mut class_def.properties, &parent.properties);
                let parent_static_slots = self
                    .static_property_slots_by_class
                    .get(parent.class_id as usize)
                    .map_or(&[][..], |slots| slots.as_ref());
                inherit_static_property_definitions(
                    &mut class_def.static_properties,
                    &mut static_property_slots,
                    &parent.static_properties,
                    parent_static_slots,
                );

                // Inherit readonly property list from parent
                for ro in &parent.readonly_props {
                    if !class_def.readonly_props.contains(ro) {
                        class_def.readonly_props.push(ro.clone());
                    }
                }

                // Inherit methods: collect ALL parent::* entries from function_table
                // (includes transitively inherited methods from grandparents)
                let child_method_names: std::collections::HashSet<String> = class_def
                    .methods
                    .iter()
                    .map(|(n, _, _, _, _)| n.to_lowercase())
                    .collect();
                let parent_prefix = format!("{}::", parent_name).to_lowercase();
                let inherited: Vec<(String, *const FunctionCommon)> = self
                    .function_table
                    .iter()
                    .filter(|(k, _)| k.starts_with(&parent_prefix))
                    .map(|(k, v)| {
                        let method_name = &k[parent_prefix.len()..];
                        (method_name.to_string(), *v)
                    })
                    .collect();
                for (method_name, func_ptr) in inherited {
                    if !child_method_names.contains(&method_name) {
                        let child_full = format!("{}::{}", class_name, method_name).to_lowercase();
                        self.function_table.insert(child_full, func_ptr);
                    }
                }
            }
        }

        // Merge traits: copy trait methods and properties into this class.
        // Must happen after parent inheritance so trait methods override inherited ones
        // (matching PHP semantics: trait > parent, class > trait).
        let trait_names = class_def.uses.clone();
        let mut composed_static_trait_names = std::collections::HashSet::new();
        for trait_name in &trait_names {
            if let Some(trait_def) = self.class_table.get(trait_name.as_str()) {
                merge_trait_property_definitions(
                    &mut class_def.properties,
                    &trait_def.properties,
                    &class_name,
                    trait_name,
                )?;
                merge_trait_static_property_definitions(
                    &mut class_def.static_properties,
                    &mut static_property_slots,
                    &trait_def.static_properties,
                    &class_name,
                    trait_name,
                    &own_static_names,
                    &mut composed_static_trait_names,
                )?;

                // Merge trait methods: copy function_table pointers
                let child_method_names: std::collections::HashSet<String> = class_def
                    .methods
                    .iter()
                    .map(|(n, _, _, _, _)| n.to_lowercase())
                    .collect();
                let trait_prefix = format!("{}::", trait_name).to_lowercase();
                let trait_methods: Vec<(String, *const FunctionCommon)> = self
                    .function_table
                    .iter()
                    .filter(|(k, _)| k.starts_with(&trait_prefix))
                    .map(|(k, v)| {
                        let method_name = &k[trait_prefix.len()..];
                        (method_name.to_string(), *v)
                    })
                    .collect();
                for (method_name, func_ptr) in trait_methods {
                    if !child_method_names.contains(&method_name) {
                        let child_full = format!("{}::{}", class_name, method_name).to_lowercase();
                        self.function_table.insert(child_full, func_ptr);
                        // Also record declaring class as this class for visibility purposes
                        self.method_declaring_class
                            .insert(func_ptr, class_name.clone());
                    }
                }
            } else {
                return Err(format!("Trait not found: {}", trait_name));
            }
        }

        // Do NOT inherit method stubs from interfaces — interface methods are contracts,
        // not implementations. The implementing class must provide its own body.
        // (Interface stub functions exist only in the interface's own ClassDef for type info.)

        // Check for final method override violations:
        // If child overrides a parent method marked as final → error.
        if let Some(parent_name) = &class_def.parent {
            let child_method_names: Vec<String> = class_def
                .methods
                .iter()
                .map(|(n, _, _, _, _)| n.to_lowercase())
                .collect();
            for child_method in &child_method_names {
                // Walk parent chain to find if this method is final
                let mut ancestor = Some(parent_name.clone());
                while let Some(ref anc_name) = ancestor {
                    if let Some(anc_def) = self.class_table.get(anc_name.as_str()) {
                        for (m_name, _vis, _is_static, is_final, _func) in &anc_def.methods {
                            if m_name.to_lowercase() == *child_method && *is_final {
                                return Err(format!(
                                    "Cannot override final method {}::{}()",
                                    anc_name, m_name
                                ));
                            }
                        }
                        ancestor = anc_def.parent.clone();
                    } else {
                        break;
                    }
                }
            }
        }

        let mut resolved_static_slots = Vec::with_capacity(static_property_slots.len());
        for (definition, inherited_slot) in class_def
            .static_properties
            .iter()
            .zip(static_property_slots)
        {
            let slot = if let Some(slot) = inherited_slot {
                slot
            } else {
                let slot = u32::try_from(self.static_property_values.len())
                    .map_err(|_| "Too many static property storage slots".to_string())?;
                self.static_property_values
                    .push(definition.default.clone().unwrap_or_else(Value::null));
                slot
            };
            resolved_static_slots.push(slot);
        }

        // Property order is now final. Build one shared storage-key → slot
        // layout for every object instance of this class.
        let property_keys = class_def
            .properties
            .iter()
            .map(|property| {
                if property.visibility == Visibility::Private {
                    mangle_private_prop(&property.declaring_class, &property.name)
                } else {
                    property.name.clone()
                }
            })
            .collect();
        class_def.property_layout =
            std::rc::Rc::new(ObjectLayout::new(class_name.as_str(), property_keys));
        class_def.property_defaults = class_def
            .properties
            .iter()
            .map(|property| {
                property.default.clone().unwrap_or_else(|| {
                    if class_def.readonly_props.contains(&property.name) {
                        crate::value::Value::undef()
                    } else {
                        crate::value::Value::null()
                    }
                })
            })
            .collect::<Vec<_>>()
            .into();

        // Box to get stable heap address for function pointers
        self.class_table
            .insert(class_name.clone(), Box::new(class_def));
        let class_ptr = &**self.class_table.get(&class_name).unwrap() as *const ClassDef;
        let class_id = unsafe { (*class_ptr).class_id as usize };
        if self.class_by_id.len() <= class_id {
            self.class_by_id.resize(class_id + 1, std::ptr::null());
        }
        self.class_by_id[class_id] = class_ptr;
        if self.static_property_slots_by_class.len() <= class_id {
            self.static_property_slots_by_class
                .resize_with(class_id + 1, || Box::new([]));
        }
        self.static_property_slots_by_class[class_id] = resolved_static_slots.into_boxed_slice();
        // Register child's own method pointers from the stable location
        let class = self.class_table.get(&class_name).unwrap();
        let method_entries: Vec<(String, *const FunctionCommon)> = class
            .methods
            .iter()
            .map(|(method_name, _vis, _is_static, _is_final, func)| {
                let full_name = format!("{}::{}", class_name, method_name).to_lowercase();
                let func_ptr = &func.common as *const FunctionCommon;
                (full_name, func_ptr)
            })
            .collect();
        for (full_name, func_ptr) in &method_entries {
            self.function_table.insert(full_name.clone(), *func_ptr);
        }
        // Populate declaring_class reverse map
        for (_full_name, func_ptr) in method_entries {
            self.method_declaring_class
                .insert(func_ptr, class_name.clone());
        }

        // Validate interface contracts for concrete classes
        let missing = self.validate_interface_contracts(&class_name);
        if !missing.is_empty() {
            let (iface, method) = &missing[0];
            return Err(format!(
                "Class {} contains 1 abstract method and must therefore be declared abstract or implement the remaining methods ({}::{})",
                class_name, iface, method
            ));
        }

        Ok(())
    }

    /// O(1) metadata lookup used after a monomorphic class site resolves.
    #[inline(always)]
    pub fn class_by_id(&self, class_id: u32) -> Option<&ClassDef> {
        let ptr = *self.class_by_id.get(class_id as usize)?;
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &*ptr })
        }
    }

    /// Resolve one class-local declaration index to its canonical mutable
    /// static-storage slot. This is used only on an inline-cache miss.
    #[inline]
    pub(crate) fn static_property_storage_slot(
        &self,
        class_id: u32,
        property_index: usize,
    ) -> Option<usize> {
        self.static_property_slots_by_class
            .get(class_id as usize)?
            .get(property_index)
            .map(|slot| *slot as usize)
    }

    #[inline(always)]
    pub(crate) fn static_property_value(&self, storage_slot: usize) -> Option<&Value> {
        self.static_property_values.get(storage_slot)
    }

    /// A warmed static-property cache can skip the bounds branch: storage is
    /// append-only for the executor lifetime and cache slots are published
    /// only after checked resolution.
    #[inline(always)]
    pub(crate) unsafe fn static_property_value_unchecked(&self, storage_slot: usize) -> &Value {
        debug_assert!(storage_slot < self.static_property_values.len());
        unsafe { self.static_property_values.get_unchecked(storage_slot) }
    }

    /// Update canonical storage while preserving a reference wrapper if one is
    /// introduced by the general reference surface in a later slice.
    #[inline(always)]
    pub(crate) fn set_static_property_value(&mut self, storage_slot: usize, value: Value) -> bool {
        if storage_slot >= self.static_property_values.len() {
            return false;
        }
        unsafe { self.set_static_property_value_unchecked(storage_slot, value) };
        true
    }

    /// Mutable counterpart of `static_property_value_unchecked`; callers must
    /// hold a cache slot produced by checked static-property resolution.
    #[inline(always)]
    pub(crate) unsafe fn set_static_property_value_unchecked(
        &mut self,
        storage_slot: usize,
        value: Value,
    ) {
        debug_assert!(storage_slot < self.static_property_values.len());
        let current = unsafe { self.static_property_values.get_unchecked_mut(storage_slot) };
        if current.is_reference() {
            unsafe {
                let target = current.as_ref_ptr();
                std::ptr::drop_in_place(target);
                target.write(value);
            }
        } else {
            *current = value;
        }
    }

    /// Check if a class is an instance of another (walks parent chain AND implements)
    pub fn class_is_a(&self, class_name: &str, target: &str) -> bool {
        if class_name.eq_ignore_ascii_case(target) {
            return true;
        }
        if let Some(class_def) = self.class_table.get(class_name) {
            // Check parent class
            if let Some(parent) = &class_def.parent {
                if self.class_is_a(parent, target) {
                    return true;
                }
            }
            // Check implemented interfaces
            for iface in &class_def.implements {
                if self.class_is_a(iface, target) {
                    return true;
                }
            }
        }
        false
    }

    /// Match a class name used by interned property metadata in one concrete
    /// class scope. Properties cannot declare `static`, so lexical and called
    /// scope are intentionally identical here.
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    pub(crate) fn class_is_a_in_generic_scope(
        &self,
        class_name: &str,
        target: &str,
        scope: &str,
    ) -> bool {
        self.generic_type_name_in_scopes(target, scope, Some(scope))
            .is_some_and(|target| self.class_is_a(class_name, target))
    }

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    pub(crate) fn class_is_a_in_generic_scopes(
        &self,
        class_name: &str,
        target: &str,
        lexical_scope: &str,
        called_scope: Option<&str>,
    ) -> bool {
        self.generic_type_name_in_scopes(target, lexical_scope, called_scope)
            .is_some_and(|target| self.class_is_a(class_name, target))
    }

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    pub(crate) fn generic_type_name_in_scopes<'a>(
        &'a self,
        target: &'a str,
        lexical_scope: &'a str,
        called_scope: Option<&'a str>,
    ) -> Option<&'a str> {
        let scope = lexical_scope
            .split_once("::")
            .map_or(lexical_scope, |(class, _)| class);
        if target.eq_ignore_ascii_case("self") {
            Some(scope)
        } else if target.eq_ignore_ascii_case("parent") {
            self.class_table
                .get(scope)
                .and_then(|class| class.parent.as_deref())
        } else if target.eq_ignore_ascii_case("static") {
            called_scope.map(|scope| scope.split_once("::").map_or(scope, |(class, _)| class))
        } else {
            Some(target)
        }
    }

    /// Resolve the lexical class scope of generic metadata. Trait bodies are
    /// special: PHP binds their `self`/`parent` pseudo-types to the nearest
    /// class in the receiver hierarchy that actually consumed the trait.
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    pub(crate) fn generic_declaration_scope<'a>(
        &'a self,
        declared_scope: &'a str,
        receiver_scope: Option<&'a str>,
    ) -> &'a str {
        let declared_scope = declared_scope
            .split_once("::")
            .map_or(declared_scope, |(class, _)| class);
        if !self
            .class_table
            .get(declared_scope)
            .is_some_and(|class| class.is_trait)
        {
            return declared_scope;
        }
        let Some(mut candidate) = receiver_scope else {
            return declared_scope;
        };
        while let Some(class) = self.class_table.get(candidate) {
            if class
                .uses
                .iter()
                .any(|used| used.eq_ignore_ascii_case(declared_scope))
            {
                return class.name.as_str();
            }
            let Some(parent) = class.parent.as_deref() else {
                break;
            };
            candidate = parent;
        }
        receiver_scope.unwrap_or(declared_scope)
    }

    /// Find the class scope that owns one concrete property declaration. The
    /// compiled property table retains its original owner across inheritance;
    /// trait owners are then rebound to their consuming class.
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    pub(crate) fn generic_property_scope<'a>(
        &'a self,
        receiver_scope: &'a str,
        property: &str,
    ) -> &'a str {
        let declared_scope = self
            .class_table
            .get(receiver_scope)
            .and_then(|class| {
                class
                    .properties
                    .iter()
                    .find(|definition| definition.name == property)
            })
            .map(|definition| definition.declaring_class.as_str())
            .unwrap_or(receiver_scope);
        self.generic_declaration_scope(declared_scope, Some(receiver_scope))
    }

    /// Get the class_id for a given class name. Returns 0 if not found.
    #[inline]
    pub fn class_id_of(&self, class_name: &str) -> u32 {
        self.class_table.get(class_name).map_or(0, |cd| cd.class_id)
    }

    /// Cold metadata query used by Reflection to distinguish an ancestor
    /// interface from a parent class with the same reachability relation.
    pub fn class_is_interface(&self, class_name: &str) -> bool {
        self.class_table
            .get(class_name)
            .is_some_and(|class| class.is_interface)
    }

    /// Get the declaring class for a function pointer.
    pub fn declaring_class_of(&self, func_ptr: *const FunctionCommon) -> Option<&str> {
        self.method_declaring_class
            .get(&func_ptr)
            .map(|s| s.as_str())
    }

    /// Return the class or trait that owns the concrete method body.
    ///
    /// Imported and inherited methods have aliases in `function_table`, while
    /// their `UserFunction` remains stored exactly once on the declaration
    /// that compiled the body. Generic metadata follows that original owner,
    /// so cold generic call resolution uses pointer identity to recover it.
    #[cold]
    #[inline(never)]
    pub fn method_definition_owner(
        &self,
        func_ptr: *const FunctionCommon,
        method_name: &str,
    ) -> Option<&str> {
        self.class_table.values().find_map(|class| {
            class
                .methods
                .iter()
                .any(|(name, _, _, _, function)| {
                    name.eq_ignore_ascii_case(method_name)
                        && std::ptr::eq(&function.common, func_ptr)
                })
                .then_some(class.name.as_str())
        })
    }

    /// Collect all required interface method signatures (recursively through interface extends).
    /// Returns Vec of (method_name_lower, visibility, num_args, required_num_args, is_static, interface_name, return_type_hint, param_type_hints).
    fn collect_interface_methods(
        &self,
        iface_name: &str,
    ) -> Vec<(
        String,
        Visibility,
        u32,
        u32,
        bool,
        String,
        crate::vm::function::ParamTypeHint,
        Vec<crate::vm::function::ParamTypeHint>,
    )> {
        let mut result = Vec::new();
        if let Some(iface_def) = self.class_table.get(iface_name) {
            for (method_name, vis, is_static, _is_final, func) in &iface_def.methods {
                result.push((
                    method_name.to_lowercase(),
                    *vis,
                    func.common.sig.num_args,
                    func.common.sig.required_num_args,
                    *is_static,
                    iface_name.to_string(),
                    func.common.sig.return_type_hint.clone(),
                    func.common.sig.param_type_hints.clone(),
                ));
            }
            // Recurse into parent interfaces
            for parent_iface in &iface_def.implements {
                result.extend(self.collect_interface_methods(parent_iface));
            }
        }
        result
    }

    /// Collect all interfaces for a class (direct + inherited from parents).
    fn collect_all_interfaces(&self, class_name: &str) -> Vec<String> {
        let mut result = Vec::new();
        if let Some(class_def) = self.class_table.get(class_name) {
            result.extend(class_def.implements.clone());
            if let Some(parent) = &class_def.parent {
                result.extend(self.collect_all_interfaces(parent));
            }
        }
        result
    }

    /// Validate that a concrete class implements all required interface methods.
    /// Checks: existence, public visibility, static compatibility, parameter arity.
    /// Returns a list of (interface_name, error_description), empty if all satisfied.
    pub fn validate_interface_contracts(&self, class_name: &str) -> Vec<(String, String)> {
        let mut errors = Vec::new();
        if let Some(class_def) = self.class_table.get(class_name) {
            if class_def.is_interface || class_def.is_abstract || class_def.is_trait {
                return errors; // interfaces/abstract classes/traits don't need to implement
            }
        }
        // Collect interfaces from the entire parent chain (fix P2: inherited obligations)
        let all_ifaces = self.collect_all_interfaces(class_name);
        let mut seen = std::collections::HashSet::new();
        for iface_name in all_ifaces {
            if !seen.insert(iface_name.clone()) {
                continue;
            }
            let required = self.collect_interface_methods(&iface_name);
            for (
                method,
                _iface_vis,
                iface_nargs,
                iface_required,
                iface_static,
                declaring_iface,
                iface_return_hint,
                iface_param_hints,
            ) in required
            {
                let full = format!("{}::{}", class_name, method).to_lowercase();
                if !self.function_table.contains_key(&full) {
                    errors.push((declaring_iface.clone(), method.clone()));
                    continue;
                }
                // Check visibility and staticness
                if let Some((impl_vis, impl_is_static, _)) =
                    self.find_method_info(class_name, &method)
                {
                    if impl_vis != Visibility::Public {
                        errors.push((
                            declaring_iface.clone(),
                            format!(
                                "{} (must be public, is {:?})",
                                method,
                                match impl_vis {
                                    Visibility::Protected => "protected",
                                    Visibility::Private => "private",
                                    _ => "public",
                                }
                            ),
                        ));
                    }
                    // Static mismatch: non-static interface method cannot be implemented as static
                    // and vice versa.
                    if iface_static && !impl_is_static {
                        errors.push((
                            declaring_iface.clone(),
                            format!("{} (must be static as declared in interface)", method),
                        ));
                    }
                    if !iface_static && impl_is_static {
                        errors.push((
                            declaring_iface.clone(),
                            format!(
                                "{} (cannot be static, interface declares it non-static)",
                                method
                            ),
                        ));
                    }
                }
                // Check parameter count compatibility:
                // - Implementation must accept at least as many total params as the interface
                // - Implementation must not REQUIRE more params than the interface DECLARES
                //   (total), because a caller following the interface contract may pass
                //   only `iface_nargs` arguments.
                // - Implementation must not REQUIRE more params than the interface REQUIRES,
                //   because a caller following the interface contract may pass only
                //   `iface_required` arguments (the rest are optional on the interface side).
                if let Some(func_ptr) = self.function_table.get(&full) {
                    let impl_common = unsafe { &**func_ptr };
                    let iface_public = iface_nargs; // interface stubs don't have this_offset
                    let impl_public = impl_common.sig.num_args - impl_common.sig.this_offset;
                    // required_num_args is NOT adjusted for this_offset in the compiler,
                    // so it already represents the public required parameter count.
                    let impl_required = impl_common.sig.required_num_args;
                    // Implementation must accept at least as many params as the interface
                    if impl_public < iface_public {
                        errors.push((
                            declaring_iface.clone(),
                            format!(
                                "{} (requires {} params, implementation has {})",
                                method, iface_public, impl_public
                            ),
                        ));
                    }
                    // Implementation must not require more params than the interface declares
                    // (total), and also must not require more than the interface requires.
                    // The stricter bound is iface_required: a valid caller may pass only
                    // that many arguments and the implementation must still work.
                    if impl_required > iface_required {
                        errors.push((declaring_iface.clone(), format!(
                            "{} (implementation requires {} params, interface requires only {})",
                            method, impl_required, iface_required
                        )));
                    }
                    // Check parameter type compatibility (contravariance):
                    // Interface param A => implementation must accept A or a supertype of A.
                    // Parametric signatures were already checked against every substituted
                    // path. Their erased raw hints are not a second, authoritative contract.
                    use crate::vm::function::ParamTypeHint;
                    let parametric = self
                        .generic_metadata
                        .method_has_parametric_signature(&declaring_iface, &method);
                    if !parametric {
                        let check_count = iface_param_hints
                            .len()
                            .max(impl_common.sig.param_type_hints.len());
                        for i in 0..check_count {
                            let iface_param = iface_param_hints.get(i);
                            let impl_param = impl_common.sig.param_type_hints.get(i);
                            match (impl_param, iface_param) {
                                // Both untyped or both absent — ok
                                (
                                    None | Some(ParamTypeHint::None),
                                    None | Some(ParamTypeHint::None),
                                ) => {}
                                // Impl has no type / mixed — always compatible
                                (
                                    None | Some(ParamTypeHint::None) | Some(ParamTypeHint::Mixed),
                                    Some(_),
                                ) => {}
                                // Interface has no type but impl adds a type — narrowing, rejected
                                (Some(impl_p), None | Some(ParamTypeHint::None)) => {
                                    if !matches!(impl_p, ParamTypeHint::Mixed) {
                                        errors.push((declaring_iface.clone(), format!(
                                        "{} (parameter {} must not add type {}, interface has no type)",
                                        method, i + 1,
                                        impl_p.display_name()
                                    )));
                                    }
                                }
                                // Both have types — check contravariance
                                (Some(impl_p), Some(iface_p)) => {
                                    if !self.is_param_type_compatible(impl_p, iface_p) {
                                        errors.push((declaring_iface.clone(), format!(
                                        "{} (parameter {} type must be compatible with {}, got {})",
                                        method, i + 1,
                                        iface_p.display_name(),
                                        impl_p.display_name()
                                    )));
                                    }
                                }
                            }
                        }

                        // Check return type compatibility: if the interface declares a return type,
                        // the implementation must declare the same or a covariant return type.
                        if !matches!(iface_return_hint, ParamTypeHint::None) {
                            let impl_return = &impl_common.sig.return_type_hint;
                            if !self.is_return_type_compatible(impl_return, &iface_return_hint) {
                                errors.push((
                                    declaring_iface.clone(),
                                    format!(
                                        "{} (return type must be compatible with {}, got {})",
                                        method,
                                        iface_return_hint.display_name(),
                                        impl_return.display_name()
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
        }
        errors
    }

    /// Look up a method's visibility in a class hierarchy.
    /// Returns (visibility, declaring_class_name) — the class where the method is actually defined.
    pub fn find_method_visibility(
        &self,
        class_name: &str,
        method_name: &str,
    ) -> Option<(Visibility, String)> {
        self.find_method_info(class_name, method_name)
            .map(|(vis, _, decl)| (vis, decl))
    }

    /// Look up method visibility AND staticness in a class hierarchy.
    /// Returns (visibility, is_static, declaring_class_name).
    pub fn find_method_info(
        &self,
        class_name: &str,
        method_name: &str,
    ) -> Option<(Visibility, bool, String)> {
        let method_lower = method_name.to_lowercase();
        if let Some(class_def) = self.class_table.get(class_name) {
            // Check own methods
            for (name, vis, is_static, _is_final, _func) in &class_def.methods {
                if name.to_lowercase() == method_lower {
                    return Some((*vis, *is_static, class_name.to_string()));
                }
            }
            // Check used traits (trait methods are copied to function_table but not to methods vec)
            for trait_name in &class_def.uses {
                if let Some(trait_def) = self.class_table.get(trait_name.as_str()) {
                    for (name, vis, is_static, _is_final, _func) in &trait_def.methods {
                        if name.to_lowercase() == method_lower {
                            // Trait method visibility applies as if declared in the using class
                            return Some((*vis, *is_static, class_name.to_string()));
                        }
                    }
                }
            }
            // Check parent
            if let Some(parent) = &class_def.parent {
                return self.find_method_info(parent, method_name);
            }
        }
        None
    }

    /// Look up a property's visibility in a class hierarchy.
    /// Returns (visibility, declaring_class_name).
    pub fn find_property_visibility(
        &self,
        class_name: &str,
        prop_name: &str,
    ) -> Option<(Visibility, String)> {
        if let Some(class_def) = self.class_table.get(class_name) {
            for property in &class_def.properties {
                if property.name == prop_name {
                    return Some((property.visibility, property.declaring_class.clone()));
                }
            }
            // Check parent
            if let Some(parent) = &class_def.parent {
                return self.find_property_visibility(parent, prop_name);
            }
        }
        None
    }

    /// Check if `caller_class` can access a member with `visibility` defined in `target_class`.
    pub fn check_visibility(
        &self,
        caller_class: Option<&str>,
        target_class: &str,
        visibility: Visibility,
    ) -> bool {
        match visibility {
            Visibility::Public => true,
            Visibility::Protected => {
                if let Some(caller) = caller_class {
                    // Caller must be same class or in inheritance chain
                    self.class_is_a(caller, target_class) || self.class_is_a(target_class, caller)
                } else {
                    false
                }
            }
            Visibility::Private => {
                if let Some(caller) = caller_class {
                    caller.eq_ignore_ascii_case(target_class)
                } else {
                    false
                }
            }
        }
    }

    /// Register a function by name. Returns error if already declared.
    pub fn register_function(
        &mut self,
        name: &str,
        func: *const FunctionCommon,
    ) -> Result<(), String> {
        let key = name.to_lowercase();
        if self.function_table.contains_key(&key) {
            return Err(format!("Cannot redeclare {}()", name));
        }
        self.function_table.insert(key, func);
        Ok(())
    }

    /// Look up a function by name.
    /// Fast path: try exact match first (names are stored lowercase),
    /// fall back to case-insensitive search only if needed.
    #[inline]
    pub fn find_function(&self, name: &str) -> Option<*const FunctionCommon> {
        // Fast path: direct lookup (works when name is already lowercase)
        if let Some(&ptr) = self.function_table.get(name) {
            stats::inc_find_function_exact_hit();
            return Some(ptr);
        }
        // Slow path: allocate lowercase string
        let lower = name.to_lowercase();
        if lower != name {
            let found = self.function_table.get(&lower).copied();
            if found.is_some() {
                stats::inc_find_function_lower_hit();
            } else {
                stats::inc_find_function_miss();
            }
            found
        } else {
            stats::inc_find_function_miss();
            None
        }
    }

    /// Define a constant. Returns error if already defined.
    pub fn define_constant(&self, name: &str, value: crate::value::Value) -> Result<(), String> {
        let mut table = self.constant_table.borrow_mut();
        if table.contains_key(name) {
            return Err(format!("Constant {} already defined", name));
        }
        table.insert(name.to_string(), value);
        Ok(())
    }

    /// Look up a constant by name (case-sensitive).
    pub fn find_constant(&self, name: &str) -> Option<crate::value::Value> {
        if let Some(val) = self.constant_table.borrow().get(name).cloned() {
            return Some(val);
        }
        // Built-in PHP constants (shared source of truth)
        crate::builtin_constant(name)
    }

    /// Check if an implementation's return type is compatible with (covariant to) an interface's
    /// declared return type. Rules:
    /// - Same type → compatible
    /// - ClassName vs ClassName → compatible if impl class `is_a` interface class (covariance)
    /// - None (no type declared) when interface declares one → incompatible
    /// - Nullable: ?T is compatible with ?T, T is compatible with ?T (narrowing is fine)
    /// - Mixed accepts anything
    fn is_return_type_compatible(
        &self,
        impl_hint: &crate::vm::function::ParamTypeHint,
        iface_hint: &crate::vm::function::ParamTypeHint,
    ) -> bool {
        use crate::vm::function::ParamTypeHint;

        // If implementation has no type hint but interface does, incompatible
        // (even if interface declares `mixed` — PHP requires explicit declaration)
        if matches!(impl_hint, ParamTypeHint::None) {
            return false;
        }

        // `never` is the bottom type — covariant with any return type
        if matches!(impl_hint, ParamTypeHint::Never) {
            return true;
        }

        // If interface says Mixed, any explicit type is compatible — except void
        if matches!(iface_hint, ParamTypeHint::Mixed) {
            return !matches!(impl_hint, ParamTypeHint::Void);
        }

        // Exact match
        if impl_hint == iface_hint {
            return true;
        }

        // Nullable unwrapping: impl T is compatible with iface ?T (narrowing)
        // impl ?T is compatible with iface ?T (checked above by equality)
        match (impl_hint, iface_hint) {
            (_, ParamTypeHint::Nullable(inner_iface)) => {
                // impl_hint (non-nullable or differently nullable) vs ?T
                // Check if impl is compatible with the inner type
                return self.is_return_type_compatible(impl_hint, inner_iface);
            }
            (ParamTypeHint::Nullable(_), _) => {
                // impl ?T vs iface T (widening) — incompatible
                return false;
            }
            _ => {}
        }

        // Covariant return compatibility is ordinary subtype checking over
        // union/intersection nodes.
        if let ParamTypeHint::Intersection(iface_parts) = iface_hint {
            return iface_parts
                .iter()
                .all(|part| self.is_return_type_compatible(impl_hint, part));
        }
        if let ParamTypeHint::Union(impl_parts) = impl_hint {
            return impl_parts
                .iter()
                .all(|part| self.is_return_type_compatible(part, iface_hint));
        }
        if let ParamTypeHint::Union(iface_parts) = iface_hint {
            return iface_parts
                .iter()
                .any(|part| self.is_return_type_compatible(impl_hint, part));
        }
        if let ParamTypeHint::Intersection(impl_parts) = impl_hint {
            return impl_parts
                .iter()
                .any(|part| self.is_return_type_compatible(part, iface_hint));
        }

        // Class name covariance
        match (impl_hint, iface_hint) {
            (ParamTypeHint::ClassName(impl_class), ParamTypeHint::ClassName(iface_class)) => {
                return self.class_is_a(impl_class, iface_class);
            }
            _ => {}
        }

        // Everything else: incompatible
        false
    }

    /// Check if an implementation's parameter type is compatible with an interface's
    /// declared parameter type (contravariance).
    /// The implementation must accept at least as much as the interface declares:
    /// - Same type → compatible
    /// - ClassName vs ClassName → compatible if iface class `is_a` impl class (contravariance:
    ///   impl accepts a supertype)
    /// - Nullable: ?T in impl is compatible with T in iface (impl accepts more)
    /// - Mixed in impl → always compatible (accepts anything)
    fn is_param_type_compatible(
        &self,
        impl_hint: &crate::vm::function::ParamTypeHint,
        iface_hint: &crate::vm::function::ParamTypeHint,
    ) -> bool {
        use crate::vm::function::ParamTypeHint;

        // Mixed accepts anything — always compatible
        if matches!(impl_hint, ParamTypeHint::Mixed) {
            return true;
        }

        // Exact match
        if impl_hint == iface_hint {
            return true;
        }

        // Nullable handling (contravariance):
        // impl ?T vs iface T → ok (impl accepts more: T + null)
        // impl ?T vs iface ?T → already caught by exact match above
        // impl T vs iface ?T → INCOMPATIBLE (impl rejects null that iface promises to accept)
        match (impl_hint, iface_hint) {
            (ParamTypeHint::Nullable(inner_impl), _) => {
                // ?T in impl vs T in iface — impl accepts more, check inner
                return self.is_param_type_compatible(inner_impl, iface_hint);
            }
            (_, ParamTypeHint::Nullable(_)) => {
                // T in impl vs ?T in iface — impl rejects null, incompatible
                return false;
            }
            _ => {}
        }

        // Parameter compatibility reverses the subtype relation: the
        // implementation must accept every value admitted by the interface.
        if let ParamTypeHint::Intersection(impl_parts) = impl_hint {
            return impl_parts
                .iter()
                .all(|part| self.is_param_type_compatible(part, iface_hint));
        }
        if let ParamTypeHint::Union(iface_parts) = iface_hint {
            return iface_parts
                .iter()
                .all(|part| self.is_param_type_compatible(impl_hint, part));
        }
        if let ParamTypeHint::Union(impl_parts) = impl_hint {
            return impl_parts
                .iter()
                .any(|part| self.is_param_type_compatible(part, iface_hint));
        }
        if let ParamTypeHint::Intersection(iface_parts) = iface_hint {
            return iface_parts
                .iter()
                .any(|part| self.is_param_type_compatible(impl_hint, part));
        }

        // Class name contravariance: iface declares A, impl declares B
        // Compatible if A is_a B (A is a subtype of B, so impl accepts wider)
        match (impl_hint, iface_hint) {
            (ParamTypeHint::ClassName(impl_class), ParamTypeHint::ClassName(iface_class)) => {
                return self.class_is_a(iface_class, impl_class);
            }
            _ => {}
        }

        false
    }

    pub fn write_output(&self, data: &[u8]) {
        self.output.borrow_mut().write_all(data).unwrap();
    }
}

fn class_is_a_in_table(
    class_table: &HashMap<String, Box<ClassDef>>,
    class_name: &str,
    target: &str,
) -> bool {
    if class_name.eq_ignore_ascii_case(target) {
        return true;
    }
    let Some(class_def) = class_table.get(class_name) else {
        return false;
    };
    class_def
        .parent
        .as_ref()
        .is_some_and(|parent| class_is_a_in_table(class_table, parent, target))
        || class_def
            .implements
            .iter()
            .any(|interface| class_is_a_in_table(class_table, interface, target))
}

#[cfg(test)]
mod stdlib_capacity_tests {
    use super::ExecutorGlobals;

    #[test]
    fn stdlib_registration_fits_the_reserved_registry_envelopes() {
        let mut eg = ExecutorGlobals::new();
        eg.reserve_stdlib_capacity();
        let capacities = (
            eg.function_table.capacity(),
            eg.class_table.capacity(),
            eg.method_declaring_class.capacity(),
            eg.class_by_id.capacity(),
        );

        let functions = crate::stdlib::register_stdlib(&mut eg);

        assert_eq!(
            (
                eg.function_table.capacity(),
                eg.class_table.capacity(),
                eg.method_declaring_class.capacity(),
                eg.class_by_id.capacity(),
            ),
            capacities,
            "fixed stdlib registration must not grow a reserved registry"
        );
        assert!(!functions.is_empty());
    }
}

#[cfg(feature = "coroutines")]
pub mod coroutine;
