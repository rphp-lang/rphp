//! Streaming `preg_replace_callback` consumer kept out of the general stdlib
//! codegen unit so its specialized ownership path cannot perturb preg_match.

use super::{ResolvedCallback, call_resolved_owned_iter, call_resolved_owned_iter_readback_arg0};
use crate::regex::Regex;
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value};
use crate::vm::execute::VmError;

/// Return `None` when the callback raised a PHP exception. No partial output is
/// published in that case; `ExecutorGlobals` retains the exception for the
/// calling opcode.
#[inline(never)]
pub(super) fn replace(
    regex: &Regex,
    subject: String,
    resolved: &ResolvedCallback,
    limit: usize,
    unmatched_as_null: bool,
    eg: &mut ExecutorGlobals,
) -> Result<Option<(String, usize)>, VmError> {
    if limit == 0 {
        return Ok(Some((subject, 0)));
    }
    let mut result = String::new();
    let mut previous_end = 0;
    let mut reusable_capture_free_matches: Option<Value> = None;
    let mut replacements = 0usize;

    regex.try_visit_captures(&subject, |caps| {
        if replacements == limit {
            return Ok(false);
        }
        if replacements == 0 {
            result.reserve(subject.len());
        }
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
                    None if unmatched_as_null => matches.push(Value::null()),
                    None => matches.push(Value::string("")),
                }
            }
            for (name, &index) in caps.named_groups() {
                if let Some(capture) = caps.get(index) {
                    matches.set_str(name, Value::string(capture.as_str(&subject)));
                } else if unmatched_as_null {
                    matches.set_str(name, Value::null());
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
                call_resolved_owned_iter_readback_arg0(eg, resolved, num_args, args)?;
            reusable_capture_free_matches = Some(matches_value);
            callback_result
        } else {
            call_resolved_owned_iter(eg, resolved, num_args, args)?
        };
        if eg.exception.is_some() {
            return Ok(false);
        }

        result.push_str(&subject[previous_end..full_match.start]);
        callback_result.append_echo_to(&mut result);
        previous_end = full_match.end;
        replacements += 1;
        Ok(true)
    })?;

    if eg.exception.is_some() {
        return Ok(None);
    }
    if replacements == 0 {
        return Ok(Some((subject, 0)));
    }
    result.push_str(&subject[previous_end..]);
    Ok(Some((result, replacements)))
}
