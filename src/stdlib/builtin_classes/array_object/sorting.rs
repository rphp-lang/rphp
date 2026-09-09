//! Native array-backed sort entry points. Comparisons and their observable
//! schedule belong to the ordinary array implementation; only storage commit
//! and the receiver-local mutation boundary are specific to these methods.
use super::*;
use std::cmp::Ordering;

const SORT_GUARD: &str = "\0rphp-native-array-sort";

#[cold]
#[inline(never)]
// SAFETY: ordinary compiler-generated code retains the normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(in crate::stdlib::builtin_classes) fn reject_mutation(
    receiver: &Value,
    eg: &mut ExecutorGlobals,
) -> bool {
    if receiver
        .as_object()
        .is_some_and(|o| o.property_guard_active(SORT_GUARD, 1))
    {
        mutation_error(eg);
        true
    } else {
        false
    }
}

#[cold]
#[inline(never)]
// SAFETY: ordinary compiler-generated code retains the normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn mutation_error(eg: &mut ExecutorGlobals) {
    eg.exception = Some(make_error_value(
        "Error",
        "Modification of ArrayObject during sorting is prohibited",
    ));
}

#[derive(Clone, Copy)]
enum Kind {
    Values,
    Keys,
    Natural,
    NaturalCase,
    UserValues,
    UserKeys,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Values => "asort",
            Self::Keys => "ksort",
            Self::Natural => "natsort",
            Self::NaturalCase => "natcasesort",
            Self::UserValues => "uasort",
            Self::UserKeys => "uksort",
        }
    }
    fn callback(self) -> bool {
        matches!(self, Self::UserValues | Self::UserKeys)
    }
    fn flags(self) -> bool {
        matches!(self, Self::Values | Self::Keys)
    }
}

#[cold]
#[inline(never)]
// SAFETY: ordinary compiler-generated code retains the normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn sort(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    kind: Kind,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let owner_name = receiver.as_object().map_or("ArrayObject", |o| {
        if array_object_storage_key(&o) == ARRAY_ITERATOR_STORAGE {
            "ArrayIterator"
        } else {
            "ArrayObject"
        }
    });
    let flags = if kind.flags()
        && let Some(argument) = arg_opt!(ed, 1)
    {
        let argument = argument.dereferenced().clone();
        let Some(flags) = typed_internal_int_value_argument_expected(
            ed,
            eg,
            &argument,
            &format!("{owner_name}::{}", kind.name()),
            0,
            "flags",
            "int",
        )?
        else {
            return Ok(());
        };
        flags
    } else {
        0
    };
    let callback = kind.callback().then(|| arg!(ed, 1).dereferenced().clone());
    let resolved = if let Some(callback) = &callback {
        let Some(resolved) = resolve_callback_at_callsite_checked(callback, eg, ed)? else {
            if eg.exception.is_none() {
                let reason = ordinary_callback_invalid_reason(callback, eg);
                eg.exception = Some(make_error_value(
                    "TypeError",
                    &format!(
                        "{}(): Argument #2 ($callback) must be a valid callback, {reason}",
                        kind.name()
                    ),
                ));
            }
            return Ok(());
        };
        Some(resolved)
    } else {
        None
    };
    let Some(Backing::Array(owner, key)) = backing(&receiver) else {
        return Err(VmError::Fatal(
            "Object-property-backed native sorting is not implemented".into(),
        ));
    };
    let source = owner
        .as_object()
        .and_then(|o| o.get_property(key).cloned())
        .expect("resolved native array storage");
    let array = source.as_array().expect("resolved array backing");
    let external = array.has_external_byte_keys();
    let mut entries = Vec::with_capacity(array.len());
    for (key, value) in array.iter() {
        entries.push((key, array_sort_snapshot_value(value)));
    }
    let mut order: Vec<usize> = (0..entries.len()).collect();
    let mut state = callback
        .as_ref()
        .zip(resolved.as_ref())
        .map(|(callback, resolved)| UserSortCallbackState::new(callback, resolved, eg));
    let already_sorting = receiver
        .as_object()
        .is_some_and(|o| o.property_guard_active(SORT_GUARD, 1));
    receiver
        .as_object_mut()
        .expect("native receiver")
        .set_property_guard(SORT_GUARD, 1, true);
    // No object/array slot borrow crosses a comparator. The owned permutation
    // remains valid even when another wrapper replaces the shared storage.
    // None represents a published PHP exception, not an arbitrary ordering.
    let mut compare = |left: &(ArrayKey, Value), right: &(ArrayKey, Value)| {
        let result: Result<Option<Ordering>, VmError> = match kind {
            Kind::Values => sort_value_order_runtime(ed, eg, &left.1, &right.1, flags).map(Some),
            Kind::Keys => Ok(Some(
                sort_key_order(&left.0, &right.0, flags, eg.precision, external)
                    .unwrap_or(Ordering::Equal),
            )),
            Kind::Natural | Kind::NaturalCase => {
                natural_value_order(ed, eg, &left.1, &right.1, matches!(kind, Kind::NaturalCase))
            }
            Kind::UserValues | Kind::UserKeys => {
                let keys = matches!(kind, Kind::UserKeys).then(|| {
                    [
                        array_key_value(&left.0, external),
                        array_key_value(&right.0, external),
                    ]
                });
                let (a, b) = keys
                    .as_ref()
                    .map_or((&left.1, &right.1), |keys| (&keys[0], &keys[1]));
                user_sort_comparison(
                    ed,
                    eg,
                    resolved.as_ref().unwrap(),
                    state.as_mut().unwrap(),
                    kind.name(),
                    a,
                    b,
                )
            }
        };
        match result {
            Err(error) => Err(error),
            Ok(Some(order)) if eg.exception.is_none() => Ok(Some(order)),
            _ => Ok(None),
        }
    };
    let outcome = if kind.callback() && entries.len() < 6 {
        // This shared callback schedule commits each swap before the next
        // comparator, which remains visible when a small sort throws.
        stable_sort_small_optional_checked(&mut entries, &mut compare)
            .map_err(Some)
            .and_then(|complete| if complete { Ok(()) } else { Err(None) })
    } else {
        php_observed_sort_schedule(&entries, &mut order, &mut |left, right| {
            compare(left, right).map_err(Some)?.ok_or(None)
        })
    };
    receiver
        .as_object_mut()
        .expect("retained native receiver")
        .set_property_guard(SORT_GUARD, 1, already_sorting);
    // PHP publishes the partial permutation even when comparison throws. Keep
    // original keys, storage provenance and externally aliased reference cells.
    let mut sorted = PhpArray::new();
    for index in order {
        sorted.set(
            entries[index].0.clone(),
            array_projection_value(&entries[index].1),
        );
    }
    copy_array_key_provenance(array, &sorted);
    replace_storage_with_cursor_policy(&owner, Value::array(sorted), eg, false)?;
    if let Err(Some(error)) = outcome {
        return Err(error);
    }
    if eg.exception.is_none() {
        ret!(rv, Value::bool(true));
    }
    Ok(())
}

macro_rules! handler {
    ($name:ident,$kind:ident) => {
        #[cold]
        #[inline(never)]
        // SAFETY: ordinary compiler-generated code retains its normal calling convention.
        #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
        fn $name(
            ed: *mut ExecuteData,
            rv: *mut Value,
            eg: &mut ExecutorGlobals,
        ) -> Result<(), VmError> {
            sort(ed, rv, eg, Kind::$kind)
        }
    };
}
handler!(values, Values);
handler!(keys, Keys);
handler!(natural, Natural);
handler!(natural_case, NaturalCase);
handler!(user_values, UserValues);
handler!(user_keys, UserKeys);

#[cold]
#[inline(never)]
// SAFETY: ordinary compiler-generated code retains the normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut functions = Vec::with_capacity(12);
    for owner in ["ArrayObject", "ArrayIterator"] {
        for (kind, handler) in [
            (Kind::Values, values as InternalFunctionHandler),
            (Kind::Keys, keys),
            (Kind::Natural, natural),
            (Kind::NaturalCase, natural_case),
            (Kind::UserValues, user_values),
            (Kind::UserKeys, user_keys),
        ] {
            let names = if kind.callback() {
                vec!["callback"]
            } else if kind.flags() {
                vec!["flags"]
            } else {
                vec![]
            };
            let hints = if kind.callback() {
                vec![ParamTypeHint::Callable]
            } else if kind.flags() {
                vec![ParamTypeHint::Int]
            } else {
                vec![]
            };
            let defaults = if kind.callback() {
                vec![None]
            } else if kind.flags() {
                vec![Some(Value::long(0))]
            } else {
                vec![]
            };
            let required = usize::from(kind.callback());
            eg.register_internal_method_contract(
                owner,
                kind.name(),
                false,
                required as u32,
                &names,
                hints.clone(),
                ParamTypeHint::ClassName("true".into()),
                &if kind.flags() {
                    vec![Some("SORT_REGULAR")]
                } else {
                    vec![None; names.len()]
                },
                true,
            );
            let mut function = Box::new(make_internal_method(
                handler,
                names.len() as u32 + 1,
                required as u32,
                names.iter().map(|n| n.to_string()).collect(),
            ));
            function.common.sig.param_type_hints = hints;
            function.handler_validates_types = true;
            let ptr = &function.common as *const FunctionCommon;
            let display = format!("{owner}::{}", kind.name());
            eg.function_table.insert(display.to_ascii_lowercase(), ptr);
            eg.method_declaring_class.insert(ptr, owner.into());
            eg.register_internal_function_display_name(ptr, display);
            eg.register_internal_function_reflection_metadata_with_diagnostics(
                ptr,
                defaults,
                if kind.flags() {
                    &[Some("SORT_REGULAR")]
                } else if kind.callback() {
                    &[None]
                } else {
                    &[]
                },
                "SPL",
            );
            functions.push(function);
        }
    }
    functions
}
