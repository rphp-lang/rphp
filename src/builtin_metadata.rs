/// Compile-time metadata for pure internal functions that expose the
/// frame-free `DirectInternalFunctionHandler` ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectInternalSpec {
    pub name: &'static str,
    pub max_args: u32,
    pub required_args: u32,
}

pub const DIRECT_INTERNAL_SPECS: &[DirectInternalSpec] = &[
    DirectInternalSpec { name: "strlen", max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "strtolower", max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "strtoupper", max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "ord", max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "abs", max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "floor", max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "sqrt", max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "chunk_split", max_args: 3, required_args: 1 },
    DirectInternalSpec { name: "sin", max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "tan", max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "asin", max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "acos", max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "atan", max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "exp", max_args: 1, required_args: 1 },
];

#[inline]
pub fn direct_internal_spec(name: &str) -> Option<DirectInternalSpec> {
    DIRECT_INTERNAL_SPECS
        .iter()
        .copied()
        .find(|spec| spec.name.eq_ignore_ascii_case(name))
}

#[inline]
pub fn supports_direct_internal_call(name: &str, num_args: usize) -> bool {
    direct_internal_spec(name).is_some_and(|spec| {
        num_args >= spec.required_args as usize && num_args <= spec.max_args as usize
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_metadata_is_case_insensitive_and_checks_arity() {
        assert!(supports_direct_internal_call("STRLEN", 1));
        assert!(!supports_direct_internal_call("strlen", 0));
        assert!(supports_direct_internal_call("chunk_split", 3));
        assert!(!supports_direct_internal_call("chunk_split", 4));
        assert!(!supports_direct_internal_call("substr", 1));
    }
}
