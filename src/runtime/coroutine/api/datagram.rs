use std::borrow::Cow;
use std::net::SocketAddr;

use super::{
    ExecuteData, ExecutorGlobals, Value, VmError, argument, positive_argument, scheduler_ptr,
    write_result,
};

pub(super) fn coroutine_udp_bind(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let address = parse_address(execute_data, 0, "coroutine_udp_bind")?;
    let scheduler = scheduler_ptr(eg)?;
    let (socket, local) = unsafe { (&mut *scheduler).create_udp_socket(address)? };
    let mut result = crate::value::PhpArray::with_packed_capacity(2);
    result.push(Value::long(socket as i64));
    result.push(Value::string(local.to_string()));
    write_result(return_value, Value::array(result));
    Ok(())
}

pub(super) fn coroutine_udp_send_to(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let socket = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_udp_send_to",
        "socket id",
    )?;
    let data = unsafe { argument(execute_data, 1) }
        .as_str()
        .ok_or_else(|| VmError::Fatal("coroutine_udp_send_to expects string data".into()))?;
    let peer = parse_address(execute_data, 2, "coroutine_udp_send_to")?;
    let bytes = php_string_to_bytes(data);
    let scheduler = scheduler_ptr(eg)?;
    let written = unsafe { (&*scheduler).send_udp(socket, bytes.as_ref(), peer)? };
    write_result(
        return_value,
        written.map_or_else(|| Value::bool(false), |written| Value::long(written as i64)),
    );
    Ok(())
}

pub(super) fn coroutine_udp_recv_from(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    const MAX_DATAGRAM_LENGTH: u64 = 65_535;

    let socket = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_udp_recv_from",
        "socket id",
    )?;
    let length = positive_argument(
        unsafe { argument(execute_data, 1) },
        "coroutine_udp_recv_from",
        "length",
    )?;
    if length > MAX_DATAGRAM_LENGTH {
        return Err(VmError::Fatal(format!(
            "coroutine_udp_recv_from length exceeds {} bytes",
            MAX_DATAGRAM_LENGTH
        )));
    }
    let scheduler = scheduler_ptr(eg)?;
    let packet = unsafe { (&*scheduler).receive_udp(socket, length as usize)? };
    write_result(
        return_value,
        packet.map_or_else(
            || Value::bool(false),
            |(bytes, peer)| {
                let mut result = crate::value::PhpArray::with_packed_capacity(2);
                result.push(php_byte_value(bytes));
                result.push(Value::string(peer.to_string()));
                Value::array(result)
            },
        ),
    );
    Ok(())
}

fn parse_address(
    execute_data: *mut ExecuteData,
    index: u32,
    function: &str,
) -> Result<SocketAddr, VmError> {
    let address = unsafe { argument(execute_data, index) }
        .as_str()
        .ok_or_else(|| VmError::Fatal(format!("{function} expects a string address")))?;
    address
        .parse()
        .map_err(|_| VmError::Fatal(format!("{function} expects a numeric IP address and port")))
}

fn php_string_to_bytes(value: &str) -> Cow<'_, [u8]> {
    if value.is_ascii() {
        Cow::Borrowed(value.as_bytes())
    } else {
        Cow::Owned(value.chars().map(|character| character as u8).collect())
    }
}

fn php_byte_value(bytes: Vec<u8>) -> Value {
    if bytes.is_ascii() {
        Value::string(String::from_utf8(bytes).expect("ASCII datagram bytes are valid UTF-8"))
    } else {
        Value::binary_string(&bytes)
    }
}
