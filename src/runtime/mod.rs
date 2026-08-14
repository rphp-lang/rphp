use std::cell::Cell;
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::compiler::compile::{ClassConstantDefinition, ClassDef, PropertyDefinition};
#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
use crate::generics::GenericType;
use crate::generics::{GenericMetadata, GenericMethodContract, ReifiedBinding};
use crate::parser::Visibility;
use crate::value::{ObjectLayout, PhpArray, Value};
use crate::vm::frame::ExecuteData;
use crate::vm::function::FunctionCommon;
use crate::vm::stack::VmStack;
use crate::vm::stats;
use crate::vm::virtual_aggregate_cache::{
    RESOLVED_VIRTUAL_AGGREGATE_CACHE_SLOTS, ResolvedVirtualAggregateCacheEntry,
};

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

#[derive(Clone, Copy)]
struct MethodDeclaration<'a> {
    owner: &'a str,
    name: &'a str,
    visibility: Visibility,
    enforces_visibility: bool,
    is_static: bool,
    is_abstract: bool,
    function: &'a FunctionCommon,
}

/// Stable sidecar for one reified static-property declaration. The weak
/// receiver guard makes pointer reuse safe without retaining assigned objects;
/// a different or already-dropped object always falls back to the full
/// interned metadata check.
#[cfg(feature = "php-generics-reified")]
struct StaticGenericPropertyContract {
    definition: *const PropertyDefinition,
    identity: std::cell::Cell<usize>,
    object: std::cell::RefCell<std::rc::Weak<std::cell::RefCell<crate::value::PhpObject>>>,
}

#[cfg(feature = "php-generics-reified")]
impl StaticGenericPropertyContract {
    fn remembers(&self, value: &Value) -> bool {
        if value.value_type() != crate::value::ValueType::Object {
            return false;
        }
        let identity = value
            .object_identity()
            .expect("object tag must expose object identity");
        self.identity.get() == identity && self.object.borrow().strong_count() != 0
    }

    fn remember(&self, value: &Value) {
        let Some(object) = value.as_object_rc() else {
            self.identity.set(0);
            self.object.replace(std::rc::Weak::new());
            return;
        };
        self.identity.set(std::rc::Rc::as_ptr(&object) as usize);
        self.object.replace(std::rc::Rc::downgrade(&object));
    }
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

include!("property_definitions.rs");
include!("class_constants.rs");

/// One callback in the request-local SPL autoload stack. Callback resolution
/// happens at registration time so visibility and callable identity do not
/// depend on the later class lookup's lexical scope.
#[derive(Clone)]
pub(crate) struct AutoloadEntry {
    pub(crate) callback: Value,
    pub(crate) func_ptr: *const FunctionCommon,
    pub(crate) prepend_args: Vec<Value>,
    pub(crate) use_vars: Vec<Value>,
    pub(crate) called_scope_class_id: u32,
    pub(crate) bound_this: Option<Value>,
}

/// Cold request-local SPL state. Executors that never register an autoloader
/// keep this behind a null `Option<Box<_>>` and allocate nothing.
#[derive(Default)]
pub(crate) struct AutoloadState {
    /// Immutable callback snapshot. Lookups clone one `Rc` without allocating;
    /// the rare register/unregister operation publishes a replacement slice.
    pub(crate) entries: std::rc::Rc<[AutoloadEntry]>,
    pub(crate) active_classes: std::collections::HashSet<String>,
    /// Request-local suffix list used by the built-in `spl_autoload()`
    /// callback. `None` keeps the PHP default without allocating state.
    pub(crate) extensions: Option<std::rc::Rc<str>>,
    /// Directory of the first default-loader registration call site. Callback
    /// helper frames intentionally detach from their caller, so retain this
    /// cold path context without touching ordinary class lookups.
    pub(crate) base_directory: Option<std::rc::Rc<str>>,
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
    /// Bounded request-local descriptors for structurally proven virtual
    /// call/return aggregates. The fixed array allocates nothing and RefCell
    /// mutation remains confined to the single VM execution thread.
    pub(crate) resolved_virtual_aggregate_cache: std::cell::RefCell<
        [ResolvedVirtualAggregateCacheEntry; RESOLVED_VIRTUAL_AGGREGATE_CACHE_SLOTS],
    >,
    /// Function table — name → pointer to FunctionCommon
    pub function_table: HashMap<String, *const FunctionCommon>,
    /// Class table — name/alias → shared ClassDef. `Rc` keeps metadata and
    /// inline-cache pointers stable while aliases reuse the exact identity.
    pub class_table: HashMap<String, std::rc::Rc<ClassDef>>,
    /// Anonymous declarations become visible only when their `new class`
    /// expression executes. Eager registration would autoload dependencies
    /// from branches that PHP never evaluates.
    pending_anonymous_classes: HashMap<String, ClassDef>,
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
    /// Request-local error mask exposed by error_reporting(). Diagnostic
    /// routing is still intentionally minimal, but libraries observe the
    /// getter/setter contract while temporarily suppressing warnings.
    pub error_reporting: i64,
    /// Suppressed call frame and the reporting mask to restore when it leaves.
    /// This cold sidecar keeps the ordinary ExecuteData layout unchanged.
    error_suppression_frames: Vec<(usize, i64)>,
    pub(crate) error_handler: Option<crate::value::Value>,
    pub(crate) error_handler_levels: i64,
    pub(crate) error_handler_stack: Vec<(Option<crate::value::Value>, i64)>,
    pub(crate) exception_handler: Option<crate::value::Value>,
    pub(crate) exception_handler_stack: Vec<Option<crate::value::Value>>,
    /// Reverse map: func_ptr → declaring class name (for visibility scope resolution)
    pub method_declaring_class: HashMap<*const FunctionCommon, String>,
    /// Output buffer — collected output for testing, or stdout
    output: std::cell::RefCell<Box<dyn Write>>,
    /// Temporary buffer for named variadic arguments.
    /// Key = call frame pointer as usize, value = vec of (name, value) pairs.
    /// Populated by SendNamed when target function is variadic and name isn't a declared param.
    /// Consumed by DoFcall during variadic packing.
    pub pending_named_variadic: HashMap<usize, Vec<(String, crate::value::Value)>>,
    /// Captures belonging to a variadic closure cannot enter their final CVs
    /// until DoFcall has packed the overlapping raw argument prefix.
    pub(crate) pending_closure_captures: HashMap<usize, Vec<crate::value::Value>>,
    /// Original public arguments of active user calls. Extra arguments occupy
    /// slots that compiled TMP operands may reuse, so argument-introspection
    /// functions need stable storage for the lifetime of the call frame.
    pub(crate) function_arguments: HashMap<usize, Vec<crate::value::Value>>,
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
    /// Object identities whose user destructor has already started. Lazily
    /// allocated because ordinary requests never declare `__destruct`.
    /// Set of absolute file paths already included via include_once/require_once
    pub included_files: std::collections::HashSet<String>,
    /// First-successful-include order exposed by get_included_files().
    /// Membership remains separate so include_once stays O(1).
    included_file_order: Vec<String>,
    /// Owned storage for functions/data from included files (prevents dangling pointers)
    pub included_functions: Vec<Box<crate::vm::function::UserFunction>>,
    /// Lazily allocated SPL autoload stack and recursion guard.
    pub(crate) autoload: Option<Box<AutoloadState>>,
    /// Monotonically increasing counter for class IDs
    next_class_id: u32,
    /// Highest class ID installed by stdlib registration. User declarations
    /// are linked afterwards and therefore always receive a larger ID.
    internal_class_id_limit: u32,
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
    #[cfg(feature = "php-generics-reified")]
    static_generic_property_contracts: Vec<Box<StaticGenericPropertyContract>>,
}

const PHP_82_SUPPRESSED_ERROR_REPORTING: i64 = 1 | 4 | 16 | 64 | 256 | 4096;

impl ExecutorGlobals {
    pub(crate) fn begin_error_suppression(&mut self, frame: usize) {
        self.error_suppression_frames
            .push((frame, self.error_reporting));
        // PHP 8.2 leaves fatal error classes visible under @.
        self.error_reporting = PHP_82_SUPPRESSED_ERROR_REPORTING;
    }

    pub(crate) fn end_error_suppression(&mut self, frame: usize) {
        if let Some(index) = self
            .error_suppression_frames
            .iter()
            .rposition(|(candidate, _)| *candidate == frame)
        {
            let (_, reporting) = self.error_suppression_frames.remove(index);
            self.error_reporting = reporting;
        }
    }

    pub(crate) fn set_error_reporting(&mut self, level: i64) {
        self.error_reporting = level;
        // An explicit error_reporting() call inside @ persists after every
        // active suppression scope is restored, as it does in PHP.
        for (_, reporting) in &mut self.error_suppression_frames {
            *reporting = level;
        }
    }

    /// Reserve the stable built-in registry envelope immediately before stdlib
    /// registration. Executors that never install stdlib stay allocation-lazy;
    /// normal executors avoid repeated hash-table growth while installing the
    /// fixed built-in class and function set.
    pub(crate) fn reserve_stdlib_capacity(&mut self) {
        // The all-features registry includes the complete Reflection and
        // generic runtime surfaces. Reserve the next hash-table envelope so
        // installing that fixed set never rehashes stored function pointers.
        self.function_table.reserve(512);
        self.class_table.reserve(64);
        self.method_declaring_class.reserve(256);
        self.class_by_id.reserve(64);
        self.static_property_slots_by_class.reserve(64);
        self.static_property_values.reserve(16);
        #[cfg(feature = "php-generics-reified")]
        self.static_generic_property_contracts.reserve(4);
    }

    pub fn new() -> Self {
        Self {
            vm_stack: VmStack::new(),
            pending_call_stack: VmStack::new_pending(),
            current_execute_data: Cell::new(std::ptr::null_mut()),
            vm_interrupt: AtomicBool::new(false),
            timed_out: AtomicBool::new(false),
            resolved_virtual_aggregate_cache: std::cell::RefCell::new(
                [ResolvedVirtualAggregateCacheEntry::EMPTY; RESOLVED_VIRTUAL_AGGREGATE_CACHE_SLOTS],
            ),
            function_table: HashMap::new(),
            class_table: HashMap::new(),
            pending_anonymous_classes: HashMap::new(),
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
            error_reporting: 32767,
            error_suppression_frames: Vec::new(),
            error_handler: None,
            error_handler_levels: 32767,
            error_handler_stack: Vec::new(),
            exception_handler: None,
            exception_handler_stack: Vec::new(),
            method_declaring_class: HashMap::new(),

            output: std::cell::RefCell::new(Box::new(std::io::stdout())),
            pending_named_variadic: HashMap::new(),
            pending_closure_captures: HashMap::new(),
            function_arguments: HashMap::new(),
            active_generator: None,
            globals: HashMap::new(),
            dirty_globals: std::collections::HashSet::new(),
            static_vars: HashMap::new(),
            pending_invoke_this: None,
            included_files: std::collections::HashSet::new(),
            included_file_order: Vec::new(),
            included_functions: Vec::new(),
            autoload: None,
            next_class_id: 1,
            internal_class_id_limit: 0,
            class_by_id: vec![std::ptr::null()],
            static_property_values: Vec::new(),
            static_property_slots_by_class: vec![Box::new([])],
            #[cfg(feature = "php-generics-reified")]
            static_generic_property_contracts: Vec::new(),
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
            resolved_virtual_aggregate_cache: std::cell::RefCell::new(
                [ResolvedVirtualAggregateCacheEntry::EMPTY; RESOLVED_VIRTUAL_AGGREGATE_CACHE_SLOTS],
            ),
            function_table: HashMap::new(),
            class_table: HashMap::new(),
            pending_anonymous_classes: HashMap::new(),
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
            error_reporting: 32767,
            error_suppression_frames: Vec::new(),
            error_handler: None,
            error_handler_levels: 32767,
            error_handler_stack: Vec::new(),
            exception_handler: None,
            exception_handler_stack: Vec::new(),
            method_declaring_class: HashMap::new(),

            output: std::cell::RefCell::new(output),
            pending_named_variadic: HashMap::new(),
            pending_closure_captures: HashMap::new(),
            function_arguments: HashMap::new(),
            active_generator: None,
            globals: HashMap::new(),
            dirty_globals: std::collections::HashSet::new(),
            static_vars: HashMap::new(),
            pending_invoke_this: None,
            included_files: std::collections::HashSet::new(),
            included_file_order: Vec::new(),
            included_functions: Vec::new(),
            autoload: None,
            next_class_id: 1,
            internal_class_id_limit: 0,
            class_by_id: vec![std::ptr::null()],
            static_property_values: Vec::new(),
            static_property_slots_by_class: vec![Box::new([])],
            #[cfg(feature = "php-generics-reified")]
            static_generic_property_contracts: Vec::new(),
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

    fn collect_abstract_method_requirements<'a>(
        &'a self,
        class_def: &'a ClassDef,
        requirements: &mut Vec<MethodDeclaration<'a>>,
        visited: &mut std::collections::HashSet<String>,
    ) {
        if !visited.insert(class_def.name.to_ascii_lowercase()) {
            return;
        }
        requirements.extend(class_def.methods.iter().filter_map(|method| {
            class_def
                .method_is_abstract(&method.0)
                .then(|| Self::method_declaration(class_def, method))
        }));
        for trait_name in &class_def.uses {
            if let Some(trait_def) = self.class_table.get(trait_name.as_str()) {
                self.collect_abstract_method_requirements(trait_def, requirements, visited);
            }
        }
        if let Some(parent_name) = &class_def.parent
            && let Some(parent_def) = self.class_table.get(parent_name.as_str())
        {
            self.collect_abstract_method_requirements(parent_def, requirements, visited);
        }
    }

    fn method_declaration<'a>(
        class_def: &'a ClassDef,
        method: &'a (
            String,
            Visibility,
            bool,
            bool,
            crate::vm::function::UserFunction,
        ),
    ) -> MethodDeclaration<'a> {
        MethodDeclaration {
            owner: &class_def.name,
            name: &method.0,
            visibility: method.1,
            // PHP retains a backwards-compatibility exception for abstract
            // trait requirements: their implementation may narrow visibility.
            enforces_visibility: !class_def.is_trait,
            is_static: method.2,
            is_abstract: class_def.method_is_abstract(&method.0),
            function: &method.4.common,
        }
    }

    fn find_effective_method<'a>(
        &'a self,
        class_def: &'a ClassDef,
        method_name: &str,
    ) -> Option<MethodDeclaration<'a>> {
        if let Some(method) = class_def
            .methods
            .iter()
            .find(|(name, _, _, _, _)| name.eq_ignore_ascii_case(method_name))
        {
            return Some(Self::method_declaration(class_def, method));
        }
        let mut abstract_trait_method = None;
        for trait_name in &class_def.uses {
            if let Some(trait_def) = self.class_table.get(trait_name.as_str())
                && let Some(declaration) = self.find_effective_method(trait_def, method_name)
            {
                if !declaration.is_abstract {
                    return Some(declaration);
                }
                abstract_trait_method.get_or_insert(declaration);
            }
        }
        let parent_method = class_def
            .parent
            .as_ref()
            .and_then(|parent| self.class_table.get(parent.as_str()))
            .and_then(|parent| self.find_effective_method(parent, method_name));
        match parent_method {
            Some(method) if !method.is_abstract => Some(method),
            Some(method) => abstract_trait_method.or(Some(method)),
            None => abstract_trait_method,
        }
    }

    fn method_contract_errors(
        &self,
        required: MethodDeclaration<'_>,
        implementation: MethodDeclaration<'_>,
        linking_class: Option<&ClassDef>,
    ) -> Vec<String> {
        use crate::vm::function::ParamTypeHint;

        let mut errors = Vec::new();
        let visibility_rank = |visibility| match visibility {
            Visibility::Private => 0,
            Visibility::Protected => 1,
            Visibility::Public => 2,
        };
        if required.enforces_visibility
            && visibility_rank(implementation.visibility) < visibility_rank(required.visibility)
        {
            errors.push(format!("access must be at least {:?}", required.visibility));
        }
        if implementation.is_static != required.is_static {
            errors.push(if required.is_static {
                "implementation must be static".to_string()
            } else {
                "implementation cannot be static".to_string()
            });
        }

        let required_signature = &required.function.sig;
        let implementation_signature = &implementation.function.sig;
        let required_public = required_signature.public_arity();
        let implementation_public = implementation_signature.public_arity();
        if implementation_public < required_public && !implementation_signature.is_variadic {
            errors.push(format!(
                "requires {} parameters, implementation accepts {}",
                required_public, implementation_public
            ));
        }
        if implementation_signature.required_num_args > required_signature.required_num_args {
            errors.push(format!(
                "implementation requires {} parameters, declaration requires only {}",
                implementation_signature.required_num_args, required_signature.required_num_args
            ));
        }
        if required_signature.is_variadic && !implementation_signature.is_variadic {
            errors.push("implementation must be variadic".to_string());
        }

        let parametric = self
            .generic_metadata
            .method_has_parametric_signature(required.owner, required.name);
        if !parametric {
            // PHP permits an implementation to add optional parameters. They
            // are outside the declaration's callable contract, so variance
            // checks apply only to parameters present in the requirement.
            let check_count = Self::variance_contract_parameter_count(
                required_signature,
                implementation_signature,
            );
            for index in 0..check_count {
                let required_parameter = Self::variance_parameter_hint(required_signature, index);
                let implementation_parameter =
                    Self::variance_parameter_hint(implementation_signature, index);
                match (implementation_parameter, required_parameter) {
                    (None | Some(ParamTypeHint::None), None | Some(ParamTypeHint::None)) => {}
                    (None | Some(ParamTypeHint::None) | Some(ParamTypeHint::Mixed), Some(_)) => {}
                    (Some(implementation_hint), None | Some(ParamTypeHint::None)) => {
                        if !matches!(implementation_hint, ParamTypeHint::Mixed) {
                            errors.push(format!(
                                "parameter {} must not add type {}, declaration has no type",
                                index + 1,
                                implementation_hint.display_name()
                            ));
                        }
                    }
                    (Some(implementation_hint), Some(required_hint)) => {
                        let implementation_hint = self.resolve_variance_type_hint(
                            implementation_hint,
                            implementation.owner,
                            linking_class,
                        );
                        let required_hint = self.resolve_variance_type_hint(
                            required_hint,
                            required.owner,
                            linking_class,
                        );
                        if !self.is_param_type_compatible(
                            &implementation_hint,
                            &required_hint,
                            implementation.owner,
                            required.owner,
                            linking_class,
                        ) {
                            errors.push(format!(
                                "parameter {} type must be compatible with {}, got {}",
                                index + 1,
                                required_hint.display_name(),
                                implementation_hint.display_name()
                            ));
                        }
                    }
                }
            }

            let required_return = &required_signature.return_type_hint;
            if !matches!(required_return, ParamTypeHint::None) {
                let implementation_return = self.resolve_variance_type_hint(
                    &implementation_signature.return_type_hint,
                    implementation.owner,
                    linking_class,
                );
                let required_return =
                    self.resolve_variance_type_hint(required_return, required.owner, linking_class);
                if !self.is_return_type_compatible(
                    &implementation_return,
                    &required_return,
                    implementation.owner,
                    required.owner,
                    linking_class,
                ) {
                    errors.push(format!(
                        "return type must be compatible with {}, got {}",
                        required_return.display_name(),
                        implementation_return.display_name()
                    ));
                }
            }
        }

        let reference_count =
            Self::variance_contract_parameter_count(required_signature, implementation_signature)
                .min(64);
        for index in 0..reference_count {
            if Self::variance_parameter_is_by_ref(required_signature, index)
                != Self::variance_parameter_is_by_ref(implementation_signature, index)
            {
                errors.push(format!(
                    "parameter {} reference mode must match the declaration",
                    index + 1
                ));
            }
        }
        errors
    }

    #[inline]
    fn variance_parameter_count(signature: &crate::vm::function::SignatureInfo) -> u32 {
        signature.public_arity() + u32::from(signature.is_variadic)
    }

    #[inline]
    fn variance_contract_parameter_count(
        required: &crate::vm::function::SignatureInfo,
        implementation: &crate::vm::function::SignatureInfo,
    ) -> u32 {
        if required.is_variadic {
            // Every explicit implementation parameter after the declaration's
            // variadic position may receive a value admitted by that variadic
            // contract, including optional fixed parameters before the new
            // variadic tail.
            Self::variance_parameter_count(required)
                .max(Self::variance_parameter_count(implementation))
        } else {
            Self::variance_parameter_count(required)
        }
    }

    #[inline]
    fn variance_parameter_index(
        signature: &crate::vm::function::SignatureInfo,
        contract_index: u32,
    ) -> Option<u32> {
        if contract_index < signature.public_arity() {
            Some(contract_index)
        } else if signature.is_variadic {
            // A variadic parameter subsumes every remaining position in an
            // inherited callable contract.
            Some(signature.public_arity())
        } else {
            None
        }
    }

    #[inline]
    fn variance_parameter_hint(
        signature: &crate::vm::function::SignatureInfo,
        contract_index: u32,
    ) -> Option<&crate::vm::function::ParamTypeHint> {
        Self::variance_parameter_index(signature, contract_index)
            .and_then(|index| signature.param_type_hints.get(index as usize))
    }

    #[inline]
    fn variance_parameter_is_by_ref(
        signature: &crate::vm::function::SignatureInfo,
        contract_index: u32,
    ) -> bool {
        Self::variance_parameter_index(signature, contract_index)
            .is_some_and(|index| signature.is_param_by_ref(index))
    }

    fn variance_scope_owner<'a>(
        &'a self,
        owner: &'a str,
        linking_class: Option<&'a ClassDef>,
    ) -> &'a str {
        if let Some(linking_class) = linking_class
            && self
                .find_class(owner)
                .is_some_and(|definition| definition.is_trait)
        {
            return linking_class.name.as_str();
        }
        owner
    }

    fn resolve_variance_type_hint(
        &self,
        hint: &crate::vm::function::ParamTypeHint,
        owner: &str,
        linking_class: Option<&ClassDef>,
    ) -> crate::vm::function::ParamTypeHint {
        use crate::vm::function::ParamTypeHint;

        let scope_owner = self.variance_scope_owner(owner, linking_class);

        match hint {
            ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("self") => {
                ParamTypeHint::ClassName(scope_owner.to_string())
            }
            ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("parent") => {
                ParamTypeHint::ClassName(
                    linking_class
                        .filter(|definition| definition.name.eq_ignore_ascii_case(scope_owner))
                        .and_then(|definition| definition.parent.clone())
                        .or_else(|| {
                            self.find_class(scope_owner)
                                .and_then(|class| class.parent.clone())
                        })
                        .unwrap_or_else(|| name.clone()),
                )
            }
            ParamTypeHint::Nullable(inner) => ParamTypeHint::Nullable(Box::new(
                self.resolve_variance_type_hint(inner, owner, linking_class),
            )),
            ParamTypeHint::Union(parts) => ParamTypeHint::Union(
                parts
                    .iter()
                    .map(|part| self.resolve_variance_type_hint(part, owner, linking_class))
                    .collect(),
            ),
            ParamTypeHint::Intersection(parts) => ParamTypeHint::Intersection(
                parts
                    .iter()
                    .map(|part| self.resolve_variance_type_hint(part, owner, linking_class))
                    .collect(),
            ),
            _ => hint.clone(),
        }
    }

    fn format_method_signature(
        &self,
        declaration: MethodDeclaration<'_>,
        linking_class: Option<&ClassDef>,
    ) -> String {
        use crate::vm::function::ParamTypeHint;

        let signature = &declaration.function.sig;
        let parameter_count = Self::variance_parameter_count(signature) as usize;
        let mut parameters = Vec::with_capacity(parameter_count);
        for index in 0..parameter_count {
            let mut parameter = String::new();
            if let Some(hint) = signature.param_type_hints.get(index)
                && !matches!(hint, ParamTypeHint::None)
            {
                parameter.push_str(
                    &self
                        .resolve_variance_type_hint(hint, declaration.owner, linking_class)
                        .display_name(),
                );
                parameter.push(' ');
            }
            if signature.is_param_by_ref(index as u32) {
                parameter.push('&');
            }
            if signature.is_variadic && index as u32 == signature.public_arity() {
                parameter.push_str("...");
            }
            parameter.push('$');
            parameter.push_str(
                signature
                    .param_names
                    .get(index)
                    .map(String::as_str)
                    .unwrap_or("arg"),
            );
            parameters.push(parameter);
        }

        let mut rendered = format!(
            "{}::{}({})",
            declaration.owner,
            declaration.name,
            parameters.join(", ")
        );
        if !matches!(signature.return_type_hint, ParamTypeHint::None) {
            rendered.push_str(": ");
            rendered.push_str(
                &self
                    .resolve_variance_type_hint(
                        &signature.return_type_hint,
                        declaration.owner,
                        linking_class,
                    )
                    .display_name(),
            );
        }
        rendered
    }

    /// Concrete parent methods carry the same parameter contravariance and
    /// return covariance contract as interfaces and abstract declarations.
    /// Constructors retain PHP's historical exemption unless the inherited
    /// declaration is itself abstract; private methods do not participate in
    /// an overriding contract.
    fn validate_parent_method_contracts(&self, class_def: &ClassDef) -> Result<(), String> {
        if class_def.is_interface || class_def.is_trait {
            return Ok(());
        }
        let Some(parent) = class_def
            .parent
            .as_deref()
            .and_then(|name| self.class_table.get(name))
        else {
            return Ok(());
        };

        for method in &class_def.methods {
            let implementation = Self::method_declaration(class_def, method);
            let Some(required) = self.find_effective_method(parent, implementation.name) else {
                continue;
            };
            if required.visibility == Visibility::Private
                || (required.name.eq_ignore_ascii_case("__construct") && !required.is_abstract)
            {
                continue;
            }
            if self
                .method_contract_errors(required, implementation, Some(class_def))
                .is_empty()
            {
                continue;
            }

            let location = class_def
                .source_file
                .as_ref()
                .map_or_else(String::new, |file| {
                    // Method line metadata is not yet retained by ClassDef. Keep
                    // the canonical source unit and a numeric placeholder so the
                    // diagnostic has PHP's stable location shape without guessing
                    // a source line.
                    format!(" in {file} on line 0")
                });
            return Err(format!(
                "Declaration of {} must be compatible with {}{}",
                self.format_method_signature(implementation, Some(class_def)),
                self.format_method_signature(required, Some(class_def)),
                location
            ));
        }
        Ok(())
    }

    fn validate_abstract_method_contracts(&self, class_def: &ClassDef) -> Result<(), String> {
        if class_def.is_interface || class_def.is_trait {
            return Ok(());
        }
        let mut requirements = Vec::new();
        self.collect_abstract_method_requirements(
            class_def,
            &mut requirements,
            &mut std::collections::HashSet::new(),
        );
        for requirement in requirements {
            let Some(implementation) = self.find_effective_method(class_def, requirement.name)
            else {
                continue;
            };
            if implementation.is_abstract && !class_def.is_abstract {
                return Err(format!(
                    "Class {} contains 1 abstract method and must therefore be declared abstract or implement the remaining methods ({}::{})",
                    class_def.name, requirement.owner, requirement.name
                ));
            }
            if let Some(reason) = self
                .method_contract_errors(requirement, implementation, Some(class_def))
                .into_iter()
                .next()
            {
                return Err(format!(
                    "Declaration of {}::{}() must be compatible with {}::{}() ({})",
                    implementation.owner,
                    implementation.name,
                    requirement.owner,
                    requirement.name,
                    reason
                ));
            }
        }
        Ok(())
    }

    /// Register an ordinary compiled class immediately, or retain an
    /// anonymous declaration until its expression executes.
    pub fn register_compiled_class(&mut self, class_def: ClassDef) -> Result<(), String> {
        if class_def.is_anonymous() {
            let name = class_def.name.to_ascii_lowercase();
            if self
                .pending_anonymous_classes
                .insert(name, class_def)
                .is_some()
            {
                return Err("Duplicate anonymous class declaration".to_string());
            }
            Ok(())
        } else {
            self.register_class(class_def)
        }
    }

    pub(crate) fn take_pending_anonymous_class(&mut self, name: &str) -> Option<ClassDef> {
        self.pending_anonymous_classes
            .remove(&name.to_ascii_lowercase())
    }

    /// Register a class definition and its methods in the function table.
    /// Resolves inheritance: merges parent properties/methods into child.
    /// For non-interface, non-abstract classes: validates interface contracts.
    pub fn register_class(&mut self, mut class_def: ClassDef) -> Result<(), String> {
        let class_name = class_def.name.clone();
        // PHP does not permit class redeclaration. Besides matching that rule,
        // this guarantees class_by_id pointers remain stable for inline caches.
        if self
            .class_table
            .keys()
            .any(|registered| registered.eq_ignore_ascii_case(&class_name))
        {
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
        let own_property_names = class_def
            .properties
            .iter()
            .map(|property| property.name.clone())
            .collect::<std::collections::HashSet<_>>();
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
                if class_def.is_readonly != parent.is_readonly {
                    return Err(if class_def.is_readonly {
                        format!(
                            "Readonly class {} cannot extend non-readonly class {}",
                            class_name, parent_name
                        )
                    } else {
                        format!(
                            "Non-readonly class {} cannot extend readonly class {}",
                            class_name, parent_name
                        )
                    });
                }
                merge_parent_constant_definitions(
                    &class_name,
                    &mut class_def.constants,
                    &parent.constants,
                )?;
            }
        }

        self.validate_parent_method_contracts(&class_def)?;
        self.validate_abstract_method_contracts(&class_def)?;

        // Resolve inheritance — merge parent's properties and methods
        if let Some(parent_name) = &class_def.parent {
            if let Some(parent) = self.class_table.get(parent_name.as_str()) {
                validate_property_inheritance(
                    &class_name,
                    &class_def.properties,
                    &class_def.static_properties,
                    &parent.properties,
                    &parent.static_properties,
                )?;
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
        let mut composed_trait_property_names = std::collections::HashSet::new();
        let mut composed_static_trait_names = std::collections::HashSet::new();
        for trait_name in &trait_names {
            if let Some(trait_def) = self.class_table.get(trait_name.as_str()) {
                merge_trait_constant_definitions(
                    &class_name,
                    trait_name,
                    &mut class_def.constants,
                    &trait_def.constants,
                )?;
                merge_trait_property_definitions(
                    &mut class_def.properties,
                    &trait_def.properties,
                    &class_name,
                    trait_name,
                    &own_property_names,
                    &mut composed_trait_property_names,
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
                        // Keep the concrete body owner stable. A single trait
                        // function pointer can be composed into many classes;
                        // overwriting this reverse map with the last consumer
                        // makes lexical private scope registration-order
                        // dependent. Call frames recover the actual consuming
                        // class from `$this` when the owner is a trait.
                        self.method_declaring_class
                            .entry(func_ptr)
                            .or_insert_with(|| trait_name.clone());
                    }
                }
            } else {
                return Err(format!("Trait not found: {}", trait_name));
            }
        }

        for adaptation in &class_def.trait_aliases {
            let source_trait = if let Some(trait_name) = &adaptation.trait_name {
                trait_names
                    .iter()
                    .find(|used| used.eq_ignore_ascii_case(trait_name))
            } else {
                trait_names.iter().find(|used| {
                    self.function_table
                        .contains_key(&format!("{}::{}", used, adaptation.method).to_lowercase())
                })
            }
            .ok_or_else(|| {
                format!(
                    "Trait method {}::{} not found",
                    adaptation.trait_name.as_deref().unwrap_or(""),
                    adaptation.method
                )
            })?;
            let source = format!("{}::{}", source_trait, adaptation.method).to_lowercase();
            let pointer = *self
                .function_table
                .get(&source)
                .ok_or_else(|| format!("Trait method {source} not found"))?;
            let alias = adaptation.alias.as_deref().unwrap_or(&adaptation.method);
            self.function_table
                .insert(format!("{}::{}", class_name, alias).to_lowercase(), pointer);
            self.method_declaring_class
                .entry(pointer)
                .or_insert_with(|| source_trait.clone());
        }

        // Interface constants are inherited without being copied into source
        // declarations. Flatten them once at class registration so reads and
        // their inline caches are an indexed lookup thereafter.
        for interface_name in &class_def.implements {
            // A small set of built-in interface contracts is registered by
            // the stdlib without a userland ClassDef. They have no userland
            // constants to inherit, so keep the existing contract path.
            let Some(interface) = self.class_table.get(interface_name.as_str()) else {
                continue;
            };
            merge_interface_constant_definitions(
                &class_name,
                &mut class_def.constants,
                &interface.constants,
            )?;
        }

        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
        if let Some(declaration) = self.generic_metadata.find_class_like_index(&class_name) {
            for property in &mut class_def.properties {
                property.generic_declaration = self
                    .generic_metadata
                    .instance_property_requires_generic_guard(declaration, &property.name)
                    .then_some(declaration);
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
                    .push(definition.default.clone().unwrap_or_else(|| {
                        if definition.is_typed() {
                            Value::undef()
                        } else {
                            Value::null()
                        }
                    }));
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
                    if property.is_typed() {
                        crate::value::Value::undef()
                    } else {
                        crate::value::Value::null()
                    }
                })
            })
            .collect::<Vec<_>>()
            .into();

        // Shared ownership keeps the allocation stable and lets class_alias()
        // publish another lookup key without duplicating metadata or identity.
        let class_def = std::rc::Rc::new(class_def);
        let class_ptr = std::rc::Rc::as_ptr(&class_def);
        self.class_table.insert(class_name.clone(), class_def);
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
            .filter(|(method_name, _, _, _, _)| {
                class.is_interface || !class.method_is_abstract(method_name)
            })
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

    /// Declaration metadata for one object-layout slot. Class definitions are
    /// boxed before publication and their property vectors are immutable
    /// afterwards, so callers may subsequently publish the returned address
    /// into an inline cache.
    #[inline(always)]
    pub(crate) fn instance_property_definition(
        &self,
        class_id: u32,
        property_slot: usize,
    ) -> Option<&PropertyDefinition> {
        self.class_by_id(class_id)?.properties.get(property_slot)
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

    #[cfg(feature = "php-generics-reified")]
    pub(crate) fn cache_static_generic_property_contract(
        &mut self,
        definition: *const PropertyDefinition,
        value: &Value,
    ) -> *const () {
        let contract = if let Some(contract) = self
            .static_generic_property_contracts
            .iter()
            .find(|contract| contract.definition == definition)
        {
            &**contract
        } else {
            self.static_generic_property_contracts
                .push(Box::new(StaticGenericPropertyContract {
                    definition,
                    identity: std::cell::Cell::new(0),
                    object: std::cell::RefCell::new(std::rc::Weak::new()),
                }));
            &**self.static_generic_property_contracts.last().unwrap()
        };
        contract.remember(value);
        (contract as *const StaticGenericPropertyContract).cast()
    }

    #[cfg(feature = "php-generics-reified")]
    pub(crate) unsafe fn static_generic_property_contract_remembers(
        &self,
        contract: *const (),
        value: &Value,
    ) -> bool {
        debug_assert!(!contract.is_null());
        unsafe { &*contract.cast::<StaticGenericPropertyContract>() }.remembers(value)
    }

    #[cfg(feature = "php-generics-reified")]
    pub(crate) unsafe fn static_generic_property_contract_definition(
        &self,
        contract: *const (),
    ) -> *const PropertyDefinition {
        debug_assert!(!contract.is_null());
        unsafe { (*contract.cast::<StaticGenericPropertyContract>()).definition }
    }

    #[cfg(feature = "php-generics-reified")]
    pub(crate) unsafe fn remember_static_generic_property_contract(
        &self,
        contract: *const (),
        value: &Value,
    ) {
        debug_assert!(!contract.is_null());
        unsafe { &*contract.cast::<StaticGenericPropertyContract>() }.remember(value);
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
        let canonical_target = self
            .find_class(target)
            .map_or(target, |class| class.name.as_str());
        let canonical_class = self
            .find_class(class_name)
            .map_or(class_name, |class| class.name.as_str());
        if canonical_class.eq_ignore_ascii_case(canonical_target) {
            return true;
        }
        if let Some(class_def) = self.find_class(class_name) {
            // PHP implicitly makes every class with an effective __toString()
            // implementation satisfy the built-in Stringable interface. The
            // relation participates in declaration variance as well as
            // instanceof/is_a checks; it is not copied into source metadata.
            if canonical_target.eq_ignore_ascii_case("Stringable")
                && self
                    .find_effective_method(class_def, "__toString")
                    .is_some()
            {
                return true;
            }
            // Check parent class
            if let Some(parent) = &class_def.parent {
                if self.class_is_a(parent, canonical_target) {
                    return true;
                }
            }
            // Check implemented interfaces
            for iface in &class_def.implements {
                if self.class_is_a(iface, canonical_target) {
                    return true;
                }
            }
        }
        false
    }

    pub(crate) fn seal_internal_class_ids(&mut self) {
        self.internal_class_id_limit = self.next_class_id.saturating_sub(1);
    }

    pub(crate) fn class_is_internal(&self, class_name: &str) -> bool {
        self.find_class(class_name).is_some_and(|class| {
            class.class_id != 0 && class.class_id <= self.internal_class_id_limit
        })
    }

    fn declared_class_like_names(
        &self,
        predicate: impl Fn(&ClassDef) -> bool,
        include_aliases: bool,
    ) -> Vec<String> {
        let mut declarations: Vec<_> = self
            .class_table
            .iter()
            .filter(|(registered, class)| {
                registered.as_str() == class.name && predicate(class.as_ref())
            })
            .map(|(_, class)| (class.class_id, class.name.clone()))
            .collect();
        declarations
            .sort_unstable_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        let mut names: Vec<String> = declarations.into_iter().map(|(_, name)| name).collect();

        if include_aliases {
            let mut aliases: Vec<_> = self
                .class_table
                .iter()
                .filter(|(registered, class)| {
                    registered.as_str() != class.name && predicate(class.as_ref())
                })
                .map(|(registered, _)| registered.to_ascii_lowercase())
                .collect();
            aliases.sort_unstable();
            aliases.dedup();
            names.extend(aliases);
        }
        names
    }

    pub(crate) fn declared_class_names(&self) -> Vec<String> {
        self.declared_class_like_names(|class| !class.is_interface && !class.is_trait, true)
    }

    pub(crate) fn declared_interface_names(&self) -> Vec<String> {
        self.declared_class_like_names(|class| class.is_interface, false)
    }

    pub(crate) fn declared_trait_names(&self) -> Vec<String> {
        self.declared_class_like_names(|class| class.is_trait, false)
    }

    /// Find a class-like symbol without allocating a normalized name. Exact
    /// declarations and aliases hit the hash table directly; unusual caller
    /// casing falls back to the cold case-insensitive scan required by PHP.
    #[inline]
    pub fn find_class(&self, name: &str) -> Option<&ClassDef> {
        let name = name.strip_prefix('\\').unwrap_or(name);
        self.class_table
            .get(name)
            .map(std::rc::Rc::as_ref)
            .or_else(|| {
                self.class_table
                    .iter()
                    .find(|(registered, _)| registered.eq_ignore_ascii_case(name))
                    .map(|(_, class)| class.as_ref())
            })
    }

    /// Publish another case-insensitive name for one existing class identity.
    /// Method aliases point at the same stable functions and static/property
    /// metadata continues to use the original numeric class ID.
    pub(crate) fn register_class_alias(
        &mut self,
        original: &str,
        alias: &str,
    ) -> Result<(), String> {
        let original = original.strip_prefix('\\').unwrap_or(original);
        let alias = alias.strip_prefix('\\').unwrap_or(alias);
        if self
            .class_table
            .keys()
            .any(|registered| registered.eq_ignore_ascii_case(alias))
        {
            return Err(format!(
                "Cannot declare class {}, because the name is already in use",
                alias
            ));
        }
        let class = self
            .class_table
            .iter()
            .find(|(registered, _)| registered.eq_ignore_ascii_case(original))
            .map(|(_, class)| class.clone())
            .ok_or_else(|| format!("Class \"{}\" not found", original))?;

        // Registration has already flattened inherited and trait-composed
        // methods under the canonical class prefix. Alias that effective
        // table, not only the methods physically declared in `class.methods`,
        // so trait factories and inherited APIs remain callable via an alias.
        let canonical_prefix = format!("{}::", class.name).to_ascii_lowercase();
        let methods: Vec<(String, *const FunctionCommon)> = self
            .function_table
            .iter()
            .filter_map(|(registered, function)| {
                registered
                    .strip_prefix(&canonical_prefix)
                    .map(|method| (method.to_string(), *function))
            })
            .collect();
        for (method, function) in methods {
            self.function_table.insert(
                format!("{}::{}", alias, method).to_ascii_lowercase(),
                function,
            );
        }
        self.class_table.insert(alias.to_string(), class);
        Ok(())
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
        receiver_scope
            .and_then(|receiver| self.trait_composition_scope(receiver, declared_scope))
            .unwrap_or(declared_scope)
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

    /// Resolve one immutable constant in the flattened class-like table.
    /// Returning the stable index lets VM call sites cache the result without
    /// retaining a pointer into metadata owned by the class registry.
    #[inline]
    pub fn find_class_constant(
        &self,
        class_id: u32,
        constant_name: &str,
    ) -> Option<(usize, &ClassConstantDefinition)> {
        self.class_by_id(class_id)?
            .constants
            .iter()
            .enumerate()
            .find(|(_, constant)| constant.name == constant_name)
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

    /// Resolve the class scope into which a trait body was composed for one
    /// concrete receiver. The nearest consumer wins, matching method lookup
    /// when a child composes the same trait as one of its ancestors.
    pub fn trait_composition_scope<'a>(
        &'a self,
        receiver_class: &str,
        trait_name: &str,
    ) -> Option<&'a str> {
        fn uses_trait(eg: &ExecutorGlobals, uses: &[String], target: &str) -> bool {
            uses.iter().any(|used| {
                used.eq_ignore_ascii_case(target)
                    || eg
                        .class_table
                        .get(used.as_str())
                        .is_some_and(|definition| uses_trait(eg, &definition.uses, target))
            })
        }

        let mut definition = self.class_table.get(receiver_class)?;
        loop {
            if uses_trait(self, &definition.uses, trait_name) {
                return Some(definition.name.as_str());
            }
            definition = self.class_table.get(definition.parent.as_deref()?)?;
        }
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

    /// Collect all required interface method declarations recursively through
    /// interface inheritance. The same declaration representation feeds both
    /// interface and abstract class/trait compatibility checks.
    fn collect_interface_methods<'a>(&'a self, iface_name: &str) -> Vec<MethodDeclaration<'a>> {
        let mut result = Vec::new();
        if let Some(iface_def) = self.class_table.get(iface_name) {
            for method in &iface_def.methods {
                result.push(Self::method_declaration(iface_def, method));
            }
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
    /// Shared contract logic covers visibility, staticness, arity, reference
    /// mode, parameter contravariance and return covariance.
    /// Returns a list of (interface_name, error_description), empty if all satisfied.
    pub fn validate_interface_contracts(&self, class_name: &str) -> Vec<(String, String)> {
        let mut errors = Vec::new();
        let Some(class_def) = self.class_table.get(class_name) else {
            return errors;
        };
        if class_def.is_interface || class_def.is_abstract || class_def.is_trait {
            return errors; // interfaces/abstract classes/traits don't need to implement
        }
        // Collect interfaces from the entire parent chain (fix P2: inherited obligations)
        let all_ifaces = self.collect_all_interfaces(class_name);
        let mut seen = std::collections::HashSet::new();
        for iface_name in all_ifaces {
            if !seen.insert(iface_name.clone()) {
                continue;
            }
            for requirement in self.collect_interface_methods(&iface_name) {
                let Some(implementation) = self.find_effective_method(class_def, requirement.name)
                else {
                    errors.push((requirement.owner.to_string(), requirement.name.to_string()));
                    continue;
                };
                if implementation.is_abstract {
                    errors.push((requirement.owner.to_string(), requirement.name.to_string()));
                    continue;
                }
                errors.extend(
                    self.method_contract_errors(requirement, implementation, None)
                        .into_iter()
                        .map(|reason| {
                            (
                                requirement.owner.to_string(),
                                format!("{} ({})", requirement.name, reason),
                            )
                        }),
                );
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
                if name.to_lowercase() == method_lower && !class_def.method_is_abstract(name) {
                    return Some((*vis, *is_static, class_name.to_string()));
                }
            }
            // Check used traits (trait methods are copied to function_table but not to methods vec)
            for trait_name in &class_def.uses {
                if let Some(trait_def) = self.class_table.get(trait_name.as_str()) {
                    for (name, vis, is_static, _is_final, _func) in &trait_def.methods {
                        if name.to_lowercase() == method_lower
                            && !trait_def.method_is_abstract(name)
                        {
                            if class_def.trait_aliases.iter().any(|adaptation| {
                                adaptation.alias.is_none()
                                    && adaptation.method.eq_ignore_ascii_case(method_name)
                            }) {
                                continue;
                            }
                            // Trait method visibility applies as if declared in the using class
                            return Some((*vis, *is_static, class_name.to_string()));
                        }
                    }
                }
            }
            for adaptation in &class_def.trait_aliases {
                let alias = adaptation.alias.as_deref().unwrap_or(&adaptation.method);
                if alias.eq_ignore_ascii_case(method_name) {
                    let source_trait = adaptation
                        .trait_name
                        .as_ref()
                        .and_then(|name| {
                            class_def
                                .uses
                                .iter()
                                .find(|used| used.eq_ignore_ascii_case(name))
                        })
                        .or_else(|| {
                            class_def.uses.iter().find(|used| {
                                self.class_table
                                    .get(used.as_str())
                                    .is_some_and(|definition| {
                                        definition.methods.iter().any(|(name, _, _, _, _)| {
                                            name.eq_ignore_ascii_case(&adaptation.method)
                                        })
                                    })
                            })
                        });
                    if let Some(trait_name) = source_trait
                        && let Some(trait_def) = self.class_table.get(trait_name.as_str())
                        && let Some((_, visibility, is_static, _, _)) =
                            trait_def.methods.iter().find(|(name, _, _, _, _)| {
                                name.eq_ignore_ascii_case(&adaptation.method)
                            })
                    {
                        return Some((
                            adaptation.visibility.unwrap_or(*visibility),
                            *is_static,
                            class_name.to_string(),
                        ));
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

    /// Record one successfully compiled main/include file exactly once while
    /// retaining PHP's first-inclusion order for runtime introspection.
    pub fn record_included_file(&mut self, path: String) {
        if self.included_files.insert(path.clone()) {
            self.included_file_order.push(path);
        }
    }

    pub fn included_file_names(&self) -> &[String] {
        &self.included_file_order
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
    fn class_is_a_while_linking(
        &self,
        class_name: &str,
        target: &str,
        linking_class: Option<&ClassDef>,
    ) -> bool {
        let Some(linking_class) =
            linking_class.filter(|definition| definition.name.eq_ignore_ascii_case(class_name))
        else {
            return self.class_is_a(class_name, target);
        };
        if linking_class.name.eq_ignore_ascii_case(target) {
            return true;
        }
        if target.eq_ignore_ascii_case("Stringable")
            && (linking_class
                .methods
                .iter()
                .any(|(name, _, _, _, _)| name.eq_ignore_ascii_case("__toString"))
                || linking_class
                    .parent
                    .as_deref()
                    .is_some_and(|parent| self.class_is_a(parent, target)))
        {
            return true;
        }
        linking_class
            .parent
            .as_deref()
            .is_some_and(|parent| self.class_is_a(parent, target))
            || linking_class
                .implements
                .iter()
                .any(|interface| self.class_is_a(interface, target))
    }

    fn variance_class_is_known(&self, class_name: &str, linking_class: Option<&ClassDef>) -> bool {
        linking_class.is_some_and(|definition| definition.name.eq_ignore_ascii_case(class_name))
            || self.find_class(class_name).is_some()
    }

    /// Class aliases and autoloaded declarations may not exist yet while an
    /// enclosing source unit is registering its classes. A known-negative
    /// relationship is an incompatibility; two unresolved names are
    /// inconclusive and must not produce a premature fatal before runtime code
    /// can publish their alias identity. Full delayed class linking remains a
    /// separate contract.
    fn variance_class_is_a(
        &self,
        class_name: &str,
        target: &str,
        linking_class: Option<&ClassDef>,
    ) -> bool {
        self.class_is_a_while_linking(class_name, target, linking_class)
            || (!self.variance_class_is_known(class_name, linking_class)
                && !self.variance_class_is_known(target, linking_class))
    }

    fn is_return_type_compatible(
        &self,
        impl_hint: &crate::vm::function::ParamTypeHint,
        iface_hint: &crate::vm::function::ParamTypeHint,
        impl_owner: &str,
        iface_owner: &str,
        linking_class: Option<&ClassDef>,
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
                return self.is_return_type_compatible(
                    impl_hint,
                    inner_iface,
                    impl_owner,
                    iface_owner,
                    linking_class,
                );
            }
            (ParamTypeHint::Nullable(_), _) => {
                // impl ?T vs iface T (widening) — incompatible
                return false;
            }
            _ => {}
        }

        // For declaration variance, iterable is precisely the built-in union
        // array|Traversable. Expanding it on both sides is important for
        // compound types such as iterable <: array|object and
        // X&Traversable <: iterable.
        if matches!(impl_hint, ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("iterable"))
        {
            let traversable = ParamTypeHint::ClassName("Traversable".to_string());
            return self.is_return_type_compatible(
                &ParamTypeHint::Array,
                iface_hint,
                impl_owner,
                iface_owner,
                linking_class,
            ) && self.is_return_type_compatible(
                &traversable,
                iface_hint,
                impl_owner,
                iface_owner,
                linking_class,
            );
        }
        if matches!(iface_hint, ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("iterable"))
        {
            let traversable = ParamTypeHint::ClassName("Traversable".to_string());
            return self.is_return_type_compatible(
                impl_hint,
                &ParamTypeHint::Array,
                impl_owner,
                iface_owner,
                linking_class,
            ) || self.is_return_type_compatible(
                impl_hint,
                &traversable,
                impl_owner,
                iface_owner,
                linking_class,
            );
        }

        // Covariant return compatibility is ordinary subtype checking over
        // union/intersection nodes.
        if let ParamTypeHint::Intersection(iface_parts) = iface_hint {
            return iface_parts.iter().all(|part| {
                self.is_return_type_compatible(
                    impl_hint,
                    part,
                    impl_owner,
                    iface_owner,
                    linking_class,
                )
            });
        }
        if let ParamTypeHint::Union(impl_parts) = impl_hint {
            return impl_parts.iter().all(|part| {
                self.is_return_type_compatible(
                    part,
                    iface_hint,
                    impl_owner,
                    iface_owner,
                    linking_class,
                )
            });
        }
        if let ParamTypeHint::Union(iface_parts) = iface_hint {
            return iface_parts.iter().any(|part| {
                self.is_return_type_compatible(
                    impl_hint,
                    part,
                    impl_owner,
                    iface_owner,
                    linking_class,
                )
            });
        }
        if let ParamTypeHint::Intersection(impl_parts) = impl_hint {
            return impl_parts.iter().any(|part| {
                self.is_return_type_compatible(
                    part,
                    iface_hint,
                    impl_owner,
                    iface_owner,
                    linking_class,
                )
            });
        }

        // PHP's `iterable` is the built-in union `array|Traversable` for
        // variance purposes. A concrete array return therefore narrows an
        // iterable declaration, while the reverse would widen it.
        if matches!(impl_hint, ParamTypeHint::Array)
            && matches!(iface_hint, ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("iterable"))
        {
            return true;
        }

        // Class name covariance
        if let (ParamTypeHint::ClassName(impl_class), ParamTypeHint::ClassName(iface_class)) =
            (impl_hint, iface_hint)
        {
            // Every class-like return type, including the late-static
            // pseudo-type, is a subtype of PHP's built-in `object` type.
            // This is what permits a trait method returning `static` to
            // implement an interface method returning `object`.
            if iface_class.eq_ignore_ascii_case("object") {
                return true;
            }
            // `static` remains late-bound in a return declaration. It may
            // narrow `self` or an ordinary ancestor contract, but replacing a
            // required `static` with `self` would widen the result for further
            // descendants and is therefore invalid.
            if iface_class.eq_ignore_ascii_case("static") {
                return impl_class.eq_ignore_ascii_case("static");
            }
            if impl_class.eq_ignore_ascii_case("static") {
                return self.variance_class_is_a(impl_owner, iface_class, linking_class);
            }
            return self.variance_class_is_a(impl_class, iface_class, linking_class);
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
        impl_owner: &str,
        iface_owner: &str,
        linking_class: Option<&ClassDef>,
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
                return self.is_param_type_compatible(
                    inner_impl,
                    iface_hint,
                    impl_owner,
                    iface_owner,
                    linking_class,
                );
            }
            (_, ParamTypeHint::Nullable(_)) => {
                // T in impl vs ?T in iface — impl rejects null, incompatible
                return false;
            }
            _ => {}
        }

        if matches!(impl_hint, ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("iterable"))
        {
            let traversable = ParamTypeHint::ClassName("Traversable".to_string());
            return self.is_param_type_compatible(
                &ParamTypeHint::Array,
                iface_hint,
                impl_owner,
                iface_owner,
                linking_class,
            ) || self.is_param_type_compatible(
                &traversable,
                iface_hint,
                impl_owner,
                iface_owner,
                linking_class,
            );
        }
        if matches!(iface_hint, ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("iterable"))
        {
            let traversable = ParamTypeHint::ClassName("Traversable".to_string());
            return self.is_param_type_compatible(
                impl_hint,
                &ParamTypeHint::Array,
                impl_owner,
                iface_owner,
                linking_class,
            ) && self.is_param_type_compatible(
                impl_hint,
                &traversable,
                impl_owner,
                iface_owner,
                linking_class,
            );
        }

        // Parameter compatibility reverses the subtype relation: the
        // implementation must accept every value admitted by the interface.
        if let ParamTypeHint::Intersection(impl_parts) = impl_hint {
            return impl_parts.iter().all(|part| {
                self.is_param_type_compatible(
                    part,
                    iface_hint,
                    impl_owner,
                    iface_owner,
                    linking_class,
                )
            });
        }
        if let ParamTypeHint::Union(iface_parts) = iface_hint {
            return iface_parts.iter().all(|part| {
                self.is_param_type_compatible(
                    impl_hint,
                    part,
                    impl_owner,
                    iface_owner,
                    linking_class,
                )
            });
        }
        if let ParamTypeHint::Union(impl_parts) = impl_hint {
            return impl_parts.iter().any(|part| {
                self.is_param_type_compatible(
                    part,
                    iface_hint,
                    impl_owner,
                    iface_owner,
                    linking_class,
                )
            });
        }
        if let ParamTypeHint::Intersection(iface_parts) = iface_hint {
            return iface_parts.iter().any(|part| {
                self.is_param_type_compatible(
                    impl_hint,
                    part,
                    impl_owner,
                    iface_owner,
                    linking_class,
                )
            });
        }

        if matches!(impl_hint, ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("iterable"))
            && matches!(iface_hint, ParamTypeHint::Array)
        {
            return true;
        }

        // Class name contravariance: iface declares A, impl declares B
        // Compatible if A is_a B (A is a subtype of B, so impl accepts wider)
        match (impl_hint, iface_hint) {
            (ParamTypeHint::ClassName(impl_class), ParamTypeHint::ClassName(iface_class)) => {
                if impl_class.eq_ignore_ascii_case("object") {
                    return true;
                }
                return self.variance_class_is_a(iface_class, impl_class, linking_class);
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
    class_table: &HashMap<String, std::rc::Rc<ClassDef>>,
    class_name: &str,
    target: &str,
) -> bool {
    let find = |name: &str| {
        class_table.get(name).map(std::rc::Rc::as_ref).or_else(|| {
            class_table
                .iter()
                .find(|(registered, _)| registered.eq_ignore_ascii_case(name))
                .map(|(_, class)| class.as_ref())
        })
    };
    let canonical_target = find(target).map_or(target, |class| class.name.as_str());
    let canonical_class = find(class_name).map_or(class_name, |class| class.name.as_str());
    if canonical_class.eq_ignore_ascii_case(canonical_target) {
        return true;
    }
    let Some(class_def) = find(class_name) else {
        return false;
    };
    class_def
        .parent
        .as_ref()
        .is_some_and(|parent| class_is_a_in_table(class_table, parent, canonical_target))
        || class_def
            .implements
            .iter()
            .any(|interface| class_is_a_in_table(class_table, interface, canonical_target))
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
