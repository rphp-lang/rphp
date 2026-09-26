//! Fiber-only native replacement continuation. The ordinary streaming PCRE
//! consumer is unchanged. The matcher publishes owned captures before PHP
//! re-entry; each callback then runs exactly once on a pinned VM activation.
use super::*;
use crate::runtime::fiber::{
    FiberInput,
    native::{NativeCallback, NativeCallbackOutcome, NativeOperation},
};
use std::collections::VecDeque;
use std::pin::Pin;
use std::rc::Rc;

struct MatchWork {
    start: usize,
    end: usize,
    matches: Value,
}
struct Replacement {
    subject: String,
    unicode: bool,
    matches: VecDeque<MatchWork>,
    result: String,
    previous_end: usize,
    callback: Option<Pin<Box<NativeCallback>>>,
}
impl Replacement {
    fn new(
        value: &Value,
        regex: &crate::regex::Regex,
        limit: usize,
        flags: i64,
        eg: &mut ExecutorGlobals,
    ) -> Option<Self> {
        let unicode = regex.is_unicode();
        let subject = if unicode {
            match pcre::prepare_utf_subject(value, Cow::Borrowed(value.as_str().unwrap()), 0) {
                Ok(subject) => subject.into_owned(),
                Err(error) => {
                    pcre::set_last_error(eg, error);
                    return None;
                }
            }
        } else {
            bytes_to_php_string(&value.php_string_bytes().unwrap_or_default())
        };
        let mapped = !unicode && !subject.is_ascii();
        let unmatched = flags & 512 != 0;
        let mut matches = VecDeque::new();
        let visited: Result<usize, std::convert::Infallible> =
            regex.try_visit_captures(&subject, |caps| {
                if matches.len() == limit {
                    return Ok(false);
                }
                let full = caps.get(0).unwrap();
                let last = if unmatched {
                    caps.len() - 1
                } else {
                    (0..caps.len())
                        .rev()
                        .find(|&index| caps.get(index).is_some())
                        .unwrap_or(0)
                };
                let mut values = PhpArray::new();
                for index in 0..=last {
                    let value = pcre_capture_value(
                        caps.get(index),
                        &subject,
                        0,
                        flags & 256 != 0,
                        unmatched,
                        mapped,
                    );
                    for (name, slot) in caps.named_groups() {
                        if *slot == index {
                            values.set_str(name, value.clone());
                        }
                    }
                    values.push(value);
                }
                if let Some(mark) = caps.mark() {
                    values.set_str("MARK", Value::string(mark));
                }
                matches.push_back(MatchWork {
                    start: full.start,
                    end: full.end,
                    matches: Value::array(values),
                });
                Ok(true)
            });
        visited.unwrap();
        Some(Self {
            subject,
            unicode,
            matches,
            result: String::new(),
            previous_end: 0,
            callback: None,
        })
    }
}

struct ReplaceCall {
    resolved: ResolvedCallback,
    patterns: Vec<String>,
    compiled: Vec<Option<Rc<crate::regex::Regex>>>,
    subjects: VecDeque<(ArrayKey, Value)>,
    source: Value,
    array: bool,
    results: PhpArray,
    current: Option<(ArrayKey, Value)>,
    pattern_index: usize,
    replacement: Option<Replacement>,
    total: usize,
    limit: usize,
    flags: i64,
}

impl NativeOperation for ReplaceCall {
    fn cycle_snapshot(&self) -> (Vec<Value>, Vec<usize>) {
        let mut values = Vec::new();
        let mut frames = Vec::new();
        for value in self
            .resolved
            .prepend_args
            .iter()
            .chain(&self.resolved.use_vars)
            .chain(self.resolved.bound_this.iter())
        {
            values.extend(value.clone_cycle_handle());
        }
        values.extend(self.source.clone_cycle_handle());
        for (_, value) in &self.subjects {
            values.extend(value.clone_cycle_handle());
        }
        if let Some((_, value)) = &self.current {
            values.extend(value.clone_cycle_handle());
        }
        for (_, value) in self.results.iter() {
            values.extend(value.clone_cycle_handle());
        }
        if let Some(replacement) = &self.replacement {
            for item in &replacement.matches {
                values.extend(item.matches.clone_cycle_handle());
            }
            if let Some(callback) = &replacement.callback {
                let (children, callback_frames) = callback.cycle_snapshot();
                values.extend(children);
                frames.extend(callback_frames);
            }
        }
        (values, frames)
    }

    fn run(
        &mut self,
        eg: &mut ExecutorGlobals,
        ed: *mut ExecuteData,
        mut input: Option<FiberInput>,
    ) -> Result<NativeCallbackOutcome, VmError> {
        loop {
            if let Some(replacement) = &mut self.replacement {
                if let Some(item) = replacement.matches.front() {
                    if replacement.callback.is_none() {
                        if callback_has_hard_reference_parameters(&self.resolved) {
                            let mut arguments = PhpArray::with_packed_capacity(1);
                            arguments.push(item.matches.clone());
                            let name = displayed_function_name(eg, self.resolved.func_ptr);
                            if !report_callback_reference_warnings(
                                eg,
                                ed,
                                &self.resolved,
                                &arguments,
                                true,
                                &name,
                            )? {
                                return Ok(NativeCallbackOutcome::Complete(Value::null()));
                            }
                        }
                        replacement.callback = Some(NativeCallback::with_arguments(
                            self.resolved.clone(),
                            vec![item.matches.clone()],
                        ));
                    }
                    let callback = replacement.callback.as_mut().unwrap();
                    let value = match NativeCallback::run(callback.as_mut(), eg, input.take(), ed)?
                    {
                        NativeCallbackOutcome::Suspended(value) => {
                            return Ok(NativeCallbackOutcome::Suspended(value));
                        }
                        NativeCallbackOutcome::Complete(value) => value,
                    };
                    replacement.callback = None;
                    if eg.exception.is_some() {
                        return Ok(NativeCallbackOutcome::Complete(Value::null()));
                    }
                    replacement
                        .result
                        .push_str(&replacement.subject[replacement.previous_end..item.start]);
                    if replacement.unicode {
                        value.append_echo_to(&mut replacement.result);
                    } else {
                        let bytes = value
                            .php_string_bytes()
                            .unwrap_or_else(|| Cow::Owned(value.echo_to_string().into_bytes()));
                        replacement.result.push_str(&bytes_to_php_string(&bytes));
                    }
                    replacement.previous_end = item.end;
                    replacement.matches.pop_front();
                    self.total += 1;
                    continue;
                }
                let mut replacement = self.replacement.take().unwrap();
                replacement
                    .result
                    .push_str(&replacement.subject[replacement.previous_end..]);
                self.current.as_mut().unwrap().1 = php_byte_result(
                    pcre_engine_result_bytes(replacement.result, replacement.unicode),
                    false,
                );
                self.pattern_index += 1;
            }
            if self.current.is_none() {
                let Some((key, value)) = self.subjects.pop_front() else {
                    publish_count(ed, eg, self.total);
                    let result = if self.array {
                        copy_array_key_provenance(self.source.as_array().unwrap(), &self.results);
                        Value::array(std::mem::replace(&mut self.results, PhpArray::new()))
                    } else {
                        self.results.get_int(0).cloned().unwrap_or_else(Value::null)
                    };
                    return Ok(NativeCallbackOutcome::Complete(result));
                };
                let Some(value) = internal_value_to_string_value(ed, eg, &value)? else {
                    return Ok(NativeCallbackOutcome::Complete(Value::null()));
                };
                self.current = Some((key, value));
                self.pattern_index = 0;
            }
            if self.pattern_index == self.patterns.len() {
                let (key, value) = self.current.take().unwrap();
                match key {
                    ArrayKey::Int(key) => self.results.set_int(key, value),
                    ArrayKey::String(key) => self.results.set_str(&key, value),
                };
                continue;
            }
            if self.compiled[self.pattern_index].is_none() {
                self.compiled[self.pattern_index] = pcre::compile_pattern(
                    eg,
                    ed,
                    "preg_replace_callback",
                    &self.patterns[self.pattern_index],
                )?;
            }
            let Some(regex) = &self.compiled[self.pattern_index] else {
                publish_count(ed, eg, self.total);
                return Ok(NativeCallbackOutcome::Complete(Value::null()));
            };
            let value = &self.current.as_ref().unwrap().1;
            self.replacement = Replacement::new(value, regex, self.limit, self.flags, eg);
            if self.replacement.is_none() {
                return Ok(NativeCallbackOutcome::Complete(Value::null()));
            }
        }
    }
}

fn publish_count(ed: *mut ExecuteData, eg: &mut ExecutorGlobals, count: usize) {
    if arg_opt!(ed, 4).is_none() {
        return;
    }
    let constraints = with_raw_argument(ed, 4, Value::reference_property_constraints);
    match crate::vm::execute::prepare_reference_assignment_scalar(
        Value::long(count as i64),
        &constraints,
        eg,
        false,
    ) {
        Ok(value) => {
            arg_mut!(ed, 4, value);
        }
        Err(message) => {
            eg.exception = Some(crate::value::make_error_value("TypeError", &message));
        }
    }
}

pub(super) fn start(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    pattern: Value,
    subject: Value,
    resolved: ResolvedCallback,
    limit: usize,
    flags: i64,
) -> Result<(), VmError> {
    let Some((patterns, _)) = preg_replace_argument_strings(ed, eg, &pattern)? else {
        return Ok(());
    };
    let array = subject.as_array().is_some();
    let subjects = if let Some(subjects) = subject.as_array() {
        subjects
            .iter()
            .map(|(key, value)| (key, value.clone()))
            .collect()
    } else {
        VecDeque::from([(ArrayKey::Int(0), subject.clone())])
    };
    let mut compiled = vec![None; patterns.len()];
    if array {
        for (pattern, slot) in patterns.iter().zip(&mut compiled) {
            *slot = pcre::compile_pattern(eg, ed, "preg_replace_callback", pattern)?;
            if slot.is_none() {
                publish_count(ed, eg, 0);
                ret!(rv, Value::null());
            }
        }
    }
    crate::vm::execute::start_native_call(
        eg,
        ed,
        rv,
        Box::new(ReplaceCall {
            resolved,
            patterns,
            compiled,
            subjects,
            source: subject,
            array,
            results: PhpArray::new(),
            current: None,
            pattern_index: 0,
            replacement: None,
            total: 0,
            limit,
            flags,
        }),
    )
}
