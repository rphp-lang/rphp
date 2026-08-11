//! Cold target decoding for function Reflection.
//!
//! A PHP closure already carries a resolved function pointer for invocation.
//! Reusing that identity keeps Reflection metadata out of `Value`,
//! `PhpClosure` and ordinary call frames.

use crate::value::Value;
use crate::vm::execute::VmError;

#[inline(never)]
pub(super) fn reflection_function_target(value: &Value) -> Result<(&'static str, String), VmError> {
    let Some(closure) = value.as_closure() else {
        return Ok((
            "function",
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.echo_to_string()),
        ));
    };
    let Some(function) = closure.user_function() else {
        return Err(VmError::Fatal(
            "ReflectionFunction expects a user closure".into(),
        ));
    };
    Ok(("closure", function.op_array.name.clone()))
}
