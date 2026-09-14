//! Public mutable iterator list and independently selected inner cursor.
//! All PHP values are traced; no borrow spans callbacks or explicit release.
use super::*;
use crate::value::{NativeIteratorDelegate, NativeObjectState};
use crate::vm::function::InternalFunctionHandler;

struct State {
    delegate: NativeIteratorDelegate,
    list: Value,
}

impl Default for State {
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
    fn default() -> Self {
        Self {
            delegate: NativeIteratorDelegate::new(Value::null(), Value::null(), 0, -1),
            list: Value::undef(),
        }
    }
}

impl NativeObjectState for State {
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        // PHP cloning is rejected; this hook is internal storage ownership.
        let mut delegate = NativeIteratorDelegate::new(
            self.delegate.inner.clone(),
            self.delegate.iterator.clone(),
            0,
            -1,
        );
        delegate.current = self.delegate.current.clone();
        delegate.key = self.delegate.key.clone();
        Box::new(Self {
            delegate,
            list: self.list.clone(),
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
        self.delegate.for_each_value(&mut *visit);
        visit(&self.list);
    }
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
    fn append_values_reversed(&mut self, pending: &mut Vec<Value>) {
        for value in [
            &mut self.list,
            &mut self.delegate.inner,
            &mut self.delegate.iterator,
            &mut self.delegate.key,
            &mut self.delegate.current,
        ] {
            pending.push(std::mem::replace(value, Value::undef()));
        }
    }
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
    eg.exception = Some(make_error_value(
        "Error",
        "The object is in an invalid state as the parent constructor was not called",
    ));
    false
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn list(receiver: &Value) -> Value {
    receiver
        .as_object()
        .unwrap()
        .native_object_state::<State>()
        .unwrap()
        .list
        .clone()
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if receiver
        .as_object()
        .unwrap()
        .native_iterator_delegate()
        .is_some()
    {
        eg.exception = Some(make_error_value(
            "BadMethodCallException",
            "AppendIterator::getIterator() must be called exactly once per instance",
        ));
        return Ok(());
    }
    let class = eg
        .find_class("ArrayIterator")
        .expect("registered list class");
    let list = Value::object(PhpObject::with_layout_from_defaults(
        class.class_id,
        class.property_layout.clone(),
        class.property_defaults.as_ref(),
    ));
    receiver
        .as_object_mut()
        .unwrap()
        .native_object_state_mut::<State>()
        .list = list;
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn deselect(receiver: &Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    iterator_delegate::clear(receiver, eg)?;
    let (iterator, inner) = {
        let mut object = receiver.as_object_mut().unwrap();
        let delegate = object.native_iterator_delegate_mut().unwrap();
        (
            std::mem::replace(&mut delegate.iterator, Value::null()),
            std::mem::replace(&mut delegate.inner, Value::null()),
        )
    };
    iterator_delegate::discard(iterator, eg)?;
    iterator_delegate::discard(inner, eg)
}

/// The caller has already observed valid(). Publication is intentionally
/// incremental: a key callback failure does not erase a successful current.
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn fetch_valid(
    receiver: &Value,
    iterator: &Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let current = iterator_delegate::protocol(eg, iterator, "current")?;
    if eg.exception.is_some() {
        return Ok(());
    }
    receiver
        .as_object_mut()
        .unwrap()
        .native_iterator_delegate_mut()
        .unwrap()
        .current = current;
    let key = iterator_delegate::protocol(eg, iterator, "key")?;
    if eg.exception.is_none() {
        receiver
            .as_object_mut()
            .unwrap()
            .native_iterator_delegate_mut()
            .unwrap()
            .key = key;
    }
    Ok(())
}

/// Select using the live public ArrayIterator position, never a snapshot of
/// its contents. A failed/empty candidate is consumed before returning error.
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn select(receiver: &Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let list = list(receiver);
    while iterator_delegate::protocol(eg, &list, "valid")?.is_truthy() {
        let inner = iterator_delegate::protocol(eg, &list, "current")?
            .dereferenced()
            .clone();
        let valid_type = inner
            .as_object()
            .is_some_and(|o| eg.class_is_a(&o.class_name, "Iterator"));
        if !valid_type {
            eg.exception = Some(make_error_value(
                "UnexpectedValueException",
                "ArrayIterator contains a value that is not an Iterator",
            ));
        }
        let iterator = if valid_type {
            super::deque::consumer(&inner, eg).unwrap_or_else(|| inner.clone())
        } else {
            Value::null()
        };
        let closed = iterator
            .as_object()
            .and_then(|o| o.generator.clone())
            .is_some_and(|g| g.borrow().state == crate::vm::generator::GeneratorState::Completed);
        if closed {
            eg.exception = Some(make_error_value(
                "Exception",
                "Cannot traverse an already closed generator",
            ));
        }
        if eg.exception.is_none() {
            // A callback can append to its owner while this cursor rewinds.
            // Publish the selected owner first so that append does not rewind
            // it a second time. Failed selection retires it below.
            let mut object = receiver.as_object_mut().unwrap();
            let delegate = object.native_iterator_delegate_mut().unwrap();
            delegate.inner = inner.clone();
            delegate.iterator = iterator.clone();
        }
        if eg.exception.is_none() {
            iterator_delegate::protocol(eg, &iterator, "rewind")?;
        }
        let valid = if eg.exception.is_none() {
            iterator_delegate::protocol(eg, &iterator, "valid")?.is_truthy()
        } else {
            false
        };
        if valid && eg.exception.is_none() {
            return fetch_valid(receiver, &iterator, eg);
        }
        deselect(receiver, eg)?;
        iterator_delegate::protocol(eg, &list, "next")?;
        iterator_delegate::discard(iterator, eg)?;
        iterator_delegate::discard(inner, eg)?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    Ok(())
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn append(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let inner = owned_argument(ed, 1);
    if !inner
        .as_object()
        .is_some_and(|o| eg.class_is_a(&o.class_name, "Iterator"))
    {
        typed_internal_argument_error(
            eg,
            "AppendIterator::append",
            &inner,
            1,
            "iterator",
            "Iterator",
        );
        return Ok(());
    }
    let list = list(&receiver);
    call_object_protocol_method(eg, &list, "ArrayIterator", "append", &[inner])?;
    if eg.exception.is_none() && iterator_delegate::inner(&receiver).value_type() == ValueType::Null
    {
        select(&receiver, eg)?;
    }
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn rewind(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    deselect(&receiver, eg)?;
    if eg.exception.is_none() {
        iterator_delegate::protocol(eg, &list(&receiver), "rewind")?;
        select(&receiver, eg)?;
    }
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn next(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    iterator_delegate::clear(&receiver, eg)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let iterator = iterator_delegate::inner(&receiver);
    if iterator.value_type() != ValueType::Null {
        let valid = iterator_delegate::protocol(eg, &iterator, "valid")?;
        if eg.exception.is_none() && valid.is_truthy() {
            iterator_delegate::protocol(eg, &iterator, "next")?;
        }
        if eg.exception.is_none()
            && iterator_delegate::protocol(eg, &iterator, "valid")?.is_truthy()
            && eg.exception.is_none()
        {
            fetch_valid(&receiver, &iterator, eg)?;
            ret!(rv, Value::null());
        }
    }
    deselect(&receiver, eg)?;
    iterator_delegate::protocol(eg, &list(&receiver), "next")?;
    if eg.exception.is_none() {
        select(&receiver, eg)?;
    }
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn current(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    iterator_delegate::clear(&receiver, eg)?;
    let iterator = iterator_delegate::inner(&receiver);
    if eg.exception.is_none() && iterator.value_type() != ValueType::Null {
        iterator_delegate::fetch(&receiver, eg)?;
    }
    let value = receiver
        .as_object()
        .unwrap()
        .native_iterator_delegate()
        .unwrap()
        .current
        .dereferenced()
        .clone();
    ret!(
        rv,
        if value.is_undef() {
            Value::null()
        } else {
            value
        }
    );
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn get_list(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    ret!(rv, list(&receiver));
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn get_index(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    ret!(
        rv,
        iterator_delegate::protocol(eg, &list(&receiver), "key")?
    );
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{ClassName, Int, Mixed, Nullable, Void};
    let mut class = empty_internal_type("AppendIterator", vec![], false, false);
    class.parent = Some("IteratorIterator".into());
    eg.register_class(class).unwrap();
    let mut functions = Vec::with_capacity(7);
    for (name, handler, result) in [
        (
            "__construct",
            construct as InternalFunctionHandler,
            ParamTypeHint::None,
        ),
        ("rewind", rewind, Void),
        ("next", next, Void),
        ("current", current, Mixed),
        (
            "getArrayIterator",
            get_list,
            ClassName("ArrayIterator".into()),
        ),
        ("getIteratorIndex", get_index, Nullable(Box::new(Int))),
    ] {
        recursive_iterator::register_method(
            eg,
            &mut functions,
            "AppendIterator",
            name,
            handler,
            &[],
            vec![],
            &[],
            result,
        );
    }
    recursive_iterator::register_method(
        eg,
        &mut functions,
        "AppendIterator",
        "append",
        append,
        &["iterator"],
        vec![ClassName("Iterator".into())],
        &[None],
        Void,
    );
    functions
}
