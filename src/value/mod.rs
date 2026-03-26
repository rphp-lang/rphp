use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::cell::RefCell;

use crate::vm::stats;
use crate::vm::generator::GeneratorRef;
use crate::vm::function::FunctionCommon;

/// PHP object — class instance with properties.
#[derive(Debug, Clone)]
pub struct PhpObject {
    pub class_name: String,
    /// Stable numeric class ID — matches ClassDef.class_id. Used for inline cache keying.
    pub class_id: u32,
    pub properties: HashMap<String, Value>,
    /// If this object is a Generator, holds the generator state
    pub generator: Option<GeneratorRef>,
}

/// PHP array — ordered hash map with integer and string keys.
/// Preserves insertion order, supports auto-incrementing integer keys.
/// Uses split HashMap indexes for O(1) key lookup + Vec for insertion order.
/// String lookups use `HashMap<String, usize>` which supports `&str` via `Borrow` — zero allocation.
#[derive(Debug, Clone)]
pub struct PhpArray {
    /// Insertion-ordered entries
    entries: Vec<(ArrayKey, Value)>,
    /// String key → index into entries. Lazily allocated (None while packed).
    /// `HashMap<String, usize>` supports `get(&str)` via `Borrow<str>` — no allocation on lookup.
    str_index: Option<HashMap<String, usize>>,
    /// Int key → index into entries. Lazily allocated (None while packed).
    int_index: Option<HashMap<i64, usize>>,
    next_int_key: i64,
    /// True when keys are exactly 0..N-1 (sequential push only).
    /// Packed arrays skip HashMap entirely — direct Vec index.
    packed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArrayKey {
    Int(i64),
    String(String),
}

impl PhpArray {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            str_index: None,
            int_index: None,
            next_int_key: 0,
            packed: true,
        }
    }

    /// Ensure the split index maps exist (transition from packed to hash mode).
    fn ensure_index(&mut self) {
        if self.str_index.is_none() {
            let mut str_map = HashMap::new();
            let mut int_map = HashMap::with_capacity(self.entries.len());
            for (i, (k, _)) in self.entries.iter().enumerate() {
                match k {
                    ArrayKey::Int(n) => { int_map.insert(*n, i); }
                    ArrayKey::String(s) => { str_map.insert(s.clone(), i); }
                }
            }
            self.str_index = Some(str_map);
            self.int_index = Some(int_map);
        }
    }

    /// Transition out of packed mode.
    fn unpack(&mut self) {
        if self.packed {
            self.packed = false;
            self.ensure_index();
        }
    }

    /// Append with auto-incrementing key ($a[] = val)
    #[inline]
    pub fn push(&mut self, val: Value) {
        let key = self.next_int_key;
        self.next_int_key = key + 1;
        if self.packed {
            // Packed: key == entries.len(), just push to Vec
            self.entries.push((ArrayKey::Int(key), val));
        } else {
            let idx = self.entries.len();
            self.entries.push((ArrayKey::Int(key), val));
            self.int_index.as_mut().unwrap().insert(key, idx);
        }
    }

    /// Set by integer key
    pub fn set_int(&mut self, key: i64, val: Value) {
        if self.packed {
            // Can stay packed if key == next sequential
            if key == self.next_int_key {
                self.next_int_key = key + 1;
                self.entries.push((ArrayKey::Int(key), val));
                return;
            }
            // Overwrite existing packed slot?
            if key >= 0 && (key as usize) < self.entries.len() {
                self.entries[key as usize].1 = val;
                return;
            }
            // Non-sequential → unpack
            self.unpack();
        }
        let int_index = self.int_index.as_mut().unwrap();
        if let Some(&idx) = int_index.get(&key) {
            self.entries[idx].1 = val;
        } else {
            let idx = self.entries.len();
            self.entries.push((ArrayKey::Int(key), val));
            int_index.insert(key, idx);
            if key >= self.next_int_key {
                self.next_int_key = key + 1;
            }
        }
    }

    /// Set by string key
    pub fn set_str(&mut self, key: &str, val: Value) {
        self.unpack(); // String key → always hash mode
        let str_index = self.str_index.as_mut().unwrap();
        if let Some(&idx) = str_index.get(key) {
            // Key exists — overwrite value, no allocation for key
            self.entries[idx].1 = val;
        } else {
            // New key — allocate once for both entry and index
            let owned = key.to_string();
            let idx = self.entries.len();
            self.entries.push((ArrayKey::String(owned.clone()), val));
            str_index.insert(owned, idx);
        }
    }

    /// Set by ArrayKey
    pub fn set(&mut self, key: ArrayKey, val: Value) {
        match key {
            ArrayKey::Int(k) => self.set_int(k, val),
            ArrayKey::String(ref k) => self.set_str(k, val),
        }
    }

    /// Get by integer key — O(1)
    #[inline]
    pub fn get_int(&self, key: i64) -> Option<&Value> {
        if self.packed && key >= 0 {
            // Direct Vec index — no hash, no alloc
            self.entries.get(key as usize).map(|(_, v)| v)
        } else {
            let int_index = self.int_index.as_ref()?;
            int_index.get(&key).map(|&idx| &self.entries[idx].1)
        }
    }

    /// Get by string key — O(1), zero allocation.
    /// Uses `HashMap<String, usize>::get(&str)` via `Borrow<str>` trait.
    #[inline]
    pub fn get_str(&self, key: &str) -> Option<&Value> {
        let str_index = self.str_index.as_ref()?;
        str_index.get(key).map(|&idx| &self.entries[idx].1)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[(ArrayKey, Value)] {
        &self.entries
    }

    /// Remove element by key
    pub fn remove(&mut self, key: &ArrayKey) -> bool {
        self.unpack(); // remove breaks packed invariant
        let found_idx = match key {
            ArrayKey::Int(n) => self.int_index.as_ref().unwrap().get(n).copied(),
            ArrayKey::String(s) => self.str_index.as_ref().unwrap().get(s.as_str()).copied(),
        };
        if let Some(idx) = found_idx {
            self.entries.remove(idx);
            // Remove the key from its index
            match key {
                ArrayKey::Int(n) => { self.int_index.as_mut().unwrap().remove(n); }
                ArrayKey::String(s) => { self.str_index.as_mut().unwrap().remove(s.as_str()); }
            }
            // Re-index entries after removed position
            self.reindex_from(idx);
            true
        } else {
            false
        }
    }

    /// Remove and return last element
    pub fn pop(&mut self) -> Option<Value> {
        if let Some((key, val)) = self.entries.pop() {
            match &key {
                ArrayKey::Int(n) => { if let Some(ref mut m) = self.int_index { m.remove(n); } }
                ArrayKey::String(s) => { if let Some(ref mut m) = self.str_index { m.remove(s.as_str()); } }
            }
            Some(val)
        } else {
            None
        }
    }

    /// Remove and return first element
    pub fn shift(&mut self) -> Option<Value> {
        if self.entries.is_empty() {
            None
        } else {
            self.unpack(); // shift breaks packed invariant
            let (key, val) = self.entries.remove(0);
            match &key {
                ArrayKey::Int(n) => { self.int_index.as_mut().unwrap().remove(n); }
                ArrayKey::String(s) => { self.str_index.as_mut().unwrap().remove(s.as_str()); }
            }
            // Re-index all entries since indices shifted
            self.reindex_from(0);
            Some(val)
        }
    }

    /// Rebuild index entries from position `from` onward (after remove/shift).
    fn reindex_from(&mut self, from: usize) {
        let int_index = self.int_index.as_mut().unwrap();
        let str_index = self.str_index.as_mut().unwrap();
        for (i, (k, _)) in self.entries.iter().enumerate() {
            if i >= from {
                match k {
                    ArrayKey::Int(n) => { int_index.insert(*n, i); }
                    ArrayKey::String(s) => { str_index.insert(s.clone(), i); }
                }
            }
        }
    }
}

/// PHP closure — function pointer + captured values.
/// Stored behind Box in Value, like String and Array.
pub struct PhpClosure {
    /// Direct pointer to the resolved function. No string lookup needed at call time.
    pub func: *const FunctionCommon,
    /// Captured `use` variable values, in declaration order.
    pub captures: Vec<Value>,
    /// True if any captured value needs_cleanup (String/Array/Object/Closure).
    /// When false, captures are all scalars — clone is a cheap memcpy.
    pub has_heap_captures: bool,
}

impl Clone for PhpClosure {
    fn clone(&self) -> Self {
        Self {
            func: self.func,
            captures: self.captures.clone(),
            has_heap_captures: self.has_heap_captures,
        }
    }
}

impl std::fmt::Debug for PhpClosure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhpClosure")
            .field("func", &self.func)
            .field("captures", &self.captures.len())
            .finish()
    }
}

/// PHP Value — tagged union, 16 bytes.
/// Layout matches zend_value + type_info.
#[repr(C)]
pub struct Value {
    data: ValueData,
    type_info: u32,
    _not_send: PhantomData<*mut ()>,
}

#[repr(C)]
union ValueData {
    long: i64,
    double: f64,
    ptr: *mut u8,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Undef = 0,
    Null = 1,
    False = 2,
    True = 3,
    Long = 4,
    Double = 5,
    String = 6,
    Array = 7,
    Object = 8,
    Resource = 9,
    Reference = 10,
    Closure = 11,
}

impl Value {
    #[inline]
    pub fn undef() -> Self {
        Self {
            data: ValueData { long: 0 },
            type_info: ValueType::Undef as u32,
            _not_send: PhantomData,
        }
    }

    #[inline]
    pub fn null() -> Self {
        Self {
            data: ValueData { long: 0 },
            type_info: ValueType::Null as u32,
            _not_send: PhantomData,
        }
    }

    #[inline]
    pub fn long(v: i64) -> Self {
        Self {
            data: ValueData { long: v },
            type_info: ValueType::Long as u32,
            _not_send: PhantomData,
        }
    }

    #[inline]
    pub fn double(v: f64) -> Self {
        Self {
            data: ValueData { double: v },
            type_info: ValueType::Double as u32,
            _not_send: PhantomData,
        }
    }

    // ── In-place scalar writers ─────────────────────────────────────────
    // Write a scalar value directly into a target slot, avoiding construction
    // of an intermediate Value on the Rust stack. Used by the slot lifecycle API
    // for hot-path arithmetic results (Add, Sub, IsSmaller, etc.).

    /// Write a Long value directly into a slot.
    #[inline(always)]
    pub unsafe fn write_long(ptr: *mut Value, v: i64) {
        ptr.write(Self {
            data: ValueData { long: v },
            type_info: ValueType::Long as u32,
            _not_send: PhantomData,
        });
    }

    /// Write a Double value directly into a slot.
    #[inline(always)]
    pub unsafe fn write_double(ptr: *mut Value, v: f64) {
        ptr.write(Self {
            data: ValueData { double: v },
            type_info: ValueType::Double as u32,
            _not_send: PhantomData,
        });
    }

    /// Write a Bool value directly into a slot.
    #[inline(always)]
    pub unsafe fn write_bool(ptr: *mut Value, v: bool) {
        ptr.write(Self {
            data: ValueData { long: 0 },
            type_info: if v { ValueType::True } else { ValueType::False } as u32,
            _not_send: PhantomData,
        });
    }

    /// Write Null directly into a slot.
    #[inline(always)]
    pub unsafe fn write_null(ptr: *mut Value) {
        ptr.write(Self {
            data: ValueData { long: 0 },
            type_info: ValueType::Null as u32,
            _not_send: PhantomData,
        });
    }

    /// Create a string value. Stores a heap-allocated String (Box).
    /// Temporary approach — will migrate to ZString later.
    #[inline]
    pub fn string(s: impl Into<String>) -> Self {
        let boxed = Box::new(s.into());
        Self {
            data: ValueData { ptr: Box::into_raw(boxed) as *mut u8 },
            type_info: ValueType::String as u32,
            _not_send: PhantomData,
        }
    }

    /// Create an array value from a PhpArray.
    #[inline]
    pub fn array(arr: PhpArray) -> Self {
        let boxed = Box::new(arr);
        Self {
            data: ValueData { ptr: Box::into_raw(boxed) as *mut u8 },
            type_info: ValueType::Array as u32,
            _not_send: PhantomData,
        }
    }

    /// Create an object value from a PhpObject (reference-counted).
    /// Stores Rc pointer directly — no Box wrapper. Clone = Rc increment, Drop = Rc decrement.
    #[inline]
    pub fn object(obj: PhpObject) -> Self {
        let rc = Rc::new(RefCell::new(obj));
        let ptr = Rc::into_raw(rc) as *mut u8;
        Self {
            data: ValueData { ptr },
            type_info: ValueType::Object as u32,
            _not_send: PhantomData,
        }
    }

    /// Create a closure value from a PhpClosure.
    #[inline]
    pub fn closure(c: PhpClosure) -> Self {
        let boxed = Box::new(c);
        Self {
            data: ValueData { ptr: Box::into_raw(boxed) as *mut u8 },
            type_info: ValueType::Closure as u32,
            _not_send: PhantomData,
        }
    }

    /// Get closure reference. Only valid for Closure values.
    #[inline]
    pub fn as_closure(&self) -> Option<&PhpClosure> {
        if self.value_type() == ValueType::Closure {
            Some(unsafe { &*(self.data.ptr as *const PhpClosure) })
        } else {
            None
        }
    }

    /// Get mutable closure reference. Only valid for Closure values.
    #[inline]
    pub fn as_closure_mut(&mut self) -> Option<&mut PhpClosure> {
        if self.value_type() == ValueType::Closure {
            Some(unsafe { &mut *(self.data.ptr as *mut PhpClosure) })
        } else {
            None
        }
    }

    /// Get the Rc<RefCell<PhpObject>> for shared access.
    /// Returns a temporary Rc handle without affecting the refcount.
    /// The caller must NOT drop the returned Rc (use for borrow/clone only).
    #[inline]
    pub fn as_object_rc(&self) -> Option<std::mem::ManuallyDrop<Rc<RefCell<PhpObject>>>> {
        if self.value_type() == ValueType::Object {
            Some(unsafe {
                std::mem::ManuallyDrop::new(Rc::from_raw(self.data.ptr as *const RefCell<PhpObject>))
            })
        } else {
            None
        }
    }

    /// Get object reference (borrows the RefCell). Only valid for Object values.
    #[inline]
    pub fn as_object(&self) -> Option<std::cell::Ref<'_, PhpObject>> {
        if self.value_type() == ValueType::Object {
            let refcell = unsafe { &*(self.data.ptr as *const RefCell<PhpObject>) };
            Some(refcell.borrow())
        } else {
            None
        }
    }

    /// Get mutable object reference (borrows the RefCell). Only valid for Object values.
    #[inline]
    pub fn as_object_mut(&self) -> Option<std::cell::RefMut<'_, PhpObject>> {
        if self.value_type() == ValueType::Object {
            let refcell = unsafe { &*(self.data.ptr as *const RefCell<PhpObject>) };
            Some(refcell.borrow_mut())
        } else {
            None
        }
    }

    /// Read class_id for Object values without RefCell borrow check.
    /// Single-threaded VM guarantees no concurrent mutations during dispatch.
    /// SAFETY: Only valid when value_type() == ValueType::Object.
    #[inline(always)]
    pub unsafe fn object_class_id_unchecked(&self) -> u32 {
        debug_assert!(self.value_type() == ValueType::Object);
        let refcell = &*(self.data.ptr as *const RefCell<PhpObject>);
        (*refcell.as_ptr()).class_id
    }

    /// Get string reference. Only valid for String values.
    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        if self.value_type() == ValueType::String {
            Some(unsafe { &*(self.data.ptr as *const String) })
        } else {
            None
        }
    }

    /// Get mutable string reference for in-place operations like `.=`.
    /// SAFETY: caller must ensure no other references exist to this string.
    #[inline]
    pub unsafe fn as_string_mut(&mut self) -> Option<&mut String> {
        if self.value_type() == ValueType::String {
            Some(&mut *(self.data.ptr as *mut String))
        } else {
            None
        }
    }

    /// Get array reference. Only valid for Array values.
    #[inline]
    pub fn as_array(&self) -> Option<&PhpArray> {
        if self.value_type() == ValueType::Array {
            Some(unsafe { &*(self.data.ptr as *const PhpArray) })
        } else {
            None
        }
    }

    /// Get mutable array reference. Only valid for Array values.
    #[inline]
    pub fn as_array_mut(&mut self) -> Option<&mut PhpArray> {
        if self.value_type() == ValueType::Array {
            Some(unsafe { &mut *(self.data.ptr as *mut PhpArray) })
        } else {
            None
        }
    }

    #[inline]
    pub fn value_type(&self) -> ValueType {
        // Safety: type_info low byte is always a valid ValueType
        unsafe { std::mem::transmute((self.type_info & 0xFF) as u8) }
    }

    /// Human-readable PHP type name for error messages.
    pub fn type_name(&self) -> &'static str {
        match self.value_type() {
            ValueType::Undef => "unknown",
            ValueType::Null => "null",
            ValueType::False | ValueType::True => "bool",
            ValueType::Long => "int",
            ValueType::Double => "float",
            ValueType::String => "string",
            ValueType::Array => "array",
            ValueType::Object => "object",
            ValueType::Resource => "resource",
            ValueType::Reference => "reference",
            ValueType::Closure => "Closure",
        }
    }

    #[inline]
    pub fn as_long(&self) -> Option<i64> {
        if self.value_type() == ValueType::Long {
            Some(unsafe { self.data.long })
        } else {
            None
        }
    }

    /// Read the raw i64 without type check. SAFETY: caller must guarantee value is Long.
    #[inline(always)]
    pub unsafe fn raw_long(&self) -> i64 {
        self.data.long
    }

    #[inline]
    pub fn as_double(&self) -> Option<f64> {
        if self.value_type() == ValueType::Double {
            Some(unsafe { self.data.double })
        } else {
            None
        }
    }

    /// Convert to f64 for arithmetic (type juggling)
    pub fn to_double(&self) -> Option<f64> {
        match self.value_type() {
            ValueType::Long => Some(unsafe { self.data.long } as f64),
            ValueType::Double => Some(unsafe { self.data.double }),
            _ => None,
        }
    }

    /// Structural equality check for compile-time constant values.
    /// Used for trait property collision detection.
    /// Supports scalars, null, and arrays (recursive). Objects always return false.
    pub fn structurally_equal(&self, other: &Value) -> bool {
        if self.value_type() != other.value_type() {
            return false;
        }
        match self.value_type() {
            ValueType::Undef | ValueType::Null | ValueType::True | ValueType::False => true,
            ValueType::Long => unsafe { self.data.long == other.data.long },
            ValueType::Double => unsafe { self.data.double == other.data.double },
            ValueType::String => self.as_str() == other.as_str(),
            ValueType::Array => {
                let a = self.as_array().unwrap();
                let b = other.as_array().unwrap();
                if a.len() != b.len() {
                    return false;
                }
                a.entries().iter().zip(b.entries().iter()).all(|((ka, va), (kb, vb))| {
                    ka == kb && va.structurally_equal(vb)
                })
            }
            _ => false,
        }
    }

    /// Display value as PHP would echo it
    pub fn echo_to_string(&self) -> String {
        match self.value_type() {
            ValueType::Undef | ValueType::Null => String::new(),
            ValueType::False => String::new(),
            ValueType::True => "1".to_string(),
            ValueType::Long => unsafe { self.data.long }.to_string(),
            ValueType::Double => {
                let d = unsafe { self.data.double };
                if d == d.floor() && d.abs() < 1e15 {
                    format!("{}", d as i64)
                } else {
                    format!("{}", d)
                }
            }
            ValueType::String => {
                unsafe { &*(self.data.ptr as *const String) }.clone()
            }
            ValueType::Array => "Array".to_string(),
            ValueType::Object => {
                let refcell = unsafe { &*(self.data.ptr as *const RefCell<PhpObject>) };
                let obj = refcell.borrow();
                format!("{} Object", obj.class_name)
            }
            _ => "<unsupported>".to_string(),
        }
    }

    /// Check if value is undef
    #[inline]
    pub fn is_undef(&self) -> bool {
        self.value_type() == ValueType::Undef
    }

    /// PHP truthiness — matches PHP's casting rules for (bool).
    #[inline]
    pub fn is_truthy(&self) -> bool {
        match self.value_type() {
            ValueType::Undef | ValueType::Null | ValueType::False => false,
            ValueType::True => true,
            ValueType::Long => (unsafe { self.data.long }) != 0,
            ValueType::Double => (unsafe { self.data.double }) != 0.0,
            ValueType::String => {
                let s = unsafe { &*(self.data.ptr as *const String) };
                !s.is_empty() && s != "0"
            }
            ValueType::Array => {
                let arr = unsafe { &*(self.data.ptr as *const PhpArray) };
                !arr.is_empty()
            }
            _ => true, // objects, resources are truthy
        }
    }

    /// Convert to i64 following PHP type juggling rules
    pub fn to_long_val(&self) -> i64 {
        match self.value_type() {
            ValueType::Long => unsafe { self.data.long },
            ValueType::Double => (unsafe { self.data.double }) as i64,
            ValueType::True => 1,
            ValueType::False | ValueType::Null | ValueType::Undef => 0,
            ValueType::String => {
                let s = unsafe { &*(self.data.ptr as *const String) };
                let s = s.trim();
                if s.is_empty() { return 0; }
                let bytes = s.as_bytes();
                let mut end = 0;
                if bytes[0] == b'-' || bytes[0] == b'+' { end = 1; }
                while end < bytes.len() && bytes[end].is_ascii_digit() { end += 1; }
                if end == 0 || (end == 1 && (bytes[0] == b'-' || bytes[0] == b'+')) { return 0; }
                s[..end].parse().unwrap_or(0)
            }
            _ => 0,
        }
    }

    /// Convert to f64 following PHP type juggling rules (extended)
    pub fn to_float_val(&self) -> f64 {
        match self.value_type() {
            ValueType::Long => (unsafe { self.data.long }) as f64,
            ValueType::Double => unsafe { self.data.double },
            ValueType::True => 1.0,
            ValueType::False | ValueType::Null | ValueType::Undef => 0.0,
            ValueType::String => {
                let s = unsafe { &*(self.data.ptr as *const String) };
                s.trim().parse::<f64>().unwrap_or(0.0)
            }
            _ => 0.0,
        }
    }

    /// Create a boolean value
    #[inline]
    pub fn bool(v: bool) -> Self {
        Self {
            data: ValueData { long: 0 },
            type_info: if v { ValueType::True as u32 } else { ValueType::False as u32 },
            _not_send: PhantomData,
        }
    }

    /// Create a reference value — points to another Value slot (e.g. caller's CV).
    /// SAFETY: `ptr` must remain valid for the lifetime of this Value.
    #[inline]
    pub fn reference(ptr: *mut Value) -> Self {
        Self {
            data: ValueData { ptr: ptr as *mut u8 },
            type_info: ValueType::Reference as u32,
            _not_send: PhantomData,
        }
    }

    /// Check if this value is a reference.
    #[inline]
    pub fn is_reference(&self) -> bool {
        self.value_type() == ValueType::Reference
    }

    #[inline]
    pub fn needs_cleanup(&self) -> bool {
        matches!(self.value_type(), ValueType::String | ValueType::Array | ValueType::Object | ValueType::Closure)
    }

    /// Get the target pointer of a reference value.
    /// SAFETY: only valid when is_reference() is true.
    #[inline]
    pub unsafe fn as_ref_ptr(&self) -> *mut Value {
        self.data.ptr as *mut Value
    }
}

impl Clone for Value {
    #[inline(always)]
    fn clone(&self) -> Self {
        stats::inc_value_clone(self.value_type() as usize);
        match self.value_type() {
            ValueType::String => {
                let s = unsafe { &*(self.data.ptr as *const String) };
                Value::string(s.clone())
            }
            ValueType::Array => {
                let arr = unsafe { &*(self.data.ptr as *const PhpArray) };
                Value::array(arr.clone())
            }
            ValueType::Object => {
                // Clone = Rc increment. No heap allocation.
                unsafe {
                    Rc::increment_strong_count(self.data.ptr as *const RefCell<PhpObject>);
                }
                Self {
                    data: ValueData { ptr: unsafe { self.data.ptr } },
                    type_info: self.type_info,
                    _not_send: PhantomData,
                }
            }
            ValueType::Closure => {
                let c = unsafe { &*(self.data.ptr as *const PhpClosure) };
                Value::closure(c.clone())
            }
            ValueType::Reference => {
                // Clone a reference: clone the TARGET value (dereference + deep clone)
                let target = unsafe { &*(self.data.ptr as *const Value) };
                target.clone()
            }
            _ => Self {
                data: ValueData {
                    long: unsafe { self.data.long },
                },
                type_info: self.type_info,
                _not_send: PhantomData,
            },
        }
    }
}

impl Drop for Value {
    #[inline(always)]
    fn drop(&mut self) {
        stats::inc_value_drop(self.value_type() as usize);
        match self.value_type() {
            ValueType::String => {
                unsafe { drop(Box::from_raw(self.data.ptr as *mut String)) };
            }
            ValueType::Array => {
                unsafe { drop(Box::from_raw(self.data.ptr as *mut PhpArray)) };
            }
            ValueType::Object => {
                // Drop = Rc decrement. Frees PhpObject when refcount reaches 0.
                unsafe { Rc::decrement_strong_count(self.data.ptr as *const RefCell<PhpObject>) };
            }
            ValueType::Closure => {
                unsafe { drop(Box::from_raw(self.data.ptr as *mut PhpClosure)) };
            }
            // Reference doesn't own the target — no-op
            _ => {}
        }
    }
}

/// Create an Error/TypeError/Exception object with a message property.
/// Used by the VM to throw PHP-compatible exceptions.
pub fn make_error_value(class_name: &str, message: &str) -> Value {
    let mut props = std::collections::HashMap::new();
    props.insert("message".to_string(), Value::string(message));
    Value::object(PhpObject {
        class_name: class_name.to_string(),
        class_id: 0, // error objects don't need cache-valid class_id
        properties: props,
        generator: None,
    })
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.value_type() {
            ValueType::Undef => write!(f, "Value(undef)"),
            ValueType::Null => write!(f, "Value(null)"),
            ValueType::False => write!(f, "Value(false)"),
            ValueType::True => write!(f, "Value(true)"),
            ValueType::Long => write!(f, "Value(long={})", unsafe { self.data.long }),
            ValueType::Double => write!(f, "Value(double={})", unsafe { self.data.double }),
            ValueType::String => write!(f, "Value(string={:?})", unsafe { &*(self.data.ptr as *const String) }),
            ValueType::Array => write!(f, "Value(array[{}])", unsafe { &*(self.data.ptr as *const PhpArray) }.len()),
            ValueType::Object => {
                let refcell = unsafe { &*(self.data.ptr as *const RefCell<PhpObject>) };
                let obj = refcell.borrow();
                write!(f, "Value(object({}))", obj.class_name)
            }
            ValueType::Reference => write!(f, "Value(ref={:p})", unsafe { self.data.ptr }),
            ValueType::Closure => write!(f, "Value(Closure)"),
            _ => write!(f, "Value({:?})", self.value_type()),
        }
    }
}
