use std::cell::Cell;
use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use crate::compiler::compile::{
    ClassConstantDefinition, ClassDef, PropertyDefinition, ReboundTraitPropertyDefault,
    enum_magic_method_is_forbidden,
};
#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
use crate::generics::GenericType;
use crate::generics::{GenericMetadata, GenericMethodContract, ReifiedBinding};
use crate::parser::Visibility;
use crate::value::{ClosureStaticVars, ObjectLayout, PhpArray, PhpObject, Value};
use crate::vm::frame::ExecuteData;
use crate::vm::function::{Function, FunctionCommon, FunctionType, ParamTypeHint, SignatureInfo};
use crate::vm::instruction::OpType;
use crate::vm::opcode::OpCode;
use crate::vm::stack::VmStack;
use crate::vm::stats;
use crate::vm::virtual_aggregate_cache::{
    RESOLVED_VIRTUAL_AGGREGATE_CACHE_SLOTS, ResolvedVirtualAggregateCacheEntry,
};

mod cycle;
pub(crate) mod fiber;
#[path = "coroutine/state.rs"]
pub(crate) mod suspended;
mod weak;

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
    source_file: Option<&'a str>,
    source_line: usize,
    signature: &'a SignatureInfo,
    parameter_default_diagnostics: Option<&'a [Option<Box<str>>]>,
    return_type_is_tentative: bool,
    suppresses_tentative_return_deprecation: bool,
}

/// Cold callable contract for one built-in method. The descriptor is used by
/// class linking only; it deliberately does not publish a callable body or
/// claim that the surrounding extension is implemented.
struct InternalMethodContract {
    name: Box<str>,
    is_static: bool,
    signature: SignatureInfo,
    parameter_default_diagnostics: Vec<Option<Box<str>>>,
    return_type_is_tentative: bool,
}

type InternalFunctionReflectionMetadata = (
    Vec<Option<Value>>,
    &'static [Option<&'static str>],
    Option<&'static str>,
);

/// Sparse request-startup metadata for internal callables. Extending the
/// existing boxed owner keeps ExecutorGlobals and every hot ABI layout stable.
#[derive(Default)]
struct InternalCallableMetadata {
    functions: HashMap<*const FunctionCommon, InternalFunctionReflectionMetadata>,
    methods: HashMap<String, Vec<InternalMethodContract>>,
}

#[derive(Clone)]
pub(crate) struct EffectiveTraitMethod {
    pub(crate) target: String,
    origin_owner: String,
    origin_method: String,
    pub(crate) visibility: Visibility,
    pub(crate) is_static: bool,
    pub(crate) is_final: bool,
}

struct TraitCompositionMethod {
    target: String,
    provider: String,
    source_method: String,
    origin_owner: String,
    origin_method: String,
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

/// Resolve the three private slots shared by PHP's two built-in Throwable
/// roots without allocating a mangled name on every construction or throw.
/// Declared objects reveal their root directly through the immutable layout;
/// VM-created dynamic errors use the registered hierarchy as a cold fallback.
#[inline(always)]
pub(crate) fn throwable_private_property_key(
    eg: &ExecutorGlobals,
    object: &PhpObject,
    property: &str,
) -> &'static str {
    let (exception_key, error_key) = match property {
        "previous" => ("Exception\0previous", "Error\0previous"),
        "string" => ("Exception\0string", "Error\0string"),
        "trace" => ("Exception\0trace", "Error\0trace"),
        _ => unreachable!("only built-in private Throwable slots use this resolver"),
    };
    if object.contains_property(exception_key) {
        exception_key
    } else if object.contains_property(error_key) {
        error_key
    } else if eg.class_is_a(&object.class_name, "Exception") {
        exception_key
    } else {
        error_key
    }
}

include!("property_definitions.rs");
include!("class_constants.rs");

/// PHP string-based symbol APIs accept one leading namespace separator. Keep
/// multiple leading separators invalid so diagnostics retain the supplied
/// spelling instead of silently repairing a malformed name.
#[inline]
pub(crate) fn normalized_dynamic_symbol_name(name: &str) -> &str {
    match name.as_bytes() {
        [b'\\', next, ..] if *next != b'\\' => &name[1..],
        _ => name,
    }
}

/// Global constant identifiers are case-sensitive, while namespace segments
/// use PHP's case-insensitive symbol spelling. The final segment remains the
/// constant identifier and must therefore match byte-for-byte.
#[inline]
fn qualified_constant_name_matches(registered: &str, requested: &str) -> bool {
    match (registered.rsplit_once('\\'), requested.rsplit_once('\\')) {
        (
            Some((registered_namespace, registered_name)),
            Some((requested_namespace, requested_name)),
        ) => {
            registered_name == requested_name
                && registered_namespace.eq_ignore_ascii_case(requested_namespace)
        }
        (None, None) => registered == requested,
        _ => false,
    }
}

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
    pub(crate) closure_static_vars: Option<ClosureStaticVars>,
    pub(crate) is_magic_call: bool,
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

/// Minimal hierarchy metadata for a runtime declaration that has been taken
/// from its source marker but has not yet completed linking. Keeping this
/// separate from the public class table lets reentrant autoload prove a cycle's
/// relationships without exposing a half-composed class to ordinary lookups.
struct ActiveRuntimeClassRelation {
    parent: Option<String>,
    implements: Vec<String>,
    has_to_string: bool,
    outstanding_variance_dependencies: Vec<String>,
    /// A class linked during this relation has already relied on it for a
    /// method-variance proof, so PHP can no longer roll the declaration back.
    has_variance_dependents: Cell<bool>,
}

impl ActiveRuntimeClassRelation {
    fn from_class(class_def: &ClassDef) -> Self {
        Self {
            parent: class_def.parent.clone(),
            implements: class_def.implements.clone(),
            has_to_string: class_def
                .methods
                .iter()
                .any(|(name, _, _, _, _)| name.eq_ignore_ascii_case("__toString")),
            outstanding_variance_dependencies: Vec::new(),
            has_variance_dependents: Cell::new(false),
        }
    }
}

pub(crate) struct OutputBuffer {
    pub(crate) data: Vec<u8>,
    pub(crate) handler: Option<Value>,
    pub(crate) flags: i64,
    pub(crate) started: bool,
}

/// Minimal ExecutorGlobals for vertical slice.
/// Will grow as we implement more features.
pub(crate) struct AssertionState {
    /// Startup compilation mode from `zend.assertions`: -1 removes assertion
    /// bytecode, while 0 and 1 retain it and may toggle at runtime.
    pub startup_mode: i8,
    pub active: bool,
    pub bail: bool,
    pub warning: bool,
    pub exception: bool,
    pub callback: Option<crate::value::Value>,
}

impl Default for AssertionState {
    fn default() -> Self {
        Self {
            startup_mode: 1,
            active: true,
            bail: false,
            warning: true,
            exception: true,
            callback: None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct PhpErrorRecord {
    pub(crate) level: i64,
    pub(crate) message: String,
    pub(crate) file: String,
    pub(crate) line: usize,
}

#[derive(Default)]
struct JsonRuntimeState {
    error_code: i64,
    serializable_objects: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LazyObjectStrategy {
    Ghost,
    Proxy,
}

/// Sparse request-local state for Reflection lazy objects. Ordinary objects
/// retain their existing compact layout; a weak owner also prevents a stale
/// allocation identity from being reused while this entry exists.
pub(crate) struct LazyObjectState {
    pub(crate) owner: std::rc::Weak<std::cell::RefCell<crate::value::PhpObject>>,
    pub(crate) strategy: LazyObjectStrategy,
    pub(crate) initializer_value: crate::value::Value,
    pub(crate) initializer: crate::stdlib::ResolvedCallback,
    pub(crate) initializing: bool,
    pub(crate) lazy_slots: Vec<usize>,
    pub(crate) proxy_instance: Option<crate::value::Value>,
    pub(crate) options: u8,
}

/// Cold request-local payload for one engine-created ReflectionAttribute.
/// Keeping this state outside PhpObject preserves the ordinary object layout
/// and exposes only Zend's public `name` property to PHP code.
#[derive(Clone, Copy)]
pub(crate) enum ReflectionAttributeDeclarationKind {
    Plain,
    Method,
    Property,
    ClassConstant,
}

#[derive(Clone)]
pub(crate) struct ReflectionAttributeDeclaration {
    pub(crate) name: crate::value::Value,
    pub(crate) class_name: Option<crate::value::Value>,
    pub(crate) kind: ReflectionAttributeDeclarationKind,
}

pub(crate) struct ReflectionAttributeState {
    pub(crate) owner: std::rc::Weak<std::cell::RefCell<crate::value::PhpObject>>,
    pub(crate) definition: crate::vm::function::AttributeDefinition,
    pub(crate) repeated: bool,
    /// Shared reflected property handles used to build Zend's AST-style
    /// Closure declaration name only when `__toString()` is actually called.
    pub(crate) declaration: ReflectionAttributeDeclaration,
}

/// Cold request-local identity carried by one engine-created
/// ReflectionReference. The reference target itself remains owned by the
/// inspected array; ReflectionReference exposes only an opaque stable ID.
pub(crate) struct ReflectionReferenceState {
    pub(crate) owner: std::rc::Weak<std::cell::RefCell<crate::value::PhpObject>>,
    pub(crate) reference_identity: usize,
}

/// Cold engine-only metadata for one ReflectionProperty wrapper. PHP exposes
/// only the wrapper's declared `name` and `class` properties; retaining the
/// target and method metadata here keeps debug/object projections canonical.
pub(crate) struct ReflectionPropertyMetadata {
    pub(crate) target: crate::value::Value,
    pub(crate) property: String,
    pub(crate) modifiers: i64,
    pub(crate) has_type: bool,
    pub(crate) type_kind: String,
    pub(crate) type_name: String,
    pub(crate) allows_null: bool,
    pub(crate) has_default: bool,
    pub(crate) default: crate::value::Value,
}

pub(crate) struct ReflectionPropertyState {
    pub(crate) owner: std::rc::Weak<std::cell::RefCell<crate::value::PhpObject>>,
    pub(crate) metadata: ReflectionPropertyMetadata,
}

/// Cold request-local scope for one engine-created ReflectionParameter.
/// Dynamic properties remain compatible with Zend while attributes on trait
/// methods and bound closures still evaluate in their effective class scope.
pub(crate) struct ReflectionParameterState {
    pub(crate) owner: std::rc::Weak<std::cell::RefCell<crate::value::PhpObject>>,
    pub(crate) attribute_scope_class: String,
}

/// Mutable cursor for PHP's request-local `strtok()` continuation form.
pub(crate) struct StrtokState {
    pub(crate) input: Vec<u8>,
    pub(crate) position: usize,
}

/// Cold state shared by string utilities that retain request-local state.
/// Ordinary requests keep only the null sidecar word.
#[derive(Default)]
pub(crate) struct StringUtilityState {
    pub(crate) strtok: Option<StrtokState>,
    pub(crate) shuffle_random: u64,
}

enum ExecutionTimerCommand {
    Reset(u64),
    Disable,
}

/// Cold raw-pointer provenance queries share one audited boundary. Both
/// variants are sourced exclusively from executor-owned live tables/stacks.
enum SourceLocationQuery {
    Declaration(*const FunctionCommon),
    CurrentOutput(*mut ExecuteData),
}

struct ExecutionTimer {
    commands: Sender<ExecutionTimerCommand>,
}

impl ExecutionTimer {
    fn start(vm_interrupt: Arc<AtomicBool>, timed_out: Arc<AtomicBool>) -> std::io::Result<Self> {
        let (commands, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("rphp-execution-timer".to_string())
            .spawn(move || {
                let mut deadline: Option<Instant> = None;
                loop {
                    let command = if let Some(active_deadline) = deadline {
                        match receiver
                            .recv_timeout(active_deadline.saturating_duration_since(Instant::now()))
                        {
                            Ok(command) => command,
                            Err(RecvTimeoutError::Timeout) => {
                                timed_out.store(true, Ordering::Relaxed);
                                vm_interrupt.store(true, Ordering::Relaxed);
                                deadline = None;
                                continue;
                            }
                            Err(RecvTimeoutError::Disconnected) => break,
                        }
                    } else {
                        let Ok(command) = receiver.recv() else {
                            break;
                        };
                        command
                    };
                    deadline = match command {
                        ExecutionTimerCommand::Reset(seconds) => {
                            Instant::now().checked_add(Duration::from_secs(seconds))
                        }
                        ExecutionTimerCommand::Disable => None,
                    };
                }
            })?;
        Ok(Self { commands })
    }

    fn reset(&self, seconds: u64) -> bool {
        self.commands
            .send(ExecutionTimerCommand::Reset(seconds))
            .is_ok()
    }

    fn disable(&self) -> bool {
        self.commands.send(ExecutionTimerCommand::Disable).is_ok()
    }
}

pub struct ExecutorGlobals {
    pub vm_stack: VmStack,
    /// Compact argument-only activations for deferred pure-scalar calls.
    pub pending_call_stack: VmStack,
    pub current_execute_data: Cell<*mut ExecuteData>,
    pub vm_interrupt: Arc<AtomicBool>,
    pub timed_out: Arc<AtomicBool>,
    execution_timer: Option<ExecutionTimer>,
    /// Bounded request-local descriptors for structurally proven virtual
    /// call/return aggregates. The fixed array allocates nothing and RefCell
    /// mutation remains confined to the single VM execution thread.
    pub(crate) resolved_virtual_aggregate_cache: std::cell::RefCell<
        [ResolvedVirtualAggregateCacheEntry; RESOLVED_VIRTUAL_AGGREGATE_CACHE_SLOTS],
    >,
    /// Function table — name → pointer to FunctionCommon
    pub function_table: HashMap<String, *const FunctionCommon>,
    /// Compiler-owned helpers that must never participate in user function
    /// lookup, callable checks, Reflection or get_defined_functions().
    private_function_table: HashMap<String, *const FunctionCommon>,
    /// Class table — name/alias → shared ClassDef. `Rc` keeps metadata and
    /// inline-cache pointers stable while aliases reuse the exact identity.
    pub class_table: HashMap<String, std::rc::Rc<ClassDef>>,
    /// Anonymous declarations become visible only when their `new class`
    /// expression executes. Eager registration would autoload dependencies
    /// from branches that PHP never evaluates.
    pending_anonymous_classes: HashMap<String, ClassDef>,
    /// Named trait-consuming classes are linked only when execution reaches
    /// their source declaration. Keys are compiler-unique so conditional or
    /// repeated declarations with the same PHP name remain distinguishable.
    pending_runtime_classes: HashMap<String, ClassDef>,
    /// Hierarchy-only view of runtime declarations currently linking through
    /// nested autoload callbacks. Never consulted by ordinary symbol probes.
    active_runtime_class_relations: HashMap<String, ActiveRuntimeClassRelation>,
    /// Retain successfully executed declaration keys so invoking the same
    /// function-local declaration twice produces PHP's redeclaration error.
    declared_runtime_classes: HashMap<String, String>,
    /// Named declarations whose invariant property contracts depend on a
    /// class alias that top-level execution has not published yet.
    pending_named_classes: Vec<ClassDef>,
    /// Cold generic declaration side table. Ordinary dispatch never reads it.
    pub generic_metadata: GenericMetadata,
    /// Constant table — name → Value (case-sensitive, like PHP)
    /// Uses RefCell to allow define() from internal functions (which receive &self).
    pub constant_table: std::cell::RefCell<HashMap<Rc<str>, crate::value::Value>>,
    /// Successful dynamic-definition order exposed by get_defined_constants().
    /// Lookup remains hash-based; only the cold inventory API walks this list.
    constant_definition_order: std::cell::RefCell<Vec<Rc<str>>>,
    /// First __halt_compiler offset compiled for each PHP source name. Zend
    /// uses that source-name identity for dynamic constant() resolution,
    /// including repeated eval() calls from the same location.
    compiler_halt_offsets: Option<Box<HashMap<String, i64>>>,
    /// Reflection-only metadata for source-level global constants.
    pub constant_attributes: HashMap<String, Vec<crate::vm::function::AttributeDefinition>>,
    /// Cold dependency expressions used only when one constant read may need
    /// to diagnose deprecated constants referenced by its declaration value.
    pub constant_expressions: HashMap<String, crate::compiler::compile::ConstantExpressionMetadata>,
    /// Invalidates opcode-local negative Deprecated caches when an include
    /// contributes additional source-level constant metadata.
    pub(crate) constant_deprecation_generation: u32,
    /// Request-wide fast rejection for programs with no constant whose use can
    /// emit a Deprecated diagnostic. Includes may turn it on but never off.
    pub(crate) constant_deprecation_metadata_present: bool,
    /// Class IDs whose deferred class constants must be materialized before
    /// the first object allocation. The sidecar is absent from ordinary
    /// requests and retains pending entries after a retryable failure.
    deferred_class_constant_activations: Option<Box<Vec<u8>>>,
    /// Recursion guard for self-referential Deprecated messages and aliases.
    pub(crate) deprecated_symbol_stack: Vec<String>,
    /// Parsed and compiled regular expressions shared by all preg_* calls for
    /// the lifetime of this executor.
    pub regex_cache: crate::regex::RegexCache,
    /// Exception being thrown — None = no exception
    pub exception: Option<crate::value::Value>,
    /// Exceptions suspended while their frame executes a finally block. This
    /// cold sidecar keeps them separate from newly raised VM exceptions.
    pub(crate) finally_exceptions: HashMap<usize, Vec<crate::value::Value>>,
    /// Per-clone readonly properties that may be reinitialized once while the
    /// engine invokes `__clone`. Kept cold to preserve object/frame layouts.
    pub(crate) clone_readonly_reinitialization: Vec<(usize, std::collections::HashSet<String>)>,
    /// Snapshot of initialized readonly properties eligible for clone-with.
    pub(crate) clone_with_readonly_updates: Vec<(usize, usize, std::collections::HashSet<String>)>,
    /// Reflection lazy-object metadata exists only after an explicit lazy
    /// construction/reset. The absent sidecar keeps ordinary object creation
    /// and request setup allocation-free.
    pub(crate) lazy_objects: Option<Box<HashMap<usize, LazyObjectState>>>,
    /// Legacy assert_options() settings are request-local and consulted only
    /// by assert(), keeping ordinary call frames and dispatch paths unchanged.
    pub(crate) assertion_state: AssertionState,
    /// Request-local error mask exposed by error_reporting(). Diagnostic
    /// routing is still intentionally minimal, but libraries observe the
    /// getter/setter contract while temporarily suppressing warnings.
    pub error_reporting: i64,
    /// Significant digits used when PHP converts a float to a string. Keeping
    /// the parsed value request-local makes CLI `-d` and `ini_set()` visible to
    /// the VM without a hash lookup on each conversion.
    pub(crate) precision: i32,
    /// Significant digits used by serialization and export functions. This is
    /// request-local and separate from ordinary float-to-string conversion.
    pub(crate) serialize_precision: i32,
    /// Suppressed call frame and the reporting mask to restore when it leaves.
    /// This cold sidecar keeps the ordinary ExecuteData layout unchanged.
    error_suppression_frames: Vec<(usize, i64)>,
    pub(crate) error_handler: Option<crate::value::Value>,
    pub(crate) error_handler_levels: i64,
    pub(crate) error_handler_stack: Vec<(Option<crate::value::Value>, i64)>,
    /// Most recent unhandled PHP diagnostic, including diagnostics hidden by
    /// `@` or the current reporting mask. Allocated strings stay on this cold
    /// observability path and do not enlarge call frames or values.
    pub(crate) last_error: Option<PhpErrorRecord>,
    pub(crate) exception_handler: Option<crate::value::Value>,
    pub(crate) exception_handler_stack: Vec<Option<crate::value::Value>>,
    /// Request-shutdown callbacks retain resolved callable state and supplied
    /// arguments until top-level execution finishes. The queue is allocated
    /// only after the first registration so ordinary requests keep one null
    /// sidecar word and no teardown scan.
    pub(crate) shutdown_functions:
        Option<Box<std::collections::VecDeque<crate::stdlib::ShutdownFunction>>>,
    /// WeakReference/WeakMap payloads are cold request state. A null sidecar
    /// leaves ordinary objects and requests at their established layouts.
    weak_objects: Option<Box<weak::WeakObjectRuntime>>,
    /// Reverse map: func_ptr → declaring class name (for visibility scope resolution)
    pub method_declaring_class: HashMap<*const FunctionCommon, String>,
    /// Sparse canonical spellings for built-ins whose public name is not the
    /// lowercase lookup key. Most internal functions need no entry.
    internal_function_display_names: Option<Box<HashMap<*const FunctionCommon, String>>>,
    /// Exact reflection defaults and link-only method contracts for sparse
    /// internal callables. Keeping the combined owner boxed preserves both
    /// the hot FunctionCommon descriptor and ExecutorGlobals field offsets.
    internal_callable_metadata: Option<Box<InternalCallableMetadata>>,
    /// Internal static methods share the hidden class-call slot used by the
    /// method ABI, so staticness cannot be inferred from `this_offset` alone.
    internal_static_methods: Option<Box<std::collections::HashSet<*const FunctionCommon>>>,
    /// Output buffer — collected output for testing, or stdout
    output: std::cell::RefCell<Box<dyn Write>>,
    output_buffers: std::cell::RefCell<Vec<OutputBuffer>>,
    /// Whether at least one non-empty byte reached the underlying request
    /// sink. Buffered and empty writes do not publish headers in PHP.
    headers_sent: Cell<bool>,
    /// Source of the first underlying write. The allocation stays absent for
    /// requests which produce no output or whose source has no PHP location.
    header_output_origin: std::cell::RefCell<Option<(String, usize)>>,
    /// Historical libxml entity-loader switch. PHP 8 keeps the deprecated API
    /// request-local even though external entity loading is disabled by
    /// default independently of this compatibility bit.
    libxml_entity_loader_disabled: Cell<bool>,
    /// Active user output-handler calls. PHP's source presentation helpers
    /// use an internal output buffer and must reject re-entry from one of
    /// these callbacks, while ordinary output remains writable there.
    output_handler_depth: Cell<usize>,
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
    /// Sparse argument snapshots and their reusable buffers. This cold owner
    /// intentionally retains the footprint of the preceding HashMap field so
    /// the offsets of later request-hot fields do not depend on whether PHP
    /// argument introspection is enabled for one call.
    pub(crate) function_argument_state: FunctionArgumentState,
    /// Active generator being executed (set during resume, used by Yield opcode)
    pub active_generator: Option<crate::vm::generator::GeneratorRef>,
    /// PHP Fiber contexts allocate alternate VM stacks only after the first
    /// Fiber object is constructed. Ordinary requests retain one null pointer.
    fiber_runtime: Option<Box<fiber::FiberRuntime>>,
    /// Global variables — shared across function calls via `global $x;`
    pub globals: HashMap<String, crate::value::Value>,
    /// Names created only through `$$name`/`${expr}` have no compiler-owned CV
    /// slot. Keep those rare entries in a frame-keyed cold symbol table while
    /// statically known names continue to live directly in their CVs.
    pub(crate) dynamic_variables: HashMap<usize, HashMap<String, crate::value::Value>>,
    /// Included code executes in its caller's variable scope. This sparse map
    /// aliases an include frame to the owning caller frame without changing
    /// the ordinary ExecuteData layout.
    pub(crate) dynamic_scope_owners: HashMap<usize, usize>,
    /// Logical callers of synchronous engine-created callback frames. Their
    /// physical predecessor stays null so `Return` exits the detached
    /// executor, while live backtraces can still cross the callback boundary.
    detached_trace_callers: Option<Box<Vec<(usize, usize, bool)>>>,
    /// Optional synthetic call sites for engine-created callbacks and source
    /// units. Attribute constructors retain their declaration origin; eval
    /// additionally publishes its logical frame name without widening the hot
    /// call-frame layout.
    detached_trace_origins: Option<Box<HashMap<usize, (String, usize, Option<String>)>>>,
    /// A discarded `call_user_func*()` result applies to the resolved callback,
    /// whose engine-created detached frame otherwise has an ordinary return
    /// slot. The wrapper publishes this one synchronous bit until the user
    /// callback entry consumes it.
    detached_return_discarded: bool,
    /// Globals modified by the last callee Return (for selective re-read by caller)
    pub dirty_globals: std::collections::HashSet<String>,
    /// Number of synchronous entries into a request-local PHP error handler.
    /// Diagnostic dimension writes snapshot this cold epoch so ordinary key
    /// side effects remain untouched while callback-driven root replacement
    /// can suppress stale writeback.
    pub(crate) error_handler_generation: u64,
    /// Latest error-handler epoch that explicitly wrote each request-global
    /// name. Lazily allocated because ordinary requests and handlers that only
    /// inspect diagnostics need no map. This distinguishes `null = null` from
    /// an untouched root, which PHP treats differently for stale writeback.
    error_handler_dirty_globals: Option<Box<HashMap<String, u64>>>,
    /// Non-zero only while the matching request error callback is executing.
    /// Global write opcodes use it to retain a write event even when the value
    /// itself and the eventual caller slot are bit-for-bit unchanged.
    pub(crate) active_error_handler_generation: u64,
    /// Static variables — persisted across function calls: func_name → (var_name → value)
    pub static_vars: HashMap<String, HashMap<String, crate::value::Value>>,
    /// Closure-owned function-static cells for active or pending frames. The
    /// sidecar is absent unless a static-bearing anonymous Closure is called.
    closure_static_frames: Option<HashMap<usize, ClosureStaticVars>>,
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
    /// Trait methods cloned per consumer/alias for independent function-static
    /// storage or consumer-specific declaration diagnostics.
    trait_static_functions: Vec<Box<crate::vm::function::UserFunction>>,
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
    /// Lazy class-id cache for destructor lookup. Catch-variable replacement
    /// and other final-root boundaries may query the same Throwable class in
    /// hot loops; method inheritance resolution is paid only once per class.
    class_destructor_flags: std::cell::RefCell<Vec<u8>>,
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
    /// Whether the canonical slot has completed its one publication walk for
    /// compiler-deferred enum-case handles. Keeping the bit beside canonical
    /// storage preserves O(1) warmed reads even for large array defaults.
    static_property_handles_published: Vec<Cell<bool>>,
    /// Runtime writes may place destructor-bearing objects in class or named-
    /// function static storage. Ordinary requests leave this false and skip
    /// the cold fixed-point scan entirely.
    request_static_values_may_retain_objects: bool,
    /// Per-class property-index → canonical storage-slot mapping. Slot zero in
    /// this outer vector is reserved alongside `class_by_id`.
    static_property_slots_by_class: Vec<Box<[u32]>>,
    #[cfg(feature = "php-generics-reified")]
    static_generic_property_contracts: Vec<Box<StaticGenericPropertyContract>>,
    /// Observable request-local state for PHP's cycle-collector controls.
    /// RPHP does not yet maintain a separate cycle queue, so this flag affects
    /// the control API without changing reference-counted value reclamation.
    pub(crate) gc_enabled: bool,
    /// Stateful tokenization and shuffle PRNG data are allocated only after
    /// the corresponding standard-library function is first called.
    pub(crate) string_utility_state: Option<Box<StringUtilityState>>,
    /// Lazily allocated request-local overrides for the admitted mutable INI
    /// subset. Requests that never call `ini_set()` retain only this null word.
    pub(crate) ini_overrides: Option<Box<HashMap<String, String>>>,
    /// Engine-created ReflectionAttribute payloads are rare and must not
    /// appear as user-visible dynamic properties.
    pub(crate) reflection_attributes: Option<Box<HashMap<usize, ReflectionAttributeState>>>,
    /// Engine-created ReflectionReference identities are equally sparse and
    /// must not leak as ordinary object properties.
    reflection_references: Option<Box<HashMap<usize, ReflectionReferenceState>>>,
    /// ReflectionProperty method state is engine-only; PHP sees only the
    /// wrapper's declared `name` and `class` properties.
    reflection_properties: Option<Box<HashMap<usize, ReflectionPropertyState>>>,
    /// Effective attribute scopes for reflected parameters are equally rare;
    /// keeping them here preserves ReflectionParameter's observable shape.
    reflection_parameters: Option<Box<HashMap<usize, ReflectionParameterState>>>,
    /// Request-local ext/json error and reentrant JsonSerializable guards.
    /// Successful ordinary requests retain only this null sidecar word.
    json_runtime: Option<Box<JsonRuntimeState>>,
}

/// Stable storage for only the positional tail that no longer has a distinct
/// compiler-owned parameter CV. Declared arguments remain readable from the
/// live frame; keeping the first tail index avoids cloning them solely to
/// preserve func_get_arg(s) and trace observability.
pub(crate) struct FunctionArgumentSnapshot {
    pub(crate) first: u32,
    pub(crate) values: Vec<crate::value::Value>,
}

/// LIFO storage for the uncommon user-call tail that outlives its raw send
/// slots. The reserve keeps `ExecutorGlobals` fields following the former
/// 48-byte HashMap at their established offsets while replacing its hashing
/// work with a stack lookup.
#[repr(C)]
pub(crate) struct FunctionArgumentState {
    snapshots: Vec<(usize, FunctionArgumentSnapshot)>,
    buffers: Option<Box<Vec<Vec<crate::value::Value>>>>,
    _layout_reserve: [usize; 2],
}

const _: [(); std::mem::size_of::<HashMap<usize, Vec<crate::value::Value>>>()] =
    [(); std::mem::size_of::<FunctionArgumentState>()];

impl FunctionArgumentState {
    pub(crate) const fn new() -> Self {
        Self {
            snapshots: Vec::new(),
            buffers: None,
            _layout_reserve: [0; 2],
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    pub(crate) fn clear(&mut self) {
        self.snapshots.clear();
    }
}

pub(crate) enum ClassAliasRegistrationError {
    NameConflict,
    DelayedLink(String),
}

const PHP_82_SUPPRESSED_ERROR_REPORTING: i64 = 1 | 4 | 16 | 64 | 256 | 4096;

impl ExecutorGlobals {
    pub(crate) fn publish_function_arguments(
        &mut self,
        frame: usize,
        snapshot: FunctionArgumentSnapshot,
    ) {
        debug_assert!(
            self.function_argument_state
                .snapshots
                .iter()
                .all(|(candidate, _)| *candidate != frame),
            "one live frame owns at most one argument snapshot"
        );
        self.function_argument_state
            .snapshots
            .push((frame, snapshot));
    }

    pub(crate) fn function_arguments_for(&self, frame: usize) -> Option<&FunctionArgumentSnapshot> {
        self.function_argument_state
            .snapshots
            .iter()
            .rev()
            .find_map(|(candidate, snapshot)| (*candidate == frame).then_some(snapshot))
    }

    pub(crate) fn take_function_arguments(
        &mut self,
        frame: usize,
    ) -> Option<FunctionArgumentSnapshot> {
        if self
            .function_argument_state
            .snapshots
            .last()
            .is_some_and(|(candidate, _)| *candidate == frame)
        {
            return self
                .function_argument_state
                .snapshots
                .pop()
                .map(|(_, snapshot)| snapshot);
        }
        let index = self
            .function_argument_state
            .snapshots
            .iter()
            .rposition(|(candidate, _)| *candidate == frame)?;
        Some(self.function_argument_state.snapshots.remove(index).1)
    }

    pub(crate) fn take_function_argument_buffer(
        &mut self,
        capacity: usize,
    ) -> Vec<crate::value::Value> {
        let mut buffer = self
            .function_argument_state
            .buffers
            .as_deref_mut()
            .and_then(Vec::pop)
            .unwrap_or_default();
        buffer.reserve(capacity.saturating_sub(buffer.capacity()));
        buffer
    }

    pub(crate) fn recycle_function_argument_buffer(
        &mut self,
        mut buffer: Vec<crate::value::Value>,
    ) {
        buffer.clear();
        let pool = self
            .function_argument_state
            .buffers
            .get_or_insert_with(|| Box::new(Vec::new()));
        if buffer.capacity() <= 64 && pool.len() < 8 {
            pool.push(buffer);
        }
    }

    pub(crate) fn mark_global_dirty(&mut self, name: String) {
        if self.active_error_handler_generation != 0 {
            let writes = self
                .error_handler_dirty_globals
                .get_or_insert_with(|| Box::new(HashMap::new()));
            let recorded = writes.entry(name.clone()).or_insert(0);
            *recorded = (*recorded).max(self.active_error_handler_generation);
        }
        self.dirty_globals.insert(name);
    }

    pub(crate) fn error_handler_wrote_global_since(&self, name: &str, generation: u64) -> bool {
        self.error_handler_dirty_globals
            .as_deref()
            .and_then(|writes| writes.get(name))
            .is_some_and(|written| *written > generation)
    }

    pub(crate) fn json_last_error(&self) -> i64 {
        self.json_runtime
            .as_deref()
            .map_or(0, |state| state.error_code)
    }

    pub(crate) fn set_json_last_error(&mut self, code: i64) {
        if code == 0
            && self
                .json_runtime
                .as_deref()
                .is_none_or(|state| state.serializable_objects.is_empty())
        {
            self.json_runtime = None;
            return;
        }
        self.json_runtime
            .get_or_insert_with(|| Box::new(JsonRuntimeState::default()))
            .error_code = code;
    }

    pub(crate) fn enter_json_serializable_object(&mut self, identity: usize) -> bool {
        let stack = self
            .json_runtime
            .get_or_insert_with(|| Box::new(JsonRuntimeState::default()));
        let stack = &mut stack.serializable_objects;
        if stack.contains(&identity) {
            return false;
        }
        stack.push(identity);
        true
    }

    pub(crate) fn leave_json_serializable_object(&mut self, identity: usize) {
        let empty = {
            let stack = self
                .json_runtime
                .as_deref_mut()
                .expect("JsonSerializable recursion state disappeared");
            let stack = &mut stack.serializable_objects;
            let left = stack.pop();
            debug_assert_eq!(left, Some(identity));
            stack.is_empty()
        };
        if empty && self.json_last_error() == 0 {
            self.json_runtime = None;
        }
    }

    /// Reset the request-local execution timer used by `set_time_limit()`.
    /// The worker is allocated only after the first positive limit; ordinary
    /// requests and explicit disable calls retain no timer thread.
    pub(crate) fn set_execution_time_limit(&mut self, seconds: i64) -> bool {
        self.timed_out.store(false, Ordering::Relaxed);
        if seconds <= 0 {
            return self
                .execution_timer
                .as_ref()
                .is_none_or(ExecutionTimer::disable);
        }
        if self.execution_timer.is_none() {
            let Ok(timer) =
                ExecutionTimer::start(Arc::clone(&self.vm_interrupt), Arc::clone(&self.timed_out))
            else {
                return false;
            };
            self.execution_timer = Some(timer);
        }
        self.execution_timer
            .as_ref()
            .is_some_and(|timer| timer.reset(seconds as u64))
    }

    #[cold]
    pub(crate) fn register_reflection_attribute(
        &mut self,
        object: &crate::value::Value,
        definition: crate::vm::function::AttributeDefinition,
        repeated: bool,
        declaration: ReflectionAttributeDeclaration,
    ) {
        let (Some(identity), Some(owner)) = (object.object_identity(), object.object_weak()) else {
            return;
        };
        self.reflection_attributes
            .get_or_insert_with(|| Box::new(HashMap::new()))
            .insert(
                identity,
                ReflectionAttributeState {
                    owner,
                    definition,
                    repeated,
                    declaration,
                },
            );
    }

    #[inline]
    pub(crate) fn reflection_attribute_state(
        &self,
        object: &crate::value::Value,
    ) -> Option<&ReflectionAttributeState> {
        let identity = object.object_identity()?;
        let state = self.reflection_attributes.as_ref()?.get(&identity)?;
        (state.owner.strong_count() != 0).then_some(state)
    }

    #[cold]
    pub(crate) fn register_reflection_reference(
        &mut self,
        object: &crate::value::Value,
        reference_identity: usize,
    ) {
        let (Some(identity), Some(owner)) = (object.object_identity(), object.object_weak()) else {
            return;
        };
        self.reflection_references
            .get_or_insert_with(|| Box::new(HashMap::new()))
            .insert(
                identity,
                ReflectionReferenceState {
                    owner,
                    reference_identity,
                },
            );
    }

    #[inline]
    pub(crate) fn reflection_reference_identity(
        &self,
        object: &crate::value::Value,
    ) -> Option<usize> {
        let identity = object.object_identity()?;
        let state = self.reflection_references.as_ref()?.get(&identity)?;
        (state.owner.strong_count() != 0).then_some(state.reference_identity)
    }

    #[cold]
    pub(crate) fn register_reflection_property(
        &mut self,
        object: &crate::value::Value,
        metadata: ReflectionPropertyMetadata,
    ) {
        let (Some(identity), Some(owner)) = (object.object_identity(), object.object_weak()) else {
            return;
        };
        let properties = self
            .reflection_properties
            .get_or_insert_with(|| Box::new(HashMap::new()));
        if properties.len() >= 256 && properties.len().is_power_of_two() {
            properties.retain(|_, state| state.owner.strong_count() != 0);
        }
        properties.insert(identity, ReflectionPropertyState { owner, metadata });
    }

    #[inline]
    pub(crate) fn reflection_property_metadata(
        &self,
        object: &crate::value::Value,
    ) -> Option<&ReflectionPropertyMetadata> {
        let identity = object.object_identity()?;
        let state = self.reflection_properties.as_ref()?.get(&identity)?;
        (state.owner.strong_count() != 0).then_some(&state.metadata)
    }

    #[cold]
    pub(crate) fn register_reflection_parameter_scope(
        &mut self,
        object: &crate::value::Value,
        attribute_scope_class: String,
    ) {
        let (Some(identity), Some(owner)) = (object.object_identity(), object.object_weak()) else {
            return;
        };
        self.reflection_parameters
            .get_or_insert_with(|| Box::new(HashMap::new()))
            .insert(
                identity,
                ReflectionParameterState {
                    owner,
                    attribute_scope_class,
                },
            );
    }

    #[inline]
    pub(crate) fn reflection_parameter_scope(&self, object: &crate::value::Value) -> Option<&str> {
        let identity = object.object_identity()?;
        let state = self.reflection_parameters.as_ref()?.get(&identity)?;
        (state.owner.strong_count() != 0).then_some(state.attribute_scope_class.as_str())
    }

    #[cold]
    pub(crate) fn register_lazy_object(
        &mut self,
        object: &crate::value::Value,
        strategy: LazyObjectStrategy,
        initializer_value: crate::value::Value,
        initializer: crate::stdlib::ResolvedCallback,
        options: u8,
        explicit_lazy_slots: Option<Vec<usize>>,
    ) -> bool {
        let Some(identity) = object.object_identity() else {
            return false;
        };
        let Some(owner) = object.object_weak() else {
            return false;
        };
        let lazy_slots: Vec<usize> = explicit_lazy_slots.unwrap_or_else(|| {
            object
                .as_object()
                .map(|object| {
                    (0..object.property_values.len())
                        .filter(|slot| {
                            self.instance_property_definition(object.class_id, *slot)
                                .is_none_or(|definition| !definition.is_virtual_hook_property())
                        })
                        .collect()
                })
                .unwrap_or_default()
        });
        if lazy_slots.is_empty() {
            return false;
        }
        self.lazy_objects
            .get_or_insert_with(|| Box::new(HashMap::new()))
            .insert(
                identity,
                LazyObjectState {
                    owner,
                    strategy,
                    initializer_value,
                    initializer,
                    initializing: false,
                    lazy_slots,
                    proxy_instance: None,
                    options,
                },
            );
        true
    }

    #[inline]
    pub(crate) fn lazy_object_state(
        &self,
        object: &crate::value::Value,
    ) -> Option<&LazyObjectState> {
        let identity = object.object_identity()?;
        let state = self.lazy_objects.as_ref()?.get(&identity)?;
        (state.owner.strong_count() != 0).then_some(state)
    }

    #[inline]
    pub(crate) fn lazy_object_state_mut(
        &mut self,
        object: &crate::value::Value,
    ) -> Option<&mut LazyObjectState> {
        let identity = object.object_identity()?;
        let state = self.lazy_objects.as_mut()?.get_mut(&identity)?;
        (state.owner.strong_count() != 0).then_some(state)
    }

    #[cold]
    pub(crate) fn take_lazy_object_state(
        &mut self,
        object: &crate::value::Value,
    ) -> Option<LazyObjectState> {
        let identity = object.object_identity()?;
        let objects = self.lazy_objects.as_mut()?;
        let state = objects.remove(&identity);
        if objects.is_empty() {
            self.lazy_objects = None;
        }
        state
    }

    #[cold]
    pub(crate) fn restore_lazy_object_state(
        &mut self,
        object: &crate::value::Value,
        state: LazyObjectState,
    ) {
        let Some(identity) = object.object_identity() else {
            return;
        };
        self.lazy_objects
            .get_or_insert_with(|| Box::new(HashMap::new()))
            .insert(identity, state);
    }

    #[inline]
    pub(crate) fn is_uninitialized_lazy_object(&self, object: &crate::value::Value) -> bool {
        self.lazy_object_state(object)
            .is_some_and(|state| state.proxy_instance.is_none())
    }

    #[inline]
    pub(crate) fn lazy_property_requires_initialization(
        &self,
        object: &crate::value::Value,
        key: &str,
    ) -> bool {
        let Some(state) = self.lazy_object_state(object) else {
            return false;
        };
        if state.initializing || state.proxy_instance.is_some() {
            return false;
        }
        let slot = object
            .as_object()
            .and_then(|object| object.property_slot(key));
        slot.map_or(true, |slot| state.lazy_slots.contains(&slot))
    }

    #[inline]
    pub(crate) fn lazy_proxy_instance(
        &self,
        object: &crate::value::Value,
    ) -> Option<crate::value::Value> {
        let mut instance = self.lazy_object_state(object)?.proxy_instance.clone()?;
        let mut identities = Vec::with_capacity(4);
        if let Some(identity) = object.object_identity() {
            identities.push(identity);
        }
        for _ in 0..15 {
            let Some(identity) = instance.object_identity() else {
                break;
            };
            if identities.contains(&identity) {
                break;
            }
            identities.push(identity);
            let Some(next) = self
                .lazy_object_state(&instance)
                .and_then(|state| state.proxy_instance.clone())
            else {
                break;
            };
            instance = next;
        }
        Some(instance)
    }

    /// Property-operation guards are shared by every endpoint of an
    /// initialized lazy-proxy chain. Zend may enter a magic method on the real
    /// instance and recursively access the proxy shell (or initialize the
    /// proxy while a shell method is active); both directions must observe the
    /// same guard without enlarging the ordinary object layout.
    #[cold]
    pub(crate) fn lazy_proxy_related_property_guard_active(
        &self,
        object: &crate::value::Value,
        name: &str,
        operation: u8,
    ) -> bool {
        let Some(start) = object.object_identity() else {
            return false;
        };
        let Some(objects) = self.lazy_objects.as_ref() else {
            return false;
        };

        let mut connected = vec![start];
        let mut cursor = 0;
        while cursor < connected.len() {
            let identity = connected[cursor];
            cursor += 1;

            if identity != start
                && let Some(state) = objects.get(&identity)
                && let Some(owner) = state.owner.upgrade()
                && owner
                    .try_borrow()
                    .is_ok_and(|object| object.property_guard_active(name, operation))
            {
                return true;
            }

            for (shell, state) in objects.iter() {
                if state.strategy != LazyObjectStrategy::Proxy {
                    continue;
                }
                let Some(instance) = state.proxy_instance.as_ref() else {
                    continue;
                };
                let Some(instance_identity) = instance.object_identity() else {
                    continue;
                };
                if instance_identity == identity {
                    if instance.as_object_rc().is_some_and(|owner| {
                        owner
                            .try_borrow()
                            .is_ok_and(|object| object.property_guard_active(name, operation))
                    }) {
                        return true;
                    }
                    if !connected.contains(shell) {
                        connected.push(*shell);
                    }
                }
                if *shell == identity && !connected.contains(&instance_identity) {
                    connected.push(instance_identity);
                }
            }
        }
        false
    }

    #[inline]
    pub(crate) fn mark_initializing_lazy_property_written(
        &mut self,
        object: &crate::value::Value,
        key: &str,
    ) {
        let Some(slot) = object
            .as_object()
            .and_then(|object| object.property_slot(key))
        else {
            return;
        };
        if let Some(state) = self.lazy_object_state_mut(object)
            && state.initializing
        {
            state.lazy_slots.retain(|candidate| *candidate != slot);
        }
    }

    #[cold]
    pub(crate) fn clone_initialized_lazy_proxy(
        &mut self,
        source: &crate::value::Value,
        clone: &crate::value::Value,
    ) {
        let Some(state) = self.lazy_object_state(source) else {
            return;
        };
        if state.strategy != LazyObjectStrategy::Proxy {
            return;
        }
        let Some(instance) = state.proxy_instance.as_ref() else {
            return;
        };
        let Some(cloned_instance) = instance
            .as_object()
            .map(|instance| crate::value::Value::object(instance.clone_for_php()))
        else {
            return;
        };
        let initializer = state.initializer.clone();
        let options = state.options;
        let Some(identity) = clone.object_identity() else {
            return;
        };
        let Some(owner) = clone.object_weak() else {
            return;
        };
        self.lazy_objects
            .get_or_insert_with(|| Box::new(HashMap::new()))
            .insert(
                identity,
                LazyObjectState {
                    owner,
                    strategy: LazyObjectStrategy::Proxy,
                    initializer_value: crate::value::Value::null(),
                    initializer,
                    initializing: false,
                    lazy_slots: Vec::new(),
                    proxy_instance: Some(cloned_instance),
                    options,
                },
            );
    }

    pub fn emit_compile_deprecations(
        &mut self,
        diagnostics: &[crate::compiler::compile::CompileDeprecation],
    ) {
        for diagnostic in diagnostics {
            self.emit_unhandled_compile_diagnostic(diagnostic);
        }
    }

    /// Runtime-compiled include/eval diagnostics enter the active PHP error
    /// handler before the ordinary output path. Startup compilation has no
    /// executable frame or installed handler and continues to use
    /// `emit_compile_deprecations` directly.
    pub(crate) fn emit_runtime_compile_deprecations(
        &mut self,
        caller: *mut ExecuteData,
        diagnostics: &[crate::compiler::compile::CompileDeprecation],
    ) -> Result<(), crate::vm::execute::VmError> {
        for diagnostic in diagnostics {
            let level = if diagnostic.warning { 2 } else { 8192 };
            let handled = crate::stdlib::dispatch_php_error(
                self,
                caller,
                level,
                &diagnostic.message,
                &diagnostic.file,
                diagnostic.line,
            )?;
            if self.exception.is_some() {
                break;
            }
            if !handled {
                self.emit_unhandled_compile_diagnostic(diagnostic);
            }
        }
        Ok(())
    }

    fn emit_unhandled_compile_diagnostic(
        &mut self,
        diagnostic: &crate::compiler::compile::CompileDeprecation,
    ) {
        let (level, label) = if diagnostic.warning {
            (2, "Warning")
        } else {
            (8192, "Deprecated")
        };
        self.record_last_error(
            level,
            &diagnostic.message,
            &diagnostic.file,
            diagnostic.line,
        );
        if self.error_reporting & level != 0 {
            self.write_output(
                format!(
                    "\n{label}: {} in {} on line {}\n",
                    diagnostic.message, diagnostic.file, diagnostic.line,
                )
                .as_bytes(),
            );
        }
    }

    pub(crate) fn record_last_error(&mut self, level: i64, message: &str, file: &str, line: usize) {
        self.last_error = Some(PhpErrorRecord {
            level,
            message: message.to_string(),
            file: file.to_string(),
            line,
        });
    }

    /// Whether startup diagnostics already established an output boundary.
    /// The CLI uses this only to preserve PHP's blank line before a following
    /// declaration fatal; the diagnostic payload remains request-private.
    pub fn has_recorded_php_error(&self) -> bool {
        self.last_error.is_some()
    }

    pub(crate) fn begin_error_suppression(&mut self, frame: usize) {
        self.error_suppression_frames
            .push((frame, self.error_reporting));
        // PHP leaves fatal error classes visible under @, intersected with
        // the reporting mask that was active before suppression began.
        self.error_reporting &= PHP_82_SUPPRESSED_ERROR_REPORTING;
    }

    /// Reporting mask a newly entered detached execution context inherits.
    /// A caller-side `@` is an execution-frame property and must not leak into
    /// a Fiber or coroutine entered by the suppressed call itself.
    pub(crate) fn unsuppressed_error_reporting(&self) -> i64 {
        self.error_suppression_frames
            .first()
            .map(|(_, reporting)| *reporting)
            .unwrap_or(self.error_reporting)
    }

    pub(crate) fn end_error_suppression(&mut self, frame: usize) {
        if let Some(index) = self
            .error_suppression_frames
            .iter()
            .rposition(|(candidate, _)| *candidate == frame)
        {
            let (_, reporting) = self.error_suppression_frames.remove(index);
            // Zend restores the saved mask only while the current mask still
            // contains fatal classes exclusively. A user change that enables
            // any non-fatal class therefore survives `@`, while zero and
            // fatal-only changes remain part of the suppressed scope.
            let current_is_fatal_only =
                self.error_reporting & !PHP_82_SUPPRESSED_ERROR_REPORTING == 0;
            let saved_is_fatal_only = reporting & !PHP_82_SUPPRESSED_ERROR_REPORTING == 0;
            if current_is_fatal_only && !saved_is_fatal_only {
                self.error_reporting = reporting;
            }
        }
    }

    pub(crate) fn set_error_reporting(&mut self, level: i64) {
        self.error_reporting = level;
    }

    /// Reserve the stable built-in registry envelope immediately before stdlib
    /// registration. Executors that never install stdlib stay allocation-lazy;
    /// normal executors avoid repeated hash-table growth while installing the
    /// fixed built-in class and function set.
    pub(crate) fn reserve_stdlib_capacity(&mut self) {
        // The fixed PHP 8.5 surface, including the synchronous process helper
        // batch, occupies the next hash-table envelope even under default
        // features. Reserve it up front so registration never rehashes stored
        // function pointers; optional I/O/resource functions still fit inside
        // the same envelope.
        self.function_table.reserve(900);
        self.class_table.reserve(66);
        self.method_declaring_class.reserve(512);
        // The ordinary ReflectionEnum/ReflectionReference family brings the
        // fixed class/interface inventory above the prior 80-entry vector
        // envelope. Keep modest headroom so registration remains allocation-
        // free without relying on Vec's growth doubling at the boundary.
        self.class_by_id.reserve(96);
        self.static_property_slots_by_class.reserve(96);
        // RoundingMode contributes eight request-local case singleton slots;
        // retain the one-shot registration invariant without relying on Vec's
        // growth policy at the former 16-value boundary.
        self.static_property_values.reserve(24);
        self.static_property_handles_published.reserve(24);
        #[cfg(feature = "php-generics-reified")]
        self.static_generic_property_contracts.reserve(4);
    }

    pub fn new() -> Self {
        Self {
            vm_stack: VmStack::new(),
            pending_call_stack: VmStack::new_pending(),
            current_execute_data: Cell::new(std::ptr::null_mut()),
            vm_interrupt: Arc::new(AtomicBool::new(false)),
            timed_out: Arc::new(AtomicBool::new(false)),
            execution_timer: None,
            resolved_virtual_aggregate_cache: std::cell::RefCell::new(
                [ResolvedVirtualAggregateCacheEntry::EMPTY; RESOLVED_VIRTUAL_AGGREGATE_CACHE_SLOTS],
            ),
            function_table: HashMap::new(),
            private_function_table: HashMap::new(),
            class_table: HashMap::new(),
            pending_anonymous_classes: HashMap::new(),
            pending_runtime_classes: HashMap::new(),
            active_runtime_class_relations: HashMap::new(),
            declared_runtime_classes: HashMap::new(),
            pending_named_classes: Vec::new(),
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
            constant_definition_order: std::cell::RefCell::new(Vec::new()),
            compiler_halt_offsets: None,
            constant_attributes: HashMap::new(),
            constant_expressions: HashMap::new(),
            constant_deprecation_generation: 1,
            constant_deprecation_metadata_present: false,
            deferred_class_constant_activations: None,
            deprecated_symbol_stack: Vec::new(),
            regex_cache: crate::regex::RegexCache::default(),
            exception: None,
            finally_exceptions: HashMap::new(),
            clone_readonly_reinitialization: Vec::new(),
            clone_with_readonly_updates: Vec::new(),
            lazy_objects: None,
            assertion_state: AssertionState::default(),
            error_reporting: crate::PHP_E_ALL,
            precision: 14,
            serialize_precision: -1,
            error_suppression_frames: Vec::new(),
            error_handler: None,
            error_handler_levels: crate::PHP_E_ALL,
            error_handler_stack: Vec::new(),
            last_error: None,
            exception_handler: None,
            exception_handler_stack: Vec::new(),
            shutdown_functions: None,
            weak_objects: None,
            method_declaring_class: HashMap::new(),
            internal_function_display_names: None,
            internal_callable_metadata: None,
            internal_static_methods: None,

            output: std::cell::RefCell::new(Box::new(std::io::stdout())),
            output_buffers: std::cell::RefCell::new(Vec::new()),
            headers_sent: Cell::new(false),
            header_output_origin: std::cell::RefCell::new(None),
            libxml_entity_loader_disabled: Cell::new(false),
            output_handler_depth: Cell::new(0),
            pending_named_variadic: HashMap::new(),
            pending_closure_captures: HashMap::new(),
            function_argument_state: FunctionArgumentState::new(),
            active_generator: None,
            fiber_runtime: None,
            globals: HashMap::new(),
            dynamic_variables: HashMap::new(),
            dynamic_scope_owners: HashMap::new(),
            detached_trace_callers: None,
            detached_trace_origins: None,
            detached_return_discarded: false,
            dirty_globals: std::collections::HashSet::new(),
            error_handler_generation: 0,
            error_handler_dirty_globals: None,
            active_error_handler_generation: 0,
            static_vars: HashMap::new(),
            closure_static_frames: None,
            pending_invoke_this: None,
            included_files: std::collections::HashSet::new(),
            included_file_order: Vec::new(),
            included_functions: Vec::new(),
            trait_static_functions: Vec::new(),
            autoload: None,
            next_class_id: 1,
            internal_class_id_limit: 0,
            class_by_id: vec![std::ptr::null()],
            class_destructor_flags: std::cell::RefCell::new(Vec::new()),
            static_property_values: Vec::new(),
            static_property_handles_published: Vec::new(),
            request_static_values_may_retain_objects: false,
            static_property_slots_by_class: vec![Box::new([])],
            #[cfg(feature = "php-generics-reified")]
            static_generic_property_contracts: Vec::new(),
            gc_enabled: true,
            string_utility_state: None,
            ini_overrides: None,
            reflection_attributes: None,
            reflection_references: None,
            reflection_properties: None,
            reflection_parameters: None,
            json_runtime: None,
        }
    }

    /// Create EG with captured output (for testing)
    pub fn with_output(output: Box<dyn Write>) -> Self {
        Self {
            vm_stack: VmStack::new(),
            pending_call_stack: VmStack::new_pending(),
            current_execute_data: Cell::new(std::ptr::null_mut()),
            vm_interrupt: Arc::new(AtomicBool::new(false)),
            timed_out: Arc::new(AtomicBool::new(false)),
            execution_timer: None,
            resolved_virtual_aggregate_cache: std::cell::RefCell::new(
                [ResolvedVirtualAggregateCacheEntry::EMPTY; RESOLVED_VIRTUAL_AGGREGATE_CACHE_SLOTS],
            ),
            function_table: HashMap::new(),
            private_function_table: HashMap::new(),
            class_table: HashMap::new(),
            pending_anonymous_classes: HashMap::new(),
            pending_runtime_classes: HashMap::new(),
            active_runtime_class_relations: HashMap::new(),
            declared_runtime_classes: HashMap::new(),
            pending_named_classes: Vec::new(),
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
            constant_definition_order: std::cell::RefCell::new(Vec::new()),
            compiler_halt_offsets: None,
            constant_attributes: HashMap::new(),
            constant_expressions: HashMap::new(),
            constant_deprecation_generation: 1,
            constant_deprecation_metadata_present: false,
            deferred_class_constant_activations: None,
            deprecated_symbol_stack: Vec::new(),
            regex_cache: crate::regex::RegexCache::default(),
            exception: None,
            finally_exceptions: HashMap::new(),
            clone_readonly_reinitialization: Vec::new(),
            clone_with_readonly_updates: Vec::new(),
            lazy_objects: None,
            assertion_state: AssertionState::default(),
            error_reporting: crate::PHP_E_ALL,
            precision: 14,
            serialize_precision: -1,
            error_suppression_frames: Vec::new(),
            error_handler: None,
            error_handler_levels: crate::PHP_E_ALL,
            error_handler_stack: Vec::new(),
            last_error: None,
            exception_handler: None,
            exception_handler_stack: Vec::new(),
            shutdown_functions: None,
            weak_objects: None,
            method_declaring_class: HashMap::new(),
            internal_function_display_names: None,
            internal_callable_metadata: None,
            internal_static_methods: None,

            output: std::cell::RefCell::new(output),
            output_buffers: std::cell::RefCell::new(Vec::new()),
            headers_sent: Cell::new(false),
            header_output_origin: std::cell::RefCell::new(None),
            libxml_entity_loader_disabled: Cell::new(false),
            output_handler_depth: Cell::new(0),
            pending_named_variadic: HashMap::new(),
            pending_closure_captures: HashMap::new(),
            function_argument_state: FunctionArgumentState::new(),
            active_generator: None,
            fiber_runtime: None,
            globals: HashMap::new(),
            dynamic_variables: HashMap::new(),
            dynamic_scope_owners: HashMap::new(),
            detached_trace_callers: None,
            detached_trace_origins: None,
            detached_return_discarded: false,
            dirty_globals: std::collections::HashSet::new(),
            error_handler_generation: 0,
            error_handler_dirty_globals: None,
            active_error_handler_generation: 0,
            static_vars: HashMap::new(),
            closure_static_frames: None,
            pending_invoke_this: None,
            included_files: std::collections::HashSet::new(),
            included_file_order: Vec::new(),
            included_functions: Vec::new(),
            trait_static_functions: Vec::new(),
            autoload: None,
            next_class_id: 1,
            internal_class_id_limit: 0,
            class_by_id: vec![std::ptr::null()],
            class_destructor_flags: std::cell::RefCell::new(Vec::new()),
            static_property_values: Vec::new(),
            static_property_handles_published: Vec::new(),
            request_static_values_may_retain_objects: false,
            static_property_slots_by_class: vec![Box::new([])],
            #[cfg(feature = "php-generics-reified")]
            static_generic_property_contracts: Vec::new(),
            gc_enabled: true,
            string_utility_state: None,
            ini_overrides: None,
            reflection_attributes: None,
            reflection_references: None,
            reflection_properties: None,
            reflection_parameters: None,
            json_runtime: None,
        }
    }

    fn fiber_runtime_ptr(&mut self) -> *mut fiber::FiberRuntime {
        self.fiber_runtime
            .get_or_insert_with(|| Box::new(fiber::FiberRuntime::new()))
            .as_mut()
    }

    pub(crate) fn register_fiber_object(
        &mut self,
        receiver: &Value,
        callback: crate::stdlib::ResolvedCallback,
    ) -> bool {
        let Some(identity) = receiver.object_identity() else {
            return false;
        };
        let Some(object) = receiver.object_weak() else {
            return false;
        };
        self.fiber_runtime
            .get_or_insert_with(|| Box::new(fiber::FiberRuntime::new()))
            .register(identity, object, callback)
    }

    pub(crate) fn fiber_status(&self, identity: usize) -> Option<fiber::FiberStatus> {
        self.fiber_runtime
            .as_deref()
            .and_then(|runtime| runtime.status(identity))
    }

    pub(crate) fn current_fiber(&self) -> Option<Value> {
        self.fiber_runtime
            .as_deref()
            .and_then(fiber::FiberRuntime::current)
    }

    pub(crate) fn has_active_fiber(&self) -> bool {
        self.fiber_runtime
            .as_deref()
            .is_some_and(fiber::FiberRuntime::has_active)
    }

    pub(crate) fn active_fiber_is_force_closing(&self) -> bool {
        self.fiber_runtime
            .as_deref()
            .is_some_and(fiber::FiberRuntime::active_is_force_closing)
    }

    pub(crate) fn has_fiber_context(&self, identity: usize) -> bool {
        self.fiber_runtime
            .as_deref()
            .is_some_and(|runtime| runtime.contains(identity))
    }

    pub(crate) fn fiber_owned_object_references(&self, identity: usize) -> usize {
        self.fiber_runtime
            .as_deref()
            .map_or(0, |runtime| runtime.owned_object_references(identity))
    }

    pub(crate) fn force_close_fiber_object(
        &mut self,
        identity: usize,
        logical_caller: *mut ExecuteData,
    ) -> Result<(), crate::vm::execute::VmError> {
        let Some(runtime) = self.fiber_runtime.as_deref_mut() else {
            return Ok(());
        };
        if runtime.status(identity) != Some(fiber::FiberStatus::Suspended) {
            return Ok(());
        }
        let runtime = runtime as *mut fiber::FiberRuntime;
        if let Some(exception) =
            fiber::FiberRuntime::force_close(runtime, self, identity, logical_caller)?
        {
            self.exception = Some(exception);
        }
        Ok(())
    }

    pub(crate) fn release_fiber_object(&mut self, identity: usize) {
        if let Some(runtime) = self.fiber_runtime.as_deref_mut() {
            runtime.release(identity);
        }
    }

    pub(crate) fn fiber_returned(&self, identity: usize) -> Result<Value, fiber::FiberReturnState> {
        self.fiber_runtime
            .as_deref()
            .ok_or(fiber::FiberReturnState::NotStarted)?
            .returned(identity)
    }

    pub(crate) fn run_fiber(
        &mut self,
        identity: usize,
        input: fiber::FiberInput,
        logical_caller: *mut ExecuteData,
    ) -> Result<fiber::FiberRunOutcome, crate::vm::execute::VmError> {
        let runtime = self.fiber_runtime_ptr();
        // The registry is boxed and every context is pinned. Fiber VM re-entry
        // may mutate the registry through nested Fiber calls without moving
        // either allocation.
        fiber::FiberRuntime::run(runtime, self, identity, input, logical_caller)
    }

    pub(crate) fn suspend_fiber(
        &mut self,
        frame: *mut ExecuteData,
        return_value: *mut Value,
        value: Value,
    ) -> Result<(), crate::vm::execute::VmError> {
        let runtime = self.fiber_runtime_ptr();
        // The active Fiber and its pinned context remain live until the
        // suspension sidecar unwinds to run_fiber().
        fiber::FiberRuntime::suspend(runtime, frame, return_value, value)
    }

    pub(crate) fn dynamic_scope_owner(&self, frame: usize) -> usize {
        let mut owner = frame;
        for _ in 0..16 {
            let Some(parent) = self.dynamic_scope_owners.get(&owner).copied() else {
                break;
            };
            if parent == owner {
                break;
            }
            owner = parent;
        }
        owner
    }

    pub(crate) fn alias_dynamic_scope(&mut self, frame: usize, owner: usize) {
        let owner = self.dynamic_scope_owner(owner);
        self.dynamic_scope_owners.insert(frame, owner);
    }

    #[inline]
    pub(crate) fn detached_return_discarded(&self) -> bool {
        self.detached_return_discarded
    }

    #[inline]
    pub(crate) fn replace_detached_return_discarded(&mut self, discarded: bool) -> bool {
        std::mem::replace(&mut self.detached_return_discarded, discarded)
    }

    #[inline]
    pub(crate) fn take_detached_return_discarded(&mut self) -> bool {
        std::mem::take(&mut self.detached_return_discarded)
    }

    pub(crate) fn discard_dynamic_scope(&mut self, frame: usize) {
        self.dynamic_scope_owners.remove(&frame);
        self.dynamic_variables.remove(&frame);
    }

    #[cold]
    pub(crate) fn register_internal_function_display_name(
        &mut self,
        function: *const FunctionCommon,
        name: String,
    ) {
        if name != name.to_ascii_lowercase() {
            self.internal_function_display_names
                .get_or_insert_with(|| Box::new(HashMap::new()))
                .insert(function, name);
        }
    }

    pub(crate) fn register_internal_static_method(&mut self, function: *const FunctionCommon) {
        self.internal_static_methods
            .get_or_insert_with(|| Box::new(std::collections::HashSet::new()))
            .insert(function);
    }

    pub(crate) fn internal_method_is_static(&self, function: *const FunctionCommon) -> bool {
        self.internal_static_methods
            .as_deref()
            .is_some_and(|methods| methods.contains(&function))
    }

    pub(crate) fn internal_function_display_name(
        &self,
        function: *const FunctionCommon,
    ) -> Option<&str> {
        self.internal_function_display_names
            .as_deref()
            .and_then(|names| names.get(&function))
            .map(String::as_str)
    }

    #[cold]
    pub(crate) fn register_internal_function_extension(
        &mut self,
        function: *const FunctionCommon,
        extension: &'static str,
    ) {
        self.internal_callable_metadata
            .get_or_insert_with(|| Box::new(InternalCallableMetadata::default()))
            .functions
            .entry(function)
            .or_insert_with(|| (Vec::new(), &[], None))
            .2 = Some(extension);
    }

    #[cold]
    pub(crate) fn register_internal_function_reflection_metadata(
        &mut self,
        function: *const FunctionCommon,
        defaults: Vec<Option<Value>>,
        extension: &'static str,
    ) {
        let metadata = self
            .internal_callable_metadata
            .get_or_insert_with(|| Box::new(InternalCallableMetadata::default()))
            .functions
            .entry(function)
            .or_insert_with(|| (Vec::new(), &[], None));
        metadata.0 = defaults;
        metadata.2 = Some(extension);
    }

    #[cold]
    pub(crate) fn register_internal_function_reflection_metadata_with_diagnostics(
        &mut self,
        function: *const FunctionCommon,
        defaults: Vec<Option<Value>>,
        default_diagnostics: &'static [Option<&'static str>],
        extension: &'static str,
    ) {
        let metadata = self
            .internal_callable_metadata
            .get_or_insert_with(|| Box::new(InternalCallableMetadata::default()))
            .functions
            .entry(function)
            .or_insert_with(|| (Vec::new(), &[], None));
        metadata.0 = defaults;
        metadata.1 = default_diagnostics;
        metadata.2 = Some(extension);
    }

    pub(crate) fn internal_function_parameter_default(
        &self,
        function: *const FunctionCommon,
        index: usize,
    ) -> Option<&Value> {
        self.internal_callable_metadata
            .as_deref()
            .and_then(|metadata| metadata.functions.get(&function))
            .and_then(|(defaults, _, _)| defaults.get(index))
            .and_then(Option::as_ref)
            .filter(|value| !value.is_undef())
    }

    /// PHP's internal stubs use `UNKNOWN` for a small number of optional
    /// parameters. Reflection reports those parameters as optional while
    /// deliberately withholding a retrievable default value. An Undef entry
    /// is a cold metadata sentinel for that distinction; it never reaches a
    /// PHP value or an ordinary call frame.
    pub(crate) fn internal_function_parameter_default_is_unknown(
        &self,
        function: *const FunctionCommon,
        index: usize,
    ) -> bool {
        self.internal_callable_metadata
            .as_deref()
            .and_then(|metadata| metadata.functions.get(&function))
            .and_then(|(defaults, _, _)| defaults.get(index))
            .and_then(Option::as_ref)
            .is_some_and(Value::is_undef)
    }

    pub(crate) fn internal_function_parameter_default_diagnostic(
        &self,
        function: *const FunctionCommon,
        index: usize,
    ) -> Option<&str> {
        self.internal_callable_metadata
            .as_deref()
            .and_then(|metadata| metadata.functions.get(&function))
            .and_then(|(_, diagnostics, _)| diagnostics.get(index))
            .copied()
            .flatten()
    }

    pub(crate) fn internal_function_extension(
        &self,
        function: *const FunctionCommon,
    ) -> Option<&str> {
        self.internal_callable_metadata
            .as_deref()
            .and_then(|metadata| metadata.functions.get(&function))
            .and_then(|(_, _, extension)| *extension)
    }

    /// Register one link-only internal method contract. Public arity excludes
    /// the hidden receiver because these descriptors never enter a call frame.
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn register_internal_method_contract(
        &mut self,
        owner: &str,
        name: &str,
        is_static: bool,
        required_num_args: u32,
        param_names: &[&str],
        param_type_hints: Vec<ParamTypeHint>,
        return_type_hint: ParamTypeHint,
        parameter_default_diagnostics: &[Option<&str>],
        return_type_is_tentative: bool,
    ) {
        debug_assert_eq!(param_names.len(), param_type_hints.len());
        debug_assert_eq!(param_names.len(), parameter_default_diagnostics.len());
        debug_assert!(required_num_args <= param_names.len() as u32);
        let contract = InternalMethodContract {
            name: name.into(),
            is_static,
            signature: SignatureInfo {
                num_args: param_names.len() as u32,
                required_num_args,
                is_variadic: false,
                variadic_cv_index: 0,
                ref_args: 0,
                prefer_ref_args: 0,
                returns_reference: false,
                needs_bound_type_scope: false,
                this_offset: 0,
                param_type_hints,
                param_names: param_names.iter().map(|name| (*name).to_string()).collect(),
                return_type_hint,
            },
            parameter_default_diagnostics: parameter_default_diagnostics
                .iter()
                .map(|diagnostic| diagnostic.map(Into::into))
                .collect(),
            return_type_is_tentative,
        };
        self.internal_callable_metadata
            .get_or_insert_with(|| Box::new(InternalCallableMetadata::default()))
            .methods
            .entry(owner.to_string())
            .or_default()
            .push(contract);
    }

    #[inline]
    fn internal_method_contracts(&self, owner: &str) -> &[InternalMethodContract] {
        self.internal_callable_metadata
            .as_deref()
            .and_then(|metadata| metadata.methods.get(owner))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    #[cold]
    pub(crate) fn publish_detached_trace_caller(&mut self, frame: usize, caller: usize) {
        if caller != 0 {
            let callers = self
                .detached_trace_callers
                .get_or_insert_with(|| Box::new(Vec::with_capacity(1)));
            if let Some(entry) = callers.iter_mut().find(|entry| entry.0 == frame) {
                *entry = (frame, caller, false);
            } else {
                callers.push((frame, caller, false));
            }
        }
    }

    /// Publish a detached caller whose source site is the instruction still
    /// active in that caller. Ordinary call frames expose the following
    /// instruction instead, so live traces need this distinction to recover
    /// the property operation's exact file and line without cloning an origin
    /// string at every magic-property dispatch.
    #[cold]
    pub(crate) fn publish_detached_trace_caller_at_current_site(
        &mut self,
        frame: usize,
        caller: usize,
    ) {
        if caller != 0 {
            let callers = self
                .detached_trace_callers
                .get_or_insert_with(|| Box::new(Vec::with_capacity(1)));
            if let Some(entry) = callers.iter_mut().find(|entry| entry.0 == frame) {
                *entry = (frame, caller, true);
            } else {
                callers.push((frame, caller, true));
            }
        }
    }

    #[cold]
    pub(crate) fn publish_detached_trace_origin(
        &mut self,
        frame: usize,
        file: String,
        line: usize,
    ) {
        self.detached_trace_origins
            .get_or_insert_with(|| Box::new(HashMap::new()))
            .insert(frame, (file, line, None));
    }

    #[cold]
    pub(crate) fn publish_synthetic_trace_frame(
        &mut self,
        frame: usize,
        file: String,
        line: usize,
        function: String,
    ) {
        self.detached_trace_origins
            .get_or_insert_with(|| Box::new(HashMap::new()))
            .insert(frame, (file, line, Some(function)));
    }

    pub(crate) fn detached_trace_origin(&self, frame: usize) -> Option<(&str, usize)> {
        self.detached_trace_origins
            .as_deref()
            .and_then(|origins| origins.get(&frame))
            .map(|(file, line, _)| (file.as_str(), *line))
    }

    pub(crate) fn detached_trace_function(&self, frame: usize) -> Option<&str> {
        self.detached_trace_origins
            .as_deref()
            .and_then(|origins| origins.get(&frame))
            .and_then(|(_, _, function)| function.as_deref())
    }

    pub(crate) fn discard_detached_trace_caller(&mut self, frame: usize) {
        if let Some(callers) = self.detached_trace_callers.as_deref_mut() {
            if let Some(index) = callers.iter().position(|entry| entry.0 == frame) {
                callers.swap_remove(index);
            }
        }
        let origins_empty = self
            .detached_trace_origins
            .as_deref_mut()
            .is_some_and(|origins| {
                origins.remove(&frame);
                origins.is_empty()
            });
        if origins_empty {
            self.detached_trace_origins = None;
        }
    }

    #[cold]
    pub(crate) fn discard_detached_trace_origin(&mut self, frame: usize) {
        let origins_empty = self
            .detached_trace_origins
            .as_deref_mut()
            .is_some_and(|origins| {
                origins.remove(&frame);
                origins.is_empty()
            });
        if origins_empty {
            self.detached_trace_origins = None;
        }
    }

    #[inline]
    pub(crate) fn trace_caller(
        &self,
        frame: usize,
        physical: *mut ExecuteData,
    ) -> *mut ExecuteData {
        if !physical.is_null() {
            return physical;
        }
        self.detached_trace_callers
            .as_deref()
            .and_then(|callers| callers.iter().find(|entry| entry.0 == frame))
            .map(|(_, caller, _)| *caller)
            .map_or(std::ptr::null_mut(), |caller| caller as *mut ExecuteData)
    }

    #[inline]
    pub(crate) fn detached_trace_caller_is_current_site(&self, frame: usize) -> bool {
        self.detached_trace_callers
            .as_deref()
            .and_then(|callers| callers.iter().find(|entry| entry.0 == frame))
            .is_some_and(|(_, _, current_site)| *current_site)
    }

    #[cold]
    pub(crate) fn publish_closure_static_vars(&mut self, frame: usize, storage: ClosureStaticVars) {
        self.closure_static_frames
            .get_or_insert_with(HashMap::new)
            .insert(frame, storage);
    }

    #[inline]
    pub(crate) fn closure_static_vars(&self, frame: usize) -> Option<ClosureStaticVars> {
        self.closure_static_frames
            .as_ref()
            .and_then(|frames| frames.get(&frame))
            .cloned()
    }

    #[inline]
    pub(crate) fn with_function_static_vars_mut<R>(
        &mut self,
        frame: usize,
        function: &str,
        callback: impl FnOnce(&mut HashMap<String, Value>) -> R,
    ) -> R {
        if let Some(storage) = self.closure_static_vars(frame) {
            let mut values = storage.borrow_mut();
            callback(&mut values)
        } else {
            // Named static cells outlive the activation and may be mutated
            // through their installed CV reference after this call returns.
            self.request_static_values_may_retain_objects = true;
            callback(self.static_vars.entry(function.to_string()).or_default())
        }
    }

    #[inline]
    pub(crate) fn discard_closure_static_vars(&mut self, frame: usize) {
        let Some(frames) = self.closure_static_frames.as_mut() else {
            return;
        };
        frames.remove(&frame);
        if frames.is_empty() {
            self.closure_static_frames = None;
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
        requirements.extend(class_def.trait_aliases.iter().filter_map(|adaptation| {
            let alias = adaptation.alias.as_deref()?;
            let declaration = self.trait_alias_declaration(class_def, adaptation)?;
            (declaration.is_abstract && !alias.eq_ignore_ascii_case("final")).then_some(declaration)
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
            source_file: (!method.4.op_array.source_file.is_empty())
                .then_some(method.4.op_array.source_file.as_str())
                .or(class_def.source_file.as_deref()),
            source_line: method
                .4
                .op_array
                .source_lines
                .last()
                .filter(|(opline, _)| *opline == u32::MAX)
                .map_or(0, |(_, line)| *line as usize),
            signature: &method.4.common.sig,
            parameter_default_diagnostics: method.4.parameter_default_diagnostics.as_deref(),
            return_type_is_tentative: false,
            suppresses_tentative_return_deprecation: method
                .4
                .attributes
                .iter()
                .any(|attribute| attribute.name.eq_ignore_ascii_case("ReturnTypeWillChange")),
        }
    }

    fn internal_method_declaration<'a>(
        class_def: &'a ClassDef,
        contract: &'a InternalMethodContract,
    ) -> MethodDeclaration<'a> {
        MethodDeclaration {
            owner: &class_def.name,
            name: &contract.name,
            visibility: Visibility::Public,
            enforces_visibility: true,
            is_static: contract.is_static,
            is_abstract: class_def.is_interface,
            source_file: None,
            source_line: 0,
            signature: &contract.signature,
            parameter_default_diagnostics: Some(&contract.parameter_default_diagnostics),
            return_type_is_tentative: contract.return_type_is_tentative,
            suppresses_tentative_return_deprecation: false,
        }
    }

    /// Resolve one trait adaptation as the declaration it contributes to its
    /// consumer. Abstract aliases are contracts rather than callables, but
    /// still acquire the consumer-relative name and owner used by PHP's link
    /// diagnostics.
    fn trait_alias_declaration<'a>(
        &'a self,
        class_def: &'a ClassDef,
        adaptation: &'a crate::compiler::compile::TraitMethodAlias,
    ) -> Option<MethodDeclaration<'a>> {
        let source_trait = adaptation
            .trait_name
            .as_deref()
            .and_then(|owner| {
                class_def
                    .uses
                    .iter()
                    .find(|used| used.eq_ignore_ascii_case(owner))
            })
            .and_then(|name| self.find_class(name))
            .or_else(|| {
                class_def.uses.iter().find_map(|used| {
                    let trait_def = self.find_class(used)?;
                    self.find_effective_method(trait_def, &adaptation.method)
                        .map(|_| trait_def)
                })
            })?;
        let mut declaration = self.find_effective_method(source_trait, &adaptation.method)?;
        declaration.owner = &class_def.name;
        declaration.name = adaptation.alias.as_deref().unwrap_or(&adaptation.method);
        if let Some(visibility) = adaptation.visibility {
            declaration.visibility = visibility;
        }
        Some(declaration)
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
        if let Some(contract) = self
            .internal_method_contracts(&class_def.name)
            .iter()
            .find(|contract| contract.name.eq_ignore_ascii_case(method_name))
        {
            return Some(Self::internal_method_declaration(class_def, contract));
        }
        for adaptation in &class_def.trait_aliases {
            let target = adaptation.alias.as_deref().unwrap_or(&adaptation.method);
            if target.eq_ignore_ascii_case(method_name)
                && let Some(declaration) = self.trait_alias_declaration(class_def, adaptation)
            {
                return Some(declaration);
            }
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
        self.method_contract_errors_mode(required, implementation, linking_class, true, false, true)
    }

    fn method_contract_potential_errors(
        &self,
        required: MethodDeclaration<'_>,
        implementation: MethodDeclaration<'_>,
        linking_class: Option<&ClassDef>,
    ) -> Vec<String> {
        self.method_contract_errors_mode(required, implementation, linking_class, true, true, true)
    }

    fn method_contract_strict_errors(
        &self,
        required: MethodDeclaration<'_>,
        implementation: MethodDeclaration<'_>,
        linking_class: Option<&ClassDef>,
    ) -> Vec<String> {
        self.method_contract_errors_mode(
            required,
            implementation,
            linking_class,
            false,
            false,
            true,
        )
    }

    fn method_contract_hard_errors(
        &self,
        required: MethodDeclaration<'_>,
        implementation: MethodDeclaration<'_>,
        linking_class: Option<&ClassDef>,
    ) -> Vec<String> {
        self.method_contract_errors_mode(
            required,
            implementation,
            linking_class,
            true,
            false,
            !required.return_type_is_tentative,
        )
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn method_contract_errors_mode(
        &self,
        required: MethodDeclaration<'_>,
        implementation: MethodDeclaration<'_>,
        linking_class: Option<&ClassDef>,
        allow_unresolved_relation: bool,
        allow_any_unresolved_relation: bool,
        enforce_return_contract: bool,
    ) -> Vec<String> {
        use crate::vm::function::ParamTypeHint;

        let required_signature = required.signature;
        let implementation_signature = implementation.signature;
        let exact_parameter_contract = required_signature.public_arity()
            == implementation_signature.public_arity()
            && required_signature.required_num_args == implementation_signature.required_num_args
            && required_signature.is_variadic == implementation_signature.is_variadic
            && required_signature.ref_args == implementation_signature.ref_args
            && required_signature.param_type_hints == implementation_signature.param_type_hints
            && !required_signature
                .param_type_hints
                .iter()
                .any(ParamTypeHint::uses_declaring_class_scope);
        let exact_return_contract = !enforce_return_contract
            || (required_signature.return_type_hint == implementation_signature.return_type_hint
                && !required_signature
                    .return_type_hint
                    .uses_declaring_class_scope());
        if exact_parameter_contract
            && exact_return_contract
            && (!required.enforces_visibility
                || Self::method_visibility_rank(implementation.visibility)
                    >= Self::method_visibility_rank(required.visibility))
            && implementation.is_static == required.is_static
            && implementation_signature.returns_reference == required_signature.returns_reference
            && !self
                .generic_metadata
                .method_has_parametric_signature(required.owner, required.name)
        {
            return Vec::new();
        }

        let mut errors = Vec::new();
        if required.enforces_visibility
            && Self::method_visibility_rank(implementation.visibility)
                < Self::method_visibility_rank(required.visibility)
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
        if required.signature.returns_reference && !implementation.signature.returns_reference {
            errors.push("implementation must return by reference".to_string());
        }

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
                        if implementation_hint == required_hint
                            && !implementation_hint.uses_declaring_class_scope()
                        {
                            continue;
                        }
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
                        let compatible = if allow_any_unresolved_relation {
                            self.is_param_type_potentially_compatible(
                                &implementation_hint,
                                &required_hint,
                                implementation.owner,
                                required.owner,
                                linking_class,
                            )
                        } else if allow_unresolved_relation {
                            self.is_param_type_compatible(
                                &implementation_hint,
                                &required_hint,
                                implementation.owner,
                                required.owner,
                                linking_class,
                            )
                        } else {
                            self.is_param_type_compatible_mode(
                                &implementation_hint,
                                &required_hint,
                                implementation.owner,
                                required.owner,
                                linking_class,
                                false,
                                false,
                            )
                        };
                        if !compatible {
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

            let setter_hook = |name: &str| name.starts_with('$') && name.ends_with("::set");
            let required_return = if setter_hook(required.name) {
                ParamTypeHint::Void
            } else {
                required_signature.return_type_hint.clone()
            };
            if enforce_return_contract && !matches!(&required_return, ParamTypeHint::None) {
                let implementation_return = if setter_hook(implementation.name) {
                    ParamTypeHint::Void
                } else {
                    implementation_signature.return_type_hint.clone()
                };
                if implementation_return != required_return
                    || implementation_return.uses_declaring_class_scope()
                {
                    let implementation_return = self.resolve_variance_type_hint(
                        &implementation_return,
                        implementation.owner,
                        linking_class,
                    );
                    let required_return = self.resolve_variance_type_hint(
                        &required_return,
                        required.owner,
                        linking_class,
                    );
                    let compatible = if allow_any_unresolved_relation {
                        self.is_return_type_potentially_compatible(
                            &implementation_return,
                            &required_return,
                            implementation.owner,
                            required.owner,
                            linking_class,
                        )
                    } else if allow_unresolved_relation {
                        self.is_return_type_compatible(
                            &implementation_return,
                            &required_return,
                            implementation.owner,
                            required.owner,
                            linking_class,
                        )
                    } else {
                        self.is_return_type_compatible_mode(
                            &implementation_return,
                            &required_return,
                            implementation.owner,
                            required.owner,
                            linking_class,
                            false,
                            false,
                        )
                    };
                    if !compatible {
                        errors.push(format!(
                            "return type must be compatible with {}, got {}",
                            required_return.display_name(),
                            implementation_return.display_name()
                        ));
                    }
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
    fn method_visibility_rank(visibility: Visibility) -> u8 {
        match visibility {
            Visibility::Private => 0,
            Visibility::Protected => 1,
            Visibility::Public => 2,
        }
    }

    #[cold]
    fn incompatible_method_contract_diagnostic(
        &self,
        required: MethodDeclaration<'_>,
        implementation: MethodDeclaration<'_>,
        linking_class: &ClassDef,
    ) -> Option<String> {
        if self
            .method_contract_hard_errors(required, implementation, Some(linking_class))
            .is_empty()
        {
            return None;
        }

        let location = implementation.source_file.map_or_else(String::new, |file| {
            format!(" in {file} on line {}", implementation.source_line)
        });
        if implementation.is_static != required.is_static {
            let required_kind = if required.is_static {
                "static"
            } else {
                "non static"
            };
            let implementation_kind = if implementation.is_static {
                "static"
            } else {
                "non static"
            };
            return Some(format!(
                "Cannot make {required_kind} method {}::{}() {implementation_kind} in class {}{}",
                self.method_diagnostic_owner(required, Some(linking_class)),
                implementation.name,
                linking_class.name,
                location
            ));
        }
        if required.enforces_visibility
            && Self::method_visibility_rank(implementation.visibility)
                < Self::method_visibility_rank(required.visibility)
        {
            let required_visibility = match required.visibility {
                Visibility::Public => "public",
                Visibility::Protected => "protected",
                Visibility::Private => "private",
            };
            let weaker = if required.visibility == Visibility::Protected {
                " or weaker"
            } else {
                ""
            };
            return Some(format!(
                "Access level to {}::{}() must be {} (as in class {}){}{}",
                self.method_diagnostic_owner(implementation, Some(linking_class)),
                implementation.name,
                required_visibility,
                required.owner,
                weaker,
                location
            ));
        }

        Some(format!(
            "Declaration of {} must be compatible with {}{}",
            self.format_method_signature(implementation, Some(linking_class)),
            self.format_method_signature(required, Some(linking_class)),
            location
        ))
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn visit_internal_link_deprecation_contracts(
        &self,
        class_def: &ClassDef,
        mut visit: impl FnMut(MethodDeclaration<'_>, MethodDeclaration<'_>),
    ) -> bool {
        if class_def.is_trait {
            return false;
        }
        let mut implements_serializable = false;
        let mut pending = Vec::new();
        let mut visit_definition = |definition: &ClassDef| {
            for contract in self
                .internal_method_contracts(&definition.name)
                .iter()
                .filter(|contract| contract.return_type_is_tentative)
            {
                let Some(implementation) =
                    self.find_effective_method(class_def, contract.name.as_ref())
                else {
                    continue;
                };
                visit(
                    Self::internal_method_declaration(definition, contract),
                    implementation,
                );
            }
            definition.name.eq_ignore_ascii_case("Serializable")
        };
        // A direct leaf relation is overwhelmingly common and needs no
        // ancestry allocation. Parent-first/reverse-interface order matches
        // the existing stack walker and therefore keeps diagnostic priority.
        for name in class_def
            .parent
            .iter()
            .chain(class_def.implements.iter().rev())
        {
            let Some(definition) = self.find_class(name) else {
                continue;
            };
            implements_serializable |= visit_definition(definition);
            pending.extend(definition.implements.iter().cloned());
            pending.extend(definition.parent.iter().cloned());
        }
        if pending.is_empty() {
            return implements_serializable;
        }
        let mut seen = std::collections::HashSet::new();
        while let Some(name) = pending.pop() {
            let Some(definition) = self.find_class(&name) else {
                continue;
            };
            if !seen.insert(definition.class_id) {
                continue;
            }
            implements_serializable |= visit_definition(definition);
            pending.extend(definition.implements.iter().cloned());
            pending.extend(definition.parent.iter().cloned());
        }
        implements_serializable
    }

    /// Derive inheritance-time E_DEPRECATED diagnostics from the same method
    /// contracts used by hard variance validation. Unknown class relations
    /// remain fatal and are deliberately not projected as tentative notices.
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    pub(crate) fn class_link_deprecations(
        &self,
        class_def: &ClassDef,
    ) -> Vec<crate::compiler::compile::CompileDeprecation> {
        let mut diagnostics = Vec::new();
        let mut seen_implementations = std::collections::HashSet::new();
        let implements_serializable = self.visit_internal_link_deprecation_contracts(
            class_def,
            |required, implementation| {
            if implementation.suppresses_tentative_return_deprecation {
                return;
            }

            if implementation.signature.return_type_hint
                == required.signature.return_type_hint
                && !implementation
                    .signature
                    .return_type_hint
                    .uses_declaring_class_scope()
            {
                return;
            }
            let required_return = self.resolve_variance_type_hint(
                &required.signature.return_type_hint,
                required.owner,
                Some(class_def),
            );
            let implementation_return = self.resolve_variance_type_hint(
                &implementation.signature.return_type_hint,
                implementation.owner,
                Some(class_def),
            );
            if self.variance_hint_mentions_unresolved_class(&required_return, class_def)
                || self.variance_hint_mentions_unresolved_class(&implementation_return, class_def)
                || self.is_return_type_compatible_mode(
                    &implementation_return,
                    &required_return,
                    implementation.owner,
                    required.owner,
                    Some(class_def),
                    false,
                    false,
            )
            {
                return;
            }
            if !self
                .method_contract_hard_errors(required, implementation, Some(class_def))
                .is_empty()
            {
                return;
            }
            if !seen_implementations.insert(implementation.name.to_ascii_lowercase()) {
                return;
            }

            diagnostics.push(crate::compiler::compile::CompileDeprecation {
                message: format!(
                    "Return type of {} should either be compatible with {}, or the #[\\ReturnTypeWillChange] attribute should be used to temporarily suppress the notice",
                    self.format_method_signature(implementation, Some(class_def)),
                    self.format_method_signature(required, Some(class_def)),
                ),
                file: implementation
                    .source_file
                    .unwrap_or_else(|| class_def.source_file.as_deref().unwrap_or_default())
                    .to_string(),
                line: implementation.source_line,
                warning: false,
            });
        },
        );
        if class_def.source_file.is_some()
            && implements_serializable
            && !class_def.is_interface
            && !class_def.is_trait
            && (class_def.is_enum
                || (!class_def.is_abstract
                    && !(self.class_like_has_effective_method(class_def, "__serialize")
                        && self.class_like_has_effective_method(class_def, "__unserialize"))))
        {
            diagnostics.push(crate::compiler::compile::CompileDeprecation {
                message: format!(
                    "{} implements the Serializable interface, which is deprecated. Implement __serialize() and __unserialize() instead (or in addition, if support for old PHP versions is necessary)",
                    class_def.name
                ),
                file: class_def.source_file.clone().unwrap_or_default(),
                line: class_def.declaration_line,
                warning: false,
            });
        }
        diagnostics
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

    #[cold]
    #[inline(never)]
    fn method_contract_variance_dependency_names(
        &self,
        required: MethodDeclaration<'_>,
        implementation: MethodDeclaration<'_>,
        linking_class: &ClassDef,
    ) -> Option<(Vec<String>, bool, bool)> {
        let mut referenced_classes = Vec::new();
        let mut referenced_seen = std::collections::HashSet::new();
        for declaration in [required, implementation] {
            for hint in &declaration.signature.param_type_hints {
                let hint =
                    self.resolve_variance_type_hint(hint, declaration.owner, Some(linking_class));
                collect_variance_class_names(&hint, &mut referenced_classes, &mut referenced_seen);
            }
            let return_hint = self.resolve_variance_type_hint(
                &declaration.signature.return_type_hint,
                declaration.owner,
                Some(linking_class),
            );
            collect_variance_class_names(
                &return_hint,
                &mut referenced_classes,
                &mut referenced_seen,
            );
        }
        if !referenced_classes.iter().any(|dependency| {
            !["self", "parent", "static", "object", "iterable"]
                .iter()
                .any(|pseudo_type| dependency.eq_ignore_ascii_case(pseudo_type))
                && !self.variance_class_is_known(dependency, Some(linking_class))
        }) {
            return None;
        }

        let errors = self.method_contract_errors(required, implementation, Some(linking_class));
        let potential_errors =
            self.method_contract_potential_errors(required, implementation, Some(linking_class));
        if !potential_errors.is_empty()
            || (errors.is_empty()
                && self
                    .method_contract_strict_errors(required, implementation, Some(linking_class))
                    .is_empty())
        {
            return None;
        }

        let required_signature = required.signature;
        let implementation_signature = implementation.signature;
        let mut mentions_linking_class = false;
        for (required_hint, implementation_hint) in required_signature
            .param_type_hints
            .iter()
            .zip(&implementation_signature.param_type_hints)
        {
            let required_hint =
                self.resolve_variance_type_hint(required_hint, required.owner, Some(linking_class));
            let implementation_hint = self.resolve_variance_type_hint(
                implementation_hint,
                implementation.owner,
                Some(linking_class),
            );
            mentions_linking_class |=
                variance_type_hint_mentions_class(&required_hint, &linking_class.name)
                    || variance_type_hint_mentions_class(&implementation_hint, &linking_class.name);
            if variance_type_hint_mentions_class(&required_hint, &linking_class.name)
                && !variance_type_hint_mentions_class(&implementation_hint, &linking_class.name)
            {
                // Parameter contravariance would require the active class to
                // inherit from a newly loaded unknown type. Its parent chain
                // is already fixed, so autoload cannot make that relation true.
                return None;
            }
        }
        let required_return = self.resolve_variance_type_hint(
            &required_signature.return_type_hint,
            required.owner,
            Some(linking_class),
        );
        let implementation_return = self.resolve_variance_type_hint(
            &implementation_signature.return_type_hint,
            implementation.owner,
            Some(linking_class),
        );
        mentions_linking_class |=
            variance_type_hint_mentions_class(&required_return, &linking_class.name)
                || variance_type_hint_mentions_class(&implementation_return, &linking_class.name);
        if variance_type_hint_mentions_class(&implementation_return, &linking_class.name)
            && !variance_type_hint_mentions_class(&required_return, &linking_class.name)
        {
            // Return covariance has the inverse direction: an active
            // implementation type cannot become a child of a new ancestor.
            return None;
        }

        let mut dependencies = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let parameter_count = required
            .signature
            .param_type_hints
            .len()
            .max(implementation.signature.param_type_hints.len());
        for index in 0..parameter_count {
            // Parameter contravariance asks whether the required input is a
            // subtype of the implementation input. PHP resolves that relation
            // in the same direction, which also fixes the observable autoload
            // and first-unavailable diagnostic order.
            for declaration in [required, implementation] {
                let Some(hint) = declaration.signature.param_type_hints.get(index) else {
                    continue;
                };
                let hint =
                    self.resolve_variance_type_hint(hint, declaration.owner, Some(linking_class));
                collect_variance_class_names(&hint, &mut dependencies, &mut seen);
            }
        }
        // Return covariance has the inverse direction: resolve the
        // implementation subtype before the required supertype.
        for declaration in [implementation, required] {
            let return_hint = self.resolve_variance_type_hint(
                &declaration.signature.return_type_hint,
                declaration.owner,
                Some(linking_class),
            );
            collect_variance_class_names(&return_hint, &mut dependencies, &mut seen);
        }
        Some((dependencies, mentions_linking_class, errors.is_empty()))
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn visit_method_variance_contracts(
        &self,
        class_def: &ClassDef,
        mut visit: impl FnMut(MethodDeclaration<'_>, MethodDeclaration<'_>) -> bool,
    ) {
        if class_def.is_trait {
            return;
        }

        if class_def.is_interface {
            let mut effective = std::collections::HashMap::new();
            for method in &class_def.methods {
                let declaration = Self::method_declaration(class_def, method);
                effective.insert(declaration.name.to_ascii_lowercase(), declaration);
            }
            for parent in &class_def.implements {
                for requirement in self.collect_interface_methods(parent) {
                    let key = requirement.name.to_ascii_lowercase();
                    let Some(implementation) = effective.get(&key).copied() else {
                        effective.insert(key, requirement);
                        continue;
                    };
                    if !visit(requirement, implementation) {
                        return;
                    }
                }
            }
            return;
        }

        if let Some(parent) = class_def
            .parent
            .as_deref()
            .and_then(|name| self.class_table.get(name))
        {
            for method in &class_def.methods {
                let implementation = Self::method_declaration(class_def, method);
                let Some(required) = self.find_effective_method(parent, implementation.name) else {
                    continue;
                };
                let required_is_implicit_property_accessor = required
                    .name
                    .strip_prefix('$')
                    .and_then(|name| name.split_once("::"))
                    .and_then(|(property_name, hook)| {
                        parent
                            .properties
                            .iter()
                            .find(|property| property.name.eq_ignore_ascii_case(property_name))
                            .map(|property| match hook.to_ascii_lowercase().as_str() {
                                "get" => !property.has_get_hook,
                                "set" => !property.has_set_hook,
                                _ => false,
                            })
                    })
                    .unwrap_or(false);
                if required.visibility == Visibility::Private
                    || required_is_implicit_property_accessor
                    || (required.name.eq_ignore_ascii_case("__construct") && !required.is_abstract)
                {
                    continue;
                }
                if !visit(required, implementation) {
                    return;
                }
            }
            for method in self.effective_composed_trait_methods(class_def) {
                let Some(implementation) =
                    self.composed_trait_method_declaration(class_def, &method)
                else {
                    continue;
                };
                let Some(required) = self.find_effective_method(parent, implementation.name) else {
                    continue;
                };
                if required.visibility == Visibility::Private
                    || (required.name.eq_ignore_ascii_case("__construct") && !required.is_abstract)
                {
                    continue;
                }
                if !visit(required, implementation) {
                    return;
                }
            }
        }

        let mut requirements = Vec::new();
        self.collect_abstract_method_requirements(
            class_def,
            &mut requirements,
            &mut std::collections::HashSet::new(),
        );
        let mut interface_roots = class_def.implements.clone();
        if let Some(parent) = &class_def.parent {
            interface_roots.extend(self.collect_all_interfaces(parent));
        }
        let mut seen_interfaces = std::collections::HashSet::new();
        for interface in interface_roots {
            if seen_interfaces.insert(interface.to_ascii_lowercase()) {
                requirements.extend(self.collect_interface_methods(&interface));
            }
        }
        for required in requirements {
            let Some(implementation) = self.find_effective_method(class_def, required.name) else {
                continue;
            };
            if !visit(required, implementation) {
                return;
            }
        }
    }

    #[cold]
    #[inline(never)]
    fn variance_hint_mentions_unresolved_class(
        &self,
        hint: &crate::vm::function::ParamTypeHint,
        linking_class: &ClassDef,
    ) -> bool {
        use crate::vm::function::ParamTypeHint;

        match hint {
            ParamTypeHint::ClassName(name) => {
                ![
                    "self", "parent", "static", "object", "iterable", "false", "true", "null",
                ]
                .iter()
                .any(|builtin| name.eq_ignore_ascii_case(builtin))
                    && !self.variance_class_is_known(name, Some(linking_class))
            }
            ParamTypeHint::Nullable(inner) => {
                self.variance_hint_mentions_unresolved_class(inner, linking_class)
            }
            ParamTypeHint::Union(parts) | ParamTypeHint::Intersection(parts) => parts
                .iter()
                .any(|part| self.variance_hint_mentions_unresolved_class(part, linking_class)),
            _ => false,
        }
    }

    /// Ordinary declarations whose complete reachable signature graph names
    /// only built-ins or already linked classes cannot trigger variance
    /// autoload. Keep them on the pre-existing registration path and reserve
    /// the dependency algebra for its cold unresolved-name boundary.
    #[cold]
    #[inline(never)]
    fn class_variance_signatures_need_dependency_resolution(&self, class_def: &ClassDef) -> bool {
        let definition_mentions_unresolved = |definition: &ClassDef| {
            definition.methods.iter().any(|(_, _, _, _, function)| {
                function
                    .common
                    .sig
                    .param_type_hints
                    .iter()
                    .any(|hint| self.variance_hint_mentions_unresolved_class(hint, class_def))
                    || self.variance_hint_mentions_unresolved_class(
                        &function.common.sig.return_type_hint,
                        class_def,
                    )
            })
        };
        if definition_mentions_unresolved(class_def) {
            return true;
        }
        if class_def.parent.is_none()
            && class_def.implements.is_empty()
            && class_def.uses.is_empty()
        {
            return false;
        }

        let mut definitions = Vec::new();
        for relation in class_def
            .parent
            .iter()
            .chain(class_def.implements.iter())
            .chain(class_def.uses.iter())
        {
            if let Some(related) = self.find_class(relation) {
                definitions.push(related);
            }
        }
        let mut seen = std::collections::HashSet::new();
        while let Some(definition) = definitions.pop() {
            if !seen.insert(definition as *const ClassDef as usize) {
                continue;
            }
            if definition_mentions_unresolved(definition) {
                return true;
            }
            for relation in definition
                .parent
                .iter()
                .chain(definition.implements.iter())
                .chain(definition.uses.iter())
            {
                if let Some(related) = self.find_class(relation) {
                    definitions.push(related);
                }
            }
        }
        false
    }

    /// Runtime declarations link after user code may have installed an
    /// autoloader. Return only unknown class names whose eventual hierarchy
    /// could turn an otherwise valid method contract into a compatible one.
    /// Definite arity, reference, staticness and scalar-type errors do not
    /// trigger observable autoload side effects.
    #[cold]
    #[inline(never)]
    fn method_variance_dependency_plan_with_delay(
        &self,
        class_def: &ClassDef,
    ) -> (Vec<String>, bool, bool) {
        if class_def.is_trait
            || !self.class_variance_signatures_need_dependency_resolution(class_def)
        {
            return (Vec::new(), false, false);
        }

        let mut dependencies = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut requires_provisional_publication = false;
        let mut requires_delayed_linking = false;

        self.visit_method_variance_contracts(class_def, |required, implementation| {
            if let Some((contract_dependencies, mentions_linking_class, inconclusive)) =
                self.method_contract_variance_dependency_names(required, implementation, class_def)
            {
                requires_provisional_publication |= mentions_linking_class;
                requires_delayed_linking |= inconclusive;
                for dependency in contract_dependencies {
                    if seen.insert(dependency.to_ascii_lowercase()) {
                        dependencies.push(dependency);
                    }
                }
            }
            true
        });

        dependencies.retain(|dependency| {
            !["self", "parent", "static", "object", "iterable"]
                .iter()
                .any(|pseudo_type| dependency.eq_ignore_ascii_case(pseudo_type))
                && !dependency.eq_ignore_ascii_case(&class_def.name)
                && self.find_class(dependency).is_none()
                && !self.runtime_class_link_is_active(dependency)
                && !self
                    .pending_named_classes
                    .iter()
                    .any(|pending| pending.name.eq_ignore_ascii_case(dependency))
        });
        (
            dependencies,
            requires_provisional_publication,
            requires_delayed_linking,
        )
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn method_variance_dependency_plan(
        &self,
        class_def: &ClassDef,
    ) -> (Vec<String>, bool) {
        let (dependencies, requires_provisional_publication, _) =
            self.method_variance_dependency_plan_with_delay(class_def);
        (dependencies, requires_provisional_publication)
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn method_variance_dependencies(&self, class_def: &ClassDef) -> Vec<String> {
        self.method_variance_dependency_plan(class_def).0
    }

    pub(crate) fn unavailable_method_variance_dependency_error(
        &self,
        class_def: &ClassDef,
        unavailable_class: &str,
    ) -> Option<String> {
        let mut error = None;
        self.visit_method_variance_contracts(class_def, |required, implementation| {
            let Some((dependencies, _, _)) = self.method_contract_variance_dependency_names(
                required,
                implementation,
                class_def,
            ) else {
                return true;
            };
            if !dependencies
                .iter()
                .any(|dependency| dependency.eq_ignore_ascii_case(unavailable_class))
            {
                return true;
            }

            let location = class_def
                .source_file
                .as_ref()
                .map_or_else(String::new, |file| {
                    format!(" in {file} on line {}", implementation.source_line)
                });
            error = Some(format!(
                "Could not check compatibility between {} and {}, because class {} is not available{}",
                self.format_method_signature(implementation, Some(class_def)),
                self.format_method_signature(required, Some(class_def)),
                unavailable_class,
                location
            ));
            false
        });
        error
    }

    pub(crate) fn active_class_unavailable_method_variance_dependency_error(
        &self,
        class_name: &str,
        unavailable_class: &str,
    ) -> Option<String> {
        let class_def = self.find_class(class_name)?;
        self.unavailable_method_variance_dependency_error(class_def, unavailable_class)
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

    fn method_diagnostic_owner<'a>(
        &'a self,
        declaration: MethodDeclaration<'a>,
        linking_class: Option<&'a ClassDef>,
    ) -> std::borrow::Cow<'a, str> {
        if declaration.owner.starts_with("class@anonymous#")
            && let Some(linking_class) = linking_class
            && let Some(public_name) = linking_class.anonymous_public_name()
        {
            return std::borrow::Cow::Owned(
                public_name
                    .split('\0')
                    .next()
                    .unwrap_or(&public_name)
                    .to_string(),
            );
        }
        if !declaration.is_abstract
            && let Some(linking_class) = linking_class
            && self
                .find_class(declaration.owner)
                .is_some_and(|definition| definition.is_trait)
        {
            return std::borrow::Cow::Borrowed(
                self.trait_composition_scope_from_definition(linking_class, declaration.owner)
                    .unwrap_or(linking_class.name.as_str()),
            );
        }
        std::borrow::Cow::Borrowed(declaration.owner)
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

        let signature = declaration.signature;
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
                        .diagnostic_display_name(),
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
            if let Some(default) = declaration
                .parameter_default_diagnostics
                .and_then(|defaults| defaults.get(index))
                .and_then(|default| default.as_deref())
            {
                parameter.push_str(" = ");
                parameter.push_str(default);
            }
            parameters.push(parameter);
        }

        let mut rendered = format!(
            "{}::{}({})",
            self.method_diagnostic_owner(declaration, linking_class),
            declaration.name,
            parameters.join(", ")
        );
        if signature.returns_reference {
            rendered.insert_str(0, "& ");
        }
        let return_type =
            if declaration.name.starts_with('$') && declaration.name.ends_with("::set") {
                Some(ParamTypeHint::Void)
            } else if !matches!(signature.return_type_hint, ParamTypeHint::None) {
                Some(signature.return_type_hint.clone())
            } else {
                None
            };
        if let Some(return_type) = return_type {
            rendered.push_str(": ");
            rendered.push_str(
                &self
                    .resolve_variance_type_hint(&return_type, declaration.owner, linking_class)
                    .diagnostic_display_name(),
            );
        }
        rendered
    }

    fn override_attribute<'a>(
        attributes: &'a [crate::vm::function::AttributeDefinition],
    ) -> Result<Option<&'a crate::vm::function::AttributeDefinition>, String> {
        let mut overrides = attributes
            .iter()
            .filter(|attribute| attribute.name.eq_ignore_ascii_case("Override"));
        let first = overrides.next();
        if let Some(repeated) = overrides.next() {
            return Err(format!(
                "Attribute \"Override\" must not be repeated{}",
                Self::attribute_source_location(repeated)
            ));
        }
        Ok(first)
    }

    fn attribute_source_location(attribute: &crate::vm::function::AttributeDefinition) -> String {
        if attribute.source_file.is_empty() {
            String::new()
        } else {
            format!(
                " in {} on line {}",
                attribute.source_file, attribute.source_line
            )
        }
    }

    fn override_owner_name(class_def: &ClassDef) -> String {
        class_def
            .anonymous_public_name()
            .unwrap_or_else(|| class_def.name.clone())
            .split('\0')
            .next()
            .unwrap_or(&class_def.name)
            .to_string()
    }

    fn collect_override_interfaces<'a>(
        &'a self,
        class_def: &ClassDef,
        interfaces: &mut Vec<&'a ClassDef>,
        visited: &mut std::collections::HashSet<String>,
    ) {
        for interface_name in &class_def.implements {
            let Some(interface) = self.find_class(interface_name) else {
                continue;
            };
            if visited.insert(interface.name.to_ascii_lowercase()) {
                interfaces.push(interface);
                self.collect_override_interfaces(interface, interfaces, visited);
            }
        }
        if let Some(parent) = class_def
            .parent
            .as_deref()
            .and_then(|parent| self.find_class(parent))
        {
            self.collect_override_interfaces(parent, interfaces, visited);
        }
    }

    fn internal_interface_declares_method(interface: &ClassDef, method: &str) -> bool {
        if interface.source_file.is_some() {
            return false;
        }
        // Core interface definitions expose their identity and ancestry here,
        // while their callable contracts remain implemented as internal stubs.
        let interface = interface.name.to_ascii_lowercase();
        let method = method.to_ascii_lowercase();
        match interface.as_str() {
            "iteratoraggregate" => method == "getiterator",
            "iterator" => matches!(
                method.as_str(),
                "current" | "key" | "next" | "rewind" | "valid"
            ),
            "recursiveiterator" => matches!(method.as_str(), "haschildren" | "getchildren"),
            "countable" => method == "count",
            "arrayaccess" => matches!(
                method.as_str(),
                "offsetexists" | "offsetget" | "offsetset" | "offsetunset"
            ),
            "stringable" => method == "__tostring",
            "serializable" => matches!(method.as_str(), "serialize" | "unserialize"),
            "jsonserializable" => method == "jsonserialize",
            "unitenum" => method == "cases",
            "backedenum" => matches!(method.as_str(), "from" | "tryfrom"),
            "sessionhandlerinterface" => matches!(
                method.as_str(),
                "open" | "close" | "read" | "write" | "destroy" | "gc"
            ),
            "sessionupdatetimestamphandlerinterface" => {
                matches!(method.as_str(), "validateid" | "updatetimestamp")
            }
            _ => false,
        }
    }

    fn override_interface_method_exists(&self, class_def: &ClassDef, method: &str) -> bool {
        let mut interfaces = Vec::new();
        self.collect_override_interfaces(
            class_def,
            &mut interfaces,
            &mut std::collections::HashSet::new(),
        );
        interfaces.into_iter().any(|interface| {
            interface
                .methods
                .iter()
                .any(|candidate| candidate.0.eq_ignore_ascii_case(method))
                || Self::internal_interface_declares_method(interface, method)
        })
    }

    fn override_interface_property_exists(&self, class_def: &ClassDef, property: &str) -> bool {
        let mut interfaces = Vec::new();
        self.collect_override_interfaces(
            class_def,
            &mut interfaces,
            &mut std::collections::HashSet::new(),
        );
        interfaces.into_iter().any(|interface| {
            interface
                .properties
                .iter()
                .any(|candidate| candidate.name == property)
        })
    }

    fn abstract_trait_method_exists(
        &self,
        class_def: &ClassDef,
        method: &str,
        excluded_trait: Option<&str>,
        visited: &mut std::collections::HashSet<String>,
    ) -> bool {
        class_def.uses.iter().any(|trait_name| {
            let Some(trait_def) = self.find_class(trait_name) else {
                return false;
            };
            if !visited.insert(trait_def.name.to_ascii_lowercase()) {
                return false;
            }
            let own_match = excluded_trait
                .is_none_or(|excluded| !trait_def.name.eq_ignore_ascii_case(excluded))
                && trait_def.methods.iter().any(|candidate| {
                    candidate.0.eq_ignore_ascii_case(method)
                        && trait_def.method_is_abstract(&candidate.0)
                });
            own_match
                || self.abstract_trait_method_exists(trait_def, method, excluded_trait, visited)
        })
    }

    fn abstract_trait_property_exists(
        &self,
        class_def: &ClassDef,
        property: &str,
        excluded_trait: Option<&str>,
        visited: &mut std::collections::HashSet<String>,
    ) -> bool {
        class_def.uses.iter().any(|trait_name| {
            let Some(trait_def) = self.find_class(trait_name) else {
                return false;
            };
            if !visited.insert(trait_def.name.to_ascii_lowercase()) {
                return false;
            }
            let own_match = excluded_trait
                .is_none_or(|excluded| !trait_def.name.eq_ignore_ascii_case(excluded))
                && trait_def.properties.iter().any(|candidate| {
                    candidate.name == property
                        && (candidate.abstract_get_hook() || candidate.abstract_set_hook())
                });
            own_match
                || self.abstract_trait_property_exists(trait_def, property, excluded_trait, visited)
        })
    }

    fn override_parent_method_exists(&self, class_def: &ClassDef, method: &str) -> bool {
        let mut parent_name = class_def.parent.as_deref();
        while let Some(name) = parent_name {
            let Some(parent) = self.find_class(name) else {
                break;
            };
            if let Some(candidate) = self.find_effective_method(parent, method) {
                if method.eq_ignore_ascii_case("__construct") {
                    return candidate.is_abstract;
                }
                if candidate.visibility != Visibility::Private {
                    return true;
                }
            }
            if let Some((property_name, hook)) = method
                .strip_prefix('$')
                .and_then(|name| name.split_once("::"))
                && let Some(property) = parent.properties.iter().find(|property| {
                    property.name == property_name && property.visibility != Visibility::Private
                })
            {
                let inherited_accessor = match hook.to_ascii_lowercase().as_str() {
                    "get" => property.has_get_hook || !property.is_virtual_hook_property(),
                    "set" => {
                        !property.is_readonly
                            && (property.has_set_hook || !property.is_virtual_hook_property())
                    }
                    _ => false,
                };
                if inherited_accessor {
                    return true;
                }
            }
            parent_name = parent.parent.as_deref();
        }
        false
    }

    fn override_parent_property_exists(&self, class_def: &ClassDef, property: &str) -> bool {
        let mut parent_name = class_def.parent.as_deref();
        while let Some(name) = parent_name {
            let Some(parent) = self.find_class(name) else {
                break;
            };
            if parent.properties.iter().any(|candidate| {
                candidate.name == property && candidate.visibility != Visibility::Private
            }) || parent.static_properties.iter().any(|candidate| {
                candidate.name == property && candidate.visibility != Visibility::Private
            }) {
                return true;
            }
            parent_name = parent.parent.as_deref();
        }
        false
    }

    fn override_method_exists(
        &self,
        class_def: &ClassDef,
        method: &str,
        excluded_trait: Option<&str>,
    ) -> bool {
        self.override_parent_method_exists(class_def, method)
            || self.override_interface_method_exists(class_def, method)
            || self.abstract_trait_method_exists(
                class_def,
                method,
                excluded_trait,
                &mut std::collections::HashSet::new(),
            )
    }

    fn override_property_exists(
        &self,
        class_def: &ClassDef,
        property: &str,
        excluded_trait: Option<&str>,
    ) -> bool {
        self.override_parent_property_exists(class_def, property)
            || self.override_interface_property_exists(class_def, property)
            || self.abstract_trait_property_exists(
                class_def,
                property,
                excluded_trait,
                &mut std::collections::HashSet::new(),
            )
    }

    fn missing_override_method_error(
        class_def: &ClassDef,
        method: &str,
        attribute: &crate::vm::function::AttributeDefinition,
    ) -> String {
        format!(
            "{}::{}() has #[\\Override] attribute, but no matching parent method exists{}",
            Self::override_owner_name(class_def),
            method,
            Self::attribute_source_location(attribute)
        )
    }

    fn missing_override_property_error(
        class_def: &ClassDef,
        property: &str,
        attribute: &crate::vm::function::AttributeDefinition,
    ) -> String {
        format!(
            "{}::${} has #[\\Override] attribute, but no matching parent property exists{}",
            Self::override_owner_name(class_def),
            property,
            Self::attribute_source_location(attribute)
        )
    }

    fn validate_composed_trait_overrides(
        &self,
        class_def: &ClassDef,
        composition_owner: &ClassDef,
        trait_def: &ClassDef,
        visited: &mut std::collections::HashSet<String>,
    ) -> Result<(), String> {
        if !visited.insert(trait_def.name.to_ascii_lowercase()) {
            return Ok(());
        }
        for method in &trait_def.methods {
            if composition_owner
                .trait_precedences
                .iter()
                .any(|precedence| {
                    precedence.method.eq_ignore_ascii_case(&method.0)
                        && precedence
                            .instead_of
                            .iter()
                            .any(|excluded| excluded.eq_ignore_ascii_case(&trait_def.name))
                })
            {
                continue;
            }
            if class_def
                .methods
                .iter()
                .any(|candidate| candidate.0.eq_ignore_ascii_case(&method.0))
            {
                continue;
            }
            let Some(attribute) = Self::override_attribute(&method.4.attributes)? else {
                continue;
            };
            if !self.override_method_exists(class_def, &method.0, Some(&trait_def.name)) {
                return Err(Self::missing_override_method_error(
                    class_def, &method.0, attribute,
                ));
            }
        }
        for property in trait_def
            .properties
            .iter()
            .chain(&trait_def.static_properties)
        {
            if class_def
                .properties
                .iter()
                .chain(&class_def.static_properties)
                .any(|candidate| candidate.name == property.name)
            {
                continue;
            }
            let Some(attribute) = Self::override_attribute(&property.attributes)? else {
                continue;
            };
            if !self.override_property_exists(class_def, &property.name, Some(&trait_def.name)) {
                return Err(Self::missing_override_property_error(
                    class_def,
                    &property.name,
                    attribute,
                ));
            }
        }
        for nested in &trait_def.uses {
            if let Some(nested) = self.find_class(nested) {
                self.validate_composed_trait_overrides(class_def, trait_def, nested, visited)?;
            }
        }
        self.validate_composed_trait_alias_overrides(class_def, trait_def)?;
        Ok(())
    }

    fn validate_composed_trait_alias_overrides(
        &self,
        class_def: &ClassDef,
        composition_owner: &ClassDef,
    ) -> Result<(), String> {
        for adaptation in &composition_owner.trait_aliases {
            let Some(alias) = adaptation.alias.as_deref() else {
                continue;
            };
            let source_trait = adaptation
                .trait_name
                .as_deref()
                .and_then(|name| {
                    composition_owner
                        .uses
                        .iter()
                        .find(|used| used.eq_ignore_ascii_case(name))
                })
                .or_else(|| {
                    composition_owner.uses.iter().find(|used| {
                        self.find_class(used).is_some_and(|trait_def| {
                            trait_def
                                .methods
                                .iter()
                                .any(|method| method.0.eq_ignore_ascii_case(&adaptation.method))
                        })
                    })
                });
            let Some(source_trait) = source_trait.and_then(|name| self.find_class(name)) else {
                continue;
            };
            let Some(source_method) = source_trait
                .methods
                .iter()
                .find(|method| method.0.eq_ignore_ascii_case(&adaptation.method))
            else {
                continue;
            };
            let Some(attribute) = Self::override_attribute(&source_method.4.attributes)? else {
                continue;
            };
            if !self.override_method_exists(class_def, alias, Some(&source_trait.name)) {
                return Err(Self::missing_override_method_error(
                    class_def, alias, attribute,
                ));
            }
        }
        Ok(())
    }

    #[cold]
    #[inline(never)]
    fn validate_override_contracts(&self, class_def: &ClassDef) -> Result<(), String> {
        let _ = Self::override_attribute(&class_def.attributes)?;
        for constant in &class_def.constants {
            let _ = Self::override_attribute(&constant.attributes)?;
        }
        for method in &class_def.methods {
            let Some(attribute) = Self::override_attribute(&method.4.attributes)? else {
                continue;
            };
            if !class_def.is_trait && !self.override_method_exists(class_def, &method.0, None) {
                return Err(Self::missing_override_method_error(
                    class_def, &method.0, attribute,
                ));
            }
        }
        for property in class_def
            .properties
            .iter()
            .chain(&class_def.static_properties)
        {
            let Some(attribute) = Self::override_attribute(&property.attributes)? else {
                continue;
            };
            if !class_def.is_trait
                && !self.override_property_exists(class_def, &property.name, None)
            {
                return Err(Self::missing_override_property_error(
                    class_def,
                    &property.name,
                    attribute,
                ));
            }
        }
        if class_def.is_trait {
            return Ok(());
        }
        let mut visited = std::collections::HashSet::new();
        for trait_name in &class_def.uses {
            if let Some(trait_def) = self.find_class(trait_name) {
                self.validate_composed_trait_overrides(
                    class_def,
                    class_def,
                    trait_def,
                    &mut visited,
                )?;
            }
        }
        self.validate_composed_trait_alias_overrides(class_def, class_def)?;
        Ok(())
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
            self.validate_parent_method_declaration(
                class_def,
                parent,
                Self::method_declaration(class_def, method),
            )?;
        }
        for method in self.effective_composed_trait_methods(class_def) {
            let Some(implementation) = self.composed_trait_method_declaration(class_def, &method)
            else {
                continue;
            };
            self.validate_parent_method_declaration(class_def, parent, implementation)?;
        }
        Ok(())
    }

    fn validate_parent_method_declaration(
        &self,
        class_def: &ClassDef,
        parent: &ClassDef,
        implementation: MethodDeclaration<'_>,
    ) -> Result<(), String> {
        let Some(required) = self.find_effective_method(parent, implementation.name) else {
            return Ok(());
        };
        let required_is_implicit_property_accessor = required
            .name
            .strip_prefix('$')
            .and_then(|name| name.split_once("::"))
            .and_then(|(property_name, hook)| {
                parent
                    .properties
                    .iter()
                    .find(|property| property.name.eq_ignore_ascii_case(property_name))
                    .map(|property| match hook.to_ascii_lowercase().as_str() {
                        "get" => !property.has_get_hook,
                        "set" => !property.has_set_hook,
                        _ => false,
                    })
            })
            .unwrap_or(false);
        if required.visibility == Visibility::Private
            || required_is_implicit_property_accessor
            || (required.name.eq_ignore_ascii_case("__construct") && !required.is_abstract)
        {
            return Ok(());
        }
        if let Some(error) =
            self.incompatible_method_contract_diagnostic(required, implementation, class_def)
        {
            return Err(error);
        }
        Ok(())
    }

    #[cold]
    #[inline(never)]
    fn validate_abstract_method_contracts(&self, class_def: &ClassDef) -> Result<(), String> {
        // Internal classes are registered from trusted startup descriptors.
        // Their callable bodies may be published after the class skeleton,
        // while userland implementations and descendants must still satisfy
        // every link-only internal interface contract below.
        if (class_def.source_file.is_none() && class_def.declaration_line == 0)
            || class_def.is_interface
            || class_def.is_trait
        {
            return Ok(());
        }
        let mut requirements = Vec::new();
        self.collect_abstract_method_requirements(
            class_def,
            &mut requirements,
            &mut std::collections::HashSet::new(),
        );
        let mut interface_roots = class_def.implements.clone();
        if let Some(parent) = &class_def.parent {
            interface_roots.extend(self.collect_all_interfaces(parent));
        }
        let mut seen_interfaces = std::collections::HashSet::new();
        for interface in interface_roots {
            if seen_interfaces.insert(interface.to_ascii_lowercase()) {
                requirements.extend(self.collect_interface_methods(&interface));
            }
        }
        let mut missing = Vec::new();
        let mut seen_missing = std::collections::HashSet::new();
        let mut requires_private_trait_implementation = false;
        for requirement in requirements {
            if self.concrete_property_implements_hook(class_def, requirement.name) {
                continue;
            }
            let Some(implementation) = self.find_effective_method(class_def, requirement.name)
            else {
                if self
                    .find_class(requirement.owner)
                    .is_some_and(|owner| owner.is_interface)
                    && Self::class_declares_property_hook_target(class_def, requirement.name)
                {
                    continue;
                }
                // Link validation is transactional: every concrete class-like
                // declaration must retain missing interface obligations before
                // it acquires an ID or enters the public class table.
                if !class_def.is_abstract
                    && seen_missing.insert(requirement.name.to_ascii_lowercase())
                {
                    missing.push(format!("{}::{}", requirement.owner, requirement.name));
                }
                continue;
            };
            let unforwarded_private_trait_requirement = implementation.is_abstract
                && implementation.visibility == Visibility::Private
                && self
                    .find_class(implementation.owner)
                    .is_some_and(|owner| owner.is_trait);
            if implementation.is_abstract
                && (!class_def.is_abstract || unforwarded_private_trait_requirement)
            {
                if self.concrete_property_implements_hook(class_def, requirement.name) {
                    continue;
                }
                if self
                    .find_class(requirement.owner)
                    .is_some_and(|owner| owner.is_interface)
                    && Self::class_declares_property_hook_target(class_def, requirement.name)
                {
                    continue;
                }
                if seen_missing.insert(requirement.name.to_ascii_lowercase()) {
                    let owner = self
                        .find_class(requirement.owner)
                        .filter(|owner| owner.is_trait)
                        .map_or(requirement.owner, |_| class_def.name.as_str());
                    missing.push(format!("{}::{}", owner, requirement.name));
                }
                requires_private_trait_implementation |=
                    class_def.is_abstract && unforwarded_private_trait_requirement;
                continue;
            }
            // Built-in classes are linked from trusted engine descriptors and
            // may expose a historically narrower signature than their public
            // interface. A userland descendant inheriting that method does
            // not revalidate PHP's own implementation, while an explicit
            // userland override below still receives the complete contract.
            if self
                .find_class(implementation.owner)
                .is_some_and(|owner| owner.source_file.is_none() && owner.declaration_line == 0)
            {
                continue;
            }
            if let Some(error) =
                self.incompatible_method_contract_diagnostic(requirement, implementation, class_def)
            {
                return Err(error);
            }
        }
        if !missing.is_empty() {
            let count = missing.len();
            if class_def.is_enum {
                let location = class_def
                    .source_file
                    .as_ref()
                    .map_or_else(String::new, |file| {
                        format!(" in {file} on line {}", class_def.declaration_line)
                    });
                return Err(format!(
                    "Enum {} must implement {count} abstract {} ({}){}",
                    class_def.name,
                    if count == 1 { "method" } else { "methods" },
                    missing.join(", "),
                    location
                ));
            }
            if requires_private_trait_implementation {
                let location = class_def
                    .source_file
                    .as_ref()
                    .map_or_else(String::new, |file| {
                        format!(" in {file} on line {}", class_def.declaration_line)
                    });
                return Err(format!(
                    "Class {} must implement {count} abstract {} ({}){}",
                    class_def.name,
                    if count == 1 { "method" } else { "methods" },
                    missing.join(", "),
                    location
                ));
            }
            if let Some(public_name) = class_def.anonymous_public_name() {
                let public_name = public_name.split('\0').next().unwrap_or(&public_name);
                return Err(format!(
                    "Class {public_name} must implement {count} abstract {} ({})",
                    if count == 1 { "method" } else { "methods" },
                    missing.join(", ")
                ));
            }
            let (method_word, remaining_word) = if count == 1 {
                ("method", "method")
            } else {
                ("methods", "methods")
            };
            let location = class_def
                .source_file
                .as_ref()
                .map_or_else(String::new, |file| {
                    format!(" in {file} on line {}", class_def.declaration_line)
                });
            return Err(format!(
                "Class {} contains {} abstract {} and must therefore be declared abstract or implement the remaining {} ({}){}",
                class_def.name,
                count,
                method_word,
                remaining_word,
                missing.join(", "),
                location
            ));
        }
        Ok(())
    }

    /// Parent interfaces may contribute the same abstract method only when
    /// their effective declarations satisfy the complete inherited callable
    /// contract. The first effective declaration (or an explicit declaration
    /// on the child interface) is the implementation side of PHP's diagnostic.
    #[cold]
    fn validate_interface_method_contracts<'a>(
        &'a self,
        class_def: &'a ClassDef,
    ) -> Result<(), String> {
        if !class_def.is_interface {
            return Ok(());
        }

        let mut effective = std::collections::HashMap::new();
        for method in &class_def.methods {
            let declaration = Self::method_declaration(class_def, method);
            effective.insert(declaration.name.to_ascii_lowercase(), declaration);
        }
        for parent in &class_def.implements {
            for requirement in self.collect_interface_methods(parent) {
                let key = requirement.name.to_ascii_lowercase();
                let Some(implementation) = effective.get(&key).copied() else {
                    effective.insert(key, requirement);
                    continue;
                };
                let linking_class = if implementation.owner.eq_ignore_ascii_case(&class_def.name) {
                    class_def
                } else {
                    self.find_class(implementation.owner).unwrap_or(class_def)
                };
                let Some(error) = self.incompatible_method_contract_diagnostic(
                    requirement,
                    implementation,
                    linking_class,
                ) else {
                    continue;
                };
                return Err(error);
            }
        }
        Ok(())
    }

    #[cold]
    #[inline(never)]
    fn class_declares_property_hook_target(class_def: &ClassDef, method_name: &str) -> bool {
        let Some((property_name, hook)) = method_name
            .strip_prefix('$')
            .and_then(|name| name.split_once("::"))
        else {
            return false;
        };
        class_def
            .properties
            .iter()
            .find(|property| property.name.eq_ignore_ascii_case(property_name))
            .is_some_and(|property| {
                if hook.eq_ignore_ascii_case("get") {
                    !property.abstract_get_hook()
                } else if hook.eq_ignore_ascii_case("set") {
                    !property.abstract_set_hook()
                } else {
                    false
                }
            })
    }

    #[cold]
    #[inline(never)]
    fn concrete_property_implements_hook(&self, class_def: &ClassDef, method_name: &str) -> bool {
        let Some((property_name, hook)) = method_name
            .strip_prefix('$')
            .and_then(|name| name.split_once("::"))
        else {
            return false;
        };
        let Some(property) = self.find_effective_property_definition(class_def, property_name)
        else {
            return false;
        };
        if hook.eq_ignore_ascii_case("get") {
            !property.abstract_get_hook() && (!property.has_set_hook || property.has_get_hook)
        } else if hook.eq_ignore_ascii_case("set") {
            !property.is_readonly
                && !property.abstract_set_hook()
                && (!property.has_get_hook || property.has_set_hook)
        } else {
            false
        }
    }

    /// Resolve the property declaration that composition will expose without
    /// mutating the class under validation. Abstract/interface obligations are
    /// checked before parent and trait properties are copied into the class,
    /// so the validator must follow the same class > trait > parent precedence
    /// as the later merge step.
    fn find_effective_property_definition<'a>(
        &'a self,
        class_def: &'a ClassDef,
        property_name: &str,
    ) -> Option<&'a PropertyDefinition> {
        if let Some(property) = class_def
            .properties
            .iter()
            .find(|property| property.name.eq_ignore_ascii_case(property_name))
        {
            return Some(property);
        }
        for trait_name in &class_def.uses {
            if let Some(trait_def) = self.find_class(trait_name)
                && let Some(property) =
                    self.find_effective_property_definition(trait_def, property_name)
            {
                return Some(property);
            }
        }
        class_def
            .parent
            .as_deref()
            .and_then(|parent| self.find_class(parent))
            .and_then(|parent| self.find_effective_property_definition(parent, property_name))
    }

    fn compose_trait_method_pointer(
        &mut self,
        source: *const FunctionCommon,
        class_name: &str,
        method_name: &str,
        is_static: bool,
        bind_lexical_static_properties: bool,
    ) -> (*const FunctionCommon, bool) {
        // SAFETY: function-table pointers remain live for the ExecutorGlobals
        // lifetime and FunctionCommon is the first field of UserFunction. The
        // discriminant is checked before the enclosing cast is dereferenced.
        let source = unsafe {
            if (*source).fn_type != crate::vm::function::FunctionType::User {
                return (source, false);
            }
            &*(source as *const crate::vm::function::UserFunction)
        };
        let bind_lexical_static_properties = bind_lexical_static_properties
            && source.op_array.instructions.iter().any(|instruction| {
                matches!(
                    instruction.opcode,
                    OpCode::FetchLateStaticProp | OpCode::AssignLateStaticProp
                ) && instruction.op1_type == OpType::Const
                    && source
                        .op_array
                        .literals
                        .get(instruction.op1 as usize)
                        .and_then(Value::as_str)
                        .is_some_and(|owner| owner.eq_ignore_ascii_case("self"))
            });
        if source.op_array.static_vars.is_empty()
            && !source.common.plan.has_no_discard_attribute()
            && !bind_lexical_static_properties
        {
            return (&source.common, false);
        }
        let function = crate::compiler::clone_trait_method_with_static_storage(
            source,
            class_name,
            method_name,
            is_static,
            bind_lexical_static_properties,
        );
        self.trait_static_functions.push(Box::new(function));
        (
            &self.trait_static_functions.last().unwrap().common,
            bind_lexical_static_properties,
        )
    }

    /// Register an ordinary compiled class immediately, or retain an
    /// anonymous declaration until its expression executes.
    pub fn register_compiled_class(&mut self, class_def: ClassDef) -> Result<(), String> {
        if !class_def.is_anonymous() {
            let diagnostics = self.class_link_deprecations(&class_def);
            self.emit_compile_deprecations(&diagnostics);
        }
        self.register_compiled_class_without_link_deprecations(class_def)
    }

    /// Runtime declarations route their link diagnostics through the active
    /// PHP handler at the DeclareClass boundary, then call this side-effect
    /// free registration core so the notice is not emitted twice.
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    pub(crate) fn register_runtime_compiled_class(
        &mut self,
        class_def: ClassDef,
    ) -> Result<(), String> {
        self.register_compiled_class_without_link_deprecations(class_def)
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn register_compiled_class_without_link_deprecations(
        &mut self,
        class_def: ClassDef,
    ) -> Result<(), String> {
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
            if let Some(previous) = self
                .pending_named_classes
                .iter()
                .find(|pending| pending.name.eq_ignore_ascii_case(&class_def.name))
            {
                return Err(Self::class_like_redeclaration_error(previous, &class_def));
            }
            let (variance_dependencies, _, variance_requires_delayed_linking) =
                self.method_variance_dependency_plan_with_delay(&class_def);
            if self.class_definition_requires_delayed_linking_with_variance(
                &class_def,
                variance_requires_delayed_linking,
            ) {
                self.pending_named_classes.push(class_def);
                return Ok(());
            }
            for dependency in variance_dependencies {
                if self.find_class(&dependency).is_none()
                    && let Some(error) =
                        self.unavailable_method_variance_dependency_error(&class_def, &dependency)
                {
                    return Err(error);
                }
            }
            self.register_class(class_def)?;
            self.retry_pending_named_classes()
        }
    }

    /// Retain a named declaration until its cold `DeclareClass` marker runs.
    /// PHP-name duplication is intentionally checked at execution time, not
    /// while the containing source unit is prepared.
    pub fn register_runtime_class_declaration(
        &mut self,
        declaration_key: String,
        class_def: ClassDef,
    ) -> Result<(), String> {
        if self.pending_runtime_classes.contains_key(&declaration_key)
            || self.declared_runtime_classes.contains_key(&declaration_key)
        {
            return Err("Duplicate runtime class declaration marker".to_string());
        }
        self.pending_runtime_classes
            .insert(declaration_key, class_def);
        Ok(())
    }

    /// Acquire one delayed declaration for dependency loading and publication.
    /// Markers for ordinary eager classes deliberately return `None` as no-ops.
    pub(crate) fn take_runtime_class_declaration(
        &mut self,
        declaration_key: &str,
    ) -> Result<Option<ClassDef>, String> {
        if let Some(class_def) = self.pending_runtime_classes.remove(declaration_key) {
            let class_key = class_def.name.to_ascii_lowercase();
            if self.active_runtime_class_relations.contains_key(&class_key) {
                let class_name = class_def.name.clone();
                self.pending_runtime_classes
                    .insert(declaration_key.to_string(), class_def);
                return Err(format!(
                    "Cannot declare class {class_name}, because the name is already in use"
                ));
            }
            self.active_runtime_class_relations.insert(
                class_key,
                ActiveRuntimeClassRelation::from_class(&class_def),
            );
            return Ok(Some(class_def));
        }
        if let Some(class_name) = self.declared_runtime_classes.get(declaration_key) {
            return Err(self.find_class(class_name).map_or_else(
                || format!("Cannot declare class {class_name}, because the name is already in use"),
                |previous| Self::class_like_redeclaration_error(previous, previous),
            ));
        }
        Ok(None)
    }

    /// An autoloader exception aborts the current declaration but PHP permits
    /// a later execution to try the same source marker again after it is caught.
    pub(crate) fn restore_runtime_class_declaration(
        &mut self,
        declaration_key: String,
        class_def: ClassDef,
    ) {
        self.active_runtime_class_relations
            .remove(&class_def.name.to_ascii_lowercase());
        self.pending_runtime_classes
            .insert(declaration_key, class_def);
    }

    pub(crate) fn abort_runtime_class_link(&mut self, class_name: &str) {
        self.active_runtime_class_relations
            .remove(&class_name.to_ascii_lowercase());
    }

    pub(crate) fn runtime_class_link_is_active(&self, class_name: &str) -> bool {
        self.active_runtime_class_relations
            .contains_key(&class_name.to_ascii_lowercase())
    }

    pub(crate) fn active_runtime_class_has_variance_dependents(&self, class_name: &str) -> bool {
        self.active_runtime_class_relations
            .get(&class_name.to_ascii_lowercase())
            .is_some_and(|relation| relation.has_variance_dependents.get())
    }

    pub(crate) fn active_parent_link_dependencies(
        &self,
        class_def: &ClassDef,
    ) -> Option<(String, Vec<String>)> {
        let parent = class_def.parent.as_ref()?;
        let relation = self
            .active_runtime_class_relations
            .get(&parent.to_ascii_lowercase())?;
        (!relation.outstanding_variance_dependencies.is_empty()).then(|| {
            (
                parent.clone(),
                relation.outstanding_variance_dependencies.clone(),
            )
        })
    }

    pub(crate) fn mark_runtime_class_declared(
        &mut self,
        declaration_key: String,
        class_name: String,
    ) {
        self.active_runtime_class_relations
            .remove(&class_name.to_ascii_lowercase());
        self.declared_runtime_classes
            .insert(declaration_key, class_name);
    }

    #[cold]
    #[inline(never)]
    fn class_definition_requires_delayed_linking(&self, class_def: &ClassDef) -> bool {
        let variance_requires_delayed_linking =
            self.method_variance_dependency_plan_with_delay(class_def).2;
        self.class_definition_requires_delayed_linking_with_variance(
            class_def,
            variance_requires_delayed_linking,
        )
    }

    #[cold]
    #[inline(never)]
    fn class_definition_requires_delayed_linking_with_variance(
        &self,
        class_def: &ClassDef,
        variance_requires_delayed_linking: bool,
    ) -> bool {
        class_def.parent.as_deref().is_some_and(|parent| {
            // A forward child cannot be checked against its interfaces
            // until a later source declaration supplies inherited
            // implementations. Classes without interface requirements
            // retain the established eager-link behavior, including
            // unsupported internal parents that are intentionally absent.
            !class_def.implements.is_empty() && self.find_class(parent).is_none()
        }) || variance_requires_delayed_linking
            || property_hook_setter_variance_requires_delayed_linking(self, class_def)
            || class_def
                .parent
                .as_deref()
                .and_then(|parent| self.find_class(parent))
                .is_some_and(|parent| {
                    property_inheritance_requires_delayed_linking(self, class_def, parent)
                })
    }

    fn retry_pending_named_classes(&mut self) -> Result<(), String> {
        loop {
            let Some(index) = self
                .pending_named_classes
                .iter()
                .position(|class_def| !self.class_definition_requires_delayed_linking(class_def))
            else {
                return Ok(());
            };
            let class_def = self.pending_named_classes.remove(index);
            self.register_class(class_def)?;
        }
    }

    /// Finish linking declarations that waited for a runtime alias. Once the
    /// source unit has executed, no later statement in it can satisfy their
    /// unresolved invariant property types, so validate them normally and
    /// surface PHP's declaration error.
    pub(crate) fn finalize_pending_named_classes(
        &mut self,
    ) -> Result<(), crate::vm::execute::VmError> {
        self.retry_pending_named_classes()
            .map_err(crate::vm::execute::VmError::Fatal)?;
        while !self.pending_named_classes.is_empty() {
            let class_def = self.pending_named_classes.remove(0);
            let class_name = class_def.name.clone();
            let relation_key = class_name.to_ascii_lowercase();
            self.active_runtime_class_relations.insert(
                relation_key.clone(),
                ActiveRuntimeClassRelation::from_class(&class_def),
            );

            let dependencies = class_def
                .parent
                .iter()
                .chain(class_def.implements.iter())
                .cloned()
                .collect::<Vec<_>>();
            let mut dependency_error = None;
            for dependency in dependencies {
                if self.find_class(&dependency).is_some() {
                    continue;
                }
                match crate::stdlib::autoload::ensure_symbol_loaded(self, &dependency) {
                    Ok(true) => {}
                    Ok(false) => {
                        if let Some(exception) = self.exception.take() {
                            dependency_error = Some(crate::vm::execute::VmError::Fatal(
                                crate::vm::execute::format_uncaught_throwable(self, &exception),
                            ));
                        } else {
                            dependency_error = Some(crate::vm::execute::VmError::Fatal(format!(
                                "Class \"{dependency}\" not found"
                            )));
                        }
                        break;
                    }
                    Err(error) => {
                        dependency_error = Some(error);
                        break;
                    }
                }
            }
            if let Some(error) = dependency_error {
                self.active_runtime_class_relations.remove(&relation_key);
                return Err(error);
            }

            let mut unavailable_variance_dependencies = Vec::new();
            for dependency in self.method_variance_dependencies(&class_def) {
                if self.find_class(&dependency).is_some() {
                    continue;
                }
                match crate::stdlib::autoload::ensure_symbol_loaded(self, &dependency) {
                    Ok(true) => {}
                    Ok(false) => {
                        if let Some(exception) = self.exception.take() {
                            self.active_runtime_class_relations.remove(&relation_key);
                            return Err(crate::vm::execute::VmError::Fatal(
                                crate::vm::execute::format_uncaught_throwable(self, &exception),
                            ));
                        }
                        unavailable_variance_dependencies.push(dependency);
                    }
                    Err(error) => {
                        self.active_runtime_class_relations.remove(&relation_key);
                        return Err(error);
                    }
                }
            }
            for dependency in unavailable_variance_dependencies {
                if self.find_class(&dependency).is_none()
                    && let Some(error) =
                        self.unavailable_method_variance_dependency_error(&class_def, &dependency)
                {
                    self.active_runtime_class_relations.remove(&relation_key);
                    return Err(crate::vm::execute::VmError::Fatal(error));
                }
            }

            let result = self
                .register_class(class_def)
                .map_err(crate::vm::execute::VmError::Fatal);
            self.active_runtime_class_relations.remove(&relation_key);
            result?;
            self.retry_pending_named_classes()
                .map_err(crate::vm::execute::VmError::Fatal)?;
        }
        Ok(())
    }

    pub(crate) fn take_pending_anonymous_class(&mut self, name: &str) -> Option<ClassDef> {
        self.pending_anonymous_classes
            .remove(&name.to_ascii_lowercase())
    }

    #[cold]
    fn interface_closure_for_roots(&self, roots: &[String]) -> Vec<String> {
        let mut closure = Vec::new();
        let mut stack = roots.iter().rev().cloned().collect::<Vec<_>>();
        let mut seen = std::collections::HashSet::new();
        while let Some(name) = stack.pop() {
            let definition = self.find_class(&name);
            let canonical = definition.map_or(name, |interface| interface.name.clone());
            if !seen.insert(canonical.to_ascii_lowercase()) {
                continue;
            }
            closure.push(canonical);
            if let Some(interface) = definition
                && interface.is_interface
            {
                stack.extend(interface.implements.iter().rev().cloned());
            }
        }
        closure
    }

    #[cold]
    fn class_like_has_effective_method(&self, class_def: &ClassDef, method: &str) -> bool {
        if class_def
            .methods
            .iter()
            .any(|(name, ..)| name.eq_ignore_ascii_case(method))
            || self
                .internal_method_contracts(&class_def.name)
                .iter()
                .any(|contract| contract.name.eq_ignore_ascii_case(method))
            || class_def.trait_aliases.iter().any(|alias| {
                alias
                    .alias
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(method))
            })
        {
            return true;
        }
        let mut stack = class_def
            .uses
            .iter()
            .chain(class_def.parent.iter())
            .cloned()
            .collect::<Vec<_>>();
        let mut seen = std::collections::HashSet::new();
        while let Some(name) = stack.pop() {
            if !seen.insert(name.to_ascii_lowercase()) {
                continue;
            }
            let Some(definition) = self.find_class(&name) else {
                continue;
            };
            if definition
                .methods
                .iter()
                .any(|(name, ..)| name.eq_ignore_ascii_case(method))
                || self
                    .internal_method_contracts(&definition.name)
                    .iter()
                    .any(|contract| contract.name.eq_ignore_ascii_case(method))
                || definition.trait_aliases.iter().any(|alias| {
                    alias
                        .alias
                        .as_deref()
                        .is_some_and(|name| name.eq_ignore_ascii_case(method))
                })
            {
                return true;
            }
            stack.extend(definition.uses.iter().cloned());
            stack.extend(definition.parent.iter().cloned());
        }
        false
    }

    #[cold]
    fn declaration_interface_contract(&mut self, class_def: &ClassDef) -> Result<(), String> {
        if let Some(error) = self.direct_interface_relation_error(class_def) {
            return Err(error);
        }
        if class_def.is_interface {
            return self.validate_interface_method_contracts(class_def);
        }
        if class_def.is_trait {
            return Ok(());
        }
        let location = class_def
            .source_file
            .as_ref()
            .map_or_else(String::new, |file| {
                format!(" in {file} on line {}", class_def.declaration_line)
            });
        let is_backed_enum = class_def.is_enum
            && class_def
                .implements
                .iter()
                .any(|name| name.eq_ignore_ascii_case("BackedEnum"));
        let mut roots = if class_def.is_enum {
            class_def
                .implements
                .iter()
                .filter(|name| {
                    !name.eq_ignore_ascii_case("UnitEnum")
                        && !(is_backed_enum && name.eq_ignore_ascii_case("BackedEnum"))
                })
                .cloned()
                .collect::<Vec<_>>()
        } else {
            class_def.implements.clone()
        };
        if !class_def.is_enum {
            let mut parent = class_def.parent.as_deref();
            let mut seen = std::collections::HashSet::new();
            while let Some(name) = parent {
                if !seen.insert(name.to_ascii_lowercase()) {
                    break;
                }
                let Some(definition) = self.find_class(name) else {
                    break;
                };
                roots.extend(definition.implements.iter().cloned());
                parent = definition.parent.as_deref();
            }
        }
        if roots.is_empty() {
            return Ok(());
        }
        let closure = self.interface_closure_for_roots(&roots);

        let implements_iterator = closure
            .iter()
            .any(|name| name.eq_ignore_ascii_case("Iterator"));
        let implements_iterator_aggregate = closure
            .iter()
            .any(|name| name.eq_ignore_ascii_case("IteratorAggregate"));
        if implements_iterator && implements_iterator_aggregate {
            return Err(format!(
                "Class {} cannot implement both Iterator and IteratorAggregate at the same time{location}",
                class_def.name
            ));
        }

        if !class_def.is_enum {
            for root in &roots {
                let inherited = self.interface_closure_for_roots(std::slice::from_ref(root));
                if inherited
                    .first()
                    .is_some_and(|name| name.eq_ignore_ascii_case("BackedEnum"))
                {
                    return Err(format!(
                        "Non-enum class {} cannot implement interface BackedEnum{location}",
                        class_def.name
                    ));
                }
                if inherited.iter().any(|name| {
                    name.eq_ignore_ascii_case("UnitEnum") || name.eq_ignore_ascii_case("BackedEnum")
                }) {
                    return Err(format!(
                        "Non-enum class {} cannot implement interface UnitEnum{location}",
                        class_def.name
                    ));
                }
            }
        } else if !is_backed_enum
            && closure
                .iter()
                .any(|name| name.eq_ignore_ascii_case("BackedEnum"))
        {
            return Err(format!(
                "Non-backed enum {} cannot implement interface BackedEnum{location}",
                class_def.name
            ));
        }

        if class_def.is_enum
            && closure
                .iter()
                .any(|name| name.eq_ignore_ascii_case("Traversable"))
            && !closure.iter().any(|name| {
                name.eq_ignore_ascii_case("Iterator")
                    || name.eq_ignore_ascii_case("IteratorAggregate")
            })
        {
            return Err(format!(
                "Enum {} must implement interface Traversable as part of either Iterator or IteratorAggregate in Unknown on line 0",
                class_def.name
            ));
        }
        if !class_def.is_enum
            && !class_def.is_abstract
            && closure
                .iter()
                .any(|name| name.eq_ignore_ascii_case("Traversable"))
            && !implements_iterator
            && !implements_iterator_aggregate
        {
            return Err(format!(
                "Class {} must implement interface Traversable as part of either Iterator or IteratorAggregate in Unknown on line 0",
                class_def.name
            ));
        }

        let implements_serializable = closure
            .iter()
            .any(|name| name.eq_ignore_ascii_case("Serializable"));
        if class_def.is_enum && implements_serializable {
            return Err(format!(
                "Enum {} cannot implement the Serializable interface{location}",
                class_def.name
            ));
        }
        if class_def.is_enum
            && closure
                .iter()
                .any(|name| name.eq_ignore_ascii_case("Throwable"))
        {
            return Err(format!(
                "Enum {} cannot implement interface Throwable{location}",
                class_def.name
            ));
        }
        Ok(())
    }

    fn evaluate_rebound_trait_property_default(
        &mut self,
        rebound: &ReboundTraitPropertyDefault,
        property: &PropertyDefinition,
        class_name: &str,
        parent_name: Option<&str>,
    ) -> Result<Value, String> {
        let mut definition = rebound.definition.clone();
        let mut scope = (*definition.evaluation_scope).clone();
        scope.lexical_class = Some(class_name.to_string());
        scope.lexical_parent = parent_name.map(str::to_string);
        definition.evaluation_scope = std::rc::Rc::new(scope);
        let Some(value) =
            crate::stdlib::reflection::evaluate_deferred_property_default_value(&definition, self)
                .map_err(|error| error.to_string())?
        else {
            return Err(format!(
                "Cannot evaluate trait property default {}::${} while composing {}",
                rebound.definition.declaring_class, rebound.definition.property_name, class_name
            ));
        };
        crate::compiler::compile::normalize_rebound_property_default(value, property, class_name)
    }

    /// Register a class definition and its methods in the function table.
    /// Resolves inheritance: merges parent properties/methods into child.
    /// For non-interface, non-abstract classes: validates interface contracts.
    pub fn register_class(&mut self, class_def: ClassDef) -> Result<(), String> {
        self.register_class_mode(class_def, false)
    }

    fn same_effective_trait_method(
        left_owner: &str,
        left_method: &str,
        right_owner: &str,
        right_method: &str,
    ) -> bool {
        left_owner.eq_ignore_ascii_case(right_owner)
            && left_method.eq_ignore_ascii_case(right_method)
    }

    /// Recover the effective concrete methods of an already-linked trait in
    /// source-composition order. The original declaration identity survives
    /// nested composition so a diamond which reaches the same implementation
    /// twice remains legal, including when independent static storage forced
    /// the runtime to clone its function pointer.
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn effective_trait_methods(
        &self,
        trait_def: &ClassDef,
        visiting: &mut std::collections::HashSet<String>,
    ) -> Vec<EffectiveTraitMethod> {
        let visit_key = trait_def.name.to_ascii_lowercase();
        if !visiting.insert(visit_key.clone()) {
            return Vec::new();
        }

        let own_concrete = trait_def
            .methods
            .iter()
            .filter(|method| !trait_def.method_is_abstract(&method.0))
            .map(|method| method.0.to_ascii_lowercase())
            .collect::<std::collections::HashSet<_>>();
        let mut effective = trait_def
            .methods
            .iter()
            .filter(|method| !trait_def.method_is_abstract(&method.0))
            .filter(|method| {
                self.function_table
                    .contains_key(&format!("{}::{}", trait_def.name, method.0).to_ascii_lowercase())
            })
            .map(|method| EffectiveTraitMethod {
                target: method.0.clone(),
                origin_owner: trait_def.name.clone(),
                origin_method: method.0.clone(),
                visibility: method.1,
                is_static: method.2,
                is_final: method.3,
            })
            .collect::<Vec<_>>();

        let nested = trait_def
            .uses
            .iter()
            .filter_map(|used| {
                self.find_class(used).map(|definition| {
                    (
                        definition.name.clone(),
                        self.effective_trait_methods(definition, visiting),
                    )
                })
            })
            .collect::<Vec<_>>();
        for (provider, methods) in &nested {
            for method in methods {
                let target_key = method.target.to_ascii_lowercase();
                if own_concrete.contains(&target_key)
                    || trait_def.trait_precedences.iter().any(|precedence| {
                        precedence.method.eq_ignore_ascii_case(&method.target)
                            && precedence
                                .instead_of
                                .iter()
                                .any(|excluded| excluded.eq_ignore_ascii_case(provider))
                    })
                {
                    continue;
                }
                if let Some(previous) = effective
                    .iter()
                    .find(|candidate| candidate.target.eq_ignore_ascii_case(&method.target))
                {
                    if Self::same_effective_trait_method(
                        &previous.origin_owner,
                        &previous.origin_method,
                        &method.origin_owner,
                        &method.origin_method,
                    ) {
                        continue;
                    }
                    // An already-linked trait cannot retain an unresolved
                    // collision. Keep its first effective entry defensively;
                    // validation of the owning trait reports the ambiguity.
                    continue;
                }
                effective.push(method.clone());
            }
        }

        for adaptation in &trait_def.trait_aliases {
            let source = adaptation
                .trait_name
                .as_ref()
                .and_then(|owner| {
                    nested
                        .iter()
                        .find(|(provider, _)| provider.eq_ignore_ascii_case(owner))
                })
                .and_then(|(_, methods)| {
                    methods
                        .iter()
                        .find(|method| method.target.eq_ignore_ascii_case(&adaptation.method))
                })
                .or_else(|| {
                    nested.iter().find_map(|(_, methods)| {
                        methods
                            .iter()
                            .find(|method| method.target.eq_ignore_ascii_case(&adaptation.method))
                    })
                });
            let Some(source) = source else {
                continue;
            };
            let Some(alias) = adaptation.alias.as_ref() else {
                if let Some(method) = effective.iter_mut().find(|candidate| {
                    candidate.target.eq_ignore_ascii_case(&adaptation.method)
                        && Self::same_effective_trait_method(
                            &candidate.origin_owner,
                            &candidate.origin_method,
                            &source.origin_owner,
                            &source.origin_method,
                        )
                }) {
                    if let Some(visibility) = adaptation.visibility {
                        method.visibility = visibility;
                    }
                    method.is_final |= adaptation.is_final;
                }
                continue;
            };
            if own_concrete.contains(&alias.to_ascii_lowercase()) {
                continue;
            }
            if let Some(previous) = effective
                .iter()
                .find(|candidate| candidate.target.eq_ignore_ascii_case(alias))
            {
                if Self::same_effective_trait_method(
                    &previous.origin_owner,
                    &previous.origin_method,
                    &source.origin_owner,
                    &source.origin_method,
                ) {
                    continue;
                }
                continue;
            }
            let mut alias_method = source.clone();
            alias_method.target = alias.clone();
            if let Some(visibility) = adaptation.visibility {
                alias_method.visibility = visibility;
            }
            alias_method.is_final |= adaptation.is_final;
            effective.push(EffectiveTraitMethod {
                target: alias.clone(),
                origin_owner: source.origin_owner.clone(),
                origin_method: source.origin_method.clone(),
                visibility: alias_method.visibility,
                is_static: alias_method.is_static,
                is_final: alias_method.is_final,
            });
        }

        visiting.remove(&visit_key);
        effective
    }

    /// Reconstruct the concrete trait methods published by one consumer,
    /// including modifier-only adaptations and renamed aliases. This cold
    /// metadata view is shared by link validation and Reflection; callable
    /// dispatch continues to use the already-composed function table.
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    pub(crate) fn effective_composed_trait_methods(
        &self,
        class_def: &ClassDef,
    ) -> Vec<EffectiveTraitMethod> {
        let providers = class_def
            .uses
            .iter()
            .filter_map(|used| {
                self.find_class(used).map(|definition| {
                    (
                        definition.name.clone(),
                        self.effective_trait_methods(
                            definition,
                            &mut std::collections::HashSet::new(),
                        ),
                    )
                })
            })
            .collect::<Vec<_>>();
        let own = class_def
            .methods
            .iter()
            .filter(|method| !class_def.method_is_abstract(&method.0))
            .map(|method| method.0.to_ascii_lowercase())
            .collect::<std::collections::HashSet<_>>();
        let mut composed = Vec::<(String, EffectiveTraitMethod)>::new();

        for (provider, methods) in &providers {
            for method in methods {
                if own.contains(&method.target.to_ascii_lowercase())
                    || class_def.trait_precedences.iter().any(|precedence| {
                        precedence.method.eq_ignore_ascii_case(&method.target)
                            && precedence.instead_of.iter().any(|excluded| {
                                excluded.eq_ignore_ascii_case(provider)
                                    || self
                                        .find_class(excluded)
                                        .zip(self.find_class(provider))
                                        .is_some_and(|(left, right)| {
                                            Self::same_trait_identity(left, right)
                                        })
                            })
                    })
                    || composed
                        .iter()
                        .any(|(_, candidate)| candidate.target.eq_ignore_ascii_case(&method.target))
                {
                    continue;
                }
                composed.push((provider.clone(), method.clone()));
            }
        }

        for adaptation in &class_def.trait_aliases {
            let source = adaptation
                .trait_name
                .as_deref()
                .and_then(|owner| {
                    providers.iter().find(|(provider, _)| {
                        provider.eq_ignore_ascii_case(owner)
                            || self
                                .find_class(provider)
                                .zip(self.find_class(owner))
                                .is_some_and(|(left, right)| Self::same_trait_identity(left, right))
                    })
                })
                .and_then(|(provider, methods)| {
                    methods
                        .iter()
                        .find(|method| method.target.eq_ignore_ascii_case(&adaptation.method))
                        .map(|method| (provider.as_str(), method))
                })
                .or_else(|| {
                    providers.iter().find_map(|(provider, methods)| {
                        methods
                            .iter()
                            .find(|method| method.target.eq_ignore_ascii_case(&adaptation.method))
                            .map(|method| (provider.as_str(), method))
                    })
                });
            let Some((provider, source)) = source else {
                continue;
            };

            if let Some(alias) = adaptation.alias.as_deref() {
                if own.contains(&alias.to_ascii_lowercase())
                    || composed
                        .iter()
                        .any(|(_, candidate)| candidate.target.eq_ignore_ascii_case(alias))
                {
                    continue;
                }
                let mut method = source.clone();
                method.target = alias.to_string();
                if let Some(visibility) = adaptation.visibility {
                    method.visibility = visibility;
                }
                method.is_final |= adaptation.is_final;
                composed.push((provider.to_string(), method));
                continue;
            }

            if let Some((_, method)) = composed.iter_mut().find(|(candidate_provider, method)| {
                candidate_provider.eq_ignore_ascii_case(provider)
                    && method.target.eq_ignore_ascii_case(&adaptation.method)
            }) {
                if let Some(visibility) = adaptation.visibility {
                    method.visibility = visibility;
                }
                method.is_final |= adaptation.is_final;
            }
        }

        composed.into_iter().map(|(_, method)| method).collect()
    }

    fn composed_trait_method_declaration<'a>(
        &'a self,
        class_def: &'a ClassDef,
        effective: &'a EffectiveTraitMethod,
    ) -> Option<MethodDeclaration<'a>> {
        let origin = self.find_class(&effective.origin_owner)?;
        let method = origin
            .methods
            .iter()
            .find(|method| method.0.eq_ignore_ascii_case(&effective.origin_method))?;
        let mut declaration = Self::method_declaration(origin, method);
        declaration.owner = &class_def.name;
        declaration.name = &effective.target;
        declaration.visibility = effective.visibility;
        declaration.is_static = effective.is_static;
        declaration.is_abstract = false;
        Some(declaration)
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn private_final_method_warnings(&mut self, class_def: &ClassDef) {
        let providers = class_def
            .uses
            .iter()
            .filter_map(|used| {
                self.find_class(used).map(|definition| {
                    (
                        definition.name.clone(),
                        self.effective_trait_methods(
                            definition,
                            &mut std::collections::HashSet::new(),
                        ),
                    )
                })
            })
            .collect::<Vec<_>>();
        let mut diagnostics = class_def
            .methods
            .iter()
            .filter(|method| {
                method.1 == Visibility::Private
                    && method.3
                    && !method.0.eq_ignore_ascii_case("__construct")
            })
            .map(|method| crate::compiler::compile::CompileDeprecation {
                message:
                    "Private methods cannot be final as they are never overridden by other classes"
                        .to_string(),
                file: class_def.source_file.clone().unwrap_or_default(),
                line: method
                    .4
                    .op_array
                    .source_lines
                    .first()
                    .map_or(class_def.declaration_line, |(_, line)| *line as usize),
                warning: true,
            })
            .collect::<Vec<_>>();
        let alias_warning_count = class_def
            .trait_aliases
            .iter()
            .filter(|adaptation| adaptation.is_final)
            .filter(|adaptation| {
                let source = adaptation
                    .trait_name
                    .as_deref()
                    .and_then(|owner| {
                        providers.iter().find(|(provider, _)| {
                            provider.eq_ignore_ascii_case(owner)
                                || self
                                    .find_class(provider)
                                    .zip(self.find_class(owner))
                                    .is_some_and(|(left, right)| {
                                        Self::same_trait_identity(left, right)
                                    })
                        })
                    })
                    .and_then(|(_, methods)| {
                        methods
                            .iter()
                            .find(|method| method.target.eq_ignore_ascii_case(&adaptation.method))
                    })
                    .or_else(|| {
                        providers.iter().find_map(|(_, methods)| {
                            methods.iter().find(|method| {
                                method.target.eq_ignore_ascii_case(&adaptation.method)
                            })
                        })
                    });
                source.is_some_and(|source| {
                    !source.is_final
                        && adaptation.visibility.unwrap_or(source.visibility) == Visibility::Private
                        && !adaptation
                            .alias
                            .as_deref()
                            .unwrap_or(&adaptation.method)
                            .eq_ignore_ascii_case("__construct")
                })
            })
            .count();
        diagnostics.extend((0..alias_warning_count).map(|_| {
            crate::compiler::compile::CompileDeprecation {
                message:
                    "Private methods cannot be final as they are never overridden by other classes"
                        .to_string(),
                file: class_def.source_file.clone().unwrap_or_default(),
                line: class_def.declaration_line,
                warning: true,
            }
        }));
        if diagnostics.is_empty() {
            return;
        }
        self.emit_compile_deprecations(&diagnostics);
    }

    /// Existing direct-method validation handles the ordinary class case.
    /// This companion catches either side of an override that entered through
    /// trait composition without publishing duplicate method declarations.
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn trait_final_override_error(&self, class_def: &ClassDef) -> Option<String> {
        let parent_name = class_def.parent.as_deref()?;
        let child_trait_methods = self.effective_composed_trait_methods(class_def);
        let mut child_names = class_def
            .methods
            .iter()
            .map(|method| method.0.as_str())
            .chain(
                child_trait_methods
                    .iter()
                    .map(|method| method.target.as_str()),
            )
            .collect::<Vec<_>>();
        child_names.sort_unstable_by_key(|name| name.to_ascii_lowercase());
        child_names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));

        let mut ancestor_name = Some(parent_name);
        while let Some(name) = ancestor_name {
            let ancestor = self.find_class(name)?;
            for method in &child_trait_methods {
                if let Some((canonical, _, _, is_final, _)) = ancestor
                    .methods
                    .iter()
                    .find(|candidate| candidate.0.eq_ignore_ascii_case(&method.target))
                    && *is_final
                {
                    return Some(format!(
                        "Cannot override final method {}::{}()",
                        ancestor.name, canonical
                    ));
                }
            }

            for method in self
                .effective_composed_trait_methods(ancestor)
                .into_iter()
                .filter(|method| method.is_final)
            {
                if child_names
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(&method.target))
                {
                    return Some(format!(
                        "Cannot override final method {}::{}()",
                        ancestor.name, method.target
                    ));
                }
            }
            ancestor_name = ancestor.parent.as_deref();
        }
        None
    }

    #[inline]
    fn trait_final_override_may_apply(&self, class_def: &ClassDef) -> bool {
        if !class_def.uses.is_empty() {
            return class_def.parent.is_some();
        }
        let mut ancestor_name = class_def.parent.as_deref();
        while let Some(name) = ancestor_name {
            let Some(ancestor) = self.find_class(name) else {
                return false;
            };
            if !ancestor.uses.is_empty() {
                return true;
            }
            ancestor_name = ancestor.parent.as_deref();
        }
        false
    }

    /// Resolve adaptations before rejecting distinct concrete trait methods
    /// which still occupy the same composed name. Keeping the two cold phases
    /// behind the existing composition call leaves declarations with no trait
    /// work on their established registration path.
    #[cold]
    fn trait_composition_error(&self, class_def: &ClassDef) -> Option<String> {
        let providers = class_def
            .uses
            .iter()
            .filter_map(|used| {
                self.find_class(used).map(|definition| {
                    (
                        definition.name.clone(),
                        self.effective_trait_methods(
                            definition,
                            &mut std::collections::HashSet::new(),
                        ),
                    )
                })
            })
            .collect::<Vec<_>>();
        if (!class_def.trait_precedences.is_empty() || !class_def.trait_aliases.is_empty())
            && let Some(error) = self.trait_adaptation_resolution_error(class_def, &providers)
        {
            let location = class_def
                .source_file
                .as_ref()
                .map_or_else(String::new, |file| {
                    format!(" in {file} on line {}", class_def.declaration_line)
                });
            return Some(format!("{error}{location}"));
        }

        let own_concrete = class_def
            .methods
            .iter()
            .filter(|method| !class_def.method_is_abstract(&method.0))
            .map(|method| method.0.to_ascii_lowercase())
            .collect::<std::collections::HashSet<_>>();
        let mut grouped = (0..providers.len())
            .map(|_| Vec::<TraitCompositionMethod>::new())
            .collect::<Vec<_>>();

        for (index, (provider, methods)) in providers.iter().enumerate() {
            for method in methods {
                if own_concrete.contains(&method.target.to_ascii_lowercase())
                    || class_def.trait_precedences.iter().any(|precedence| {
                        precedence.method.eq_ignore_ascii_case(&method.target)
                            && precedence
                                .instead_of
                                .iter()
                                .any(|excluded| excluded.eq_ignore_ascii_case(provider))
                    })
                {
                    continue;
                }
                grouped[index].push(TraitCompositionMethod {
                    target: method.target.clone(),
                    provider: provider.clone(),
                    source_method: method.target.clone(),
                    origin_owner: method.origin_owner.clone(),
                    origin_method: method.origin_method.clone(),
                });
            }
        }

        for adaptation in &class_def.trait_aliases {
            let Some(alias) = adaptation.alias.as_ref() else {
                continue;
            };
            if alias.eq_ignore_ascii_case("final") {
                continue;
            }
            if own_concrete.contains(&alias.to_ascii_lowercase()) {
                continue;
            }
            let source = adaptation
                .trait_name
                .as_ref()
                .and_then(|owner| {
                    providers
                        .iter()
                        .enumerate()
                        .find(|(_, (provider, _))| provider.eq_ignore_ascii_case(owner))
                })
                .and_then(|(index, (_, methods))| {
                    methods
                        .iter()
                        .find(|method| method.target.eq_ignore_ascii_case(&adaptation.method))
                        .map(|method| (index, method))
                })
                .or_else(|| {
                    providers
                        .iter()
                        .enumerate()
                        .find_map(|(index, (_, methods))| {
                            methods
                                .iter()
                                .find(|method| {
                                    method.target.eq_ignore_ascii_case(&adaptation.method)
                                })
                                .map(|method| (index, method))
                        })
                });
            let Some((index, source)) = source else {
                continue;
            };
            grouped[index].push(TraitCompositionMethod {
                target: alias.clone(),
                provider: providers[index].0.clone(),
                source_method: source.target.clone(),
                origin_owner: source.origin_owner.clone(),
                origin_method: source.origin_method.clone(),
            });
        }

        let mut occupied = std::collections::HashMap::<String, TraitCompositionMethod>::new();
        for candidate in grouped.into_iter().flatten() {
            let target_key = candidate.target.to_ascii_lowercase();
            let Some(previous) = occupied.get(&target_key) else {
                occupied.insert(target_key, candidate);
                continue;
            };
            if Self::same_effective_trait_method(
                &previous.origin_owner,
                &previous.origin_method,
                &candidate.origin_owner,
                &candidate.origin_method,
            ) {
                continue;
            }
            let source_method = if candidate.provider.eq_ignore_ascii_case(&previous.provider) {
                &candidate.target
            } else {
                &candidate.source_method
            };
            let location = class_def
                .source_file
                .as_ref()
                .map_or_else(String::new, |file| {
                    format!(" in {file} on line {}", class_def.declaration_line)
                });
            return Some(format!(
                "Trait method {}::{} has not been applied as {}::{}, because of collision with {}::{}{}",
                candidate.provider,
                source_method,
                class_def.name,
                candidate.target,
                previous.provider,
                previous.target,
                location
            ));
        }
        None
    }

    /// Enum cases and class constants share one case-sensitive symbol table.
    /// Trait resolution and method-adaptation errors precede this cold link
    /// check, while property and forbidden-magic validation follows it.
    #[cold]
    fn enum_trait_case_constant_conflict(&self, class_def: &ClassDef) -> Option<String> {
        if !class_def.is_enum {
            return None;
        }
        let providers = class_def
            .uses
            .iter()
            .map(|used| self.find_class(used))
            .collect::<Option<Vec<_>>>()?;
        let (trait_definition, constant) = providers.iter().find_map(|trait_definition| {
            trait_definition.constants.iter().find_map(|constant| {
                class_def
                    .static_properties
                    .iter()
                    .any(|case| case.name == constant.name)
                    .then_some((*trait_definition, constant))
            })
        })?;
        let location = class_def
            .source_file
            .as_ref()
            .map_or_else(String::new, |file| {
                format!(" in {file} on line {}", class_def.declaration_line)
            });
        Some(format!(
            "Cannot use trait {}, because {}::{} conflicts with enum case {}::{}{}",
            trait_definition.name,
            trait_definition.name,
            constant.name,
            class_def.name,
            constant.name,
            location
        ))
    }

    /// Class-declaration execution renders this link error like compilation,
    /// but only after higher-priority trait adaptation/method collisions have
    /// been ruled out. Registration repeats the checks as a safety net for
    /// non-opcode callers.
    pub(crate) fn enum_trait_case_constant_compile_fatal(
        &self,
        class_def: &ClassDef,
    ) -> Option<String> {
        if !class_def.is_enum || self.trait_composition_error(class_def).is_some() {
            return None;
        }
        self.enum_trait_case_constant_conflict(class_def)
    }

    /// A resolved `use` edge must name a trait. Runtime declarations surface
    /// this relation as a catchable Error at the declaration opcode, so keep
    /// the message separate from registration's fallback fatal formatting.
    #[cold]
    pub(crate) fn direct_non_trait_use_error(&self, class_def: &ClassDef) -> Option<String> {
        class_def.uses.iter().find_map(|trait_name| {
            self.find_class(trait_name)
                .filter(|definition| !definition.is_trait)
                .map(|definition| {
                    format!(
                        "{} cannot use {} - it is not a trait",
                        class_def.name, definition.name
                    )
                })
        })
    }

    #[inline]
    fn same_trait_identity(left: &ClassDef, right: &ClassDef) -> bool {
        std::ptr::eq(left, right)
    }

    #[cold]
    fn used_trait_matches(&self, class_def: &ClassDef, trait_def: &ClassDef) -> bool {
        class_def.uses.iter().any(|used| {
            self.find_class(used)
                .is_some_and(|candidate| Self::same_trait_identity(candidate, trait_def))
        })
    }

    /// Resolve one absolute adaptation owner with PHP's class-alias identity
    /// semantics. Diagnostics name the canonical linked symbol, while an
    /// unknown reference retains the resolved source spelling.
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn resolve_trait_adaptation_owner<'a>(
        &'a self,
        class_def: &ClassDef,
        owner: &str,
    ) -> Result<&'a ClassDef, String> {
        let trait_def = self
            .find_class(owner)
            .ok_or_else(|| format!("Could not find trait {owner}"))?;
        if !trait_def.is_trait {
            return Err(format!(
                "Class {} is not a trait, Only traits may be used in 'as' and 'insteadof' statements",
                trait_def.name
            ));
        }
        if !self.used_trait_matches(class_def, trait_def) {
            return Err(format!(
                "Required Trait {} wasn't added to {}",
                trait_def.name, class_def.name
            ));
        }
        Ok(trait_def)
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn trait_provider_exposes_method(
        trait_def: &ClassDef,
        methods: &[EffectiveTraitMethod],
        method: &str,
    ) -> bool {
        methods
            .iter()
            .any(|candidate| candidate.target.eq_ignore_ascii_case(method))
            || trait_def
                .methods
                .iter()
                .any(|candidate| candidate.0.eq_ignore_ascii_case(method))
    }

    /// Resolve and validate `insteadof` followed by `as`, matching Zend's
    /// deterministic pre-composition phase. This must run before collision
    /// detection: invalid rules never get to suppress or manufacture methods.
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn trait_adaptation_resolution_error(
        &self,
        class_def: &ClassDef,
        providers: &[(String, Vec<EffectiveTraitMethod>)],
    ) -> Option<String> {
        let mut exclusions = std::collections::HashSet::<(usize, String)>::new();
        for precedence in &class_def.trait_precedences {
            let source =
                match self.resolve_trait_adaptation_owner(class_def, &precedence.trait_name) {
                    Ok(source) => source,
                    Err(error) => return Some(error),
                };
            let source_methods = providers
                .iter()
                .find(|(provider, _)| provider.eq_ignore_ascii_case(&source.name))
                .map_or(&[][..], |(_, methods)| methods.as_slice());
            if !Self::trait_provider_exposes_method(source, source_methods, &precedence.method) {
                return Some(format!(
                    "A precedence rule was defined for {}::{} but this method does not exist",
                    source.name, precedence.method
                ));
            }

            for owner in &precedence.instead_of {
                let excluded_trait = match self.resolve_trait_adaptation_owner(class_def, owner) {
                    Ok(excluded_trait) => excluded_trait,
                    Err(error) => return Some(error),
                };
                let exclusion = (
                    excluded_trait as *const ClassDef as usize,
                    precedence.method.to_ascii_lowercase(),
                );
                if !exclusions.insert(exclusion) {
                    return Some(format!(
                        "Failed to evaluate a trait precedence ({}). Method of trait {} was defined to be excluded multiple times",
                        precedence.method, excluded_trait.name
                    ));
                }
                if Self::same_trait_identity(source, excluded_trait) {
                    return Some(format!(
                        "Inconsistent insteadof definition. The method {} is to be used from {}, but {} is also on the exclude list",
                        precedence.method, source.name, source.name
                    ));
                }
            }
        }

        for adaptation in &class_def.trait_aliases {
            if let Some(owner) = adaptation.trait_name.as_deref() {
                let source = match self.resolve_trait_adaptation_owner(class_def, owner) {
                    Ok(source) => source,
                    Err(error) => return Some(error),
                };
                let source_methods = providers
                    .iter()
                    .find(|(provider, _)| provider.eq_ignore_ascii_case(&source.name))
                    .map_or(&[][..], |(_, methods)| methods.as_slice());
                if !Self::trait_provider_exposes_method(source, source_methods, &adaptation.method)
                {
                    return Some(format!(
                        "An alias was defined for {}::{} but this method does not exist",
                        source.name, adaptation.method
                    ));
                }
                continue;
            }

            let mut source: Option<&ClassDef> = None;
            for (provider, methods) in providers {
                let Some(candidate) = self.find_class(provider) else {
                    continue;
                };
                if !Self::trait_provider_exposes_method(candidate, methods, &adaptation.method) {
                    continue;
                }
                if let Some(previous) = source {
                    return Some(format!(
                        "An alias was defined for method {}(), which exists in both {} and {}. Use {}::{} or {}::{} to resolve the ambiguity",
                        adaptation.method,
                        previous.name,
                        candidate.name,
                        previous.name,
                        adaptation.method,
                        candidate.name,
                        adaptation.method
                    ));
                }
                source = Some(candidate);
            }

            if source.is_none() {
                match adaptation.alias.as_deref() {
                    Some(alias) if !alias.eq_ignore_ascii_case("final") => {
                        return Some(format!(
                            "An alias ({alias}) was defined for method {}(), but this method does not exist",
                            adaptation.method
                        ));
                    }
                    _ => {
                        return Some(format!(
                            "The modifiers of the trait method {}() are changed, but this method does not exist. Error",
                            adaptation.method
                        ));
                    }
                }
            }
        }

        None
    }

    /// Compose and internally publish a runtime class before its outstanding
    /// method-variance dependencies have finished autoloading. The active-link
    /// guard keeps the class hidden from userland symbol probes while a new
    /// descendant may use the complete parent layout during its own link.
    pub(crate) fn register_provisional_runtime_class(
        &mut self,
        class_def: ClassDef,
        outstanding_variance_dependencies: Vec<String>,
    ) -> Result<(), String> {
        let relation = self
            .active_runtime_class_relations
            .get_mut(&class_def.name.to_ascii_lowercase())
            .ok_or_else(|| {
                format!(
                    "Runtime class {} is not active during provisional linking",
                    class_def.name
                )
            })?;
        relation.outstanding_variance_dependencies = outstanding_variance_dependencies;
        self.register_class_mode(class_def, true)?;
        self.retry_pending_named_classes()
    }

    pub(crate) fn finalize_provisional_runtime_class(
        &self,
        class_name: &str,
    ) -> Result<(), String> {
        let class_def = self
            .find_class(class_name)
            .ok_or_else(|| format!("Provisional class {class_name} disappeared while linking"))?;
        self.validate_parent_method_contracts(class_def)?;
        self.validate_abstract_method_contracts(class_def)
    }

    fn register_class_mode(
        &mut self,
        mut class_def: ClassDef,
        defer_method_contracts: bool,
    ) -> Result<(), String> {
        let class_name = class_def.name.clone();
        let declaration_file = class_def.source_file.clone();
        let declaration_line = class_def.declaration_line;
        let (
            own_deferred_instance_defaults,
            own_deferred_static_defaults,
            own_rebound_trait_defaults,
        ) = class_def
            .deferred_instance_defaults
            .take()
            .map(|defaults| {
                (
                    defaults.entries().as_ref().clone(),
                    defaults.static_entries().as_ref().clone(),
                    defaults.rebound_trait_entries().as_ref().clone(),
                )
            })
            .unwrap_or_default();
        // PHP materializes a child's inherited defaults before its own
        // declarations. Keep that semantic order independently from RPHP's
        // storage-slot order, which deliberately places child declarations
        // first for property lookup.
        let mut deferred_instance_defaults = Vec::new();
        let mut deferred_static_defaults = Vec::new();
        let mut rebound_trait_defaults = own_rebound_trait_defaults;
        let mut inherited_rebound_trait_defaults = Vec::new();
        // PHP does not permit class redeclaration. Besides matching that rule,
        // this guarantees class_by_id pointers remain stable for inline caches.
        if let Some(previous) = self
            .class_table
            .iter()
            .find(|(registered, _)| registered.eq_ignore_ascii_case(&class_name))
            .map(|(_, class)| class.as_ref())
        {
            return Err(Self::class_like_redeclaration_error(previous, &class_def));
        }
        // Class-like names are case-insensitive in PHP. Keep the canonical
        // registered parent spelling in linked metadata so the later layout
        // and method materialization hits the same parent that validation
        // already resolved case-insensitively.
        if let Some(parent_name) = class_def.parent.as_deref()
            && let Some(parent) = self.find_class(parent_name)
        {
            class_def.parent = Some(parent.name.clone());
        }
        let relation_location = || {
            class_def
                .source_file
                .as_ref()
                .map_or_else(String::new, |file| {
                    format!(" in {file} on line {}", class_def.declaration_line)
                })
        };
        if !class_def.uses.is_empty() {
            if let Some(error) = self.direct_non_trait_use_error(&class_def) {
                return Err(format!("{error}{}", relation_location()));
            }
            if let Some(error) = self.trait_composition_error(&class_def) {
                return Err(error);
            }
            if let Some(error) = self.enum_trait_case_constant_conflict(&class_def) {
                return Err(error);
            }
        }
        if class_def.source_file.is_some()
            && (class_def.trait_aliases.iter().any(|alias| alias.is_final)
                || class_def.methods.iter().any(|method| {
                    method.1 == Visibility::Private
                        && method.3
                        && !method.0.eq_ignore_ascii_case("__construct")
                }))
        {
            self.private_final_method_warnings(&class_def);
        }
        if class_def.source_file.is_some()
            && self.trait_final_override_may_apply(&class_def)
            && let Some(error) = self.trait_final_override_error(&class_def)
        {
            return Err(format!("{error}{}", relation_location()));
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
        validate_property_hook_setter_variance(self, &class_def)?;
        self.validate_override_contracts(&class_def)?;
        // Assign stable class ID
        let id = self.next_class_id;
        self.next_class_id += 1;
        class_def.class_id = id;
        if class_def.is_enum {
            for definition in &class_def.static_properties {
                if let Some(value) = &definition.default
                    && let Some(mut case) = value.as_object_mut()
                {
                    case.class_id = id;
                }
            }
        }
        let own_property_names = class_def
            .properties
            .iter()
            .map(|property| property.name.clone())
            .collect::<std::collections::HashSet<_>>();
        let own_explicit_property_hooks = class_def
            .properties
            .iter()
            .flat_map(|property| {
                let getter = property
                    .has_get_hook
                    .then(|| format!("${}::get", property.name).to_lowercase());
                let setter = property
                    .has_set_hook
                    .then(|| format!("${}::set", property.name).to_lowercase());
                getter.into_iter().chain(setter)
            })
            .collect::<std::collections::HashSet<_>>();
        let own_static_names = class_def
            .static_properties
            .iter()
            .map(|property| property.name.clone())
            .collect::<std::collections::HashSet<_>>();
        let mut inherited_concrete_property_hooks = std::collections::HashSet::new();
        // `None` denotes a declaration composed by this class and therefore a
        // fresh slot. Inherited entries carry the parent's canonical slot.
        let mut static_property_slots = vec![None; class_def.static_properties.len()];
        let declaration_location = || {
            declaration_file
                .as_deref()
                .map_or_else(String::new, |file| {
                    format!(" in {file} on line {declaration_line}")
                })
        };

        // Enums and final classes cannot be used as parents.
        if let Some(parent_name) = &class_def.parent {
            if let Some(parent) = self.find_class(parent_name) {
                if parent.is_trait {
                    return Err(format!(
                        "Class {} cannot extend trait {}{}",
                        class_name,
                        parent.name,
                        declaration_location()
                    ));
                }
                if parent.is_enum {
                    let child_name = if class_def.is_anonymous() {
                        format!("{}@anonymous", parent.name)
                    } else {
                        class_name.clone()
                    };
                    return Err(format!(
                        "Class {} cannot extend enum {}{}",
                        child_name,
                        parent.name,
                        declaration_location()
                    ));
                }
                if parent.is_final {
                    return Err(format!(
                        "Class {} cannot extend final class {}{}",
                        class_name,
                        parent_name,
                        declaration_location()
                    ));
                }
                if class_def.is_readonly != parent.is_readonly {
                    return Err(if class_def.is_readonly {
                        format!(
                            "Readonly class {} cannot extend non-readonly class {}{}",
                            class_name,
                            parent_name,
                            declaration_location()
                        )
                    } else {
                        format!(
                            "Non-readonly class {} cannot extend readonly class {}{}",
                            class_name,
                            parent_name,
                            declaration_location()
                        )
                    });
                }
                // PHP propagates #[AllowDynamicProperties] through the class
                // hierarchy, so descendants inherit the opt-out even when
                // they do not repeat the attribute.
                class_def.allow_dynamic_properties |= parent.allow_dynamic_properties;
                let mut constants = std::mem::take(&mut class_def.constants);
                let result = merge_parent_constant_definitions(
                    &class_name,
                    &mut constants,
                    &parent.constants,
                    declaration_file.as_deref(),
                    declaration_line,
                    &|actual, target| {
                        self.class_is_a_while_linking(actual, target, Some(&class_def))
                    },
                );
                class_def.constants = constants;
                result?;
            }
        }

        self.declaration_interface_contract(&class_def)?;
        if !defer_method_contracts {
            self.validate_parent_method_contracts(&class_def)?;
            self.validate_abstract_method_contracts(&class_def)?;
        }

        // Resolve inheritance — merge parent's properties and methods
        if let Some(parent_name) = &class_def.parent {
            if let Some(parent) = self.class_table.get(parent_name.as_str()) {
                if let Some(defaults) = &parent.deferred_instance_defaults {
                    deferred_instance_defaults.extend(defaults.entries().iter().cloned());
                    deferred_static_defaults.extend(defaults.static_entries().iter().cloned());
                    inherited_rebound_trait_defaults.extend(
                        defaults
                            .rebound_trait_entries()
                            .iter()
                            .filter(|entry| entry.is_static)
                            .cloned(),
                    );
                }
                validate_property_inheritance(
                    self,
                    &class_def,
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
                let inherited: Vec<(String, *const FunctionCommon, bool)> = self
                    .function_table
                    .iter()
                    .filter(|(k, _)| k.starts_with(&parent_prefix))
                    .map(|(k, v)| {
                        let method_name = &k[parent_prefix.len()..];
                        let concrete_property_hook = method_name
                            .strip_prefix('$')
                            .and_then(|name| name.split_once("::"))
                            .and_then(|(property_name, hook)| {
                                parent
                                    .properties
                                    .iter()
                                    .find(|property| {
                                        property.name.eq_ignore_ascii_case(property_name)
                                    })
                                    .map(|property| match hook {
                                        "get" => {
                                            property.has_get_hook && !property.abstract_get_hook()
                                        }
                                        "set" => {
                                            property.has_set_hook && !property.abstract_set_hook()
                                        }
                                        _ => false,
                                    })
                            })
                            .unwrap_or(false);
                        (method_name.to_string(), *v, concrete_property_hook)
                    })
                    .collect();
                for (method_name, func_ptr, concrete_property_hook) in inherited {
                    let replaces_synthetic_property_accessor = concrete_property_hook
                        && method_name.starts_with('$')
                        && !own_explicit_property_hooks.contains(&method_name);
                    if !child_method_names.contains(&method_name)
                        || replaces_synthetic_property_accessor
                    {
                        let child_full = format!("{}::{}", class_name, method_name).to_lowercase();
                        self.function_table.insert(child_full, func_ptr);
                        if replaces_synthetic_property_accessor {
                            inherited_concrete_property_hooks.insert(method_name);
                        }
                    }
                }
            }
        }
        deferred_instance_defaults.extend(own_deferred_instance_defaults);
        deferred_static_defaults.extend(own_deferred_static_defaults);

        // An inherited trait-composed static property keeps its parent's
        // storage slot and therefore its actual value. PHP nevertheless
        // exposes a child-relative default through ReflectionClass, so only
        // rebind the copied declaration metadata here and carry the recipe to
        // further descendants.
        for rebound in inherited_rebound_trait_defaults {
            let Some(property_index) = class_def.static_properties.iter().position(|property| {
                property.name == rebound.definition.property_name
                    && property.declaring_class == rebound.definition.declaring_class
            }) else {
                continue;
            };
            let value = self.evaluate_rebound_trait_property_default(
                &rebound,
                &class_def.static_properties[property_index],
                &class_name,
                class_def.parent.as_deref(),
            )?;
            class_def.static_properties[property_index].default = Some(value);
            rebound_trait_defaults.push(rebound);
        }

        // Merge traits: copy trait methods and properties into this class.
        // Must happen after parent inheritance so trait methods override inherited ones
        // (matching PHP semantics: trait > parent, class > trait).
        let trait_names = class_def.uses.clone();
        let mut composed_trait_constant_origins = std::collections::HashMap::new();
        let mut composed_trait_property_names = std::collections::HashMap::new();
        for trait_name in &trait_names {
            let trait_definition =
                self.class_table
                    .get(trait_name.as_str())
                    .cloned()
                    .or_else(|| {
                        self.class_table
                            .iter()
                            .find(|(registered, _)| registered.eq_ignore_ascii_case(trait_name))
                            .map(|(_, definition)| std::rc::Rc::clone(definition))
                    });
            if let Some(trait_def) = trait_definition {
                let trait_deferred_defaults = trait_def
                    .deferred_instance_defaults
                    .as_ref()
                    .map(|defaults| defaults.entries())
                    .unwrap_or_else(|| std::rc::Rc::new(Vec::new()));
                let trait_rebound_defaults = trait_def
                    .deferred_instance_defaults
                    .as_ref()
                    .map(|defaults| defaults.rebound_trait_entries());
                let trait_rebound_defaults = trait_rebound_defaults
                    .as_deref()
                    .map_or(&[][..], |entries| entries.as_slice());
                let mut trait_properties = trait_def.properties.clone();
                let mut trait_static_properties = trait_def.static_properties.clone();
                for rebound in trait_rebound_defaults {
                    let properties = if rebound.is_static {
                        &mut trait_static_properties
                    } else {
                        &mut trait_properties
                    };
                    let Some(property_index) = properties.iter().position(|property| {
                        property.name == rebound.definition.property_name
                            && property.declaring_class == rebound.definition.declaring_class
                    }) else {
                        continue;
                    };
                    let value = self.evaluate_rebound_trait_property_default(
                        rebound,
                        &properties[property_index],
                        &class_name,
                        class_def.parent.as_deref(),
                    )?;
                    properties[property_index].default = Some(value);
                }
                let accepted_rebound_defaults = trait_rebound_defaults
                    .iter()
                    .filter(|rebound| {
                        (if rebound.is_static {
                            !own_static_names.contains(&rebound.definition.property_name)
                        } else {
                            !own_property_names.contains(&rebound.definition.property_name)
                        }) && !composed_trait_property_names
                            .contains_key(&rebound.definition.property_name)
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                // Trait collision checks compare resolved default values. A
                // direct global constant may have been published by an
                // earlier include/define() after this trait's source unit was
                // compiled, so resolve that non-autoloading form solely for
                // the composition comparison. The consumer still retains the
                // deferred expression and performs its canonical first-use
                // materialization below.
                for deferred in trait_deferred_defaults.iter() {
                    if !matches!(
                        deferred.expression.as_ref(),
                        crate::parser::Expr::Constant { .. }
                    ) {
                        continue;
                    }
                    match crate::stdlib::reflection::evaluate_deferred_property_default_value(
                        deferred, self,
                    )
                    .map_err(|error| error.to_string())?
                    {
                        Some(value) => {
                            if let Some(property) = trait_properties.iter_mut().find(|property| {
                                property.name == deferred.property_name
                                    && property.declaring_class == deferred.declaring_class
                            }) {
                                property.default = Some(value);
                            }
                        }
                        None => {
                            // A missing direct constant has no side effects and
                            // remains a first-use dependency. Do not leak the
                            // comparison probe's synthetic Error into linking.
                            self.exception = None;
                        }
                    }
                }
                if class_def.is_enum {
                    let declaration_location = class_def
                        .source_file
                        .as_ref()
                        .map_or_else(String::new, |file| {
                            format!(" in {file} on line {}", class_def.declaration_line)
                        });
                    if !trait_def.properties.is_empty() || !trait_def.static_properties.is_empty() {
                        return Err(format!(
                            "Enum {class_name} cannot include properties{declaration_location}"
                        ));
                    }
                    if let Some((method, ..)) = trait_def
                        .methods
                        .iter()
                        .find(|(method, ..)| enum_magic_method_is_forbidden(method))
                    {
                        return Err(format!(
                            "Enum {class_name} cannot include magic method {method}{declaration_location}"
                        ));
                    }
                }
                merge_trait_constant_definitions(
                    &class_name,
                    trait_name,
                    &mut class_def.constants,
                    &trait_def.constants,
                    &mut composed_trait_constant_origins,
                    declaration_file.as_deref(),
                    declaration_line,
                )?;
                merge_trait_property_definitions(
                    &mut class_def.properties,
                    &trait_properties,
                    &class_name,
                    trait_name,
                    &own_property_names,
                    &own_static_names,
                    &mut composed_trait_property_names,
                    class_def.source_file.as_deref(),
                    class_def.declaration_line,
                )?;
                for deferred in trait_deferred_defaults.iter() {
                    if own_property_names.contains(&deferred.property_name)
                        || composed_trait_property_names
                            .get(&deferred.property_name)
                            .is_none_or(|origin| {
                                origin.is_static
                                    || !origin.trait_name.eq_ignore_ascii_case(trait_name)
                            })
                    {
                        continue;
                    }
                    let mut deferred = deferred.clone();
                    deferred.declaring_class = class_name.clone();
                    let mut scope = (*deferred.evaluation_scope).clone();
                    scope.lexical_class = Some(class_name.clone());
                    scope.lexical_parent = class_def.parent.clone();
                    deferred.evaluation_scope = std::rc::Rc::new(scope);
                    deferred_instance_defaults.push(deferred);
                }
                merge_trait_static_property_definitions(
                    &mut class_def.static_properties,
                    &mut static_property_slots,
                    &trait_static_properties,
                    &class_name,
                    trait_name,
                    &own_static_names,
                    &own_property_names,
                    &mut composed_trait_property_names,
                    class_def.source_file.as_deref(),
                    class_def.declaration_line,
                )?;
                for mut rebound in accepted_rebound_defaults {
                    if !class_def.is_trait && !rebound.is_static {
                        continue;
                    }
                    rebound.definition.declaring_class = class_name.clone();
                    let mut scope = (*rebound.definition.evaluation_scope).clone();
                    scope.lexical_class = Some(class_name.clone());
                    scope.lexical_parent = class_def.parent.clone();
                    rebound.definition.evaluation_scope = std::rc::Rc::new(scope);
                    rebound_trait_defaults.push(rebound);
                }

                // Merge trait methods: copy function_table pointers
                let child_method_names: std::collections::HashSet<String> = class_def
                    .methods
                    .iter()
                    .map(|(n, _, _, _, _)| n.to_lowercase())
                    .collect();
                let trait_prefix = format!("{}::", trait_name).to_lowercase();
                let trait_methods: Vec<(String, *const FunctionCommon, bool)> = self
                    .function_table
                    .iter()
                    .filter(|(k, _)| k.starts_with(&trait_prefix))
                    .filter(|(key, _)| {
                        let method_name = &key[trait_prefix.len()..];
                        !class_def.trait_precedences.iter().any(|precedence| {
                            precedence.method.eq_ignore_ascii_case(method_name)
                                && precedence
                                    .instead_of
                                    .iter()
                                    .any(|excluded| excluded.eq_ignore_ascii_case(trait_name))
                        })
                    })
                    .map(|(k, v)| {
                        let method_name = &k[trait_prefix.len()..];
                        let is_static =
                            trait_def.methods.iter().any(|(name, _, is_static, _, _)| {
                                *is_static && name.eq_ignore_ascii_case(method_name)
                            });
                        (method_name.to_string(), *v, is_static)
                    })
                    .collect();
                for (method_name, func_ptr, is_static) in trait_methods {
                    if !child_method_names.contains(&method_name) {
                        let (func_ptr, bound_lexical_static_properties) = self
                            .compose_trait_method_pointer(
                                func_ptr,
                                &class_name,
                                &method_name,
                                is_static,
                                !class_def.is_trait,
                            );
                        let child_full = format!("{}::{}", class_name, method_name).to_lowercase();
                        self.function_table.insert(child_full, func_ptr);
                        // A specialized trait function is unique to this
                        // concrete composer, so publish that lexical owner
                        // directly. Shared pointers retain their trait owner
                        // and recover the active composer from the call frame.
                        let declaring_class =
                            if !class_def.is_trait && bound_lexical_static_properties {
                                &class_name
                            } else {
                                trait_name
                            };
                        self.method_declaring_class
                            .entry(func_ptr)
                            .or_insert_with(|| declaring_class.clone());
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
            if class_def.is_enum && enum_magic_method_is_forbidden(alias) {
                let location = class_def
                    .source_file
                    .as_ref()
                    .map_or_else(String::new, |file| {
                        format!(" in {file} on line {}", class_def.declaration_line)
                    });
                return Err(format!(
                    "Enum {class_name} cannot include magic method {alias}{location}"
                ));
            }
            let is_static = self
                .class_table
                .get(source_trait.as_str())
                .and_then(|trait_def| {
                    trait_def
                        .methods
                        .iter()
                        .find_map(|(name, _, is_static, _, _)| {
                            name.eq_ignore_ascii_case(&adaptation.method)
                                .then_some(*is_static)
                        })
                })
                .unwrap_or(false);
            let (pointer, bound_lexical_static_properties) = self.compose_trait_method_pointer(
                pointer,
                &class_name,
                alias,
                is_static,
                !class_def.is_trait,
            );
            self.function_table
                .insert(format!("{}::{}", class_name, alias).to_lowercase(), pointer);
            let declaring_class = if !class_def.is_trait && bound_lexical_static_properties {
                &class_name
            } else {
                source_trait
            };
            self.method_declaring_class
                .entry(pointer)
                .or_insert_with(|| declaring_class.clone());
        }

        // Interface constants are inherited without being copied into source
        // declarations. Flatten them once at class registration so reads and
        // their inline caches are an indexed lookup thereafter.
        let class_like_kind = || {
            if class_def.is_interface {
                "Interface"
            } else if class_def.is_enum {
                "Enum"
            } else {
                "Class"
            }
        };
        for interface_name in &class_def.implements {
            // A small set of built-in interface contracts is registered by
            // the stdlib without a userland ClassDef. They have no userland
            // constants to inherit, so keep the existing contract path.
            let Some(interface) = self.class_table.get(interface_name.as_str()) else {
                continue;
            };
            let mut constants = std::mem::take(&mut class_def.constants);
            let result = merge_interface_constant_definitions(
                class_like_kind(),
                &class_name,
                &mut constants,
                &interface.constants,
                declaration_file.as_deref(),
                declaration_line,
                &|actual, target| self.class_is_a_while_linking(actual, target, Some(&class_def)),
            );
            class_def.constants = constants;
            result?;
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
                                let (message, function) = if m_name.starts_with('$') {
                                    (
                                        format!(
                                            "Cannot override final property hook {}::{}()",
                                            anc_name, m_name
                                        ),
                                        class_def
                                            .methods
                                            .iter()
                                            .find(|(name, _, _, _, _)| {
                                                name.eq_ignore_ascii_case(m_name)
                                            })
                                            .map(|(_, _, _, _, function)| function),
                                    )
                                } else {
                                    (
                                        format!(
                                            "Cannot override final method {}::{}()",
                                            anc_name, m_name
                                        ),
                                        None,
                                    )
                                };
                                if let Some(function) = function
                                    && !function.op_array.source_file.is_empty()
                                {
                                    let line = function
                                        .op_array
                                        .source_lines
                                        .first()
                                        .map(|(_, line)| *line)
                                        .unwrap_or(1);
                                    return Err(format!(
                                        "{message} in {} on line {line}",
                                        function.op_array.source_file
                                    ));
                                }
                                return Err(message);
                            }
                        }
                        ancestor = anc_def.parent.clone();
                    } else {
                        break;
                    }
                }
            }
        }

        deferred_static_defaults.retain_mut(|deferred| {
            let Some((property_index, _)) =
                class_def
                    .static_properties
                    .iter()
                    .enumerate()
                    .find(|(_, property)| {
                        property.name == deferred.property_name
                            && property.declaring_class == deferred.declaring_class
                    })
            else {
                return false;
            };
            deferred.property_index = property_index;
            true
        });

        let mut resolved_static_slots = Vec::with_capacity(static_property_slots.len());
        for (property_index, (definition, inherited_slot)) in class_def
            .static_properties
            .iter()
            .zip(static_property_slots)
            .enumerate()
        {
            let slot = if let Some(slot) = inherited_slot {
                slot
            } else {
                let slot = u32::try_from(self.static_property_values.len())
                    .map_err(|_| "Too many static property storage slots".to_string())?;
                self.static_property_values
                    .push(definition.default.clone().unwrap_or_else(|| {
                        if deferred_static_defaults
                            .iter()
                            .any(|deferred| deferred.property_index == property_index)
                        {
                            return Value::undef();
                        }
                        if definition.is_typed() {
                            Value::undef()
                        } else {
                            Value::null()
                        }
                    }));
                self.static_property_handles_published
                    .push(Cell::new(false));
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

        deferred_instance_defaults.retain_mut(|deferred| {
            let Some((property_index, _)) =
                class_def
                    .properties
                    .iter()
                    .enumerate()
                    .find(|(_, property)| {
                        property.name == deferred.property_name
                            && property.declaring_class == deferred.declaring_class
                    })
            else {
                return false;
            };
            deferred.property_index = property_index;
            true
        });
        rebound_trait_defaults.retain(|rebound| {
            let properties = if rebound.is_static {
                &class_def.static_properties
            } else {
                &class_def.properties
            };
            properties.iter().any(|property| {
                property.name == rebound.definition.property_name
                    && property.declaring_class == rebound.definition.declaring_class
            })
        });
        class_def.deferred_instance_defaults = (!deferred_instance_defaults.is_empty()
            || !deferred_static_defaults.is_empty()
            || !rebound_trait_defaults.is_empty())
        .then(|| {
            Box::new(
                crate::compiler::compile::DeferredInstancePropertyDefaults::with_all_entries(
                    deferred_instance_defaults,
                    deferred_static_defaults,
                    rebound_trait_defaults,
                ),
            )
        });

        if class_def
            .constants
            .iter()
            .any(|constant| constant.value_is_deferred)
        {
            self.register_deferred_class_constant_activation(class_def.class_id);
        }

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
                (class.is_interface || !class.method_is_abstract(method_name))
                    && !inherited_concrete_property_hooks.contains(&method_name.to_lowercase())
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
        let constant_expression_lexical_functions = self
            .class_table
            .get(&class_name)
            .map(|class| {
                let constant_functions = class.constants.iter().flat_map(|constant| {
                    constant
                        .callable_factory
                        .iter()
                        .flat_map(|factory| factory.lexical_functions.iter())
                        .map(|function| (function.clone(), constant.declaring_class.clone()))
                });
                let property_functions =
                    class
                        .deferred_instance_defaults
                        .iter()
                        .flat_map(|defaults| {
                            defaults
                                .entries()
                                .iter()
                                .chain(defaults.static_entries().iter())
                                .filter_map(|definition| {
                                    definition.callable_factory.as_ref().map(|factory| {
                                        factory.lexical_functions.iter().map(|function| {
                                            (function.clone(), definition.declaring_class.clone())
                                        })
                                    })
                                })
                                .flatten()
                                .collect::<Vec<_>>()
                        });
                constant_functions
                    .chain(property_functions)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for (function, declaring_class) in constant_expression_lexical_functions {
            if let Some(function) = self.find_function(&function) {
                self.method_declaring_class
                    .insert(function, declaring_class);
            }
        }

        self.validate_interface_property_contracts(&class_name)?;

        // Internal startup descriptors are trusted and may publish their
        // callable handlers immediately after their class skeleton. Userland
        // classes still pass the complete post-registration interface gate.
        let missing = if self
            .class_table
            .get(&class_name)
            .is_some_and(|class| class.source_file.is_none() && class.declaration_line == 0)
        {
            Vec::new()
        } else {
            self.validate_interface_contracts(&class_name)
        };
        if !missing.is_empty() {
            let count = missing.len();
            let (method_word, remaining_word) = if count == 1 {
                ("method", "method")
            } else {
                ("methods", "methods")
            };
            let requirements = missing
                .iter()
                .map(|(interface, method)| format!("{interface}::{method}"))
                .collect::<Vec<_>>()
                .join(", ");
            let location = self
                .class_table
                .get(&class_name)
                .and_then(|class| class.source_file.as_ref())
                .map_or_else(String::new, |file| format!(" in {file} on line 0"));
            return Err(format!(
                "Class {} contains {} abstract {} and must therefore be declared abstract or implement the remaining {} ({}){}",
                class_name, count, method_word, remaining_word, requirements, location
            ));
        }

        Ok(())
    }

    /// Class-like names share one case-insensitive registry. PHP attributes a
    /// collision to the first declaration's kind and spelling, except that an
    /// enum colliding with another kind uses that non-enum kind.
    #[cold]
    fn class_like_redeclaration_error(previous: &ClassDef, current: &ClassDef) -> String {
        let diagnostic_owner = if previous.is_enum && !current.is_enum {
            current
        } else {
            previous
        };
        let kind = if diagnostic_owner.is_interface {
            "interface"
        } else if diagnostic_owner.is_trait {
            "trait"
        } else if diagnostic_owner.is_enum {
            "enum"
        } else {
            "class"
        };
        let previous_location = previous
            .source_file
            .as_ref()
            .map_or_else(String::new, |file| {
                format!(
                    " (previously declared in {file}:{})",
                    previous.declaration_line
                )
            });
        let current_location = current
            .source_file
            .as_ref()
            .map_or_else(String::new, |file| {
                format!(" in {file} on line {}", current.declaration_line)
            });
        format!(
            "Cannot redeclare {kind} {}{previous_location}{current_location}",
            diagnostic_owner.name
        )
    }

    #[cold]
    #[inline(never)]
    fn validate_interface_property_contracts(&self, class_name: &str) -> Result<(), String> {
        let Some(class_def) = self.class_table.get(class_name) else {
            return Ok(());
        };
        if class_def.is_interface || class_def.is_abstract || class_def.is_trait {
            return Ok(());
        }
        for interface_name in self.collect_all_interfaces(class_name) {
            let Some(interface) = self.class_table.get(&interface_name) else {
                continue;
            };
            for required in &interface.properties {
                let Some(property) = class_def
                    .properties
                    .iter()
                    .find(|property| property.name == required.name)
                else {
                    continue;
                };
                if required.has_set_hook && property.is_readonly {
                    let location = property
                        .source_file
                        .as_ref()
                        .map_or_else(String::new, |file| {
                            format!(" in {file} on line {}", property.declaration_line())
                        });
                    return Err(format!(
                        "Set access level of {}::${} must be omitted (as in class {}){}",
                        class_name, property.name, interface.name, location
                    ));
                }
                if required.has_set_hook
                    && let Some(required_setter) = interface.methods.iter().find(|method| {
                        method
                            .0
                            .eq_ignore_ascii_case(&format!("${}::set", required.name))
                    })
                    && let Some(implementation) =
                        self.find_effective_method(class_def, &format!("${}::set", property.name))
                    && let (Some(required_hint), Some(implementation_hint)) = (
                        required_setter.4.common.sig.param_type_hints.first(),
                        implementation.signature.param_type_hints.first(),
                    )
                    && !self.is_param_type_compatible_strict(
                        implementation_hint,
                        required_hint,
                        implementation.owner,
                        &interface.name,
                        Some(class_def),
                    )
                {
                    let location = property
                        .source_file
                        .as_ref()
                        .map_or_else(String::new, |file| {
                            format!(" in {file} on line {}", property.declaration_line())
                        });
                    return Err(format!(
                        "Set type of {}::${} must be supertype of {} (as in interface {}){}",
                        class_name,
                        property.name,
                        required_hint.property_declaration_display_name(),
                        interface.name,
                        location,
                    ));
                }
                if required.has_get_hook
                    && let Some(required_getter) = interface.methods.iter().find(|method| {
                        method
                            .0
                            .eq_ignore_ascii_case(&format!("${}::get", required.name))
                    })
                    && required_getter.4.common.sig.returns_reference
                    && property.has_get_hook
                    && let Some(implementation) =
                        self.find_effective_method(class_def, &format!("${}::get", property.name))
                    && !implementation.signature.returns_reference
                {
                    let required = Self::method_declaration(interface, required_getter);
                    let location = class_def
                        .source_file
                        .as_ref()
                        .map_or_else(String::new, |file| format!(" in {file} on line 0"));
                    return Err(format!(
                        "Declaration of {} must be compatible with {}{}",
                        self.format_method_signature(implementation, Some(class_def)),
                        self.format_method_signature(required, Some(class_def)),
                        location
                    ));
                }
            }
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

    /// Resolve and cache whether a stable runtime class inherits a live user
    /// destructor. Dynamic class-id-zero objects retain the general lookup
    /// because their names do not share one class identity.
    #[inline]
    pub(crate) fn class_has_destructor(&self, class_id: u32, class_name: &str) -> bool {
        if class_id != 0
            && let Some(flag) = self
                .class_destructor_flags
                .borrow()
                .get(class_id as usize)
                .copied()
            && flag != 0
        {
            return flag == 2;
        }
        let has_destructor = self.find_method_info(class_name, "__destruct").is_some();
        if class_id != 0 {
            let mut flags = self.class_destructor_flags.borrow_mut();
            if flags.len() <= class_id as usize {
                flags.resize(class_id as usize + 1, 0);
            }
            flags[class_id as usize] = if has_destructor { 2 } else { 1 };
        }
        has_destructor
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

    /// Visible declared instance-property slots in PHP object iteration order.
    /// Ancestor declarations precede child declarations, while a private
    /// property visible to its owner masks another declaration with the same
    /// public name without moving that array/iteration position.
    pub(crate) fn visible_instance_property_slots(
        &self,
        class_id: u32,
        caller_class: Option<&str>,
    ) -> Vec<usize> {
        let Some(class) = self.class_by_id(class_id) else {
            return Vec::new();
        };
        let candidates = self.instance_property_slots_in_iteration_order(class_id);

        let mut visible = Vec::<(String, usize)>::new();
        let mut positions = std::collections::HashMap::<String, usize>::new();
        let mut private_names = std::collections::HashSet::<String>::new();
        for slot in candidates {
            let property = &class.properties[slot];
            if !self.check_instance_property_visibility(
                caller_class,
                &class.name,
                &property.name,
                &property.declaring_class,
                property.visibility,
            ) {
                continue;
            }
            let is_private = property.visibility == Visibility::Private;
            if is_private || !private_names.contains(&property.name) {
                if let Some(position) = positions.get(&property.name).copied() {
                    visible[position].1 = slot;
                } else {
                    positions.insert(property.name.clone(), visible.len());
                    visible.push((property.name.clone(), slot));
                }
            }
            if is_private {
                private_names.insert(property.name.clone());
            }
        }
        visible.into_iter().map(|(_, slot)| slot).collect()
    }

    /// Declared slots in the order exposed by object iteration and diagnostic
    /// renderers. A non-private override retains the first ancestor bucket's
    /// position even though the compact object layout stores the overriding
    /// child declaration in its own slot.
    pub(crate) fn instance_property_slots_in_iteration_order(&self, class_id: u32) -> Vec<usize> {
        let Some(class) = self.class_by_id(class_id) else {
            return Vec::new();
        };
        let mut lineage = Vec::new();
        let mut current = Some(class.name.as_str());
        while let Some(class_name) = current {
            let Some(definition) = self.find_class(class_name) else {
                break;
            };
            lineage.push(definition.name.as_str());
            current = definition.parent.as_deref();
        }
        lineage.reverse();

        let mut slots = (0..class.properties.len()).collect::<Vec<_>>();
        slots.sort_by_key(|slot| {
            let property = &class.properties[*slot];
            if property.visibility != Visibility::Private {
                for (rank, owner) in lineage.iter().enumerate() {
                    let inherited_bucket = self.find_class(owner).and_then(|definition| {
                        definition.properties.iter().position(|candidate| {
                            candidate.visibility != Visibility::Private
                                && candidate.name == property.name
                                && candidate.declaring_class.eq_ignore_ascii_case(owner)
                        })
                    });
                    if let Some(position) = inherited_bucket {
                        return (rank, position);
                    }
                }
            }
            let rank = lineage
                .iter()
                .position(|owner| owner.eq_ignore_ascii_case(&property.declaring_class))
                .unwrap_or(lineage.len());
            let position = lineage
                .get(rank)
                .and_then(|owner| self.find_class(owner))
                .and_then(|definition| {
                    definition.properties.iter().position(|candidate| {
                        candidate.name == property.name
                            && candidate.visibility == property.visibility
                            && candidate
                                .declaring_class
                                .eq_ignore_ascii_case(&property.declaring_class)
                    })
                })
                .unwrap_or(*slot);
            (rank, position)
        });
        slots
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

    /// Publish every singleton owned by one backed enum in declaration order.
    /// Zend builds the backing lookup table before reporting a table-wide
    /// validation error, so even the failing ordinary-fetch path consumes the
    /// same object handles. `cases()` and constant expressions intentionally
    /// publish only the individual values they materialize.
    pub(crate) fn publish_backed_enum_case_handles(&self, class_id: u32) {
        let Some(class) = self.class_by_id(class_id) else {
            return;
        };
        debug_assert!(class.is_enum);
        let case_count = class.static_properties.len();
        for case_index in 0..case_count {
            if let Some(storage_slot) = self.static_property_storage_slot(class_id, case_index) {
                self.publish_static_property_object_handles(storage_slot);
            }
        }
    }

    /// Complete the deferred enum-case publication walk for one canonical
    /// static slot exactly once. Runtime writes cannot introduce an unpublished
    /// case: evaluating the assigned enum expression publishes it first.
    #[inline]
    pub(crate) fn publish_static_property_object_handles(&self, storage_slot: usize) {
        let Some(published) = self.static_property_handles_published.get(storage_slot) else {
            return;
        };
        if published.get() {
            return;
        }
        let Some(value) = self.static_property_values.get(storage_slot) else {
            return;
        };
        value.publish_deferred_object_handles();
        published.set(true);
    }

    /// Canonical PHP static-property roots for cold ownership diagnostics.
    /// Compiler defaults and inline caches are deliberately excluded.
    pub(crate) fn static_property_values(&self) -> &[Value] {
        &self.static_property_values
    }

    #[inline(always)]
    pub(crate) fn static_property_value_mut(&mut self, storage_slot: usize) -> Option<&mut Value> {
        // Indirect/reference access can publish an object without returning
        // through the ordinary static-property setter.
        self.request_static_values_may_retain_objects = true;
        self.static_property_values.get_mut(storage_slot)
    }

    /// Reference assignment replaces the property location itself. This is
    /// deliberately distinct from `set_static_property_value()`, which writes
    /// through an already referenced property for ordinary `=` assignment.
    #[inline(always)]
    pub(crate) fn rebind_static_property_value(
        &mut self,
        storage_slot: usize,
        value: Value,
    ) -> bool {
        self.note_request_static_value(&value);
        let Some(current) = self.static_property_values.get_mut(storage_slot) else {
            return false;
        };
        *current = value;
        true
    }

    /// A warmed static-property cache can skip the bounds branch: storage is
    /// append-only for the executor lifetime and cache slots are published
    /// only after checked resolution and deferred-handle publication.
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
        self.note_request_static_value(&value);
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

    #[inline(always)]
    fn note_request_static_value(&mut self, value: &Value) {
        self.request_static_values_may_retain_objects |= matches!(
            value.value_type(),
            crate::value::ValueType::Array
                | crate::value::ValueType::Object
                | crate::value::ValueType::Reference
                | crate::value::ValueType::Closure
        );
    }

    #[inline(always)]
    pub(crate) fn request_static_values_may_retain_objects(&self) -> bool {
        self.request_static_values_may_retain_objects
    }

    /// Snapshot heap-backed class-static roots for the request shutdown phase.
    /// Canonical slots remain readable while destructors run; shallow Value
    /// clones preserve container identity so later mutations become visible to
    /// the next fixed-point pass.
    #[cold]
    pub(crate) fn shutdown_class_static_values(&self) -> Vec<Value> {
        self.static_property_values
            .iter()
            .filter(|value| value.needs_cleanup())
            .cloned()
            .collect()
    }

    /// Snapshot request-defined constant roots after main-scope teardown and
    /// before class/function statics. Runtime object constants are immutable,
    /// but their canonical table entries stay visible while destructors run.
    #[cold]
    pub(crate) fn shutdown_constant_values(&self) -> Vec<Value> {
        let table = self.constant_table.borrow();
        self.constant_definition_order
            .borrow()
            .iter()
            .filter_map(|name| table.get(name))
            .filter(|value| value.needs_cleanup())
            .cloned()
            .collect()
    }

    /// Snapshot named-function static roots after class statics while leaving
    /// their canonical cells visible to reentrant destructor code.
    #[cold]
    pub(crate) fn shutdown_function_static_values(&self) -> Vec<Value> {
        self.static_vars
            .values()
            .flat_map(HashMap::values)
            .filter(|value| value.needs_cleanup())
            .cloned()
            .collect()
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
            if !class_def.is_trait
                && canonical_target.eq_ignore_ascii_case("Stringable")
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

    /// Whether this declaration itself contributes an effective __toString()
    /// method. Parent and interface relations are deliberately excluded: the
    /// ordered interface walkers visit those declarations separately.
    pub(crate) fn class_contributes_stringable(&self, class: &ClassDef) -> bool {
        class
            .methods
            .iter()
            .any(|(name, _, _, _, _)| name.eq_ignore_ascii_case("__toString"))
            || class.uses.iter().any(|trait_name| {
                self.find_class(trait_name)
                    .and_then(|trait_def| self.find_effective_method(trait_def, "__toString"))
                    .is_some()
            })
    }

    fn collect_class_interface_names(
        &self,
        owner: &str,
        names: &mut Vec<String>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        let Some(class) = self.find_class(owner) else {
            return;
        };
        if class.is_trait {
            return;
        }

        let canonical_owner = class.name.clone();
        let parent = class.parent.clone();
        let interfaces = class.implements.clone();
        let contributes_stringable = self.class_contributes_stringable(class);

        // PHP exposes interfaces inherited from the parent before this
        // declaration's own interfaces. An interface's ancestors follow the
        // interface itself in source order.
        if let Some(parent) = parent {
            self.collect_class_interface_names(&parent, names, seen);
        }
        for interface in interfaces {
            let canonical = self
                .find_class(&interface)
                .map_or(interface, |class| class.name.clone());
            if seen.insert(canonical.to_ascii_lowercase()) {
                names.push(canonical.clone());
                self.collect_class_interface_names(&canonical, names, seen);
            }
        }

        // A class or interface with an effective __toString() implicitly
        // implements Stringable. Keep this as a projected relationship: the
        // source declaration metadata remains unchanged, while Reflection and
        // class_implements() observe the same relation as instanceof/is_a().
        if !canonical_owner.eq_ignore_ascii_case("Stringable") && contributes_stringable {
            let canonical = self
                .find_class("Stringable")
                .map_or_else(|| "Stringable".to_string(), |class| class.name.clone());
            if seen.insert(canonical.to_ascii_lowercase()) {
                names.push(canonical);
            }
        }
    }

    /// Return the canonical, ordered interface projection exposed by PHP's
    /// Reflection APIs.
    pub(crate) fn class_interface_names(&self, owner: &str) -> Vec<String> {
        let mut names = Vec::new();
        self.collect_class_interface_names(
            owner,
            &mut names,
            &mut std::collections::HashSet::new(),
        );
        names
    }

    pub(crate) fn seal_internal_class_ids(&mut self) {
        self.internal_class_id_limit = self.next_class_id.saturating_sub(1);
    }

    pub(crate) fn class_is_internal(&self, class_name: &str) -> bool {
        self.find_class(class_name).is_some_and(|class| {
            class.class_id != 0 && class.class_id <= self.internal_class_id_limit
        })
    }

    #[inline(always)]
    pub(crate) fn class_id_is_internal(&self, class_id: u32) -> bool {
        class_id != 0 && class_id <= self.internal_class_id_limit
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
                registered.as_str() == class.name
                    && !self.runtime_class_link_is_active(&class.name)
                    && predicate(class.as_ref())
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
                    registered.as_str() != class.name
                        && !self.runtime_class_link_is_active(&class.name)
                        && predicate(class.as_ref())
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
        self.declared_class_like_names(|class| class.is_trait, true)
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
                    .find(|(registered, class)| {
                        registered.eq_ignore_ascii_case(name)
                            || class
                                .anonymous_public_name()
                                .is_some_and(|public| public.eq_ignore_ascii_case(name))
                    })
                    .map(|(_, class)| class.as_ref())
            })
    }

    /// Userland lookup must not observe a class whose inheritance transaction
    /// is still active. Internal linking continues to use `find_class()` so a
    /// newly loaded descendant can compose against the provisional parent.
    #[inline]
    pub(crate) fn find_public_class(&self, name: &str) -> Option<&ClassDef> {
        if self.active_runtime_class_relations.is_empty() {
            return self.find_class(name);
        }
        (!self.runtime_class_link_is_active(name))
            .then(|| self.find_class(name))
            .flatten()
    }

    /// Publish another case-insensitive name for one existing class identity.
    /// Method aliases point at the same stable functions and static/property
    /// metadata continues to use the original numeric class ID.
    pub(crate) fn register_class_alias(
        &mut self,
        original: &str,
        alias: &str,
    ) -> Result<Option<String>, ClassAliasRegistrationError> {
        let original = original.strip_prefix('\\').unwrap_or(original);
        let alias = alias.strip_prefix('\\').unwrap_or(alias);
        if self
            .class_table
            .keys()
            .any(|registered| registered.eq_ignore_ascii_case(alias))
        {
            return Err(ClassAliasRegistrationError::NameConflict);
        }
        let class = self
            .class_table
            .iter()
            .find(|(registered, class)| {
                registered.eq_ignore_ascii_case(original)
                    || class
                        .anonymous_public_name()
                        .is_some_and(|public| public.eq_ignore_ascii_case(original))
            })
            .map(|(_, class)| class.clone())
            .ok_or(ClassAliasRegistrationError::NameConflict)?;
        let aliases_interface = class.is_interface;

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
        self.retry_pending_named_classes()
            .map_err(ClassAliasRegistrationError::DelayedLink)?;
        if let Some(error) = self.interface_relation_error() {
            return Err(ClassAliasRegistrationError::DelayedLink(error));
        }
        if let Some(error) = aliases_interface
            .then(|| self.interface_method_contract_error())
            .flatten()
        {
            return Err(ClassAliasRegistrationError::DelayedLink(error));
        }
        Ok(None)
    }

    /// Top-level declarations may be linked before a later runtime
    /// `class_alias()` publishes one of their interface edges. Recheck the
    /// complete method contract in declaration order on that alias boundary.
    fn interface_method_contract_error(&self) -> Option<String> {
        (1..self.next_class_id).find_map(|class_id| {
            self.class_by_id(class_id)
                .filter(|class| class.is_interface)
                .and_then(|class| self.validate_interface_method_contracts(class).err())
        })
    }

    /// Validate only direct interface edges. PHP permits ordinary and aliased
    /// diamonds that converge on one inherited ancestor, but rejects two
    /// direct spellings that resolve to the same canonical interface identity.
    #[cold]
    fn direct_interface_relation_error(&self, class: &ClassDef) -> Option<String> {
        let location = || {
            class.source_file.as_ref().map_or_else(String::new, |file| {
                format!(" in {file} on line {}", class.declaration_line)
            })
        };
        let mut direct_interfaces =
            (class.implements.len() > 1).then(std::collections::HashSet::new);
        for name in &class.implements {
            let interface = self
                .class_table
                .get(name)
                .map(std::rc::Rc::as_ref)
                .or_else(|| {
                    // A single unresolved forward edge is not part of this
                    // checkpoint. Avoid turning its cold lookup into a second
                    // request-wide case-insensitive scan; multi-edge identity
                    // checks still need canonical alias/casing resolution.
                    (class.implements.len() > 1)
                        .then(|| self.find_class(name))
                        .flatten()
                });
            let Some(interface) = interface else {
                continue;
            };
            if !interface.is_interface {
                return Some(format!(
                    "{} cannot implement {} - it is not an interface{}",
                    class.name,
                    interface.name,
                    location()
                ));
            }
            if direct_interfaces
                .as_mut()
                .is_some_and(|interfaces| !interfaces.insert(interface.class_id))
            {
                if self
                    .generic_metadata
                    .has_distinct_direct_interface_bindings(&class.name, &interface.name)
                {
                    continue;
                }
                let kind = if class.is_interface {
                    "Interface"
                } else if class.is_enum {
                    "Enum"
                } else {
                    "Class"
                };
                return Some(format!(
                    "{kind} {} cannot implement previously implemented interface {}{}",
                    class.name,
                    interface.name,
                    location()
                ));
            }
        }
        None
    }

    /// A runtime alias may resolve a direct edge after top-level declarations
    /// were eagerly registered. Recheck declarations in stable registration
    /// order only on this explicit cold alias-publication boundary.
    fn interface_relation_error(&self) -> Option<String> {
        (1..self.next_class_id).find_map(|class_id| {
            self.class_by_id(class_id)
                .and_then(|class| self.direct_interface_relation_error(class))
        })
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
        let class_name = class_name.strip_prefix('\\').unwrap_or(class_name);
        self.class_table
            .get(class_name)
            .map(|class| class.class_id)
            .or_else(|| self.find_class(class_name).map(|class| class.class_id))
            .unwrap_or(0)
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
        // Dispatch preserves the source spelling used at the call site, while
        // PHP class identity is case-insensitive. Resolve that spelling before
        // walking the canonical inheritance/composition chain.
        let definition = self.find_class(receiver_class)?;
        self.trait_composition_scope_from_definition(definition, trait_name)
    }

    fn trait_composition_scope_from_definition<'a>(
        &'a self,
        mut definition: &'a ClassDef,
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
        self.collect_interface_methods_inner(
            iface_name,
            &mut result,
            &mut std::collections::HashSet::new(),
        );
        result
    }

    fn collect_interface_methods_inner<'a>(
        &'a self,
        iface_name: &str,
        result: &mut Vec<MethodDeclaration<'a>>,
        visited: &mut std::collections::HashSet<String>,
    ) {
        if !visited.insert(iface_name.to_ascii_lowercase()) {
            return;
        }
        if let Some(iface_def) = self.find_class(iface_name) {
            for method in &iface_def.methods {
                result.push(Self::method_declaration(iface_def, method));
            }
            for contract in self.internal_method_contracts(&iface_def.name) {
                result.push(Self::internal_method_declaration(iface_def, contract));
            }
            for parent_iface in &iface_def.implements {
                self.collect_interface_methods_inner(parent_iface, result, visited);
            }
        }
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
    #[cold]
    #[inline(never)]
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
                if self.concrete_property_implements_hook(class_def, requirement.name) {
                    continue;
                }
                let Some(implementation) = self.find_effective_method(class_def, requirement.name)
                else {
                    errors.push((requirement.owner.to_string(), requirement.name.to_string()));
                    continue;
                };
                if implementation.is_abstract {
                    if self.concrete_property_implements_hook(class_def, requirement.name) {
                        continue;
                    }
                    errors.push((requirement.owner.to_string(), requirement.name.to_string()));
                    continue;
                }
                errors.extend(
                    self.method_contract_errors(requirement, implementation, Some(class_def))
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
        if let Some(class_def) = self.find_class(class_name) {
            let class_name = class_def.name.as_str();
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
            // A directly used trait may itself publish an adapted method from
            // one of its traits. Resolve that already-linked metadata before
            // falling through to the consumer's parent hierarchy.
            for trait_name in &class_def.uses {
                if let Some((visibility, is_static, _)) =
                    self.find_method_info(trait_name, method_name)
                {
                    return Some((visibility, is_static, class_name.to_string()));
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
        if let Some(class_def) = self.find_class(class_name) {
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

    /// Look up the visibility governing writes, unsets and reference access.
    /// Ordinary properties use their read visibility; asymmetric properties
    /// retain a narrower source-level set visibility.
    pub fn find_property_set_visibility(
        &self,
        class_name: &str,
        prop_name: &str,
    ) -> Option<(Visibility, String)> {
        if let Some(class_def) = self.class_table.get(class_name) {
            for property in &class_def.properties {
                if property.name == prop_name {
                    return Some((
                        property.set_visibility.unwrap_or(property.visibility),
                        property.declaring_class.clone(),
                    ));
                }
            }
            if let Some(parent) = &class_def.parent {
                return self.find_property_set_visibility(parent, prop_name);
            }
        }
        None
    }

    pub fn property_has_asymmetric_set_visibility(
        &self,
        class_name: &str,
        prop_name: &str,
    ) -> bool {
        self.class_table.get(class_name).is_some_and(|class_def| {
            if let Some(property) = class_def
                .properties
                .iter()
                .find(|property| property.name == prop_name)
            {
                property.set_visibility.is_some()
            } else {
                class_def.parent.as_deref().is_some_and(|parent| {
                    self.property_has_asymmetric_set_visibility(parent, prop_name)
                })
            }
        })
    }

    /// Check instance-property visibility against the oldest non-private
    /// declaration in its override family. PHP scopes protected properties by
    /// that prototype, allowing sibling implementations of one abstract
    /// property to access each other without widening unrelated same-name
    /// declarations.
    pub(crate) fn check_instance_property_visibility(
        &self,
        caller_class: Option<&str>,
        receiver_class: &str,
        prop_name: &str,
        defining_class: &str,
        visibility: Visibility,
    ) -> bool {
        if self.check_visibility(caller_class, defining_class, visibility) {
            return true;
        }
        if visibility != Visibility::Protected {
            return false;
        }

        let mut current = Some(receiver_class);
        let mut prototype = None;
        while let Some(class_name) = current {
            let Some(class_def) = self.find_class(class_name) else {
                break;
            };
            let declared_here = class_def.properties.iter().find(|property| {
                property.name == prop_name
                    && (property
                        .declaring_class
                        .eq_ignore_ascii_case(&class_def.name)
                        || property.type_scope.eq_ignore_ascii_case(&class_def.name))
            });
            if let Some(property) = declared_here {
                if property.visibility == Visibility::Private {
                    break;
                }
                prototype = Some(class_def.name.as_str());
            }
            current = class_def.parent.as_deref();
        }

        prototype.is_some_and(|prototype| {
            self.check_visibility(caller_class, prototype, Visibility::Protected)
        })
    }

    /// An object's private method is selected in the caller's declaring scope
    /// before considering overrides on the actual receiver.
    #[inline]
    pub(crate) fn method_dispatch_class<'a>(
        &self,
        receiver: &'a str,
        method: &str,
        caller: Option<&'a str>,
    ) -> &'a str {
        let Some(caller) = caller else {
            return receiver;
        };
        if caller == receiver || caller.eq_ignore_ascii_case(receiver) {
            return receiver;
        }
        self.private_method_dispatch_class(receiver, method, caller)
    }

    #[cold]
    #[inline(never)]
    fn private_method_dispatch_class<'a>(
        &self,
        receiver: &'a str,
        method: &str,
        caller: &'a str,
    ) -> &'a str {
        if let Some((Visibility::Private, defining)) = self.find_method_visibility(caller, method)
            && defining.eq_ignore_ascii_case(caller)
            && self.class_is_a(receiver, caller)
        {
            return caller;
        }
        receiver
    }

    /// Check protected method access against the oldest non-private
    /// declaration in the receiver's override family. Trait implementations
    /// participate as declarations of their consumer, while an abstract trait
    /// requirement satisfied by a parent leaves that inherited prototype
    /// intact.
    #[inline(always)]
    pub(crate) fn check_method_visibility(
        &self,
        caller_class: Option<&str>,
        receiver_class: &str,
        method_name: &str,
        defining_class: &str,
        visibility: Visibility,
    ) -> bool {
        match visibility {
            Visibility::Public => true,
            Visibility::Private => caller_class.is_some_and(|caller| {
                caller == defining_class || caller.eq_ignore_ascii_case(defining_class)
            }),
            Visibility::Protected => self.check_protected_method_visibility(
                caller_class,
                receiver_class,
                method_name,
                defining_class,
            ),
        }
    }

    #[cold]
    #[inline(never)]
    fn check_protected_method_visibility(
        &self,
        caller_class: Option<&str>,
        receiver_class: &str,
        method_name: &str,
        defining_class: &str,
    ) -> bool {
        if self.check_visibility(caller_class, defining_class, Visibility::Protected) {
            return true;
        }
        if method_name.eq_ignore_ascii_case("__construct") {
            return false;
        }

        let mut current = Some(receiver_class);
        let mut prototype = None;
        while let Some(class_name) = current {
            let Some(class_def) = self.find_class(class_name) else {
                break;
            };
            let declared_here = class_def
                .methods
                .iter()
                .find(|method| {
                    method.0.eq_ignore_ascii_case(method_name)
                        && !class_def.method_is_abstract(&method.0)
                })
                .map(|method| method.1)
                .or_else(|| {
                    self.effective_composed_trait_methods(class_def)
                        .into_iter()
                        .find(|method| method.target.eq_ignore_ascii_case(method_name))
                        .map(|method| method.visibility)
                });
            if let Some(declaration_visibility) = declared_here {
                if declaration_visibility == Visibility::Private {
                    break;
                }
                prototype = Some(class_def.name.as_str());
            }
            current = class_def.parent.as_deref();
        }

        prototype.is_some_and(|prototype| {
            self.check_visibility(caller_class, prototype, Visibility::Protected)
        })
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
        if name.starts_with('\0') {
            if self
                .private_function_table
                .insert(name.to_string(), func)
                .is_some()
            {
                return Err("Cannot redeclare private compiler function".to_string());
            }
            return Ok(());
        }
        let key = name.to_lowercase();
        if let Some(alias) = crate::builtin_metadata::internal_function_alias(&key)
            && let Some(&previous) = self.function_table.get(alias.target)
        {
            return Err(Self::function_redeclaration_error(previous, func, name));
        }
        if let Some(&previous) = self.function_table.get(&key) {
            return Err(Self::function_redeclaration_error(previous, func, name));
        }
        self.function_table.insert(key, func);
        Ok(())
    }

    pub(crate) fn find_private_function(&self, name: &str) -> Option<*const FunctionCommon> {
        name.starts_with('\0')
            .then(|| self.private_function_table.get(name).copied())
            .flatten()
    }

    #[cold]
    fn source_location(query: SourceLocationQuery) -> Option<(String, usize)> {
        // SAFETY: declaration pointers come from live executor-owned function
        // tables or registration candidates. CurrentOutput comes from the
        // synchronous active VM stack, whose complete predecessor chain stays
        // live during an output write. In both cases FunctionType selects the
        // enclosing descriptor before user OpArray metadata is accessed.
        unsafe {
            match query {
                SourceLocationQuery::Declaration(function) => {
                    if function.is_null() {
                        return None;
                    }
                    Function::from_common_ptr(function).dispatch(
                        |user| {
                            user.op_array
                                .declaration_line()
                                .filter(|_| !user.op_array.source_file.is_empty())
                                .map(|line| (user.op_array.source_file.to_string(), line))
                        },
                        |_| None,
                    )
                }
                SourceLocationQuery::CurrentOutput(mut frame) => {
                    let mut below_internal = false;
                    for _ in 0..64 {
                        if frame.is_null() || (*frame).func.is_null() {
                            return None;
                        }
                        let function = Function::from_common_ptr((*frame).func);
                        if function.fn_type() == FunctionType::User {
                            let op_array = &function.as_user().op_array;
                            if op_array.instructions.is_empty() {
                                return None;
                            }
                            let offset =
                                (*frame).opline.offset_from(op_array.instructions.as_ptr());
                            let mut instruction = usize::try_from(offset)
                                .ok()?
                                .min(op_array.instructions.len());
                            if below_internal {
                                instruction = instruction.saturating_sub(1);
                            }
                            instruction = instruction.min(op_array.instructions.len() - 1);
                            let line = op_array.source_line(instruction).or_else(|| {
                                op_array
                                    .source_lines
                                    .iter()
                                    .rev()
                                    .find(|(index, _)| *index <= instruction as u32)
                                    .map(|(_, line)| *line as usize)
                            })?;
                            let file = if op_array.source_file.is_empty() {
                                op_array.name.clone()
                            } else {
                                op_array.source_file.to_string()
                            };
                            return (!file.is_empty() && line != 0).then_some((file, line));
                        }
                        below_internal = true;
                        frame = (*frame).prev_execute_data;
                    }
                    None
                }
            }
        }
    }

    #[cold]
    fn function_declaration_location(function: *const FunctionCommon) -> Option<(String, usize)> {
        Self::source_location(SourceLocationQuery::Declaration(function))
    }

    #[cold]
    #[inline(never)]
    fn function_redeclaration_error(
        previous: *const FunctionCommon,
        current: *const FunctionCommon,
        current_name: &str,
    ) -> String {
        let previous_location = Self::function_declaration_location(previous)
            .map_or_else(String::new, |(file, line)| {
                format!(" (previously declared in {file}:{line})")
            });
        let current_location = Self::function_declaration_location(current)
            .map_or_else(String::new, |(file, line)| {
                format!(" in {file} on line {line}")
            });
        format!("Cannot redeclare function {current_name}(){previous_location}{current_location}")
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
            let found = self
                .function_table
                .get(&lower)
                .copied()
                .or_else(|| self.find_inherited_function(&lower));
            if found.is_some() {
                stats::inc_find_function_lower_hit();
            } else {
                stats::inc_find_function_miss();
            }
            found
        } else {
            let found = self.find_inherited_function(name);
            if found.is_none() {
                stats::inc_find_function_miss();
            }
            found
        }
    }

    /// Recover immutable metadata for a pointer retained by the function
    /// table. Reflection uses this checked cold boundary instead of
    /// dereferencing raw lookup results at each call site.
    pub(crate) fn registered_function_common(
        &self,
        function: *const FunctionCommon,
    ) -> Option<&FunctionCommon> {
        if function.is_null()
            || !self
                .function_table
                .values()
                .any(|candidate| std::ptr::eq(*candidate, function))
        {
            return None;
        }
        // SAFETY: membership above proves that this is a non-null pointer
        // retained by ExecutorGlobals for at least the returned borrow.
        Some(unsafe { &*function })
    }

    /// Eager top-level registration can observe a parent name before a
    /// preceding runtime `class_alias()` publishes it. Ordinary inheritance
    /// is flattened into `function_table`; on that rare miss, follow the now
    /// resolvable parent chain without adding work to exact-hit dispatch.
    #[cold]
    fn find_inherited_function(&self, name: &str) -> Option<*const FunctionCommon> {
        if let Some(alias) = crate::builtin_metadata::internal_function_alias(name) {
            return self.function_table.get(alias.target).copied();
        }
        let (class_name, method) = name.split_once("::")?;
        let mut class = self.find_class(class_name)?;
        for _ in 0..self.class_table.len() {
            let parent_name = class.parent.as_deref()?;
            if let Some(function) = self
                .function_table
                .get(&format!("{}::{method}", parent_name.to_ascii_lowercase()))
            {
                return Some(*function);
            }
            class = self.find_class(parent_name)?;
        }
        None
    }

    /// Define a constant. Returns error if already defined.
    pub fn define_constant(
        &mut self,
        name: &str,
        value: crate::value::Value,
    ) -> Result<(), String> {
        if self.find_constant(name).is_some() {
            return Err(constant_redefinition_message(name));
        }
        self.note_request_static_value(&value);
        let mut table = self.constant_table.borrow_mut();
        let name: Rc<str> = Rc::from(name);
        table.insert(name.clone(), value);
        self.constant_definition_order.borrow_mut().push(name);
        Ok(())
    }

    pub(crate) fn defined_dynamic_constants(&self) -> Vec<(String, crate::value::Value)> {
        let table = self.constant_table.borrow();
        self.constant_definition_order
            .borrow()
            .iter()
            .filter_map(|name| {
                table
                    .get(name)
                    .cloned()
                    .map(|value| (name.to_string(), value))
            })
            .collect()
    }

    /// Look up a constant by name (case-sensitive).
    #[inline]
    pub fn find_constant(&self, name: &str) -> Option<crate::value::Value> {
        let table = self.constant_table.borrow();
        if let Some(val) = table.get(name).cloned() {
            return Some(val);
        }
        drop(table);
        self.find_constant_slow(name)
    }

    /// Keep namespace-case fallback and the builtin inventory out of the
    /// request-local exact-hit path used by source constants and `constant()`.
    #[cold]
    #[inline(never)]
    fn find_constant_slow(&self, name: &str) -> Option<crate::value::Value> {
        if name.contains('\\')
            && let Some(value) =
                self.constant_table
                    .borrow()
                    .iter()
                    .find_map(|(registered, value)| {
                        qualified_constant_name_matches(registered, name).then(|| value.clone())
                    })
        {
            return Some(value);
        }
        // Built-in PHP constants (shared source of truth)
        crate::builtin_constant(name)
    }

    pub fn register_compiler_halt_offset(&mut self, source_file: String, offset: i64) {
        self.compiler_halt_offsets
            .get_or_insert_with(|| Box::new(HashMap::new()))
            .entry(source_file)
            .or_insert(offset);
    }

    pub fn compiler_halt_offset(&self, source_file: &str) -> Option<i64> {
        self.compiler_halt_offsets
            .as_deref()
            .and_then(|offsets| offsets.get(source_file))
            .copied()
    }

    /// Whether reading this source constant can emit a Deprecated diagnostic.
    /// Kept cold behind an opcode-local generation cache so ordinary constant
    /// loops do not repeat metadata hash lookups.
    pub(crate) fn constant_requires_deprecated_use_check(&self, name: &str) -> bool {
        self.constant_expressions.contains_key(name)
            || self
                .constant_attributes
                .get(name)
                .is_some_and(|attributes| {
                    attributes
                        .iter()
                        .any(|attribute| attribute.name.eq_ignore_ascii_case("Deprecated"))
                })
    }

    pub(crate) fn bump_constant_deprecation_generation(&mut self) {
        self.constant_deprecation_generation = self.constant_deprecation_generation.wrapping_add(1);
        if self.constant_deprecation_generation == 0 {
            self.constant_deprecation_generation = 1;
        }
    }

    pub fn refresh_constant_deprecation_metadata_presence(&mut self) {
        if !self.constant_deprecation_metadata_present {
            self.constant_deprecation_metadata_present = !self.constant_expressions.is_empty()
                || self.constant_attributes.values().any(|attributes| {
                    attributes
                        .iter()
                        .any(|attribute| attribute.name.eq_ignore_ascii_case("Deprecated"))
                });
        }
    }

    fn register_deferred_class_constant_activation(&mut self, class_id: u32) {
        let activations = self
            .deferred_class_constant_activations
            .get_or_insert_with(|| Box::new(Vec::new()));
        let class_id = class_id as usize;
        if activations.len() <= class_id {
            activations.resize(class_id + 1, 0);
        }
        activations[class_id] = 1;
    }

    #[inline]
    pub(crate) fn deferred_class_constants_require_activation(&self, class_id: u32) -> bool {
        self.deferred_class_constant_activations
            .as_deref()
            .and_then(|activations| activations.get(class_id as usize))
            .is_some_and(|state| *state == 1)
    }

    pub(crate) fn complete_deferred_class_constant_activation(&mut self, class_id: u32) {
        if let Some(state) = self
            .deferred_class_constant_activations
            .as_deref_mut()
            .and_then(|activations| activations.get_mut(class_id as usize))
        {
            *state = 2;
        }
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
        self.class_is_a_while_linking_inner(
            class_name,
            target,
            linking_class,
            &mut std::collections::HashSet::new(),
        )
    }

    fn class_is_a_while_linking_inner(
        &self,
        class_name: &str,
        target: &str,
        linking_class: Option<&ClassDef>,
        visited: &mut std::collections::HashSet<String>,
    ) -> bool {
        let canonical_target = self
            .find_class(target)
            .map_or(target, |class| class.name.as_str());
        let canonical_class = self
            .find_class(class_name)
            .map_or(class_name, |class| class.name.as_str());
        if canonical_class.eq_ignore_ascii_case(canonical_target) {
            return true;
        }
        if !visited.insert(canonical_class.to_ascii_lowercase()) {
            return false;
        }

        if let Some(class_def) = self.find_class(class_name) {
            return (canonical_target.eq_ignore_ascii_case("Stringable")
                && self
                    .find_effective_method(class_def, "__toString")
                    .is_some())
                || class_def.parent.as_deref().is_some_and(|parent| {
                    self.class_is_a_while_linking_inner(
                        parent,
                        canonical_target,
                        linking_class,
                        visited,
                    )
                })
                || class_def.implements.iter().any(|interface| {
                    self.class_is_a_while_linking_inner(
                        interface,
                        canonical_target,
                        linking_class,
                        visited,
                    )
                });
        }

        if let Some(linking_class) =
            linking_class.filter(|definition| definition.name.eq_ignore_ascii_case(class_name))
        {
            return (canonical_target.eq_ignore_ascii_case("Stringable")
                && linking_class
                    .methods
                    .iter()
                    .any(|(name, _, _, _, _)| name.eq_ignore_ascii_case("__toString")))
                || linking_class.parent.as_deref().is_some_and(|parent| {
                    self.class_is_a_while_linking_inner(
                        parent,
                        target,
                        Some(linking_class),
                        visited,
                    )
                })
                || linking_class.implements.iter().any(|interface| {
                    self.class_is_a_while_linking_inner(
                        interface,
                        target,
                        Some(linking_class),
                        visited,
                    )
                });
        }

        let Some(active) = self
            .active_runtime_class_relations
            .get(&class_name.to_ascii_lowercase())
        else {
            return false;
        };
        let compatible = (canonical_target.eq_ignore_ascii_case("Stringable")
            && active.has_to_string)
            || active.parent.as_deref().is_some_and(|parent| {
                self.class_is_a_while_linking_inner(parent, target, linking_class, visited)
            })
            || active.implements.iter().any(|interface| {
                self.class_is_a_while_linking_inner(interface, target, linking_class, visited)
            });
        if compatible {
            active.has_variance_dependents.set(true);
        }
        compatible
    }

    fn variance_class_is_known(&self, class_name: &str, linking_class: Option<&ClassDef>) -> bool {
        linking_class.is_some_and(|definition| definition.name.eq_ignore_ascii_case(class_name))
            || self.find_class(class_name).is_some()
            || self.runtime_class_link_is_active(class_name)
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
        self.is_return_type_compatible_mode(
            impl_hint,
            iface_hint,
            impl_owner,
            iface_owner,
            linking_class,
            true,
            false,
        )
    }

    fn is_return_type_potentially_compatible(
        &self,
        impl_hint: &crate::vm::function::ParamTypeHint,
        iface_hint: &crate::vm::function::ParamTypeHint,
        impl_owner: &str,
        iface_owner: &str,
        linking_class: Option<&ClassDef>,
    ) -> bool {
        self.is_return_type_compatible_mode(
            impl_hint,
            iface_hint,
            impl_owner,
            iface_owner,
            linking_class,
            true,
            true,
        )
    }

    fn is_return_type_compatible_mode(
        &self,
        impl_hint: &crate::vm::function::ParamTypeHint,
        iface_hint: &crate::vm::function::ParamTypeHint,
        impl_owner: &str,
        iface_owner: &str,
        linking_class: Option<&ClassDef>,
        allow_unresolved_relation: bool,
        allow_any_unresolved_relation: bool,
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

        // Closure is the concrete object subtype of PHP's callable type.
        if matches!(iface_hint, ParamTypeHint::Callable)
            && matches!(impl_hint, ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("Closure"))
        {
            return true;
        }

        // Exact match
        if impl_hint == iface_hint {
            return true;
        }

        // The runtime representation retains source-level `T|null` as a union
        // while `?T` is one nullable node. For covariance they denote the same
        // two branches: both T and null must fit the required union.
        if let (ParamTypeHint::Nullable(inner_impl), ParamTypeHint::Union(_)) =
            (impl_hint, iface_hint)
        {
            return iface_hint.allows_null()
                && (matches!(inner_impl.as_ref(), ParamTypeHint::None)
                    || self.is_return_type_compatible_mode(
                        inner_impl,
                        iface_hint,
                        impl_owner,
                        iface_owner,
                        linking_class,
                        allow_unresolved_relation,
                        allow_any_unresolved_relation,
                    ));
        }

        // Nullable unwrapping: impl T is compatible with iface ?T (narrowing)
        // impl ?T is compatible with iface ?T (checked above by equality)
        match (impl_hint, iface_hint) {
            (ParamTypeHint::Nullable(inner_impl), ParamTypeHint::Nullable(inner_iface)) => {
                return self.is_return_type_compatible_mode(
                    inner_impl,
                    inner_iface,
                    impl_owner,
                    iface_owner,
                    linking_class,
                    allow_unresolved_relation,
                    allow_any_unresolved_relation,
                );
            }
            (_, ParamTypeHint::Nullable(inner_iface)) => {
                // impl_hint (non-nullable or differently nullable) vs ?T
                // Check if impl is compatible with the inner type
                return self.is_return_type_compatible_mode(
                    impl_hint,
                    inner_iface,
                    impl_owner,
                    iface_owner,
                    linking_class,
                    allow_unresolved_relation,
                    allow_any_unresolved_relation,
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
            return self.is_return_type_compatible_mode(
                &ParamTypeHint::Array,
                iface_hint,
                impl_owner,
                iface_owner,
                linking_class,
                allow_unresolved_relation,
                allow_any_unresolved_relation,
            ) && self.is_return_type_compatible_mode(
                &traversable,
                iface_hint,
                impl_owner,
                iface_owner,
                linking_class,
                allow_unresolved_relation,
                allow_any_unresolved_relation,
            );
        }
        if matches!(iface_hint, ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("iterable"))
        {
            let traversable = ParamTypeHint::ClassName("Traversable".to_string());
            return self.is_return_type_compatible_mode(
                impl_hint,
                &ParamTypeHint::Array,
                impl_owner,
                iface_owner,
                linking_class,
                allow_unresolved_relation,
                allow_any_unresolved_relation,
            ) || self.is_return_type_compatible_mode(
                impl_hint,
                &traversable,
                impl_owner,
                iface_owner,
                linking_class,
                allow_unresolved_relation,
                allow_any_unresolved_relation,
            );
        }

        // Covariant return compatibility is ordinary subtype checking over
        // union/intersection nodes.
        if let ParamTypeHint::Intersection(iface_parts) = iface_hint {
            return iface_parts.iter().all(|part| {
                self.is_return_type_compatible_mode(
                    impl_hint,
                    part,
                    impl_owner,
                    iface_owner,
                    linking_class,
                    allow_unresolved_relation,
                    allow_any_unresolved_relation,
                )
            });
        }
        if let ParamTypeHint::Union(impl_parts) = impl_hint {
            return impl_parts.iter().all(|part| {
                self.is_return_type_compatible_mode(
                    part,
                    iface_hint,
                    impl_owner,
                    iface_owner,
                    linking_class,
                    allow_unresolved_relation,
                    allow_any_unresolved_relation,
                )
            });
        }
        if let ParamTypeHint::Union(iface_parts) = iface_hint {
            return iface_parts.iter().any(|part| {
                self.is_return_type_compatible_mode(
                    impl_hint,
                    part,
                    impl_owner,
                    iface_owner,
                    linking_class,
                    allow_unresolved_relation,
                    allow_any_unresolved_relation,
                )
            });
        }
        if let ParamTypeHint::Intersection(impl_parts) = impl_hint {
            return impl_parts.iter().any(|part| {
                self.is_return_type_compatible_mode(
                    part,
                    iface_hint,
                    impl_owner,
                    iface_owner,
                    linking_class,
                    allow_unresolved_relation,
                    allow_any_unresolved_relation,
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
                return impl_class.eq_ignore_ascii_case("static")
                    || self.variance_class_is_known(impl_class, linking_class)
                    || allow_any_unresolved_relation;
            }
            // `static` remains late-bound in a return declaration. Replacing
            // it with the implementation class is therefore safe only when
            // that class is final and cannot acquire a later called scope.
            if iface_class.eq_ignore_ascii_case("static") {
                return impl_class.eq_ignore_ascii_case("static")
                    || linking_class.is_some_and(|definition| {
                        definition.is_final && impl_class.eq_ignore_ascii_case(&definition.name)
                    });
            }
            if allow_any_unresolved_relation
                && (!self.variance_class_is_known(impl_class, linking_class)
                    || !self.variance_class_is_known(iface_class, linking_class))
            {
                return true;
            }
            if impl_class.eq_ignore_ascii_case("static") {
                return if allow_unresolved_relation {
                    self.variance_class_is_a(impl_owner, iface_class, linking_class)
                } else {
                    self.class_is_a_while_linking(impl_owner, iface_class, linking_class)
                };
            }
            return if allow_unresolved_relation {
                self.variance_class_is_a(impl_class, iface_class, linking_class)
            } else {
                self.class_is_a_while_linking(impl_class, iface_class, linking_class)
            };
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
        self.is_param_type_compatible_mode(
            impl_hint,
            iface_hint,
            impl_owner,
            iface_owner,
            linking_class,
            true,
            false,
        )
    }

    /// Property setter declarations need a conclusive relation: unlike an
    /// ordinary method contract that may be revisited after an alias appears,
    /// two distinct unresolved class names do not establish contravariance.
    fn is_param_type_compatible_strict(
        &self,
        impl_hint: &crate::vm::function::ParamTypeHint,
        iface_hint: &crate::vm::function::ParamTypeHint,
        impl_owner: &str,
        iface_owner: &str,
        linking_class: Option<&ClassDef>,
    ) -> bool {
        self.is_param_type_compatible_mode(
            impl_hint,
            iface_hint,
            impl_owner,
            iface_owner,
            linking_class,
            false,
            false,
        )
    }

    /// Determine whether publishing currently unknown class-like declarations
    /// could make a setter relation valid. This is intentionally broader than
    /// ordinary method linking, because either side may be declared later in
    /// the same source unit or supplied independently by an autoloader.
    fn is_param_type_potentially_compatible(
        &self,
        impl_hint: &crate::vm::function::ParamTypeHint,
        iface_hint: &crate::vm::function::ParamTypeHint,
        impl_owner: &str,
        iface_owner: &str,
        linking_class: Option<&ClassDef>,
    ) -> bool {
        self.is_param_type_compatible_mode(
            impl_hint,
            iface_hint,
            impl_owner,
            iface_owner,
            linking_class,
            true,
            true,
        )
    }

    fn is_param_type_compatible_mode(
        &self,
        impl_hint: &crate::vm::function::ParamTypeHint,
        iface_hint: &crate::vm::function::ParamTypeHint,
        impl_owner: &str,
        iface_owner: &str,
        linking_class: Option<&ClassDef>,
        allow_unresolved_relation: bool,
        allow_any_unresolved_relation: bool,
    ) -> bool {
        use crate::vm::function::ParamTypeHint;

        // Mixed accepts anything — always compatible
        if matches!(impl_hint, ParamTypeHint::Mixed) {
            return true;
        }

        // Parameter variance reverses the relation: an implementation that
        // accepts callable is wider than a declaration restricted to Closure.
        if matches!(impl_hint, ParamTypeHint::Callable)
            && matches!(iface_hint, ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("Closure"))
        {
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
            (ParamTypeHint::Nullable(inner_impl), ParamTypeHint::Nullable(inner_iface)) => {
                return self.is_param_type_compatible_mode(
                    inner_impl,
                    inner_iface,
                    impl_owner,
                    iface_owner,
                    linking_class,
                    allow_unresolved_relation,
                    allow_any_unresolved_relation,
                );
            }
            (ParamTypeHint::Nullable(inner_impl), _) => {
                // ?T in impl vs T in iface — impl accepts more, check inner
                return self.is_param_type_compatible_mode(
                    inner_impl,
                    iface_hint,
                    impl_owner,
                    iface_owner,
                    linking_class,
                    allow_unresolved_relation,
                    allow_any_unresolved_relation,
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
            return self.is_param_type_compatible_mode(
                &ParamTypeHint::Array,
                iface_hint,
                impl_owner,
                iface_owner,
                linking_class,
                allow_unresolved_relation,
                allow_any_unresolved_relation,
            ) || self.is_param_type_compatible_mode(
                &traversable,
                iface_hint,
                impl_owner,
                iface_owner,
                linking_class,
                allow_unresolved_relation,
                allow_any_unresolved_relation,
            );
        }
        if matches!(iface_hint, ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("iterable"))
        {
            let traversable = ParamTypeHint::ClassName("Traversable".to_string());
            return self.is_param_type_compatible_mode(
                impl_hint,
                &ParamTypeHint::Array,
                impl_owner,
                iface_owner,
                linking_class,
                allow_unresolved_relation,
                allow_any_unresolved_relation,
            ) && self.is_param_type_compatible_mode(
                impl_hint,
                &traversable,
                impl_owner,
                iface_owner,
                linking_class,
                allow_unresolved_relation,
                allow_any_unresolved_relation,
            );
        }

        // Parameter compatibility reverses the subtype relation: the
        // implementation must accept every value admitted by the interface.
        if let ParamTypeHint::Intersection(impl_parts) = impl_hint {
            return impl_parts.iter().all(|part| {
                self.is_param_type_compatible_mode(
                    part,
                    iface_hint,
                    impl_owner,
                    iface_owner,
                    linking_class,
                    allow_unresolved_relation,
                    allow_any_unresolved_relation,
                )
            });
        }
        if let ParamTypeHint::Union(iface_parts) = iface_hint {
            return iface_parts.iter().all(|part| {
                self.is_param_type_compatible_mode(
                    impl_hint,
                    part,
                    impl_owner,
                    iface_owner,
                    linking_class,
                    allow_unresolved_relation,
                    allow_any_unresolved_relation,
                )
            });
        }
        if let ParamTypeHint::Union(impl_parts) = impl_hint {
            return impl_parts.iter().any(|part| {
                self.is_param_type_compatible_mode(
                    part,
                    iface_hint,
                    impl_owner,
                    iface_owner,
                    linking_class,
                    allow_unresolved_relation,
                    allow_any_unresolved_relation,
                )
            });
        }
        if let ParamTypeHint::Intersection(iface_parts) = iface_hint {
            return iface_parts.iter().any(|part| {
                self.is_param_type_compatible_mode(
                    impl_hint,
                    part,
                    impl_owner,
                    iface_owner,
                    linking_class,
                    allow_unresolved_relation,
                    allow_any_unresolved_relation,
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
                    return iface_class.eq_ignore_ascii_case("static")
                        || self.variance_class_is_known(iface_class, linking_class)
                        || allow_any_unresolved_relation;
                }
                return if allow_any_unresolved_relation
                    && (!self.variance_class_is_known(iface_class, linking_class)
                        || !self.variance_class_is_known(impl_class, linking_class))
                {
                    true
                } else if allow_unresolved_relation {
                    self.variance_class_is_a(iface_class, impl_class, linking_class)
                } else {
                    self.class_is_a_while_linking(iface_class, impl_class, linking_class)
                };
            }
            _ => {}
        }

        false
    }

    /// Resolve the PHP source site which caused an underlying output write.
    /// Direct `echo` executes in a user frame at its current opcode; output
    /// produced by an internal function observes the suspended user caller's
    /// preceding call opcode instead.
    #[cold]
    #[inline(never)]
    fn current_output_origin(&self) -> Option<(String, usize)> {
        Self::source_location(SourceLocationQuery::CurrentOutput(
            self.current_execute_data.get(),
        ))
    }

    #[cold]
    #[inline(never)]
    fn record_first_output(&self) {
        debug_assert!(!self.headers_sent.get());
        self.headers_sent.set(true);
        self.header_output_origin
            .replace(self.current_output_origin());
    }

    #[inline]
    pub(crate) fn headers_sent(&self) -> bool {
        self.headers_sent.get()
    }

    /// Return PHP's request-local first-output origin. Callers only materialize
    /// the owned filename when one of `headers_sent()`'s by-reference outputs
    /// was actually supplied.
    pub(crate) fn header_output_origin(&self) -> (String, usize) {
        let origin = self.header_output_origin.borrow();
        origin
            .as_ref()
            .map(|(file, line)| (file.clone(), *line))
            .unwrap_or_else(|| (String::new(), 0))
    }

    /// Replace the deprecated request-local libxml switch and return its prior
    /// value, matching `libxml_disable_entity_loader()`'s historical API.
    pub(crate) fn replace_libxml_entity_loader_disabled(&self, disabled: bool) -> bool {
        self.libxml_entity_loader_disabled.replace(disabled)
    }

    pub fn write_output(&self, data: &[u8]) {
        if let Some(buffer) = self.output_buffers.borrow_mut().last_mut() {
            buffer.data.extend_from_slice(data);
            return;
        }
        if !data.is_empty() && !self.headers_sent.get() {
            self.record_first_output();
        }
        self.output.borrow_mut().write_all(data).unwrap();
    }

    /// Flush the active request output sink. PHP's `flush()` does not bypass
    /// user-level output buffers; it only asks the underlying SAPI/output
    /// transport to publish bytes already handed to it.
    pub fn flush_output(&self) {
        let _ = self.output.borrow_mut().flush();
    }

    pub(crate) fn push_output_buffer(&self, handler: Option<Value>, flags: i64) {
        self.output_buffers.borrow_mut().push(OutputBuffer {
            data: Vec::new(),
            handler,
            flags,
            started: false,
        });
    }

    pub(crate) fn pop_output_buffer(&self) -> Option<OutputBuffer> {
        self.output_buffers.borrow_mut().pop()
    }

    pub(crate) fn restore_output_buffer(&self, buffer: OutputBuffer) {
        self.output_buffers.borrow_mut().push(buffer);
    }

    pub(crate) fn output_buffer_level(&self) -> usize {
        self.output_buffers.borrow().len()
    }

    pub(crate) fn output_buffer_contents(&self) -> Option<Vec<u8>> {
        self.output_buffers
            .borrow()
            .last()
            .map(|buffer| buffer.data.clone())
    }

    pub(crate) fn enter_output_handler(&self) -> usize {
        let previous = self.output_handler_depth.get();
        self.output_handler_depth.set(previous.saturating_add(1));
        previous
    }

    pub(crate) fn leave_output_handler(&self, previous: usize) {
        debug_assert_eq!(self.output_handler_depth.get(), previous + 1);
        self.output_handler_depth.set(previous);
    }

    pub(crate) fn is_output_handler_active(&self) -> bool {
        self.output_handler_depth.get() != 0
    }
}

pub(crate) fn constant_redefinition_message(name: &str) -> String {
    format!("Constant {name} already defined, this will be an error in PHP 9")
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
            eg.static_property_slots_by_class.capacity(),
            eg.static_property_values.capacity(),
            eg.static_property_handles_published.capacity(),
        );

        let functions = crate::stdlib::register_stdlib(&mut eg);

        assert_eq!(
            (
                eg.function_table.capacity(),
                eg.class_table.capacity(),
                eg.method_declaring_class.capacity(),
                eg.class_by_id.capacity(),
                eg.static_property_slots_by_class.capacity(),
                eg.static_property_values.capacity(),
                eg.static_property_handles_published.capacity(),
            ),
            capacities,
            "fixed stdlib registration must not grow a reserved registry"
        );
        assert!(!functions.is_empty());
    }

    #[test]
    fn suppressed_reporting_restores_only_fatal_only_changes() {
        let mut eg = ExecutorGlobals::new();

        eg.begin_error_suppression(1);
        assert_eq!(eg.error_reporting, 4_437);
        eg.set_error_reporting(0);
        eg.end_error_suppression(1);
        assert_eq!(eg.error_reporting, crate::PHP_E_ALL);

        eg.begin_error_suppression(2);
        eg.set_error_reporting(8);
        eg.end_error_suppression(2);
        assert_eq!(eg.error_reporting, 8);

        eg.begin_error_suppression(3);
        assert_eq!(eg.error_reporting, 0);
        eg.set_error_reporting(crate::PHP_E_ALL);
        eg.end_error_suppression(3);
        assert_eq!(eg.error_reporting, crate::PHP_E_ALL);
    }
}

#[cfg(feature = "coroutines")]
pub mod coroutine;
