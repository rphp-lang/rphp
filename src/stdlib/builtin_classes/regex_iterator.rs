//! Sparse SPL filtering over existing iterator and regex protocols. PHP values
//! stay in traced projection slots/properties; no borrow crosses a callback.
use super::*;
use crate::regex::{CaptureView, Match, Regex};
use crate::value::{NativeIteratorDelegate, RegexIteratorState};
use std::rc::Rc;

#[cold]
fn error(eg: &mut ExecutorGlobals, kind: &str, message: &str) {
    eg.exception = Some(make_error_value(kind, message));
}

#[cold]
fn mode_error(eg: &mut ExecutorGlobals, method: &str, number: usize) {
    error(
        eg,
        "ValueError",
        &format!(
            "{method}(): Argument #{number} ($mode) must be RegexIterator::MATCH, RegexIterator::GET_MATCH, RegexIterator::ALL_MATCHES, RegexIterator::SPLIT, or RegexIterator::REPLACE"
        ),
    );
}

#[cold]
fn number(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    index: u32,
    method: &str,
    name: &str,
) -> Result<Option<i64>, VmError> {
    let value = owned_argument(ed, index);
    typed_internal_int_value_argument_expected(ed, eg, &value, method, index - 1, name, "int")
}

#[cold]
fn construct_filter(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    let source = owned_argument(ed, 1);
    if !validate_inner(&receiver, &source, eg, "FilterIterator", "Iterator") {
        return Ok(());
    }
    install(&receiver, source, None, eg);
    ret!(rv, Value::null());
}

#[cold]
pub(super) fn validate_inner(
    receiver: &Value,
    source: &Value,
    eg: &mut ExecutorGlobals,
    owner: &str,
    expected: &str,
) -> bool {
    if receiver
        .as_object()
        .is_some_and(|o| o.native_iterator_delegate().is_some())
    {
        error(
            eg,
            "BadMethodCallException",
            &format!("{owner}::getIterator() must be called exactly once per instance"),
        );
        return false;
    }
    if !source
        .as_object()
        .is_some_and(|o| eg.class_is_a(&o.class_name, expected))
    {
        typed_internal_argument_error(
            eg,
            &format!("{owner}::__construct"),
            source,
            1,
            "iterator",
            expected,
        );
        return false;
    }
    true
}

#[cold]
pub(super) fn install(
    receiver: &Value,
    inner: Value,
    regex: Option<Box<RegexIteratorState>>,
    eg: &mut ExecutorGlobals,
) {
    let iterator = super::deque::consumer(&inner, eg).unwrap_or_else(|| inner.clone());
    let mut state = NativeIteratorDelegate::new(inner, iterator, 0, -1);
    state.regex = regex;
    receiver
        .as_object_mut()
        .unwrap()
        .set_native_iterator_delegate(state);
}

#[cold]
fn construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let method = "RegexIterator::__construct";
    let receiver = owned_argument(ed, 0);
    let inner = owned_argument(ed, 1);
    if !validate_inner(&receiver, &inner, eg, "RegexIterator", "Iterator") {
        return Ok(());
    }
    let pattern = owned_argument(ed, 2);
    let Some(pattern) = typed_internal_string_value_expected(
        ed, eg, &pattern, method, 1, "pattern", "string", "string",
    )?
    else {
        return Ok(());
    };
    let pattern_is_binary = pattern.is_binary_string();
    let pattern = pattern.echo_to_string();
    let mut options = [0; 3];
    for (i, name) in ["mode", "flags", "pregFlags"].iter().enumerate() {
        if arg_opt!(ed, i as u32 + 3).is_some() {
            let Some(value) = number(ed, eg, i as u32 + 3, method, name)? else {
                return Ok(());
            };
            options[i] = value;
        }
    }
    if !(0..=4).contains(&options[0]) {
        mode_error(eg, method, 3);
        return Ok(());
    }
    let trimmed = pattern.trim();
    if trimmed
        .as_bytes()
        .first()
        .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, 0 | b'\\'))
    {
        error(
            eg,
            "InvalidArgumentException",
            &format!("{method}(): Delimiter must not be alphanumeric, backslash, or NUL byte"),
        );
        return Ok(());
    }
    let (body, flags) = match crate::regex::parse_php_regex(&pattern) {
        Ok(parsed) => parsed,
        Err(message) => {
            error(
                eg,
                "InvalidArgumentException",
                &format!("{method}(): {message}"),
            );
            return Ok(());
        }
    };
    let unicode = trimmed
        .get(body.len() + 2..)
        .is_some_and(|flags| flags.contains('u'));
    let body = if unicode && pattern_is_binary && !body.is_ascii() {
        let bytes: Vec<u8> = body.chars().map(|c| c as u8).collect();
        match std::string::String::from_utf8(bytes) {
            Ok(body) => body,
            Err(_) => {
                error(
                    eg,
                    "InvalidArgumentException",
                    &format!("{method}(): Compilation failed: invalid UTF-8 string"),
                );
                return Ok(());
            }
        }
    } else if unicode || pattern_is_binary || body.is_ascii() {
        body
    } else {
        body.as_bytes().iter().map(|b| char::from(*b)).collect()
    };
    let compiled = match Regex::new(&body, flags) {
        Ok(regex) => Rc::new(regex),
        Err(message) => {
            let message = if message == "Unterminated character class" {
                format!(
                    "Compilation failed: missing terminating ] for character class at offset {}",
                    body.len()
                )
            } else {
                message
            };
            error(
                eg,
                "InvalidArgumentException",
                &format!("{method}(): {message}"),
            );
            return Ok(());
        }
    };
    install(
        &receiver,
        inner,
        Some(Box::new(RegexIteratorState {
            pattern,
            pattern_is_binary,
            compiled,
            unicode,
            mode: options[0],
            flags: options[1],
            preg_flags: options[2],
        })),
        eg,
    );
    ret!(rv, Value::null());
}

#[cold]
fn filter(receiver: &Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    loop {
        iterator_delegate::fetch(receiver, eg)?;
        if eg.exception.is_some() {
            iterator_delegate::clear(receiver, eg)?;
            return Ok(());
        }
        let present = receiver
            .as_object()
            .unwrap()
            .native_iterator_delegate()
            .unwrap()
            .current
            .is_undef();
        if present {
            return Ok(());
        }
        let accepted = call_object_protocol_method(eg, receiver, "FilterIterator", "accept", &[])?;
        let keep = accepted.as_ref().is_some_and(Value::is_truthy);
        if let Some(accepted) = accepted {
            iterator_delegate::discard(accepted, eg)?;
        }
        if eg.exception.is_some() || keep {
            return Ok(());
        }
        iterator_delegate::clear(receiver, eg)?;
        if eg.exception.is_some() {
            return Ok(());
        }
        iterator_delegate::protocol(eg, &iterator_delegate::inner(receiver), "next")?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
}

#[cold]
fn advance(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    operation: &str,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !iterator_delegate::initialized(&receiver, eg) {
        return Ok(());
    }
    iterator_delegate::clear(&receiver, eg)?;
    if eg.exception.is_none() {
        iterator_delegate::protocol(eg, &iterator_delegate::inner(&receiver), operation)?;
    }
    if eg.exception.is_none() {
        filter(&receiver, eg)?;
    }
    ret!(rv, Value::null());
}

#[cold]
fn rewind(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    advance(ed, rv, eg, "rewind")
}
#[cold]
fn next(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    advance(ed, rv, eg, "next")
}

#[cold]
fn abstract_accept(
    _ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    error(
        eg,
        "Error",
        "Cannot call abstract method FilterIterator::accept()",
    );
    Ok(())
}

#[cold]
fn byte_value(text: &str, unicode: bool) -> Value {
    if unicode || text.is_ascii() {
        Value::string(text)
    } else {
        php_byte_result(text.chars().map(|c| c as u8).collect(), false)
    }
}

#[cold]
fn capture_value(m: Option<&Match>, subject: &str, flags: i64, unicode: bool) -> Value {
    let value = m.map_or_else(
        || {
            if flags & 512 != 0 {
                Value::null()
            } else {
                Value::string("")
            }
        },
        |m| byte_value(m.as_str(subject), unicode),
    );
    if flags & 256 == 0 {
        return value;
    }
    let offset = m.map_or(-1, |m| {
        if unicode {
            m.start as i64
        } else {
            subject[..m.start].chars().count() as i64
        }
    });
    let mut pair = PhpArray::with_packed_capacity(2);
    pair.push(value);
    pair.push(Value::long(offset));
    Value::array(pair)
}

#[cold]
fn match_row(caps: CaptureView<'_>, subject: &str, flags: i64, unicode: bool) -> PhpArray {
    let last = if flags & 512 != 0 {
        caps.len() - 1
    } else {
        (0..caps.len())
            .rev()
            .find(|i| caps.get(*i).is_some())
            .unwrap_or(0)
    };
    let mut row = PhpArray::new();
    for index in 0..=last {
        let value = capture_value(caps.get(index), subject, flags, unicode);
        for (name, _) in caps.named_groups() {
            if caps.named_group_output_slot(name) == Some(index) {
                let alias_slot = caps.named_group_slot(name).unwrap_or(index);
                let alias = if alias_slot == index {
                    value.clone()
                } else {
                    capture_value(caps.get(alias_slot), subject, flags, unicode)
                };
                row.set_str(name, alias);
            }
        }
        row.push(value);
    }
    if let Some(mark) = caps.mark() {
        row.set_str("MARK", Value::string(mark));
    }
    row
}

#[cold]
fn matches(regex: &Regex, subject: &str, mode: i64, flags: i64, unicode: bool) -> (Value, bool) {
    let mut out = PhpArray::new();
    let mut columns = if mode == 2 && flags & 2 == 0 {
        (0..regex.capture_count())
            .map(|_| PhpArray::new())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let count: Result<usize, std::convert::Infallible> =
        regex.try_visit_captures(subject, |caps| {
            if mode == 1 {
                out = match_row(caps, subject, flags, unicode);
                return Ok(false);
            }
            if flags & 2 != 0 {
                out.push(Value::array(match_row(caps, subject, flags, unicode)));
            } else {
                for (index, column) in columns.iter_mut().enumerate() {
                    column.push(capture_value(caps.get(index), subject, flags, unicode));
                }
            }
            Ok(true)
        });
    let matched = count.unwrap() != 0;
    if mode == 2 && flags & 2 == 0 {
        if regex.has_duplicate_named_groups() {
            let values = columns.into_iter().map(Value::array).collect::<Vec<_>>();
            for (index, value) in values.iter().enumerate() {
                for (name, slot) in regex.capture_names() {
                    if regex.capture_name_output_slot(name) == Some(index) {
                        out.set_str(name, values[*slot].clone());
                    }
                }
                out.push(value.clone());
            }
        } else {
            for (index, column) in columns.into_iter().enumerate() {
                let value = Value::array(column);
                for (name, slot) in regex.capture_names() {
                    if *slot == index {
                        out.set_str(name, value.clone());
                    }
                }
                out.push(value);
            }
        }
    }
    (Value::array(out), matched)
}

#[cold]
fn split(regex: &Regex, subject: &str, flags: i64, unicode: bool) -> (Value, bool) {
    let mut out = PhpArray::new();
    let mut end = 0;
    let mut append = |text: &str, offset: usize| {
        if text.is_empty() && flags & 1 != 0 {
            return;
        }
        let value = byte_value(text, unicode);
        if flags & 4 == 0 {
            out.push(value);
        } else {
            let mut pair = PhpArray::with_packed_capacity(2);
            pair.push(value);
            pair.push(Value::long(if unicode {
                offset
            } else {
                subject[..offset].chars().count()
            } as i64));
            out.push(Value::array(pair));
        }
    };
    let count: Result<usize, std::convert::Infallible> =
        regex.try_visit_captures(subject, |caps| {
            let full = caps.get(0).unwrap();
            append(&subject[end..full.start], end);
            if flags & 2 != 0 {
                for index in 1..caps.len() {
                    if let Some(m) = caps.get(index) {
                        append(m.as_str(subject), m.start);
                    }
                }
            }
            end = full.end;
            Ok(true)
        });
    append(&subject[end..], end);
    (Value::array(out), count.unwrap() != 0)
}

#[cold]
fn accept(_ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(_ed, 0);
    if !iterator_delegate::initialized(&receiver, eg) {
        return Ok(());
    }
    let subject = {
        let object = receiver.as_object().unwrap();
        let state = object.native_iterator_delegate().unwrap();
        let Some(regex) = state.regex.as_ref() else {
            ret!(rv, Value::bool(false));
        };
        let subject = if regex.flags & 1 != 0 {
            &state.key
        } else {
            &state.current
        };
        if subject.is_undef() || subject.dereferenced().value_type() == ValueType::Array {
            ret!(rv, Value::bool(false));
        }
        subject.dereferenced().clone()
    };
    let converted = if subject.value_type() == ValueType::Object {
        let converted = crate::vm::execute::call_object_string_conversion(eg, &subject)?;
        let Some(converted) = converted else {
            if eg.exception.is_none() {
                error(
                    eg,
                    "Error",
                    &format!(
                        "Object of class {} could not be converted to string",
                        subject.as_object().unwrap().class_name
                    ),
                );
            }
            return Ok(());
        };
        converted
    } else if subject.value_type() == ValueType::String {
        subject
    } else {
        Value::string(subject.echo_to_string_with_precision(eg.precision))
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    // Subject selection precedes conversion, but a user __toString() may
    // change the mode/flags used to project that selected subject.
    let (compiled, unicode, mode, flags, preg_flags) = {
        let object = receiver.as_object().unwrap();
        let regex = object
            .native_iterator_delegate()
            .unwrap()
            .regex
            .as_ref()
            .unwrap();
        (
            regex.compiled.clone(),
            regex.unicode,
            regex.mode,
            regex.flags,
            regex.preg_flags,
        )
    };
    let bytes = php_bytes_after_weak_string_coercion(&converted).0;
    let text = if unicode {
        match std::str::from_utf8(&bytes) {
            Ok(s) => s.to_owned(),
            Err(_) => {
                ret!(rv, Value::bool(false));
            }
        }
    } else if bytes.is_ascii() {
        String::from_utf8(bytes.into_owned()).unwrap()
    } else {
        bytes.iter().map(|b| char::from(*b)).collect()
    };
    let (projection, matched) = match mode {
        0 => (None, compiled.is_match(&text)),
        1 | 2 => {
            let (value, matched) = matches(&compiled, &text, mode, preg_flags, unicode);
            (Some(value), matched)
        }
        3 => {
            let (value, matched) = split(&compiled, &text, preg_flags, unicode);
            (Some(value), matched)
        }
        4 => {
            let replacement = receiver
                .as_object()
                .unwrap()
                .get_property("replacement")
                .cloned()
                .unwrap_or_else(Value::null);
            let replacement = php_bytes_after_weak_string_coercion(replacement.dereferenced())
                .0
                .into_owned();
            let replacement: String = if unicode {
                String::from_utf8_lossy(&replacement).into_owned()
            } else {
                replacement.iter().map(|b| char::from(*b)).collect()
            };
            let (value, count) = compiled.replace_limit(&text, &replacement, usize::MAX);
            (Some(byte_value(&value, unicode)), count != 0)
        }
        _ => unreachable!("validated mode"),
    };
    if let Some(projection) = projection {
        let old = {
            let mut object = receiver.as_object_mut().unwrap();
            let state = object.native_iterator_delegate_mut().unwrap();
            let slot = if mode == 4 && flags & 1 != 0 {
                &mut state.key
            } else {
                &mut state.current
            };
            std::mem::replace(slot, projection)
        };
        iterator_delegate::discard(old, eg)?;
    }
    ret!(rv, Value::bool(matched ^ (flags & 2 != 0)));
}

#[cold]
fn configured(receiver: &Value, eg: &mut ExecutorGlobals) -> bool {
    if !iterator_delegate::initialized(receiver, eg) {
        return false;
    }
    if receiver
        .as_object()
        .unwrap()
        .native_iterator_delegate()
        .unwrap()
        .regex
        .is_none()
    {
        // A subclass may explicitly invoke a different ancestor constructor.
        // Never treat that delegate's unrelated payload as a compiled regex.
        error(
            eg,
            "Error",
            "The object is in an invalid state as the parent constructor was not called",
        );
        return false;
    }
    true
}

#[cold]
fn option(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    index: usize,
    set: bool,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !configured(&receiver, eg) {
        return Ok(());
    }
    let value = if set {
        let method = [
            "RegexIterator::setMode",
            "RegexIterator::setFlags",
            "RegexIterator::setPregFlags",
        ][index];
        let Some(value) = number(ed, eg, 1, method, ["mode", "flags", "pregFlags"][index])? else {
            return Ok(());
        };
        if index == 0 && !(0..=4).contains(&value) {
            mode_error(eg, method, 1);
            return Ok(());
        }
        Some(value)
    } else {
        None
    };
    let mut object = receiver.as_object_mut().unwrap();
    let state = object
        .native_iterator_delegate_mut()
        .unwrap()
        .regex
        .as_mut()
        .unwrap();
    let slot = match index {
        0 => &mut state.mode,
        1 => &mut state.flags,
        _ => &mut state.preg_flags,
    };
    if let Some(value) = value {
        *slot = value;
        ret!(rv, Value::null());
    }
    ret!(rv, Value::long(*slot));
}

macro_rules! option_handler {
    ($name:ident, $index:expr, $set:expr) => {
        #[cold]
        fn $name(
            ed: *mut ExecuteData,
            rv: *mut Value,
            eg: &mut ExecutorGlobals,
        ) -> Result<(), VmError> {
            option(ed, rv, eg, $index, $set)
        }
    };
}
option_handler!(get_mode, 0, false);
option_handler!(set_mode, 0, true);
option_handler!(get_flags, 1, false);
option_handler!(set_flags, 1, true);
option_handler!(get_preg, 2, false);
option_handler!(set_preg, 2, true);

#[cold]
fn get_regex(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !configured(&receiver, eg) {
        return Ok(());
    }
    let object = receiver.as_object().unwrap();
    let regex = object
        .native_iterator_delegate()
        .unwrap()
        .regex
        .as_ref()
        .unwrap();
    ret!(
        rv,
        if regex.pattern_is_binary {
            Value::binary_string_from_storage(regex.pattern.clone())
        } else {
            Value::string(&regex.pattern)
        }
    );
}

#[cold]
#[inline(never)]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{Bool, ClassName, Int, Nullable, String, Void};
    let mut filter = empty_internal_type("FilterIterator", vec![], false, false);
    filter.parent = Some("IteratorIterator".into());
    filter.is_abstract = true;
    filter.abstract_methods.push("accept".into());
    eg.register_class_with_complete_native_parent(filter)
        .unwrap();
    let mut regex = empty_internal_type("RegexIterator", vec![], false, false);
    regex.parent = Some("FilterIterator".into());
    let mut replacement = PropertyDefinition::new(
        "replacement".into(),
        Some(Value::null()),
        Visibility::Public,
        "RegexIterator".into(),
    );
    replacement.type_hint = Nullable(Box::new(String));
    regex.properties.push(replacement);
    for (name, value) in [
        ("MATCH", 0),
        ("GET_MATCH", 1),
        ("ALL_MATCHES", 2),
        ("SPLIT", 3),
        ("REPLACE", 4),
        ("USE_KEY", 1),
        ("INVERT_MATCH", 2),
    ] {
        regex
            .constants
            .push(recursive_iterator::constant("RegexIterator", name, value));
    }
    let mut functions = Vec::with_capacity(13);
    macro_rules! method {
        ($owner:expr, $name:expr, $handler:expr, $names:expr, $hints:expr, $defaults:expr, $result:expr $(,)?) => {{
            recursive_iterator::register_method(
                eg,
                &mut functions,
                $owner,
                $name,
                $handler,
                $names,
                $hints,
                $defaults,
                $result,
            );
            if $owner == "FilterIterator" {
                let body = &functions.last().unwrap().common as *const FunctionCommon;
                eg.bind_latest_internal_method_body($owner, $name, body);
            }
        }};
    }
    method!(
        "FilterIterator",
        "__construct",
        construct_filter,
        &["iterator"],
        vec![ClassName("Iterator".into())],
        &[None],
        ParamTypeHint::None,
    );
    method!(
        "FilterIterator",
        "accept",
        abstract_accept,
        &[],
        vec![],
        &[],
        Bool,
    );
    method!("FilterIterator", "rewind", rewind, &[], vec![], &[], Void);
    method!("FilterIterator", "next", next, &[], vec![], &[], Void);
    eg.set_internal_method_access("FilterIterator", "accept", Visibility::Public, true);
    eg.register_class_with_complete_native_parent(regex)
        .unwrap();
    method!(
        "RegexIterator",
        "__construct",
        construct,
        &["iterator", "pattern", "mode", "flags", "pregFlags"],
        vec![ClassName("Iterator".into()), String, Int, Int, Int],
        &[
            None,
            None,
            Some("RegexIterator::MATCH"),
            Some("0"),
            Some("0"),
        ],
        ParamTypeHint::None,
    );
    method!("RegexIterator", "accept", accept, &[], vec![], &[], Bool);
    method!("RegexIterator", "getMode", get_mode, &[], vec![], &[], Int);
    method!(
        "RegexIterator",
        "setMode",
        set_mode,
        &["mode"],
        vec![Int],
        &[None],
        Void,
    );
    method!(
        "RegexIterator",
        "getFlags",
        get_flags,
        &[],
        vec![],
        &[],
        Int,
    );
    method!(
        "RegexIterator",
        "setFlags",
        set_flags,
        &["flags"],
        vec![Int],
        &[None],
        Void,
    );
    method!(
        "RegexIterator",
        "getPregFlags",
        get_preg,
        &[],
        vec![],
        &[],
        Int,
    );
    method!(
        "RegexIterator",
        "setPregFlags",
        set_preg,
        &["pregFlags"],
        vec![Int],
        &[None],
        Void,
    );
    method!(
        "RegexIterator",
        "getRegex",
        get_regex,
        &[],
        vec![],
        &[],
        String,
    );
    functions
}
