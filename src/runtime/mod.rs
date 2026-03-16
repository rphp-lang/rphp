use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::io::Write;

use crate::vm::stack::VmStack;
use crate::vm::frame::ExecuteData;
use crate::vm::function::FunctionCommon;
use crate::compiler::compile::ClassDef;

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

            output: std::cell::RefCell::new(output),
        }
    }

    /// Register a class definition and its methods in the function table.
    /// Resolves inheritance: merges parent properties/methods into child.
    pub fn register_class(&mut self, mut class_def: ClassDef) {
        let class_name = class_def.name.clone();

        // Resolve inheritance — merge parent's properties and methods
        if let Some(parent_name) = &class_def.parent {
            if let Some(parent) = self.class_table.get(parent_name.as_str()) {
                // Inherit properties: parent props first, child overrides
                let child_prop_names: std::collections::HashSet<&str> = class_def.properties
                    .iter().map(|(n, _)| n.as_str()).collect();
                let mut merged_props = Vec::new();
                for (name, default) in &parent.properties {
                    if !child_prop_names.contains(name.as_str()) {
                        merged_props.push((name.clone(), default.clone()));
                    }
                }
                merged_props.extend(class_def.properties.drain(..));
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
        for (full_name, func_ptr) in method_entries {
            self.function_table.insert(full_name, func_ptr);
        }
    }

    /// Check if a class is an instance of another (walks parent chain)
    pub fn class_is_a(&self, class_name: &str, target: &str) -> bool {
        if class_name.eq_ignore_ascii_case(target) {
            return true;
        }
        if let Some(class_def) = self.class_table.get(class_name) {
            if let Some(parent) = &class_def.parent {
                return self.class_is_a(parent, target);
            }
        }
        false
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

    /// Look up a function by name
    pub fn find_function(&self, name: &str) -> Option<*const FunctionCommon> {
        self.function_table.get(&name.to_lowercase()).copied()
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
