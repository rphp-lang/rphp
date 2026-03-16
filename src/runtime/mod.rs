use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::io::Write;

use crate::vm::stack::VmStack;
use crate::vm::frame::ExecuteData;
use crate::vm::function::FunctionCommon;
use crate::compiler::compile::ClassDef;
use crate::parser::Visibility;

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
        if let Some((Visibility::Private, defining_class)) = eg.find_property_visibility(caller, prop_name) {
            if defining_class.eq_ignore_ascii_case(caller) {
                return mangle_private_prop(&defining_class, prop_name);
            }
        }
    }
    // Otherwise, check if the property is private in the target class hierarchy
    if let Some((Visibility::Private, defining_class)) = eg.find_property_visibility(obj_class, prop_name) {
        return mangle_private_prop(&defining_class, prop_name);
    }
    prop_name.to_string()
}

/// Minimal ExecutorGlobals for vertical slice.
/// Will grow as we implement more features.
pub struct ExecutorGlobals {
    pub vm_stack: VmStack,
    pub current_execute_data: Cell<*mut ExecuteData>,
    pub vm_interrupt: AtomicBool,
    pub timed_out: AtomicBool,
    /// Function table — name → pointer to FunctionCommon
    pub function_table: HashMap<String, *const FunctionCommon>,
    /// Class table — name → ClassDef (Boxed for stable pointer addresses)
    pub class_table: HashMap<String, Box<ClassDef>>,
    /// Constant table — name → Value (case-sensitive, like PHP)
    /// Uses RefCell to allow define() from internal functions (which receive &self).
    pub constant_table: std::cell::RefCell<HashMap<String, crate::value::Value>>,
    /// Exception being thrown — None = no exception
    pub exception: Option<crate::value::Value>,
    /// Reverse map: func_ptr → declaring class name (for visibility scope resolution)
    pub method_declaring_class: HashMap<*const FunctionCommon, String>,
    /// Output buffer — collected output for testing, or stdout
    output: std::cell::RefCell<Box<dyn Write>>,
}

impl ExecutorGlobals {
    pub fn new() -> Self {
        Self {
            vm_stack: VmStack::new(),
            current_execute_data: Cell::new(std::ptr::null_mut()),
            vm_interrupt: AtomicBool::new(false),
            timed_out: AtomicBool::new(false),
            function_table: HashMap::new(),
            class_table: HashMap::new(),
            constant_table: std::cell::RefCell::new(HashMap::new()),
            exception: None,
            method_declaring_class: HashMap::new(),

            output: std::cell::RefCell::new(Box::new(std::io::stdout())),
        }
    }

    /// Create EG with captured output (for testing)
    pub fn with_output(output: Box<dyn Write>) -> Self {
        Self {
            vm_stack: VmStack::new(),
            current_execute_data: Cell::new(std::ptr::null_mut()),
            vm_interrupt: AtomicBool::new(false),
            timed_out: AtomicBool::new(false),
            function_table: HashMap::new(),
            class_table: HashMap::new(),
            constant_table: std::cell::RefCell::new(HashMap::new()),
            exception: None,
            method_declaring_class: HashMap::new(),

            output: std::cell::RefCell::new(output),
        }
    }

    /// Register a class definition and its methods in the function table.
    /// Resolves inheritance: merges parent properties/methods into child.
    /// For non-interface, non-abstract classes: validates interface contracts.
    pub fn register_class(&mut self, mut class_def: ClassDef) -> Result<(), String> {
        let class_name = class_def.name.clone();

        // Resolve inheritance — merge parent's properties and methods
        if let Some(parent_name) = &class_def.parent {
            if let Some(parent) = self.class_table.get(parent_name.as_str()) {
                // Inherit properties: child's own props first, then parent's.
                // This ensures find_property_visibility finds the class's own
                // declaration before inherited ones with the same name.
                // Private properties with the same name in parent and child are BOTH kept
                // (they occupy separate mangled slots). For public/protected, child overrides.
                let child_prop_names: std::collections::HashSet<&str> = class_def.properties
                    .iter().map(|(n, _, _, _)| n.as_str()).collect();
                let mut parent_props = Vec::new();
                for (name, default, vis, declaring) in &parent.properties {
                    if child_prop_names.contains(name.as_str()) {
                        // Child has same name — only keep parent's if both are private
                        // (separate slots). Otherwise child overrides.
                        if *vis == Visibility::Private {
                            let child_also_private = class_def.properties.iter()
                                .any(|(n, _, v, _)| n == name && *v == Visibility::Private);
                            if child_also_private {
                                parent_props.push((name.clone(), default.clone(), *vis, declaring.clone()));
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

                // Inherit methods: collect ALL parent::* entries from function_table
                // (includes transitively inherited methods from grandparents)
                let child_method_names: std::collections::HashSet<String> = class_def.methods
                    .iter().map(|(n, _, _, _)| n.to_lowercase()).collect();
                let parent_prefix = format!("{}::", parent_name).to_lowercase();
                let inherited: Vec<(String, *const FunctionCommon)> = self.function_table.iter()
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

        // Do NOT inherit method stubs from interfaces — interface methods are contracts,
        // not implementations. The implementing class must provide its own body.
        // (Interface stub functions exist only in the interface's own ClassDef for type info.)

        // Box to get stable heap address for function pointers
        self.class_table.insert(class_name.clone(), Box::new(class_def));
        // Register child's own method pointers from the stable location
        let class = self.class_table.get(&class_name).unwrap();
        let method_entries: Vec<(String, *const FunctionCommon)> = class.methods.iter()
            .map(|(method_name, _vis, _is_static, func)| {
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
            self.method_declaring_class.insert(func_ptr, class_name.clone());
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

    /// Get the declaring class for a function pointer.
    pub fn declaring_class_of(&self, func_ptr: *const FunctionCommon) -> Option<&str> {
        self.method_declaring_class.get(&func_ptr).map(|s| s.as_str())
    }

    /// Collect all required interface method signatures (recursively through interface extends).
    /// Returns Vec of (method_name_lower, visibility, num_args, required_num_args, is_static, interface_name).
    fn collect_interface_methods(&self, iface_name: &str) -> Vec<(String, Visibility, u32, u32, bool, String)> {
        let mut result = Vec::new();
        if let Some(iface_def) = self.class_table.get(iface_name) {
            for (method_name, vis, is_static, func) in &iface_def.methods {
                result.push((
                    method_name.to_lowercase(),
                    *vis,
                    func.common.num_args,
                    func.common.required_num_args,
                    *is_static,
                    iface_name.to_string(),
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
            if class_def.is_interface || class_def.is_abstract {
                return errors; // interfaces/abstract classes don't need to implement
            }
        }
        // Collect interfaces from the entire parent chain (fix P2: inherited obligations)
        let all_ifaces = self.collect_all_interfaces(class_name);
        let mut seen = std::collections::HashSet::new();
        for iface_name in all_ifaces {
            if !seen.insert(iface_name.clone()) { continue; }
            let required = self.collect_interface_methods(&iface_name);
            for (method, _iface_vis, iface_nargs, iface_required, iface_static, declaring_iface) in required {
                let full = format!("{}::{}", class_name, method).to_lowercase();
                if !self.function_table.contains_key(&full) {
                    errors.push((declaring_iface.clone(), method.clone()));
                    continue;
                }
                // Check visibility and staticness
                if let Some((impl_vis, impl_is_static, _)) = self.find_method_info(class_name, &method) {
                    if impl_vis != Visibility::Public {
                        errors.push((declaring_iface.clone(), format!(
                            "{} (must be public, is {:?})", method,
                            match impl_vis { Visibility::Protected => "protected", Visibility::Private => "private", _ => "public" }
                        )));
                    }
                    // Static mismatch: non-static interface method cannot be implemented as static
                    // and vice versa.
                    if iface_static && !impl_is_static {
                        errors.push((declaring_iface.clone(), format!(
                            "{} (must be static as declared in interface)", method
                        )));
                    }
                    if !iface_static && impl_is_static {
                        errors.push((declaring_iface.clone(), format!(
                            "{} (cannot be static, interface declares it non-static)", method
                        )));
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
                    let impl_public = impl_common.num_args - impl_common.this_offset;
                    // required_num_args is NOT adjusted for this_offset in the compiler,
                    // so it already represents the public required parameter count.
                    let impl_required = impl_common.required_num_args;
                    // Implementation must accept at least as many params as the interface
                    if impl_public < iface_public {
                        errors.push((declaring_iface.clone(), format!(
                            "{} (requires {} params, implementation has {})",
                            method, iface_public, impl_public
                        )));
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
                }
            }
        }
        errors
    }

    /// Look up a method's visibility in a class hierarchy.
    /// Returns (visibility, declaring_class_name) — the class where the method is actually defined.
    pub fn find_method_visibility(&self, class_name: &str, method_name: &str) -> Option<(Visibility, String)> {
        self.find_method_info(class_name, method_name).map(|(vis, _, decl)| (vis, decl))
    }

    /// Look up method visibility AND staticness in a class hierarchy.
    /// Returns (visibility, is_static, declaring_class_name).
    pub fn find_method_info(&self, class_name: &str, method_name: &str) -> Option<(Visibility, bool, String)> {
        let method_lower = method_name.to_lowercase();
        if let Some(class_def) = self.class_table.get(class_name) {
            for (name, vis, is_static, _func) in &class_def.methods {
                if name.to_lowercase() == method_lower {
                    return Some((*vis, *is_static, class_name.to_string()));
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
    pub fn find_property_visibility(&self, class_name: &str, prop_name: &str) -> Option<(Visibility, String)> {
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
    pub fn check_visibility(&self, caller_class: Option<&str>, target_class: &str, visibility: Visibility) -> bool {
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
    pub fn register_function(&mut self, name: &str, func: *const FunctionCommon) -> Result<(), String> {
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
            return Some(ptr);
        }
        // Slow path: allocate lowercase string
        let lower = name.to_lowercase();
        if lower != name {
            self.function_table.get(&lower).copied()
        } else {
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
        self.constant_table.borrow().get(name).cloned()
    }

    pub fn write_output(&self, data: &[u8]) {
        self.output.borrow_mut().write_all(data).unwrap();
    }
}
