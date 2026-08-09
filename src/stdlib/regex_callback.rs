//! Streaming `preg_replace_callback` consumer kept out of the general stdlib
//! codegen unit so its specialized ownership path cannot perturb preg_match.

use super::resolve_callback_or_fatal;
use crate::regex::Regex;
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value};
use crate::vm::execute::{
    VmError, call_function_owned_iter, call_function_owned_iter_readback_arg0,
};
use crate::vm::frame::ExecuteData;

/// Return `None` when the callback raised a PHP exception. No partial output is
/// published in that case; `ExecutorGlobals` retains the exception for the
/// calling opcode.
#[inline(never)]
pub(super) fn replace(
    regex: &Regex,
    subject: String,
    callback: &Value,
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Result<Option<String>, VmError> {
    let mut resolved = None;
    let mut result = String::new();
    let mut previous_end = 0;
    let mut reusable_capture_free_matches: Option<Value> = None;

    let count = regex.try_visit_captures(&subject, |caps| {
        if resolved.is_none() {
            resolved = Some(resolve_callback_or_fatal(eg, callback, execute_data)?);
            result.reserve(subject.len());
        }
        let resolved = resolved.as_ref().unwrap();
        let full_match = caps.get(0).unwrap();
        let capture_free = caps.len() == 1 && caps.named_groups().is_empty();

        let matches_value = if capture_free {
            debug_assert_eq!(caps.len(), 1);
            debug_assert!(caps.named_groups().is_empty());
            let matched = Value::string(full_match.as_str(&subject));
            if let Some(mut value) = reusable_capture_free_matches.take() {
                if let Some(array) = value.as_array_mut_if_unique() {
                    array.set_int(0, matched);
                    value
                } else {
                    let mut array = PhpArray::with_packed_capacity(1);
                    array.push(matched);
                    Value::array(array)
                }
            } else {
                let mut array = PhpArray::with_packed_capacity(1);
                array.push(matched);
                Value::array(array)
            }
        } else {
            let mut matches = PhpArray::new();
            for index in 0..caps.len() {
                match caps.get(index) {
                    Some(capture) => matches.push(Value::string(capture.as_str(&subject))),
                    None => matches.push(Value::string("")),
                }
            }
            for (name, &index) in caps.named_groups() {
                if let Some(capture) = caps.get(index) {
                    matches.set_str(name, Value::string(capture.as_str(&subject)));
                }
            }
            Value::array(matches)
        };

        let num_args = resolved.prepend_args.len() + 1 + resolved.use_vars.len();
        let args = resolved
            .prepend_args
            .iter()
            .cloned()
            .chain(std::iter::once(matches_value))
            .chain(resolved.use_vars.iter().cloned());
        let callback_result = if capture_free {
            let (callback_result, matches_value) =
                call_function_owned_iter_readback_arg0(eg, resolved.func_ptr, num_args, args)?;
            reusable_capture_free_matches = Some(matches_value);
            callback_result
        } else {
            call_function_owned_iter(eg, resolved.func_ptr, num_args, args)?
        };
        if eg.exception.is_some() {
            return Ok(false);
        }

        result.push_str(&subject[previous_end..full_match.start]);
        callback_result.append_echo_to(&mut result);
        previous_end = full_match.end;
        Ok(true)
    })?;

    if eg.exception.is_some() {
        return Ok(None);
    }
    if count == 0 {
        return Ok(Some(subject));
    }
    result.push_str(&subject[previous_end..]);
    Ok(Some(result))
}
