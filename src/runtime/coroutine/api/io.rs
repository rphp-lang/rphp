use std::borrow::Cow;
use std::net::SocketAddr;
use std::time::Duration;

use super::{
    ExecuteData, ExecutorGlobals, SuspendKind, Value, VmError, argument, positive_argument,
    scheduler_ptr, suspend_from_internal_call, suspension_caller, write_result,
};

#[cfg(any(target_vendor = "apple", target_os = "linux"))]
enum TcpConnectTarget {
    Numeric(SocketAddr),
    Host { host: String, port: u16 },
}

pub(super) fn coroutine_stream_pair(
    _execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let scheduler = scheduler_ptr(eg)?;
    let (first, second) = unsafe { (&mut *scheduler).create_stream_pair()? };
    let mut streams = crate::value::PhpArray::with_packed_capacity(2);
    streams.push(Value::long(first as i64));
    streams.push(Value::long(second as i64));
    write_result(return_value, Value::array(streams));
    Ok(())
}

pub(super) fn coroutine_tcp_listen(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let address = unsafe { argument(execute_data, 0) }
        .as_str()
        .ok_or_else(|| VmError::Fatal("coroutine_tcp_listen expects a string address".into()))?;
    let address = address.parse::<SocketAddr>().map_err(|_| {
        VmError::Fatal(
            "coroutine_tcp_listen expects a numeric IP address and port (for example 127.0.0.1:8080)"
                .into(),
        )
    })?;
    let scheduler = scheduler_ptr(eg)?;
    let (listener, local) = unsafe { (&mut *scheduler).create_tcp_listener(address)? };
    let mut result = crate::value::PhpArray::with_packed_capacity(2);
    result.push(Value::long(listener as i64));
    result.push(Value::string(local.to_string()));
    write_result(return_value, Value::array(result));
    Ok(())
}

#[cfg(any(target_vendor = "apple", target_os = "linux"))]
pub(super) fn coroutine_tcp_connect(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let address = unsafe { argument(execute_data, 0) }
        .as_str()
        .ok_or_else(|| VmError::Fatal("coroutine_tcp_connect expects a string address".into()))?;
    let target = parse_tcp_connect_target(address)?;
    let timeout = if unsafe { (*execute_data).num_args } > 1 {
        let milliseconds = unsafe { argument(execute_data, 1) }
            .as_long()
            .filter(|milliseconds| *milliseconds >= 0)
            .map(|milliseconds| milliseconds as u64)
            .ok_or_else(|| {
                VmError::Fatal(
                    "coroutine_tcp_connect expects non-negative timeout milliseconds".into(),
                )
            })?;
        Some(Duration::from_millis(milliseconds))
    } else {
        None
    };
    let caller = suspension_caller(execute_data)?;
    let scheduler = scheduler_ptr(eg)?;
    match target {
        TcpConnectTarget::Numeric(address) => {
            match unsafe { (&mut *scheduler).connect_tcp(address, timeout, caller, return_value)? }
            {
                Some(stream) => {
                    write_result(return_value, Value::long(stream as i64));
                    Ok(())
                }
                None => {
                    write_result(return_value, Value::null());
                    suspend_from_internal_call(caller, SuspendKind::Waiting)
                }
            }
        }
        TcpConnectTarget::Host { host, port } => {
            unsafe {
                (&mut *scheduler).resolve_and_connect_tcp(
                    host,
                    port,
                    timeout,
                    caller,
                    return_value,
                )?;
            }
            write_result(return_value, Value::null());
            suspend_from_internal_call(caller, SuspendKind::Waiting)
        }
    }
}

#[cfg(any(target_vendor = "apple", target_os = "linux"))]
fn parse_tcp_connect_target(address: &str) -> Result<TcpConnectTarget, VmError> {
    if let Ok(address) = address.parse::<SocketAddr>() {
        return Ok(TcpConnectTarget::Numeric(address));
    }
    let (host, port) = address.rsplit_once(':').ok_or_else(connect_address_error)?;
    let host = host.trim();
    let port = port.parse::<u16>().map_err(|_| connect_address_error())?;
    if host.is_empty() || host.contains(':') {
        return Err(connect_address_error());
    }
    Ok(TcpConnectTarget::Host {
        host: host.to_owned(),
        port,
    })
}

#[cfg(any(target_vendor = "apple", target_os = "linux"))]
fn connect_address_error() -> VmError {
    VmError::Fatal(
        "coroutine_tcp_connect expects an IP address or hostname followed by a port".into(),
    )
}

pub(super) fn coroutine_tcp_accept(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let listener = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_tcp_accept",
        "listener id",
    )?;
    let scheduler = scheduler_ptr(eg)?;
    let accepted = unsafe { (&mut *scheduler).accept_tcp(listener)? };
    write_result(
        return_value,
        accepted.map_or_else(
            || Value::bool(false),
            |(stream, peer)| {
                let mut result = crate::value::PhpArray::with_packed_capacity(2);
                result.push(Value::long(stream as i64));
                result.push(Value::string(peer.to_string()));
                Value::array(result)
            },
        ),
    );
    Ok(())
}

pub(super) fn coroutine_wait_readable(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let descriptor = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_wait_readable",
        "descriptor id",
    )?;
    let caller = suspension_caller(execute_data)?;
    let scheduler = scheduler_ptr(eg)?;
    unsafe { (&mut *scheduler).wait_readable(descriptor)? };
    write_result(return_value, Value::null());
    suspend_from_internal_call(caller, SuspendKind::Waiting)
}

pub(super) fn coroutine_wait_writable(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let descriptor = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_wait_writable",
        "descriptor id",
    )?;
    let caller = suspension_caller(execute_data)?;
    let scheduler = scheduler_ptr(eg)?;
    unsafe { (&mut *scheduler).wait_writable(descriptor)? };
    write_result(return_value, Value::null());
    suspend_from_internal_call(caller, SuspendKind::Waiting)
}

pub(super) fn coroutine_stream_read(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    const MAX_READ_LENGTH: u64 = 8 * 1024 * 1024;

    let stream = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_stream_read",
        "stream id",
    )?;
    let length = positive_argument(
        unsafe { argument(execute_data, 1) },
        "coroutine_stream_read",
        "length",
    )?;
    if length > MAX_READ_LENGTH {
        return Err(VmError::Fatal(format!(
            "coroutine_stream_read length exceeds {} bytes",
            MAX_READ_LENGTH
        )));
    }
    let length = usize::try_from(length).map_err(|_| {
        VmError::Fatal("coroutine_stream_read length exceeds the platform limit".into())
    })?;
    let scheduler = scheduler_ptr(eg)?;
    let result = unsafe { (&mut *scheduler).read_stream(stream, length)? };
    write_result(
        return_value,
        result.map_or_else(
            || Value::bool(false),
            |bytes| Value::string(bytes_to_php_string(&bytes)),
        ),
    );
    Ok(())
}

pub(super) fn coroutine_stream_write(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let stream = positive_argument(
        unsafe { argument(execute_data, 0) },
        "coroutine_stream_write",
        "stream id",
    )?;
    let data = unsafe { argument(execute_data, 1) }
        .as_str()
        .ok_or_else(|| VmError::Fatal("coroutine_stream_write expects string data".into()))?;
    let bytes = php_string_to_bytes(data);
    let scheduler = scheduler_ptr(eg)?;
    let result = unsafe { (&mut *scheduler).write_stream(stream, bytes.as_ref())? };
    write_result(
        return_value,
        result.map_or_else(|| Value::bool(false), |written| Value::long(written as i64)),
    );
    Ok(())
}

fn bytes_to_php_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| *byte as char).collect()
}

fn php_string_to_bytes(value: &str) -> Cow<'_, [u8]> {
    if value.is_ascii() {
        Cow::Borrowed(value.as_bytes())
    } else {
        Cow::Owned(value.chars().map(|character| character as u8).collect())
    }
}
