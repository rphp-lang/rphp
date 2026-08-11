use std::cell::Cell;
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::compiler::compile::ClassDef;
#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
use crate::generics::GenericType;
use crate::generics::{GenericMetadata, GenericMethodContract, ReifiedBinding};
use crate::parser::Visibility;
use crate::value::ObjectLayout;
use crate::vm::frame::ExecuteData;
use crate::vm::function::FunctionCommon;
use crate::vm::stack::VmStack;
use crate::vm::stats;

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[path = "generic_properties.rs"]
mod generic_properties;
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
    /// LIFO binding sidecar used only by explicit reified calls. It stays last
    /// so every pre-existing ExecutorGlobals field keeps the feature-off offset.
    #[cfg(feature = "php-generics-reified")]
    pub reified_bindings: Vec<ReifiedBinding>,
    /// Explicit generic bindings are born in a caller before its call frame is
    /// active. Scope sidecars make success, abandoned arguments and exception
    /// unwinding remove the exact binding instead of leaving stale LIFO state.
    #[cfg(feature = "php-generics-reified")]
    pending_reified_binding_scopes: Vec<PendingReifiedBindingScope>,
    #[cfg(feature = "php-generics-reified")]
    active_reified_binding_scopes: Vec<ActiveReifiedBindingScope>,
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
            reified_objects: HashMap::new(),
            #[cfg(feature = "php-generics-reified")]
            reified_object_cache: std::cell::RefCell::new(None),
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
            reified_objects: HashMap::new(),
            #[cfg(feature = "php-generics-reified")]
            reified_object_cache: std::cell::RefCell::new(None),
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
            if let Some(contract) = self
                .generic_metadata
                .reified_instance_method_contract(binding, method)
            {
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
        let contract = std::rc::Rc::new(
            self.generic_metadata
                .linked_instance_method_contract(declaration, method)?,
        );
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
                // Inherit properties: child's own props first, then parent's.
                // This ensures find_property_visibility finds the class's own
                // declaration before inherited ones with the same name.
                // Private properties with the same name in parent and child are BOTH kept
                // (they occupy separate mangled slots). For public/protected, child overrides.
                let child_prop_names: std::collections::HashSet<&str> = class_def
                    .properties
                    .iter()
                    .map(|(n, _, _, _)| n.as_str())
                    .collect();
                let mut parent_props = Vec::new();
                for (name, default, vis, declaring) in &parent.properties {
                    if child_prop_names.contains(name.as_str()) {
                        // Child has same name — only keep parent's if both are private
                        // (separate slots). Otherwise child overrides.
                        if *vis == Visibility::Private {
                            let child_also_private = class_def
                                .properties
                                .iter()
                                .any(|(n, _, v, _)| n == name && *v == Visibility::Private);
                            if child_also_private {
                                parent_props.push((
                                    name.clone(),
                                    default.clone(),
                                    *vis,
                                    declaring.clone(),
                                ));
                            }
                        }
                    } else {
                        parent_props.push((name.clone(), default.clone(), *vis, declaring.clone()));
                    }
                }
                // Own props first, then inherited (so lookups find own first)
                let mut merged_props: Vec<_> = class_def.properties.drain(..).collect();
                merged_props.extend(parent_props);
                class_def.properties = merged_props;

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
        for trait_name in &trait_names {
            if let Some(trait_def) = self.class_table.get(trait_name.as_str()) {
                // Merge trait properties (class's own props take precedence)
                // Check for incompatible trait property collisions
                let mut new_props = Vec::new();
                for (name, default, vis, _declaring) in &trait_def.properties {
                    // Check if this property already exists (from class itself or another trait)
                    let existing = class_def.properties.iter().find(|(n, _, _, _)| n == name);
                    if let Some((_, _existing_default, existing_vis, existing_declaring)) = existing
                    {
                        // If declared by the class itself (not a trait), class takes precedence
                        if existing_declaring == &class_name {
                            continue;
                        }
                        // Two traits define the same property — check compatibility
                        // PHP checks both visibility and default value compatibility.
                        if existing_vis != vis {
                            return Err(format!(
                                "{} and {} define the same property (${}) in the composition of {}. \
                                 However, the definition differs and is considered incompatible",
                                existing_declaring, trait_name, name, class_name
                            ));
                        }
                        // Check default value compatibility
                        let defaults_compatible = match (default, _existing_default) {
                            (None, None) => true,
                            (Some(a), Some(b)) => a.structurally_equal(b),
                            _ => false, // one has default, other doesn't
                        };
                        if !defaults_compatible {
                            return Err(format!(
                                "{} and {} define the same property (${}) in the composition of {}. \
                                 However, the definition differs and is considered incompatible",
                                existing_declaring, trait_name, name, class_name
                            ));
                        }
                        // Same visibility and default — compatible, skip duplicate
                        continue;
                    }
                    new_props.push((name.clone(), default.clone(), *vis, trait_name.clone()));
                }
                class_def.properties.extend(new_props);

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

        // Property order is now final. Build one shared storage-key → slot
        // layout for every object instance of this class.
        let property_keys = class_def
            .properties
            .iter()
            .map(|(name, _default, visibility, declaring)| {
                if *visibility == Visibility::Private {
                    mangle_private_prop(declaring, name)
                } else {
                    name.clone()
                }
            })
            .collect();
        class_def.property_layout =
            std::rc::Rc::new(ObjectLayout::new(class_name.as_str(), property_keys));
        class_def.property_defaults = class_def
            .properties
            .iter()
            .map(|(name, default, _visibility, _declaring)| {
                default.clone().unwrap_or_else(|| {
                    if class_def.readonly_props.contains(name) {
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
            for (name, _default, vis, declaring) in &class_def.properties {
                if name == prop_name {
                    return Some((*vis, declaring.clone()));
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
