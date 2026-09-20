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
    offset_capture: bool,
    byte_view: bool,
    eg: &mut ExecutorGlobals,
) -> Result<Option<(String, usize)>, VmError> {
    // A byte-view subject (non-`u` pattern) hands captures and takes callback
    // results as PHP bytes; ASCII text is identical in both projections.
    let mapped = byte_view && !subject.is_ascii();
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
        let capture_free = !offset_capture && caps.len() == 1 && caps.named_groups().is_empty();

        let matches_value = if capture_free {
            debug_assert_eq!(caps.len(), 1);
            debug_assert!(caps.named_groups().is_empty());
            let matched = super::pcre_view_value(full_match.as_str(&subject), mapped);
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
            let last_capture = if unmatched_as_null {
                caps.len() - 1
            } else {
                (0..caps.len())
                    .rev()
                    .find(|&index| caps.get(index).is_some())
                    .unwrap_or(0)
            };
            for index in 0..=last_capture {
                let value = super::pcre_capture_value(
                    caps.get(index),
                    &subject,
                    0,
                    offset_capture,
                    unmatched_as_null,
                    mapped,
                );
                for (name, slot) in caps.named_groups() {
                    if *slot == index {
                        matches.set_str(name, value.clone());
                    }
                }
                matches.push(value);
            }
            if let Some(mark) = caps.mark() {
                matches.set_str("MARK", Value::string(mark));
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
        if byte_view {
            match callback_result.php_string_bytes() {
                Some(bytes) => result.push_str(&super::bytes_to_php_string(&bytes)),
                None => {
                    let rendered = callback_result.echo_to_string();
                    result.push_str(&super::bytes_to_php_string(rendered.as_bytes()));
                }
            }
        } else {
            callback_result.append_echo_to(&mut result);
        }
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
