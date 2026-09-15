//! Tree decoration over the existing recursive driver and cached child cursor.
//! Only tree instances own prefix state; traversal and PHP lifetime handling
//! remain shared with RecursiveIteratorIterator.
use super::*;
use crate::value::NativeObjectState;
use crate::vm::function::InternalFunctionHandler;

struct State {
    flags: i64,
    parts: [Value; 6],
    postfix: Value,
}

impl Default for State {
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
    fn default() -> Self {
        Self {
            flags: 8,
            parts: ["", "| ", "  ", "|-", "\\-", ""].map(Value::string),
            postfix: Value::string(""),
        }
    }
}

impl NativeObjectState for State {
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        Box::new(Self {
            flags: self.flags,
            parts: self.parts.clone(),
            postfix: self.postfix.clone(),
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
    fn for_each_value(&self, visit: &mut dyn FnMut(&Value)) {
        for part in &self.parts {
            visit(part);
        }
        visit(&self.postfix);
    }
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
    fn append_values_reversed(&mut self, pending: &mut Vec<Value>) {
        pending.push(std::mem::replace(&mut self.postfix, Value::undef()));
        for part in self.parts.iter_mut().rev() {
            pending.push(std::mem::replace(part, Value::undef()));
        }
    }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn initialized(receiver: &Value, eg: &mut ExecutorGlobals) -> bool {
    if !recursive_iterator::initialized(receiver, eg) {
        return false;
    }
    if receiver
        .as_object()
        .unwrap()
        .native_object_state::<State>()
        .is_some()
    {
        return true;
    }
    eg.exception = Some(make_error_value(
        "Error",
        &format!(
            "The {} instance wasn't initialized properly",
            receiver.as_object().unwrap().class_name
        ),
    ));
    false
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    const METHOD: &str = "RecursiveTreeIterator::__construct";
    let receiver = owned_argument(ed, 0);
    let mut iterator = owned_argument(ed, 1);
    if iterator.value_type() != ValueType::Object {
        typed_internal_argument_error(eg, METHOD, &iterator, 1, "iterator", "object");
        return Ok(());
    }
    let mut flags = 8;
    let mut caching_flags = 16;
    let mut mode = 1;
    for (index, name, target) in [
        (2, "flags", &mut flags),
        (3, "cachingIteratorFlags", &mut caching_flags),
        (4, "mode", &mut mode),
    ] {
        if let Some(argument) = arg_opt!(ed, index) {
            let argument = argument.clone();
            let Some(value) = typed_internal_int_value_argument_expected(
                ed,
                eg,
                &argument,
                METHOD,
                index - 1,
                name,
                "int",
            )?
            else {
                return Ok(());
            };
            *target = value;
        }
    }
    // An aggregate supplies the recursive input; the cached wrapper performs
    // RecursiveIterator validation and owns its canonical error diagnostic.
    let mut seen = Vec::new();
    loop {
        let aggregate = iterator.as_object().is_some_and(|o| {
            !eg.class_is_a(&o.class_name, "RecursiveIterator")
                && eg.class_is_a(&o.class_name, "IteratorAggregate")
        });
        if !aggregate {
            break;
        }
        let identity = iterator.object_identity().expect("aggregate");
        if seen.contains(&identity) {
            break;
        }
        seen.push(identity);
        let name = iterator.as_object().unwrap().class_name.to_string();
        let next =
            call_object_protocol_method(eg, &iterator, "IteratorAggregate", "getIterator", &[])?;
        if eg.exception.is_some() {
            return Ok(());
        }
        let Some(next) = next.filter(|value| {
            value
                .as_object()
                .is_some_and(|o| eg.class_is_a(&o.class_name, "Traversable"))
        }) else {
            eg.exception = Some(make_error_value(
                "LogicException",
                &format!("{name}::getIterator() must return an object that implements Traversable"),
            ));
            return Ok(());
        };
        let retired = std::mem::replace(&mut iterator, next);
        iterator_delegate::discard(retired, eg)?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    let cached = caching_iterator::recursive_wrapper(&iterator, caching_flags, eg);
    iterator_delegate::discard(iterator, eg)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    // Prefix customization survives repeated construction. Initialize the
    // projection before retiring the previous driver's callback-owned edges.
    receiver
        .as_object_mut()
        .unwrap()
        .native_object_state_mut::<State>()
        .flags = flags;
    recursive_iterator::initialize(&receiver, cached, mode, flags, eg)?;
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn append_bytes(output: &mut Vec<u8>, value: &Value) {
    output.extend_from_slice(&php_bytes_after_weak_string_coercion(value).0);
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn prefix(receiver: &Value, eg: &mut ExecutorGlobals) -> Result<Value, VmError> {
    let (parts, iterators) = {
        let object = receiver.as_object().unwrap();
        let parts = object.native_object_state::<State>().unwrap().parts.clone();
        let frames = &object
            .native_iterator_delegate()
            .unwrap()
            .recursive
            .as_ref()
            .unwrap()
            .frames;
        let mut iterators = Vec::with_capacity(frames.len());
        for frame in frames {
            iterators.push(frame.iterator.clone());
        }
        (parts, iterators)
    };
    let mut output = Vec::new();
    append_bytes(&mut output, &parts[0]);
    for (index, iterator) in iterators.iter().enumerate() {
        let next = call_object_protocol_method(eg, iterator, "CachingIterator", "hasNext", &[])?;
        if eg.exception.is_some() {
            break;
        }
        let last = index + 1 == iterators.len();
        let part = match (last, next.is_some_and(|value| value.is_truthy())) {
            (false, true) => 1,
            (false, false) => 2,
            (true, true) => 3,
            (true, false) => 4,
        };
        append_bytes(&mut output, &parts[part]);
    }
    for iterator in iterators {
        iterator_delegate::discard(iterator, eg)?;
    }
    append_bytes(&mut output, &parts[5]);
    Ok(php_byte_result(output, false))
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn entry(
    ed: *mut ExecuteData,
    receiver: &Value,
    eg: &mut ExecutorGlobals,
) -> Result<Value, VmError> {
    let value = recursive_iterator::inner_protocol(receiver, "current", eg)?;
    if eg.exception.is_some() {
        return Ok(Value::null());
    }
    let result = if value.dereferenced().value_type() == ValueType::Array {
        Ok(Value::string("Array"))
    } else {
        caching_iterator::string_value(ed, eg, &value)
    };
    iterator_delegate::discard(value, eg)?;
    result
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn projection(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    key: bool,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let flags = receiver
        .as_object()
        .unwrap()
        .native_object_state::<State>()
        .unwrap()
        .flags;
    if flags & if key { 8 } else { 4 } != 0 {
        let value =
            recursive_iterator::inner_protocol(&receiver, if key { "key" } else { "current" }, eg)?;
        ret!(rv, value);
    }
    // These are native projections, not virtual calls to user decoration
    // overrides. Public getPrefix/getEntry/getPostfix retain normal dispatch.
    let prefix = prefix(&receiver, eg)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let text = if key {
        let value = recursive_iterator::inner_protocol(&receiver, "key", eg)?;
        let text = caching_iterator::string_value(ed, eg, &value)?;
        iterator_delegate::discard(value, eg)?;
        text
    } else {
        entry(ed, &receiver, eg)?
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    let postfix = receiver
        .as_object()
        .unwrap()
        .native_object_state::<State>()
        .unwrap()
        .postfix
        .clone();
    let mut output = Vec::new();
    append_bytes(&mut output, &prefix);
    append_bytes(&mut output, &text);
    append_bytes(&mut output, &postfix);
    ret!(rv, php_byte_result(output, false));
}

macro_rules! projection_handler {
    ($name:ident, $key:expr) => {
        #[cold]
        #[inline(never)]
        #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
        fn $name(
            ed: *mut ExecuteData,
            rv: *mut Value,
            eg: &mut ExecutorGlobals,
        ) -> Result<(), VmError> {
            projection(ed, rv, eg, $key)
        }
    };
}
projection_handler!(key, true);
projection_handler!(current, false);

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn get_prefix(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let value = prefix(&receiver, eg)?;
    ret!(rv, value);
}
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn get_entry(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let value = entry(ed, &receiver, eg)?;
    ret!(rv, value);
}
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn get_postfix(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    let value = receiver
        .as_object()
        .unwrap()
        .native_object_state::<State>()
        .unwrap()
        .postfix
        .clone();
    ret!(rv, value);
}
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn set_postfix(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = owned_argument(ed, 1);
    let Some(value) = typed_internal_string_value_expected(
        ed,
        eg,
        &value,
        "RecursiveTreeIterator::setPostfix",
        0,
        "postfix",
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    receiver
        .as_object_mut()
        .unwrap()
        .native_object_state_mut::<State>()
        .postfix = value;
    ret!(rv, Value::null());
}
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn set_prefix_part(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    const METHOD: &str = "RecursiveTreeIterator::setPrefixPart";
    let part = owned_argument(ed, 1);
    let Some(part) =
        typed_internal_int_value_argument_expected(ed, eg, &part, METHOD, 0, "part", "int")?
    else {
        return Ok(());
    };
    let value = owned_argument(ed, 2);
    let Some(value) = typed_internal_string_value_expected(
        ed, eg, &value, METHOD, 1, "value", "string", "string",
    )?
    else {
        return Ok(());
    };
    let receiver = owned_argument(ed, 0);
    if !initialized(&receiver, eg) {
        return Ok(());
    }
    if !(0..6).contains(&part) {
        eg.exception = Some(make_error_value(
            "ValueError",
            "RecursiveTreeIterator::setPrefixPart(): Argument #1 ($part) must be a RecursiveTreeIterator::PREFIX_* constant",
        ));
        return Ok(());
    }
    receiver
        .as_object_mut()
        .unwrap()
        .native_object_state_mut::<State>()
        .parts[part as usize] = value;
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{ClassName, Int, Mixed, String, Union, Void};
    const OWNER: &str = "RecursiveTreeIterator";
    let mut class = empty_internal_type(OWNER, vec![], false, false);
    class.parent = Some("RecursiveIteratorIterator".into());
    // Internal inheritance installs parent constants before this class adds
    // its own declarations. Keep original declaring-class metadata intact.
    class.constants = eg
        .find_class("RecursiveIteratorIterator")
        .unwrap()
        .constants
        .clone();
    for (name, value) in [
        ("BYPASS_CURRENT", 4),
        ("BYPASS_KEY", 8),
        ("PREFIX_LEFT", 0),
        ("PREFIX_MID_HAS_NEXT", 1),
        ("PREFIX_MID_LAST", 2),
        ("PREFIX_END_HAS_NEXT", 3),
        ("PREFIX_END_LAST", 4),
        ("PREFIX_RIGHT", 5),
    ] {
        class
            .constants
            .push(recursive_iterator::constant(OWNER, name, value));
    }
    eg.register_class_with_complete_native_parent(class)
        .unwrap();
    eg.reserve_internal_method_contracts(OWNER, 8);
    let mut functions = Vec::with_capacity(8);
    recursive_iterator::register_method(
        eg,
        &mut functions,
        OWNER,
        "__construct",
        construct,
        &["iterator", "flags", "cachingIteratorFlags", "mode"],
        vec![
            Union(vec![
                ClassName("RecursiveIterator".into()),
                ClassName("IteratorAggregate".into()),
            ]),
            Int,
            Int,
            Int,
        ],
        &[
            None,
            Some("RecursiveTreeIterator::BYPASS_KEY"),
            Some("CachingIterator::CATCH_GET_CHILD"),
            Some("RecursiveTreeIterator::SELF_FIRST"),
        ],
        ParamTypeHint::None,
    );
    let pointer = &functions.last().unwrap().common as *const FunctionCommon;
    eg.register_internal_function_reflection_metadata_with_diagnostics(
        pointer,
        vec![
            None,
            Some(Value::long(8)),
            Some(Value::long(16)),
            Some(Value::long(1)),
        ],
        &[
            None,
            Some("RecursiveTreeIterator::BYPASS_KEY"),
            Some("CachingIterator::CATCH_GET_CHILD"),
            Some("RecursiveTreeIterator::SELF_FIRST"),
        ],
        "SPL",
    );
    for (name, handler, result) in [
        ("key", key as InternalFunctionHandler, Mixed),
        ("current", current, Mixed),
        ("getPrefix", get_prefix, String),
    ] {
        recursive_iterator::register_method(
            eg,
            &mut functions,
            OWNER,
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
        OWNER,
        "setPostfix",
        set_postfix,
        &["postfix"],
        vec![String],
        &[None],
        Void,
    );
    recursive_iterator::register_method(
        eg,
        &mut functions,
        OWNER,
        "setPrefixPart",
        set_prefix_part,
        &["part", "value"],
        vec![Int, String],
        &[None, None],
        Void,
    );
    recursive_iterator::register_method(
        eg,
        &mut functions,
        OWNER,
        "getEntry",
        get_entry,
        &[],
        vec![],
        &[],
        String,
    );
    recursive_iterator::register_method(
        eg,
        &mut functions,
        OWNER,
        "getPostfix",
        get_postfix,
        &[],
        vec![],
        &[],
        String,
    );
    functions
}
