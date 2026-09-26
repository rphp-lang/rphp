//! Cold request-startup configuration shared by the CLI and source compiler.

use super::{ExecutorGlobals, FunctionType};

/// CLI INI string values retain whitespace and spelling; only bare boolean
/// keywords are normalized. Directive names themselves are case-sensitive.
pub fn ini_string(value: &str) -> &str {
    if ["true", "on", "yes"]
        .iter()
        .any(|word| value.eq_ignore_ascii_case(word))
    {
        "1"
    } else if ["false", "off", "no", "none"]
        .iter()
        .any(|word| value.eq_ignore_ascii_case(word))
    {
        ""
    } else {
        value
    }
}

pub fn setting<'a>(settings: &'a [(String, String)], name: &str) -> Option<&'a str> {
    settings
        .iter()
        .rev()
        .find(|(key, _)| key == name)
        .map(|(_, value)| ini_string(value))
}

/// This list uses literal spaces and commas, not arbitrary whitespace or
/// case-insensitive matching. Names in the internal function table are folded.
pub fn disabled_function_names(value: &str) -> impl Iterator<Item = &str> {
    value
        .split([' ', ','])
        .filter(|name| !name.is_empty() && !matches!(*name, "exit" | "die"))
}

pub fn variables_order(value: &str) -> &str {
    if value.is_empty() { "EGPCS" } else { value }
}

/// Startup diagnostics preserve the unconsumed list spelling, including later
/// entries. Repeated non-disableable names each produce a warning.
pub fn disable_function_warnings(value: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut offset = 0;
    for name in value.split([' ', ',']) {
        if matches!(name, "exit" | "die") {
            result.push(format!("Cannot disable function {}()", &value[offset..]));
        }
        offset += name.len() + 1;
    }
    result
}

impl ExecutorGlobals {
    pub fn startup_disabled_functions(&self) -> &str {
        self.ini_overrides
            .as_deref()
            .and_then(|settings| settings.get("disable_functions"))
            .map_or("", String::as_str)
    }

    pub(crate) fn internal_function_is_disabled(&self, normalized_name: &str) -> bool {
        disabled_function_names(self.startup_disabled_functions())
            .any(|name| name == normalized_name)
    }

    /// Remove only internal global entries, after all extension registration
    /// and before publishing user declarations. Descriptors retain their normal
    /// owners. No runtime call-site guard or extra executor field is needed.
    pub fn apply_disabled_functions(&mut self) {
        if self.startup_disabled_functions().is_empty() {
            return;
        }
        // An immutable alias must remain callable when only its target name is
        // disabled. Materialize it before removing either public name.
        for alias in crate::builtin_metadata::INTERNAL_FUNCTION_ALIASES {
            if !self.internal_function_is_disabled(alias.alias)
                && let Some(&function) = self.function_table.get(alias.target)
            {
                self.function_table
                    .entry(alias.alias.to_string())
                    .or_insert(function);
            }
        }
        let names: Vec<String> = disabled_function_names(self.startup_disabled_functions())
            .filter(|name| !name.contains("::"))
            .map(str::to_owned)
            .collect();
        for name in names {
            let internal = self
                .function_table
                .get(&name)
                .and_then(|&function| self.registered_function_common(function))
                .is_some_and(|function| function.fn_type == FunctionType::Internal);
            if internal {
                self.function_table.remove(&name);
            }
        }
    }
}
