// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickStringFetchCacheEntry {
    key_data: *const u8,
    key_len: usize,
    array_slot: u16,
    value: i64,
    value_ptr: *mut Value,
    valid: bool,
}

#[cfg(feature = "quick-loops")]
impl QuickStringFetchCacheEntry {
    const EMPTY: Self = Self {
        key_data: std::ptr::null(),
        key_len: 0,
        array_slot: 0,
        value: 0,
        value_ptr: std::ptr::null_mut(),
        valid: false,
    };
}

#[cfg(feature = "quick-loops")]
struct QuickStringFetchCache {
    entries: [QuickStringFetchCacheEntry; QUICK_STRING_FETCH_CACHE_LIMIT],
    capacity: usize,
    next: usize,
}

/// Retained string CV state for one closed quick region. The frame keeps the
/// original string alive while the region runs; assignments only redirect a
/// slot to immutable OpArray literals. Drop commits every dirty CV on all
/// completion and deoptimization returns.
#[cfg(feature = "quick-loops")]
struct QuickStringSlotState {
    slot_base: *mut Value,
    values: [*const Value; 64],
    dirty_mask: u64,
}

#[cfg(feature = "quick-loops")]
impl QuickStringSlotState {
    #[inline]
    unsafe fn new(slot_base: *mut Value, mut input_mask: u64) -> Self {
        let mut values = [std::ptr::null(); 64];
        while input_mask != 0 {
            let slot = input_mask.trailing_zeros() as usize;
            input_mask &= input_mask - 1;
            values[slot] = slot_base.add(slot);
        }
        Self {
            slot_base,
            values,
            dirty_mask: 0,
        }
    }

    #[inline(always)]
    unsafe fn value(&self, slot: u16) -> &Value {
        &*self.values[slot as usize]
    }

    #[inline(always)]
    fn assign_literal(&mut self, slot: u16, value: *const Value) {
        debug_assert!(!value.is_null());
        self.values[slot as usize] = value;
        self.dirty_mask |= 1u64 << slot;
    }

    #[inline(always)]
    fn assign_slot(&mut self, destination: u16, source: u16) {
        let value = self.values[source as usize];
        debug_assert!(!value.is_null());
        self.values[destination as usize] = value;
        self.dirty_mask |= 1u64 << destination;
    }

    #[inline]
    unsafe fn commit(&mut self) {
        while self.dirty_mask != 0 {
            let slot = self.dirty_mask.trailing_zeros() as usize;
            self.dirty_mask &= self.dirty_mask - 1;
            let value = (&*self.values[slot]).clone();
            debug_assert_eq!(value.value_type(), ValueType::String);
            slot_set(self.slot_base.add(slot), value);
        }
    }
}

#[cfg(feature = "quick-loops")]
impl Drop for QuickStringSlotState {
    fn drop(&mut self) {
        unsafe { self.commit() };
    }
}

#[cfg(feature = "quick-loops")]
impl QuickStringFetchCache {
    #[inline]
    const fn new(capacity: u8) -> Self {
        Self {
            entries: [QuickStringFetchCacheEntry::EMPTY; QUICK_STRING_FETCH_CACHE_LIMIT],
            capacity: capacity as usize,
            next: 0,
        }
    }

    /// Cache a successful long fetch by immutable string allocation identity.
    /// The planner proves that both the array slot and string key can only be
    /// read or replaced by immutable literals for the lifetime of this region.
    #[inline(always)]
    unsafe fn long_at(
        &mut self,
        array_slot: u16,
        array: QuickLongArray,
        key: &str,
    ) -> Option<i64> {
        let key_data = key.as_ptr();
        let key_len = key.len();
        if self.capacity != 0
            && self.entries[0].valid
            && self.entries[0].array_slot == array_slot
            && self.entries[0].key_data == key_data
            && self.entries[0].key_len == key_len
        {
            return Some(self.entries[0].value);
        }
        if self.capacity > 1
            && self.entries[1].valid
            && self.entries[1].array_slot == array_slot
            && self.entries[1].key_data == key_data
            && self.entries[1].key_len == key_len
        {
            return Some(self.entries[1].value);
        }
        if self.capacity > 2
            && self.entries[2].valid
            && self.entries[2].array_slot == array_slot
            && self.entries[2].key_data == key_data
            && self.entries[2].key_len == key_len
        {
            return Some(self.entries[2].value);
        }
        if self.capacity > 3
            && self.entries[3].valid
            && self.entries[3].array_slot == array_slot
            && self.entries[3].key_data == key_data
            && self.entries[3].key_len == key_len
        {
            return Some(self.entries[3].value);
        }

        let value = match canonical_decimal_array_key(key) {
            Some(key) => array.long_at_int(key),
            None => array.long_at_str(key),
        }?;
        if self.capacity != 0 {
            self.entries[self.next] = QuickStringFetchCacheEntry {
                key_data,
                key_len,
                array_slot,
                value,
                value_ptr: std::ptr::null_mut(),
                valid: true,
            };
            self.next += 1;
            if self.next == self.capacity {
                self.next = 0;
            }
        }
        Some(value)
    }

    /// Resolve a writable existing entry only from the array pointer whose COW
    /// uniqueness was guarded at region entry. Cached pointers are retained
    /// only for plans without structural writes to this array.
    #[inline(always)]
    unsafe fn long_entry_at_mut(
        &mut self,
        array_slot: u16,
        array: *mut PhpArray,
        key: &str,
    ) -> Option<(i64, *mut Value)> {
        let key_data = key.as_ptr();
        let key_len = key.len();
        let mut cached_index = None;
        for (index, entry) in self.entries.iter().take(self.capacity).enumerate() {
            if entry.valid
                && entry.array_slot == array_slot
                && entry.key_data == key_data
                && entry.key_len == key_len
            {
                if !entry.value_ptr.is_null() {
                    return Some((entry.value, entry.value_ptr));
                }
                cached_index = Some(index);
                break;
            }
        }

        let value = match canonical_decimal_array_key(key) {
            Some(key) => (*array).get_int_mut(key),
            None => (*array).get_str_mut(key),
        }?;
        if value.value_type() != ValueType::Long {
            return None;
        }
        let resolved = (value.raw_long(), value as *mut Value);
        let entry = QuickStringFetchCacheEntry {
            key_data,
            key_len,
            array_slot,
            value: resolved.0,
            value_ptr: resolved.1,
            valid: true,
        };
        if let Some(index) = cached_index {
            self.entries[index] = entry;
        } else if self.capacity != 0 {
            self.entries[self.next] = entry;
            self.next += 1;
            if self.next == self.capacity {
                self.next = 0;
            }
        }
        Some(resolved)
    }

    #[inline(always)]
    fn store_long(&mut self, array_slot: u16, key: &str, value: i64) {
        let key_data = key.as_ptr();
        let key_len = key.len();
        for entry in self.entries.iter_mut().take(self.capacity) {
            if entry.valid
                && entry.array_slot == array_slot
                && entry.key_data == key_data
                && entry.key_len == key_len
            {
                entry.value = value;
            }
        }
    }
}
