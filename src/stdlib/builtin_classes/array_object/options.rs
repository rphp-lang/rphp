//! Constructor and view policy for native array wrappers. Scalar options live
//! in the existing cold object auxiliary state, never in PHP property slots.
use super::*;
use crate::value::NativeArrayOptions;

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn iterator_class(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    argument: &Value,
    method: &str,
    index: u32,
) -> Result<Option<u32>, VmError> {
    let function = format!("ArrayObject::{method}");
    let Some(name) = typed_internal_string_value_expected(
        ed,
        eg,
        argument,
        &function,
        index,
        "iteratorClass",
        "string",
        "string",
    )?
    else {
        return Ok(None);
    };
    let name = name.as_str().expect("converted string");
    crate::stdlib::autoload::ensure_symbol_loaded(eg, name)?;
    if eg.exception.is_some() {
        return Ok(None);
    }
    if let Some(class) = find_class_case_insensitive(eg, name)
        && eg.class_is_a(&class.name, "ArrayIterator")
    {
        return Ok(Some(class.class_id));
    }
    eg.exception = Some(make_error_value(
        "TypeError",
        &format!(
            "{function}(): Argument #{} ($iteratorClass) must be a class name derived from ArrayIterator, {name} given",
            index + 1
        ),
    ));
    Ok(None)
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn construct_options(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    storage: &Value,
    owner: &str,
) -> Result<Option<NativeArrayOptions>, VmError> {
    let mut options = arg!(ed, 0)
        .as_object()
        .expect("native receiver")
        .native_array_options();
    let supplied = if let Some(value) = arg_opt!(ed, 2) {
        let value = value.dereferenced().clone();
        let Some(flags) = typed_internal_int_value_argument_expected(
            ed,
            eg,
            &value,
            &format!("{owner}::__construct"),
            1,
            "flags",
            "int",
        )?
        else {
            return Ok(None);
        };
        flags as u16
    } else {
        storage
            .as_object()
            .map_or(0, |object| object.native_array_options().flags)
    };
    if owner == "ArrayObject" {
        if let Some(argument) = arg_opt!(ed, 3) {
            let argument = argument.dereferenced().clone();
            let Some(class) = iterator_class(ed, eg, &argument, "__construct", 2)? else {
                return Ok(None);
            };
            options.iterator_class_id = class;
        }
    }
    options.flags |= supplied;
    Ok(Some(options))
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn get_flags(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::long(
            arg!(ed, 0)
                .as_object()
                .expect("native receiver")
                .native_array_options()
                .flags as i64
        )
    );
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn set_flags(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let owner = receiver.as_object().map_or("ArrayObject", |object| {
        if array_object_storage_key(&object) == ARRAY_ITERATOR_STORAGE {
            "ArrayIterator"
        } else {
            "ArrayObject"
        }
    });
    let argument = arg!(ed, 1).dereferenced().clone();
    let Some(flags) = typed_internal_int_value_argument_expected(
        ed,
        eg,
        &argument,
        &format!("{owner}::setFlags"),
        0,
        "flags",
        "int",
    )?
    else {
        return Ok(());
    };
    let mut object = receiver.as_object_mut().expect("native receiver");
    let mut options = object.native_array_options();
    options.flags = flags as u16;
    object.set_native_array_options(options);
    drop(object);
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn get_iterator_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let id = arg!(ed, 0)
        .as_object()
        .expect("native receiver")
        .native_array_options()
        .iterator_class_id;
    ret!(
        rv,
        Value::string(
            eg.class_by_id(id)
                .map_or("ArrayIterator", |class| class.name.as_str())
        )
    );
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn set_iterator_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let argument = arg!(ed, 1).dereferenced().clone();
    let Some(id) = iterator_class(ed, eg, &argument, "setIteratorClass", 0)? else {
        return Ok(());
    };
    let mut object = receiver.as_object_mut().expect("native receiver");
    let mut options = object.native_array_options();
    options.iterator_class_id = id;
    object.set_native_array_options(options);
    drop(object);
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut result = Vec::with_capacity(8);
    for owner in ["ArrayObject", "ArrayIterator"] {
        let storage_hint = ParamTypeHint::Union(vec![
            ParamTypeHint::ClassName("object".into()),
            ParamTypeHint::Array,
        ]);
        let mut constructors = (
            if owner == "ArrayObject" {
                &["array", "flags", "iteratorClass"] as &'static [&'static str]
            } else {
                &["array", "flags"]
            },
            vec![storage_hint, ParamTypeHint::Int],
            vec![Some(Value::array(PhpArray::new())), Some(Value::long(0))],
        );
        if owner == "ArrayObject" {
            constructors.1.push(ParamTypeHint::String);
            constructors.2.push(Some(Value::string("ArrayIterator")));
        }
        let mut methods = Vec::with_capacity(if owner == "ArrayObject" { 5 } else { 3 });
        methods.extend([
            (
                "__construct",
                fn_array_iterator_construct as InternalFunctionHandler,
                0,
                constructors.0,
                constructors.1,
                constructors.2,
                ParamTypeHint::None,
            ),
            (
                "getFlags",
                get_flags,
                0,
                &[],
                vec![],
                vec![],
                ParamTypeHint::Int,
            ),
            (
                "setFlags",
                set_flags,
                1,
                &["flags"],
                vec![ParamTypeHint::Int],
                vec![],
                ParamTypeHint::Void,
            ),
        ]);
        if owner == "ArrayObject" {
            methods.extend([
                (
                    "getIteratorClass",
                    get_iterator_class as InternalFunctionHandler,
                    0,
                    &[] as &'static [&'static str],
                    vec![],
                    vec![],
                    ParamTypeHint::String,
                ),
                (
                    "setIteratorClass",
                    set_iterator_class,
                    1,
                    &["iteratorClass"],
                    vec![ParamTypeHint::String],
                    vec![],
                    ParamTypeHint::Void,
                ),
            ]);
        }
        for (name, handler, required, names, hints, defaults, return_type) in methods {
            eg.register_internal_method_contract(
                owner,
                name,
                false,
                required,
                names,
                hints.clone(),
                return_type,
                &[None; 3][..names.len()],
                name != "__construct",
            );
            let mut function = boxed_method(handler, required, names);
            function.common.sig.param_type_hints = hints;
            function.handler_validates_types = true;
            let ptr = &function.common as *const FunctionCommon;
            register_method_identity(eg, ptr, owner, name);
            eg.register_internal_function_reflection_metadata(ptr, defaults, "SPL");
            result.push(function);
        }
    }
    result
}
