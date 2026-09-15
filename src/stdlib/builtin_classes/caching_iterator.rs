//! Lookahead and cached projections over the existing traced iterator owner.
//! Callback execution and retirement always happen outside native-state borrows.
use super::*;
use crate::value::{NativeIteratorDelegate, NativeObjectState};
use crate::vm::function::InternalFunctionHandler;

const VALID: i64 = 0x10000;
const FULL: i64 = 0x100;

/// One opaque owner keeps cache growth out of every ordinary Value destructor.
/// Delegation remains accessible through a borrowed native protocol view.
struct State {
    delegate: NativeIteratorDelegate,
    flags: i64,
    text: Value,
    cache: Value,
    // Only recursive caches allocate a child slot. Ordinary caches keep their
    // existing delegate, with no additional edges in the common Value layout.
    child: Option<Box<Value>>,
}

impl Default for State {
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
    fn default() -> Self {
        Self {
            delegate: NativeIteratorDelegate::new(Value::undef(), Value::undef(), 0, -1),
            flags: 0,
            text: Value::undef(),
            cache: Value::undef(),
            child: None,
        }
    }
}

impl NativeObjectState for State {
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        // Public PHP cloning is rejected before this internal storage hook.
        let mut delegate = NativeIteratorDelegate::new(
            self.delegate.inner.clone(),
            self.delegate.iterator.clone(),
            self.delegate.offset,
            self.delegate.limit,
        );
        delegate.current = self.delegate.current.clone();
        delegate.key = self.delegate.key.clone();
        delegate.position = self.delegate.position;
        Box::new(Self {
            delegate,
            flags: self.flags,
            text: self.text.clone(),
            cache: self.cache.clone(),
            child: self.child.clone(),
        })
    }
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
    fn iterator_delegate(&self) -> Option<&NativeIteratorDelegate> {
        Some(&self.delegate)
    }
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
    fn iterator_delegate_mut(&mut self) -> Option<&mut NativeIteratorDelegate> {
        Some(&mut self.delegate)
    }
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
    fn for_each_value(&self, visit: &mut dyn FnMut(&Value)) {
        visit(&self.text);
        visit(&self.cache);
        if let Some(child) = &self.child {
            visit(child);
        }
        self.delegate.for_each_value(visit);
    }
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
    fn append_values_reversed(&mut self, pending: &mut Vec<Value>) {
        // LIFO release: current, key, text, cache, input cursor, input owner.
        for value in [
            &mut self.delegate.inner,
            &mut self.delegate.iterator,
            &mut self.cache,
            &mut self.text,
            &mut self.delegate.key,
            &mut self.delegate.current,
        ] {
            pending.push(std::mem::replace(value, Value::undef()));
        }
        if let Some(child) = self.child.take() {
            pending.push(*child);
        }
    }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn error(eg: &mut ExecutorGlobals, kind: &str, message: &str) {
    eg.exception = Some(make_error_value(kind, message));
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn initialized(receiver: &Value, eg: &mut ExecutorGlobals) -> bool {
    if receiver
        .as_object()
        .is_some_and(|o| o.native_object_state::<State>().is_some())
    {
        return true;
    }
    error(
        eg,
        "Error",
        "The object is in an invalid state as the parent constructor was not called",
    );
    false
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn flags(receiver: &Value) -> i64 {
    receiver
        .as_object()
        .unwrap()
        .native_object_state::<State>()
        .unwrap()
        .flags
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn validate_flags(value: i64, method: &str, number: usize, eg: &mut ExecutorGlobals) -> bool {
    if (value & 15).count_ones() > 1 {
        error(
            eg,
            "ValueError",
            &format!(
                "{method}(): Argument #{number} ($flags) must contain only one of CachingIterator::CALL_TOSTRING, CachingIterator::TOSTRING_USE_KEY, CachingIterator::TOSTRING_USE_CURRENT, or CachingIterator::TOSTRING_USE_INNER"
            ),
        );
        return false;
    }
    true
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    construct_kind(ed, rv, eg, false)
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn construct_recursive(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    construct_kind(ed, rv, eg, true)
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn construct_kind(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    recursive: bool,
) -> Result<(), VmError> {
    let owner = if recursive {
        "RecursiveCachingIterator"
    } else {
        "CachingIterator"
    };
    let method = if recursive {
        "RecursiveCachingIterator::__construct"
    } else {
        "CachingIterator::__construct"
    };
    let receiver = owned_argument(ed, 0);
    if receiver
        .as_object()
        .unwrap()
        .native_iterator_delegate()
        .is_some()
    {
        error(
            eg,
            "BadMethodCallException",
            &format!("{owner}::getIterator() must be called exactly once per instance"),
        );
        return Ok(());
    }
    let inner = owned_argument(ed, 1);
    let protocol = if recursive {
        "RecursiveIterator"
    } else {
        "Iterator"
    };
    if !inner
        .as_object()
        .is_some_and(|o| eg.class_is_a(&o.class_name, protocol))
    {
        typed_internal_argument_error(eg, method, &inner, 1, "iterator", protocol);
        return Ok(());
    }
    let mut options = 1;
    if arg_opt!(ed, 2).is_some() {
        let value = owned_argument(ed, 2);
        let Some(value) =
            typed_internal_int_value_argument_expected(ed, eg, &value, method, 1, "flags", "int")?
        else {
            return Ok(());
        };
        options = value;
    }
    if !validate_flags(options, method, 2, eg) {
        return Ok(());
    }
    initialize(&receiver, inner, options, recursive, eg);
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn initialize(
    receiver: &Value,
    inner: Value,
    options: i64,
    recursive: bool,
    eg: &mut ExecutorGlobals,
) {
    let iterator = super::deque::consumer(&inner, eg).unwrap_or_else(|| inner.clone());
    let state = State {
        delegate: NativeIteratorDelegate::new(inner, iterator, 0, -1),
        flags: options & 0xffff,
        text: Value::string(""),
        cache: Value::array(PhpArray::new()),
        child: recursive.then(|| Box::new(Value::null())),
    };
    *receiver
        .as_object_mut()
        .unwrap()
        .native_object_state_mut::<State>() = state;
}

/// Children are base RecursiveCachingIterator objects, never copies of a
/// user subclass. Publishing their state does not execute a user constructor.
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
pub(super) fn recursive_wrapper(inner: &Value, options: i64, eg: &mut ExecutorGlobals) -> Value {
    if !inner
        .as_object()
        .is_some_and(|o| eg.class_is_a(&o.class_name, "RecursiveIterator"))
    {
        typed_internal_argument_error(
            eg,
            "RecursiveCachingIterator::__construct",
            inner,
            1,
            "iterator",
            "RecursiveIterator",
        );
        return Value::null();
    }
    if !validate_flags(options, "RecursiveCachingIterator::__construct", 2, eg) {
        return Value::null();
    }
    let class = eg
        .find_class("RecursiveCachingIterator")
        .expect("registered recursive cache");
    let receiver = Value::object(PhpObject::with_layout_from_defaults(
        class.class_id,
        class.property_layout.clone(),
        class.property_defaults.as_ref(),
    ));
    initialize(&receiver, inner.clone(), options, true, eg);
    receiver
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn reset_projection(receiver: &Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let (text, child) = {
        let mut object = receiver.as_object_mut().unwrap();
        let cache = object.native_object_state_mut::<State>();
        cache.flags &= !VALID;
        let child = cache
            .child
            .as_mut()
            .map(|value| std::mem::replace(&mut **value, Value::null()));
        (std::mem::replace(&mut cache.text, Value::string("")), child)
    };
    if let Some(child) = child {
        iterator_delegate::discard(child, eg)?;
    }
    iterator_delegate::clear(receiver, eg)?;
    iterator_delegate::discard(text, eg)
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn empty_cache(receiver: &Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let retired = {
        let mut object = receiver.as_object_mut().unwrap();
        let cache = object.native_object_state_mut::<State>();
        std::mem::replace(&mut cache.cache, Value::array(PhpArray::new()))
    };
    iterator_delegate::discard(retired, eg)
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
pub(super) fn string_value(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    source: &Value,
) -> Result<Value, VmError> {
    let value = source.dereferenced();
    match value.value_type() {
        ValueType::String => Ok(value.clone()),
        ValueType::Object => {
            let result = crate::vm::execute::call_object_string_conversion(eg, value)?;
            if let Some(value) = result {
                return Ok(value);
            }
            if eg.exception.is_none() {
                error(
                    eg,
                    "Error",
                    &format!(
                        "Object of class {} could not be converted to string",
                        value.as_object().unwrap().class_name
                    ),
                );
            }
            Ok(Value::string(""))
        }
        ValueType::Array => {
            report_internal_diagnostic(eg, ed, 2, "Warning", "Array to string conversion")?;
            Ok(Value::string("Array"))
        }
        _ => Ok(Value::string(
            value.echo_to_string_with_precision(eg.precision),
        )),
    }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn advance(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    rewind: bool,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    reset_projection(&receiver, eg)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let inner = iterator_delegate::inner(&receiver);
    if rewind {
        empty_cache(&receiver, eg)?;
        if eg.exception.is_some() {
            return Ok(());
        }
        iterator_delegate::protocol(eg, &inner, "rewind")?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    let valid = iterator_delegate::protocol(eg, &inner, "valid")?;
    if eg.exception.is_some() || !valid.is_truthy() {
        ret!(rv, Value::null());
    }
    let current = iterator_delegate::protocol(eg, &inner, "current")?;
    if eg.exception.is_some() {
        return Ok(());
    }
    receiver
        .as_object_mut()
        .unwrap()
        .native_iterator_delegate_mut()
        .unwrap()
        .current = current;
    let key = iterator_delegate::protocol(eg, &inner, "key")?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let recursive = {
        let mut object = receiver.as_object_mut().unwrap();
        let state = object.native_object_state_mut::<State>();
        state.delegate.key = key;
        state.flags |= VALID;
        state.child.is_some()
    };
    // FULL_CACHE is committed before child acquisition or string conversion.
    // Both callbacks can throw or reenter and must observe that publication.
    if flags(&receiver) & FULL != 0 {
        let (key, value) = {
            let object = receiver.as_object().unwrap();
            let state = object.native_iterator_delegate().unwrap();
            (
                state.key.dereferenced().clone(),
                state.current.dereferenced().clone(),
            )
        };
        // Native integer keys already have their durable cache representation;
        // avoid allocating and reparsing a decimal string for every element.
        let key = if key.value_type() == ValueType::Long {
            key
        } else {
            string_value(ed, eg, &key)?
        };
        if eg.exception.is_some() {
            return Ok(());
        }
        store(&receiver, &key, value, eg)?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    if recursive {
        capture_child(&receiver, &inner, eg)?;
    }
    if eg.exception.is_none() {
        let mode = flags(&receiver) & 15;
        if mode == 1 || mode == 8 {
            let source = if mode == 8 {
                inner.clone()
            } else {
                receiver
                    .as_object()
                    .unwrap()
                    .native_iterator_delegate()
                    .unwrap()
                    .current
                    .clone()
            };
            let text = string_value(ed, eg, &source)?;
            if eg.exception.is_none() {
                receiver
                    .as_object_mut()
                    .unwrap()
                    .native_object_state_mut::<State>()
                    .text = text;
            }
        }
    }
    // Native cursor movement still completes with a pending conversion error.
    // Do not enter a fresh user call frame, which would replace that error.
    if eg.exception.is_none() || array_object::cursor::native_protocol(&inner, eg) {
        iterator_delegate::protocol(eg, &inner, "next")?;
    }
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn capture_child(receiver: &Value, inner: &Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let has = call_object_protocol_method(eg, inner, "RecursiveIterator", "hasChildren", &[])?;
    if eg.exception.is_none() && has.is_some_and(|value| value.is_truthy()) {
        let child =
            call_object_protocol_method(eg, inner, "RecursiveIterator", "getChildren", &[])?
                .unwrap_or_else(Value::null);
        if eg.exception.is_none() {
            let cached = recursive_wrapper(&child, flags(receiver), eg);
            if eg.exception.is_none() {
                let retired = {
                    let mut object = receiver.as_object_mut().unwrap();
                    let slot = object
                        .native_object_state_mut::<State>()
                        .child
                        .as_mut()
                        .expect("recursive cache");
                    std::mem::replace(&mut **slot, cached)
                };
                iterator_delegate::discard(retired, eg)?;
            }
        }
        iterator_delegate::discard(child, eg)?;
    }
    if eg.exception.is_some() && flags(receiver) & 16 != 0 {
        let exception = eg.exception.take().expect("child exception");
        iterator_delegate::discard(exception, eg)?;
    }
    Ok(())
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn has_children(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let has = receiver
        .as_object()
        .unwrap()
        .native_object_state::<State>()
        .unwrap()
        .child
        .as_ref()
        .is_some_and(|value| value.value_type() == ValueType::Object);
    ret!(rv, Value::bool(has));
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn get_children(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let child = receiver
        .as_object()
        .unwrap()
        .native_object_state::<State>()
        .unwrap()
        .child
        .as_ref()
        .map_or_else(Value::null, |value| (**value).clone());
    ret!(rv, child);
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn rewind(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    advance(ed, rv, eg, true)
}
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn next(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    advance(ed, rv, eg, false)
}
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn valid(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    ret!(rv, Value::bool(flags(&receiver) & VALID != 0));
}
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn has_next(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let next = iterator_delegate::protocol(eg, &iterator_delegate::inner(&receiver), "valid")?;
    ret!(rv, Value::bool(next.is_truthy()));
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let mode = flags(&receiver) & 15;
    if mode == 0 {
        let class = receiver.as_object().unwrap().class_name.to_string();
        error(
            eg,
            "BadMethodCallException",
            &format!("{class} does not fetch string value (see CachingIterator::__construct)"),
        );
        return Ok(());
    }
    let source = {
        let object = receiver.as_object().unwrap();
        let state = object.native_object_state::<State>().unwrap();
        match mode {
            2 => state.delegate.key.clone(),
            4 => state.delegate.current.clone(),
            _ => state.text.clone(),
        }
    };
    let result = string_value(ed, eg, &source)?;
    ret!(rv, result);
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn get_flags(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    ret!(rv, Value::long(flags(&receiver)));
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn set_flags(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let value = owned_argument(ed, 1);
    let Some(value) = typed_internal_int_value_argument_expected(
        ed,
        eg,
        &value,
        "CachingIterator::setFlags",
        0,
        "flags",
        "int",
    )?
    else {
        return Ok(());
    };
    if !validate_flags(value, "CachingIterator::setFlags", 1, eg) {
        return Ok(());
    }
    let previous = flags(&receiver);
    for (bit, name) in [(1, "CALL_TO_STRING"), (8, "TOSTRING_USE_INNER")] {
        if previous & bit != 0 && value & bit == 0 {
            error(
                eg,
                "InvalidArgumentException",
                &format!("Unsetting flag {name} is not possible"),
            );
            return Ok(());
        }
    }
    receiver
        .as_object_mut()
        .unwrap()
        .native_object_state_mut::<State>()
        .flags = (previous & !0xffff) | (value & 0xffff);
    if value & FULL != 0 && previous & FULL == 0 {
        empty_cache(&receiver, eg)?;
    }
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn full_cache(receiver: &Value, eg: &mut ExecutorGlobals) -> bool {
    if !initialized(receiver, eg) {
        return false;
    }
    if flags(receiver) & FULL != 0 {
        return true;
    }
    let class = receiver.as_object().unwrap().class_name.to_string();
    error(
        eg,
        "BadMethodCallException",
        &format!("{class} does not use a full cache (see CachingIterator::__construct)"),
    );
    false
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn get_cache(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !full_cache(&receiver, eg) {
        return Ok(());
    }
    let object = receiver.as_object().unwrap();
    ret!(
        rv,
        object.native_object_state::<State>().unwrap().cache.clone()
    );
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn count(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !full_cache(&receiver, eg) {
        return Ok(());
    }
    let object = receiver.as_object().unwrap();
    ret!(
        rv,
        Value::long(
            object
                .native_object_state::<State>()
                .unwrap()
                .cache
                .as_array()
                .unwrap()
                .len() as i64
        )
    );
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn normalized_key(array: &PhpArray, key: &Value) -> ArrayKey {
    let key_value = crate::vm::execute::value_to_array_key(key)
        .unwrap_or_else(|_| unreachable!("validated string"));
    array.normalize_string_key(key_value, key)
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn cached_slot<'a>(array: &'a PhpArray, key: &ArrayKey) -> Option<&'a Value> {
    match key {
        ArrayKey::Int(key) => array.get_int(*key),
        ArrayKey::String(key) => array.get_str(key),
    }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn store(
    receiver: &Value,
    key: &Value,
    value: Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let retired = {
        let mut object = receiver.as_object_mut().unwrap();
        let state = object.native_object_state_mut::<State>();
        let array = state.cache.as_array_mut().unwrap();
        let normalized = array.prepare_string_key_for_write(
            crate::vm::execute::value_to_array_key(key)
                .unwrap_or_else(|_| unreachable!("validated integer or string")),
            key,
        );
        let old = cached_slot(&array, &normalized).cloned();
        array.set(normalized, value);
        old
    };
    if let Some(value) = retired {
        iterator_delegate::discard(value, eg)?;
    }
    Ok(())
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn offset(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    operation: usize,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let method = [
        "CachingIterator::offsetGet",
        "CachingIterator::offsetSet",
        "CachingIterator::offsetUnset",
        "CachingIterator::offsetExists",
    ][operation];
    let key = owned_argument(ed, 1);
    let Some(key) =
        typed_internal_string_value_expected(ed, eg, &key, method, 0, "key", "string", "string")?
    else {
        return Ok(());
    };
    if !full_cache(&receiver, eg) {
        return Ok(());
    }
    if operation == 1 {
        let value = owned_argument(ed, 2).dereferenced().clone();
        store(&receiver, &key, value, eg)?;
        ret!(rv, Value::null());
    }
    let (normalized, found) = {
        let object = receiver.as_object().unwrap();
        let array = object
            .native_object_state::<State>()
            .unwrap()
            .cache
            .as_array()
            .unwrap();
        let normalized = normalized_key(array, &key);
        let found = cached_slot(array, &normalized).cloned();
        (normalized, found)
    };
    if operation == 3 {
        ret!(rv, Value::bool(found.is_some()));
    }
    if operation == 2 {
        {
            let mut object = receiver.as_object_mut().unwrap();
            object
                .native_object_state_mut::<State>()
                .cache
                .as_array_mut()
                .unwrap()
                .remove(&normalized);
        }
        if let Some(value) = found {
            iterator_delegate::discard(value, eg)?;
        }
        ret!(rv, Value::null());
    }
    if let Some(value) = found {
        ret!(rv, value);
    }
    // The public offset parameter is a string even when lookup normalizes it
    // to an integer array index. Diagnostics retain that validated spelling.
    let displayed = format!("\"{}\"", key.echo_to_string());
    report_internal_diagnostic(
        eg,
        ed,
        2,
        "Warning",
        &format!("Undefined array key {displayed}"),
    )?;
    ret!(rv, Value::null());
}

macro_rules! offset_handler {
    ($name:ident,$operation:expr) => {
        #[cold]
        #[inline(never)]
        #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
        fn $name(
            ed: *mut ExecuteData,
            rv: *mut Value,
            eg: &mut ExecutorGlobals,
        ) -> Result<(), VmError> {
            offset(ed, rv, eg, $operation)
        }
    };
}
offset_handler!(offset_get, 0);
offset_handler!(offset_set, 1);
offset_handler!(offset_unset, 2);
offset_handler!(offset_exists, 3);

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{Array, Bool, ClassName, Int, Mixed, None, String, Void};
    let mut class = empty_internal_type(
        "CachingIterator",
        vec![
            "ArrayAccess".into(),
            "Countable".into(),
            "Stringable".into(),
        ],
        false,
        false,
    );
    class.parent = Some("IteratorIterator".into());
    for (name, value) in [
        ("CALL_TOSTRING", 1),
        ("CATCH_GET_CHILD", 16),
        ("TOSTRING_USE_KEY", 2),
        ("TOSTRING_USE_CURRENT", 4),
        ("TOSTRING_USE_INNER", 8),
        ("FULL_CACHE", 256),
    ] {
        class
            .constants
            .push(recursive_iterator::constant("CachingIterator", name, value));
    }
    eg.register_class_with_complete_native_parent(class)
        .unwrap();
    eg.reserve_internal_method_contracts("CachingIterator", 14);
    let mut functions = Vec::with_capacity(14);
    let entries: &[(&str, InternalFunctionHandler, &[&str])] = &[
        ("rewind", rewind, &[]),
        ("valid", valid, &[]),
        ("next", next, &[]),
        ("hasNext", has_next, &[]),
        ("__toString", to_string, &[]),
        ("getFlags", get_flags, &[]),
        ("getCache", get_cache, &[]),
        ("count", count, &[]),
    ];
    recursive_iterator::register_method(
        eg,
        &mut functions,
        "CachingIterator",
        "__construct",
        construct,
        &["iterator", "flags"],
        vec![ClassName("Iterator".into()), Int],
        &[Option::None, Some("CachingIterator::CALL_TOSTRING")],
        None,
    );
    let pointer = &functions.last().unwrap().common as *const FunctionCommon;
    eg.register_internal_function_reflection_metadata(
        pointer,
        vec![Option::None, Some(Value::long(1))],
        "SPL",
    );
    for &(name, handler, names) in entries {
        let result = match name {
            "valid" | "hasNext" => Bool,
            "__toString" => String,
            "getFlags" | "count" => Int,
            "getCache" => Array,
            _ => Void,
        };
        recursive_iterator::register_method(
            eg,
            &mut functions,
            "CachingIterator",
            name,
            handler,
            names,
            vec![],
            &[],
            result,
        );
    }
    recursive_iterator::register_method(
        eg,
        &mut functions,
        "CachingIterator",
        "setFlags",
        set_flags,
        &["flags"],
        vec![Int],
        &[Option::None],
        Void,
    );
    for (name, handler, result) in [
        ("offsetGet", offset_get as InternalFunctionHandler, Mixed),
        ("offsetUnset", offset_unset, Void),
        ("offsetExists", offset_exists, Bool),
    ] {
        recursive_iterator::register_method(
            eg,
            &mut functions,
            "CachingIterator",
            name,
            handler,
            &["key"],
            vec![None],
            &[Option::None],
            result,
        );
    }
    recursive_iterator::register_method(
        eg,
        &mut functions,
        "CachingIterator",
        "offsetSet",
        offset_set,
        &["key", "value"],
        vec![None, Mixed],
        &[Option::None, Option::None],
        Void,
    );
    functions
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
pub(super) fn register_recursive(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{Bool, ClassName, Int, Nullable};
    let owner = "RecursiveCachingIterator";
    let mut class = empty_internal_type(owner, vec!["RecursiveIterator".into()], false, false);
    class.parent = Some("CachingIterator".into());
    eg.register_class_with_complete_native_parent(class)
        .unwrap();
    eg.reserve_internal_method_contracts(owner, 3);
    let mut functions = Vec::with_capacity(3);
    recursive_iterator::register_method(
        eg,
        &mut functions,
        owner,
        "__construct",
        construct_recursive,
        &["iterator", "flags"],
        vec![ClassName("Iterator".into()), Int],
        &[None, Some("RecursiveCachingIterator::CALL_TOSTRING")],
        ParamTypeHint::None,
    );
    let pointer = &functions.last().unwrap().common as *const FunctionCommon;
    eg.register_internal_function_reflection_metadata_with_diagnostics(
        pointer,
        vec![None, Some(Value::long(1))],
        &[None, Some("RecursiveCachingIterator::CALL_TOSTRING")],
        "SPL",
    );
    recursive_iterator::register_method(
        eg,
        &mut functions,
        owner,
        "hasChildren",
        has_children,
        &[],
        vec![],
        &[],
        Bool,
    );
    recursive_iterator::register_method(
        eg,
        &mut functions,
        owner,
        "getChildren",
        get_children,
        &[],
        vec![],
        &[],
        Nullable(Box::new(ClassName(owner.into()))),
    );
    functions
}
