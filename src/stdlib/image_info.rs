//! Header-only image introspection and its output-reference lifetime boundary.
//! No raster decoding or GD dependency: bounds are checked before every read.
use super::*;

struct Header {
    width: u32,
    height: u32,
    kind: i64,
    bits: u8,
    channels: Option<u8>,
    mime: &'static str,
}
enum Probe {
    More,
    Unknown,
    Found(Header, PhpArray),
}
fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}
fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}
fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes(b[..4].try_into().unwrap())
}
fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes(b[..4].try_into().unwrap())
}

fn probe(data: &[u8]) -> Probe {
    if data.len() < 3 {
        return Probe::More;
    }
    let found = |width, height, kind, bits, channels, mime| {
        Probe::Found(
            Header {
                width,
                height,
                kind,
                bits,
                channels,
                mime,
            },
            PhpArray::new(),
        )
    };
    if data.starts_with(b"GIF") {
        if data.len() < 13 {
            return Probe::More;
        }
        if !matches!(&data[..6], b"GIF87a" | b"GIF89a") {
            return Probe::Unknown;
        }
        return found(
            le16(&data[6..]) as u32,
            le16(&data[8..]) as u32,
            1,
            (data[10] & 7) + 1,
            Some(3),
            "image/gif",
        );
    }
    if data.starts_with(b"\x89PN") {
        if data.len() < 26 {
            return Probe::More;
        }
        if &data[..8] != b"\x89PNG\r\n\x1a\n" || &data[12..16] != b"IHDR" {
            return Probe::Unknown;
        }
        return found(
            be32(&data[16..]),
            be32(&data[20..]),
            3,
            data[24],
            None,
            "image/png",
        );
    }
    if data.starts_with(b"BM") {
        if data.len() < 26 {
            return Probe::More;
        }
        let size = le32(&data[14..]);
        if size == 12 {
            return found(
                le16(&data[18..]) as u32,
                le16(&data[20..]) as u32,
                6,
                le16(&data[24..]) as u8,
                None,
                "image/bmp",
            );
        }
        if size >= 40 {
            if data.len() < 30 {
                return Probe::More;
            }
            return found(
                le32(&data[18..]),
                (le32(&data[22..]) as i32).unsigned_abs(),
                6,
                le16(&data[28..]) as u8,
                None,
                "image/bmp",
            );
        }
        return Probe::Unknown;
    }
    if data.starts_with(b"\xff\xd8\xff") {
        let mut offset = 2;
        let mut info = PhpArray::new();
        loop {
            if offset + 2 > data.len() {
                return Probe::More;
            }
            if data[offset] != 0xff {
                return Probe::Unknown;
            }
            while data.get(offset) == Some(&0xff) {
                offset += 1;
            }
            let Some(&marker) = data.get(offset) else {
                return Probe::More;
            };
            offset += 1;
            if matches!(marker, 0xd9 | 0xda | 0) {
                return Probe::Unknown;
            }
            if marker == 1 || (0xd0..=0xd8).contains(&marker) {
                continue;
            }
            if offset + 2 > data.len() {
                return Probe::More;
            }
            let length = be16(&data[offset..]) as usize;
            if length < 2 {
                return Probe::Unknown;
            }
            let Some(end) = offset.checked_add(length) else {
                return Probe::Unknown;
            };
            if end > data.len() {
                return Probe::More;
            }
            let payload = &data[offset + 2..end];
            if (0xe0..=0xef).contains(&marker) {
                let key = format!("APP{}", marker - 0xe0);
                if info.get_str(&key).is_none() {
                    info.set_str(&key, Value::binary_string(payload));
                }
            }
            if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
                if payload.len() < 6 {
                    return Probe::Unknown;
                }
                return Probe::Found(
                    Header {
                        width: be16(&payload[3..]) as u32,
                        height: be16(&payload[1..]) as u32,
                        kind: 2,
                        bits: payload[0],
                        channels: Some(payload[5]),
                        mime: "image/jpeg",
                    },
                    info,
                );
            }
            offset = end;
        }
    }
    Probe::Unknown
}

fn output(header: Header) -> Value {
    let mut values = PhpArray::new();
    values.push(Value::long(header.width as i64));
    values.push(Value::long(header.height as i64));
    values.push(Value::long(header.kind));
    values.push(Value::string(format!(
        "width=\"{}\" height=\"{}\"",
        header.width, header.height
    )));
    values.set_str("bits", Value::long(header.bits as i64));
    if let Some(channels) = header.channels {
        values.set_str("channels", Value::long(channels as i64));
    }
    values.set_str("mime", Value::string(header.mime));
    values.set_str("width_unit", Value::string("px"));
    values.set_str("height_unit", Value::string("px"));
    Value::array(values)
}

/// Validate the eventual array before releasing anything. PHP temporarily
/// publishes NULL during the old value's destructor and commits [] even when
/// that destructor throws; opening the image must not follow that exception.
fn reset_info(ed: *mut ExecuteData, eg: &mut ExecutorGlobals) -> Result<bool, VmError> {
    if arg_opt!(ed, 1).is_none() {
        return Ok(true);
    }
    let empty = Value::array(PhpArray::new());
    let constraints = with_raw_argument(ed, 1, Value::reference_property_constraints);
    if let Err(message) = crate::vm::execute::prepare_reference_assignment_scalar(
        empty.clone(),
        &constraints,
        eg,
        true,
    ) {
        eg.exception = Some(crate::value::make_error_value("TypeError", &message));
        return Ok(false);
    }
    let release = prepare_replaced_value_release(eg, arg!(ed, 1));
    arg_mut!(ed, 1, Value::null());
    let trace = release
        .as_ref()
        .map(|_| crate::runtime::FunctionArgumentSnapshot {
            first: 0,
            values: vec![arg!(ed, 0).clone(), Value::null()],
        });
    let result = with_internal_trace_origin(ed, eg, |eg| {
        run_prepared_value_destructors_from_internal(eg, ed, [(release, trace)])
    });
    arg_mut!(ed, 1, empty);
    result?;
    Ok(eg.exception.is_none())
}

#[cold]
fn inspect(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    from_string: bool,
) -> Result<(), VmError> {
    let name = if from_string {
        "getimagesizefromstring"
    } else {
        "getimagesize"
    };
    let parameter = if from_string { "string" } else { "filename" };
    let Some(input) =
        typed_internal_string_value_argument_expected(ed, eg, name, 0, parameter, "string")?
    else {
        return Ok(());
    };
    if !reset_info(ed, eg)? {
        return Ok(());
    }
    let data;
    let bytes = if from_string {
        input.php_string_bytes().unwrap()
    } else {
        let filename = input.as_str().unwrap();
        if filename.is_empty() {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "Path must not be empty",
            ));
            return Ok(());
        }
        if !filesystem::validate_stream_path(eg, filename, name) {
            return Ok(());
        }
        if !filesystem::url_open_allowed(ed, eg, filename, name)? {
            ret!(rv, Value::bool(false));
        }
        let mut stream = match phar::open_or_native(eg, filename, "rb") {
            Ok(stream) => stream,
            Err(error) => {
                let reason = if error.kind() == std::io::ErrorKind::NotFound {
                    "No such file or directory".into()
                } else {
                    error.to_string()
                };
                report_internal_diagnostic(
                    eg,
                    ed,
                    2,
                    "Warning",
                    &format!("{name}({filename}): Failed to open stream: {reason}"),
                )?;
                ret!(rv, Value::bool(false));
            }
        };
        if stream.is_plain_file() {
            filesystem::clear_filesystem_stat_cache(eg);
        }
        let mut buffer = Vec::new();
        let mut chunk = [0; 1024];
        loop {
            let read = stream.read(&mut chunk).unwrap_or(0);
            buffer.extend_from_slice(&chunk[..read]);
            if read == 0 || !matches!(probe(&buffer), Probe::More) {
                break;
            }
        }
        data = buffer;
        Cow::Borrowed(data.as_slice())
    };
    if bytes.len() < 3 || (bytes.len() < 12 && matches!(probe(&bytes), Probe::Unknown)) {
        report_internal_diagnostic(
            eg,
            ed,
            8,
            "Notice",
            &format!("{name}(): Error reading from {}!", input.as_str().unwrap()),
        )?;
    }
    if let Probe::Found(header, info) = probe(&bytes) {
        if arg_opt!(ed, 1).is_some() {
            arg_mut!(ed, 1, Value::array(info));
        }
        ret!(rv, output(header));
    }
    ret!(rv, Value::bool(false));
}
fn from_file(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    inspect(ed, rv, eg, false)
}
fn from_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    inspect(ed, rv, eg, true)
}

pub(super) fn register(eg: &mut ExecutorGlobals, functions: &mut Vec<Box<InternalFunction>>) {
    for (name, parameter, handler) in [
        (
            "getimagesize",
            "filename",
            from_file as crate::vm::function::InternalFunctionHandler,
        ),
        ("getimagesizefromstring", "string", from_string),
    ] {
        let mut function = Box::new(make_internal_function_ref(
            handler,
            2,
            1,
            0b10,
            vec![parameter.into(), "image_info".into()],
        ));
        function.common.sig.param_type_hints = vec![ParamTypeHint::String, ParamTypeHint::None];
        function.common.sig.return_type_hint = ParamTypeHint::Union(vec![
            ParamTypeHint::Array,
            ParamTypeHint::ClassName("false".into()),
        ]);
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(name, pointer).unwrap();
        eg.register_internal_function_reflection_metadata(
            pointer,
            vec![None, Some(Value::null())],
            "standard",
        );
        functions.push(function);
    }
}
