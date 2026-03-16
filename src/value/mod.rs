use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::cell::RefCell;

/// PHP object — class instance with properties.
#[derive(Debug, Clone)]
pub struct PhpObject {
    pub class_name: String,
    pub properties: HashMap<String, Value>,
}

/// PHP array — ordered hash map with integer and string keys.
/// Preserves insertion order, supports auto-incrementing integer keys.
/// Uses HashMap index for O(1) key lookup + Vec for insertion order.
#[derive(Debug, Clone)]
pub struct PhpArray {
    /// Insertion-ordered entries
    entries: Vec<(ArrayKey, Value)>,
    /// Key → index into entries for O(1) lookup
    index: HashMap<ArrayKey, usize>,
    next_int_key: i64,
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
            index: HashMap::new(),
            next_int_key: 0,
        }
    }

    /// Append with auto-incrementing key ($a[] = val)
    pub fn push(&mut self, val: Value) {
        let key = self.next_int_key;
        self.next_int_key = key + 1;
        let ak = ArrayKey::Int(key);
        let idx = self.entries.len();
        self.entries.push((ak.clone(), val));
        self.index.insert(ak, idx);
    }

    /// Set by integer key
    pub fn set_int(&mut self, key: i64, val: Value) {
        let ak = ArrayKey::Int(key);
        if let Some(&idx) = self.index.get(&ak) {
            self.entries[idx].1 = val;
        } else {
            let idx = self.entries.len();
            self.entries.push((ak.clone(), val));
            self.index.insert(ak, idx);
            if key >= self.next_int_key {
                self.next_int_key = key + 1;
            }
        }
    }

    /// Set by string key
    pub fn set_str(&mut self, key: &str, val: Value) {
        let ak = ArrayKey::String(key.to_string());
        if let Some(&idx) = self.index.get(&ak) {
            self.entries[idx].1 = val;
        } else {
            let idx = self.entries.len();
            self.entries.push((ak.clone(), val));
            self.index.insert(ak, idx);
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
    pub fn get_int(&self, key: i64) -> Option<&Value> {
        let ak = ArrayKey::Int(key);
        self.index.get(&ak).map(|&idx| &self.entries[idx].1)
    }

    /// Get by string key — O(1)
    pub fn get_str(&self, key: &str) -> Option<&Value> {
        let ak = ArrayKey::String(key.to_string());
        self.index.get(&ak).map(|&idx| &self.entries[idx].1)
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
        if let Some(&idx) = self.index.get(key) {
            self.entries.remove(idx);
            self.index.remove(key);
            // Re-index entries after removed position
            for (i, (k, _)) in self.entries.iter().enumerate() {
                if i >= idx {
                    self.index.insert(k.clone(), i);
                }
            }
            true
        } else {
            false
        }
    }

    /// Remove and return last element
    pub fn pop(&mut self) -> Option<Value> {
        if let Some((key, val)) = self.entries.pop() {
            self.index.remove(&key);
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
            let (key, val) = self.entries.remove(0);
            self.index.remove(&key);
            // Re-index all entries since indices shifted
            for (i, (k, _)) in self.entries.iter().enumerate() {
                self.index.insert(k.clone(), i);
            }
            Some(val)
        }
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
    #[inline]
    pub fn object(obj: PhpObject) -> Self {
        let rc = Rc::new(RefCell::new(obj));
        let boxed = Box::new(rc);
        Self {
            data: ValueData { ptr: Box::into_raw(boxed) as *mut u8 },
            type_info: ValueType::Object as u32,
            _not_send: PhantomData,
        }
    }

    /// Get the Rc<RefCell<PhpObject>> for shared access.
    #[inline]
    pub fn as_object_rc(&self) -> Option<&Rc<RefCell<PhpObject>>> {
        if self.value_type() == ValueType::Object {
            Some(unsafe { &*(self.data.ptr as *const Rc<RefCell<PhpObject>>) })
        } else {
            None
        }
    }

    /// Get object reference (borrows the RefCell). Only valid for Object values.
    #[inline]
    pub fn as_object(&self) -> Option<std::cell::Ref<'_, PhpObject>> {
        if self.value_type() == ValueType::Object {
            let rc = unsafe { &*(self.data.ptr as *const Rc<RefCell<PhpObject>>) };
            Some(rc.borrow())
        } else {
            None
        }
    }

    /// Get mutable object reference (borrows the RefCell). Only valid for Object values.
    #[inline]
    pub fn as_object_mut(&self) -> Option<std::cell::RefMut<'_, PhpObject>> {
        if self.value_type() == ValueType::Object {
            let rc = unsafe { &*(self.data.ptr as *const Rc<RefCell<PhpObject>>) };
            Some(rc.borrow_mut())
        } else {
            None
        }
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

    #[inline]
    pub fn as_long(&self) -> Option<i64> {
        if self.value_type() == ValueType::Long {
            Some(unsafe { self.data.long })
        } else {
            None
        }
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
                let rc = unsafe { &*(self.data.ptr as *const Rc<RefCell<PhpObject>>) };
                let obj = rc.borrow();
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
}

impl Clone for Value {
    fn clone(&self) -> Self {
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
                // Clone shares the Rc — PHP objects are reference types
                let rc = unsafe { &*(self.data.ptr as *const Rc<RefCell<PhpObject>>) };
                let cloned_rc = rc.clone();
                let boxed = Box::new(cloned_rc);
                Self {
                    data: ValueData { ptr: Box::into_raw(boxed) as *mut u8 },
                    type_info: self.type_info,
                    _not_send: PhantomData,
                }
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
    fn drop(&mut self) {
        match self.value_type() {
            ValueType::String => {
                unsafe { drop(Box::from_raw(self.data.ptr as *mut String)) };
            }
            ValueType::Array => {
                unsafe { drop(Box::from_raw(self.data.ptr as *mut PhpArray)) };
            }
            ValueType::Object => {
                unsafe { drop(Box::from_raw(self.data.ptr as *mut Rc<RefCell<PhpObject>>)) };
            }
            _ => {}
        }
    }
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
                let rc = unsafe { &*(self.data.ptr as *const Rc<RefCell<PhpObject>>) };
                let obj = rc.borrow();
                write!(f, "Value(object({}))", obj.class_name)
            }
            _ => write!(f, "Value({:?})", self.value_type()),
        }
    }
}
