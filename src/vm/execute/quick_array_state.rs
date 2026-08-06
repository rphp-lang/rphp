// Kept in the execute module through include! so this structural split does not change visibility or code generation.

/// Borrowed view of an immutable PHP array for one guarded region.
///
/// The planner rejects writes and calls in the region, the array slot cannot
/// overlap a scalar output, and PHP array aliases detach through copy-on-write.
/// The source `Value` therefore keeps this allocation alive and stable until
/// the region completes or takes a side exit.
#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
enum QuickLongArray {
    Empty,
    Packed {
        values: *const Value,
        len: usize,
    },
    Hash {
        array: *const PhpArray,
    },
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongIntPositionHint {
    first_key: i64,
    stride: i64,
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongExactIntLayout {
    layout: ExactOrderedIntLayout,
}

#[cold]
#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn fallback_indexed_long(array: *const PhpArray, key: i64) -> Option<i64> {
    (*array).get_indexed_long(key)
}

#[cfg(feature = "quick-loops")]
impl QuickLongExactIntLayout {
    #[inline(always)]
    unsafe fn long_at(self, array: *const PhpArray, key: i64) -> Option<i64> {
        match self.layout.positioned_value(key) {
            Some(value) => {
                let value = &*value;
                (value.value_type() == ValueType::Long).then(|| value.raw_long())
            }
            None => fallback_indexed_long(array, key),
        }
    }
}

#[cfg(feature = "quick-loops")]
impl QuickLongArray {
    const EMPTY: Self = Self::Empty;

    #[inline]
    fn from_array(array: &PhpArray) -> Self {
        match array.packed_values() {
            Some(values) => Self::Packed {
                values: values.as_ptr(),
                len: values.len(),
            },
            None => Self::Hash {
                array: array as *const PhpArray,
            },
        }
    }

    #[inline(always)]
    unsafe fn long_at_int(self, index: i64) -> Option<i64> {
        let value = match self {
            Self::Packed { values, len } if index >= 0 && (index as usize) < len => {
                &*values.add(index as usize)
            }
            Self::Hash { array } => (*array).get_int(index)?,
            Self::Empty | Self::Packed { .. } => return None,
        };
        (value.value_type() == ValueType::Long).then(|| value.raw_long())
    }

    #[inline(always)]
    unsafe fn long_at_str(self, key: &str) -> Option<i64> {
        let Self::Hash { array } = self else {
            return None;
        };
        let value = (*array).get_str(key)?;
        (value.value_type() == ValueType::Long).then(|| value.raw_long())
    }

    #[inline(always)]
    unsafe fn long_at(
        self,
        index: QuickArrayIndex,
        slots: &[i64; 64],
        op_array: &crate::compiler::OpArray,
    ) -> Option<i64> {
        match index {
            QuickArrayIndex::Long(index) => {
                self.long_at_int(quick_long_operand(slots, index))
            }
            QuickArrayIndex::StringLiteral(literal) => self.long_at_str(
                op_array
                    .literals
                    .get_unchecked(literal as usize)
                    .as_str()
                    .unwrap_unchecked(),
            ),
            QuickArrayIndex::ValueSlot(_) => None,
        }
    }
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn mutable_long_entry_at(
    array: *mut PhpArray,
    index: QuickArrayIndex,
    slots: &[i64; 64],
    op_array: &crate::compiler::OpArray,
) -> Option<(i64, *mut Value)> {
    let value = match index {
        QuickArrayIndex::Long(index) => {
            (*array).get_int_mut(quick_long_operand(slots, index))
        }
        QuickArrayIndex::StringLiteral(literal) => {
            let key = op_array
                .literals
                .get_unchecked(literal as usize)
                .as_str()
                .unwrap_unchecked();
            match canonical_decimal_array_key(key) {
                Some(key) => (*array).get_int_mut(key),
                None => (*array).get_str_mut(key),
            }
        }
        QuickArrayIndex::ValueSlot(_) => return None,
    }?;
    (value.value_type() == ValueType::Long)
        .then(|| (value.raw_long(), value as *mut Value))
}
