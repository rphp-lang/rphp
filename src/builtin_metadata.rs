/// Compile-time metadata for pure internal functions that expose the
/// frame-free `DirectInternalFunctionHandler` ABI.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectInternalKind {
    Strlen,
    Strtolower,
    Strtoupper,
    Ord,
    Abs,
    Floor,
    Sqrt,
    ChunkSplit,
    Sin,
    Tan,
    Asin,
    Acos,
    Atan,
    Exp,
    Intdiv,
    JsonDecode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectInternalLowering {
    Generic,
    Generic2,
    Strlen,
}

impl DirectInternalKind {
    #[inline(always)]
    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            0 => Some(Self::Strlen),
            1 => Some(Self::Strtolower),
            2 => Some(Self::Strtoupper),
            3 => Some(Self::Ord),
            4 => Some(Self::Abs),
            5 => Some(Self::Floor),
            6 => Some(Self::Sqrt),
            7 => Some(Self::ChunkSplit),
            8 => Some(Self::Sin),
            9 => Some(Self::Tan),
            10 => Some(Self::Asin),
            11 => Some(Self::Acos),
            12 => Some(Self::Atan),
            13 => Some(Self::Exp),
            14 => Some(Self::Intdiv),
            15 => Some(Self::JsonDecode),
            _ => None,
        }
    }

    #[inline(always)]
    pub fn result_may_need_cleanup(self) -> bool {
        matches!(
            self,
            Self::Strtolower
                | Self::Strtoupper
                | Self::ChunkSplit
                | Self::JsonDecode
        )
    }

    #[inline(always)]
    pub fn lowering(self) -> DirectInternalLowering {
        match self {
            Self::Strlen => DirectInternalLowering::Strlen,
            Self::Intdiv | Self::JsonDecode => DirectInternalLowering::Generic2,
            _ => DirectInternalLowering::Generic,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectInternalSpec {
    pub name: &'static str,
    pub kind: DirectInternalKind,
    pub max_args: u32,
    pub required_args: u32,
}

pub const DIRECT_INTERNAL_SPECS: &[DirectInternalSpec] = &[
    DirectInternalSpec { name: "strlen", kind: DirectInternalKind::Strlen, max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "strtolower", kind: DirectInternalKind::Strtolower, max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "strtoupper", kind: DirectInternalKind::Strtoupper, max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "ord", kind: DirectInternalKind::Ord, max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "abs", kind: DirectInternalKind::Abs, max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "floor", kind: DirectInternalKind::Floor, max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "sqrt", kind: DirectInternalKind::Sqrt, max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "chunk_split", kind: DirectInternalKind::ChunkSplit, max_args: 3, required_args: 1 },
    DirectInternalSpec { name: "sin", kind: DirectInternalKind::Sin, max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "tan", kind: DirectInternalKind::Tan, max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "asin", kind: DirectInternalKind::Asin, max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "acos", kind: DirectInternalKind::Acos, max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "atan", kind: DirectInternalKind::Atan, max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "exp", kind: DirectInternalKind::Exp, max_args: 1, required_args: 1 },
    DirectInternalSpec { name: "intdiv", kind: DirectInternalKind::Intdiv, max_args: 2, required_args: 2 },
    DirectInternalSpec { name: "json_decode", kind: DirectInternalKind::JsonDecode, max_args: 2, required_args: 1 },
];

#[inline]
pub fn direct_internal_spec(name: &str) -> Option<DirectInternalSpec> {
    // This lookup also runs at polymorphic dynamic-callback sites. Dispatch
    // by length first so an unsupported name does not scan every direct
    // builtin and supported names need only a handful of ASCII comparisons.
    let index = match name.len() {
        3 if name.eq_ignore_ascii_case("ord") => 3,
        3 if name.eq_ignore_ascii_case("abs") => 4,
        3 if name.eq_ignore_ascii_case("sin") => 8,
        3 if name.eq_ignore_ascii_case("tan") => 9,
        3 if name.eq_ignore_ascii_case("exp") => 13,
        4 if name.eq_ignore_ascii_case("sqrt") => 6,
        4 if name.eq_ignore_ascii_case("asin") => 10,
        4 if name.eq_ignore_ascii_case("acos") => 11,
        4 if name.eq_ignore_ascii_case("atan") => 12,
        5 if name.eq_ignore_ascii_case("floor") => 5,
        6 if name.eq_ignore_ascii_case("strlen") => 0,
        10 if name.eq_ignore_ascii_case("strtolower") => 1,
        10 if name.eq_ignore_ascii_case("strtoupper") => 2,
        11 if name.eq_ignore_ascii_case("chunk_split") => 7,
        11 if name.eq_ignore_ascii_case("json_decode") => 15,
        6 if name.eq_ignore_ascii_case("intdiv") => 14,
        _ => return None,
    };
    Some(DIRECT_INTERNAL_SPECS[index])
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
        assert!(supports_direct_internal_call("intdiv", 2));
        assert!(!supports_direct_internal_call("intdiv", 1));
        assert!(supports_direct_internal_call("json_decode", 2));
        assert!(supports_direct_internal_call("json_decode", 1));
        assert!(supports_direct_internal_call("chunk_split", 3));
        assert!(!supports_direct_internal_call("chunk_split", 4));
        assert!(!supports_direct_internal_call("substr", 1));
        assert_eq!(
            direct_internal_spec("STRTOUPPER").map(|spec| spec.kind),
            Some(DirectInternalKind::Strtoupper),
        );
        assert_eq!(direct_internal_spec("soundex"), None);
        assert!(!DirectInternalKind::Strlen.result_may_need_cleanup());
        assert!(DirectInternalKind::Strtolower.result_may_need_cleanup());
        assert_eq!(DirectInternalKind::Strlen.lowering(), DirectInternalLowering::Strlen);
        assert_eq!(DirectInternalKind::Abs.lowering(), DirectInternalLowering::Generic);
        assert_eq!(DirectInternalKind::Intdiv.lowering(), DirectInternalLowering::Generic2);
        assert_eq!(DirectInternalKind::JsonDecode.lowering(), DirectInternalLowering::Generic2);
        assert!(DirectInternalKind::JsonDecode.result_may_need_cleanup());
    }
}
