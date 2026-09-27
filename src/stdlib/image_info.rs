//! Header-only image introspection and its output-reference lifetime boundary.
//! No raster decoding or GD dependency: bounds are checked before every read.
use super::*;

struct Header {
    width: u32,
    height: u32,
    kind: i64,
    bits: Option<u8>,
    channels: Option<u8>,
    mime: &'static str,
    width_unit: String,
    height_unit: String,
    dimensions: bool,
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

fn svg_dimension(value: &[u8]) -> Option<(u32, String)> {
    let digits = value
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits == 0 || !value[digits..].iter().all(u8::is_ascii_alphabetic) {
        return None;
    }
    let number = value[..digits].iter().fold(0_u32, |number, byte| {
        number.wrapping_mul(10).wrapping_add(u32::from(byte - b'0'))
    });
    let unit = if digits == value.len() {
        "px".to_string()
    } else {
        String::from_utf8(value[digits..].to_vec()).ok()?
    };
    Some((number, unit))
}

fn xml_name_end(data: &[u8], mut offset: usize) -> usize {
    while data.get(offset).is_some_and(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
    }) {
        offset += 1;
    }
    offset
}

fn skip_xml_markup(data: &[u8], mut offset: usize, terminator: &[u8]) -> Result<usize, Probe> {
    while offset + terminator.len() <= data.len() {
        if &data[offset..offset + terminator.len()] == terminator {
            return Ok(offset + terminator.len());
        }
        offset += 1;
    }
    Err(Probe::More)
}

/// Recognize the SVG header contract exposed by PHP's image extension without
/// constructing an XML document. The first root element and its quoted width
/// and height attributes are sufficient; malformed or incomplete markup never
/// publishes partial metadata.
fn probe_svg(data: &[u8]) -> Probe {
    let mut offset = usize::from(data.starts_with(b"\xef\xbb\xbf")) * 3;
    loop {
        while data.get(offset).is_some_and(u8::is_ascii_whitespace) {
            offset += 1;
        }
        let Some(rest) = data.get(offset..) else {
            return Probe::More;
        };
        if rest.starts_with(b"<?") {
            offset = match skip_xml_markup(data, offset + 2, b"?>") {
                Ok(offset) => offset,
                Err(probe) => return probe,
            };
            continue;
        }
        if rest.starts_with(b"<!--") {
            offset = match skip_xml_markup(data, offset + 4, b"-->") {
                Ok(offset) => offset,
                Err(probe) => return probe,
            };
            continue;
        }
        break;
    }

    if data.get(offset) != Some(&b'<') {
        return Probe::Unknown;
    }
    offset += 1;
    let name_start = offset;
    let name_end = xml_name_end(data, name_start);
    if name_end == name_start {
        return Probe::Unknown;
    }
    let name = &data[name_start..name_end];
    let (prefix, local) = match name.iter().position(|byte| *byte == b':') {
        Some(colon) if colon != 0 && colon + 1 < name.len() => {
            (Some(&name[..colon]), &name[colon + 1..])
        }
        Some(_) => return Probe::Unknown,
        None => (None, name),
    };
    if !local.eq_ignore_ascii_case(b"svg") {
        return Probe::Unknown;
    }

    let mut width = None;
    let mut height = None;
    let mut namespace_matches = prefix.is_none();
    offset = name_end;
    loop {
        while data.get(offset).is_some_and(u8::is_ascii_whitespace) {
            offset += 1;
        }
        match data.get(offset) {
            Some(b'>') => break,
            Some(b'/') if data.get(offset + 1) == Some(&b'>') => break,
            None => return Probe::More,
            _ => {}
        }
        let attribute_start = offset;
        let attribute_end = xml_name_end(data, attribute_start);
        if attribute_end == attribute_start {
            return Probe::Unknown;
        }
        let attribute = &data[attribute_start..attribute_end];
        offset = attribute_end;
        while data.get(offset).is_some_and(u8::is_ascii_whitespace) {
            offset += 1;
        }
        if data.get(offset) != Some(&b'=') {
            return Probe::Unknown;
        }
        offset += 1;
        while data.get(offset).is_some_and(u8::is_ascii_whitespace) {
            offset += 1;
        }
        let Some(&quote @ (b'\'' | b'"')) = data.get(offset) else {
            return if offset == data.len() {
                Probe::More
            } else {
                Probe::Unknown
            };
        };
        offset += 1;
        let value_start = offset;
        while data.get(offset).is_some_and(|byte| *byte != quote) {
            offset += 1;
        }
        if data.get(offset) != Some(&quote) {
            return Probe::More;
        }
        let value = &data[value_start..offset];
        offset += 1;

        match attribute {
            b"width" => width = svg_dimension(value),
            b"height" => height = svg_dimension(value),
            _ => {
                if let Some(prefix) = prefix
                    && attribute.starts_with(b"xmlns:")
                    && &attribute[b"xmlns:".len()..] == prefix
                {
                    namespace_matches = value == b"http://www.w3.org/2000/svg";
                }
            }
        }
    }
    let (Some((width, width_unit)), Some((height, height_unit))) = (width, height) else {
        return Probe::Unknown;
    };
    if !namespace_matches {
        return Probe::Unknown;
    }
    let dimensions = width_unit == "px" && height_unit == "px";
    Probe::Found(
        Header {
            width,
            height,
            kind: 21,
            bits: None,
            channels: None,
            mime: "image/svg+xml",
            width_unit,
            height_unit,
            dimensions,
        },
        PhpArray::new(),
    )
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
                bits: Some(bits),
                channels,
                mime,
                width_unit: "px".to_string(),
                height_unit: "px".to_string(),
                dimensions: true,
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
                        bits: Some(payload[0]),
                        channels: Some(payload[5]),
                        mime: "image/jpeg",
                        width_unit: "px".to_string(),
                        height_unit: "px".to_string(),
                        dimensions: true,
                    },
                    info,
                );
            }
            offset = end;
        }
    }
    probe_svg(data)
}

fn output(header: Header) -> Value {
    let mut values = PhpArray::new();
    values.push(Value::long(header.width as i64));
    values.push(Value::long(header.height as i64));
    values.push(Value::long(header.kind));
    if header.dimensions {
        values.push(Value::string(format!(
            "width=\"{}\" height=\"{}\"",
            header.width, header.height
        )));
    }
    if let Some(bits) = header.bits {
        values.set_str("bits", Value::long(bits as i64));
    }
    if let Some(channels) = header.channels {
        values.set_str("channels", Value::long(channels as i64));
    }
    values.set_str("mime", Value::string(header.mime));
    values.set_str("width_unit", Value::string(header.width_unit));
    values.set_str("height_unit", Value::string(header.height_unit));
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

fn image_type_extension(kind: i64) -> Option<&'static str> {
    match kind {
        1 => Some("gif"),
        2 => Some("jpeg"),
        3 => Some("png"),
        4 | 13 => Some("swf"),
        5 => Some("psd"),
        6 | 15 => Some("bmp"),
        7 | 8 => Some("tiff"),
        9 => Some("jpc"),
        10 => Some("jp2"),
        11 => Some("jpx"),
        12 => Some("jb2"),
        14 => Some("iff"),
        16 => Some("xbm"),
        17 => Some("ico"),
        18 => Some("webp"),
        19 => Some("avif"),
        20 => Some("heif"),
        21 => Some("svg"),
        _ => None,
    }
}

fn image_type_to_extension(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(kind) =
        typed_internal_int_argument(ed, eg, "image_type_to_extension", 0, "image_type")?
    else {
        return Ok(());
    };
    let include_dot = if arg_opt!(ed, 1).is_some() {
        let Some(include_dot) =
            typed_internal_bool_argument(ed, eg, "image_type_to_extension", 1, "include_dot")?
        else {
            return Ok(());
        };
        include_dot
    } else {
        true
    };
    let Some(extension) = image_type_extension(kind) else {
        ret!(rv, Value::bool(false));
    };
    ret!(
        rv,
        Value::string(if include_dot {
            format!(".{extension}")
        } else {
            extension.to_string()
        })
    );
}

fn image_type_to_mime_type(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(kind) =
        typed_internal_int_argument(ed, eg, "image_type_to_mime_type", 0, "image_type")?
    else {
        return Ok(());
    };
    let mime = match kind {
        1 => "image/gif",
        2 => "image/jpeg",
        3 => "image/png",
        4 | 13 => "application/x-shockwave-flash",
        5 => "image/psd",
        6 => "image/bmp",
        7 | 8 => "image/tiff",
        10 => "image/jp2",
        14 => "image/iff",
        15 => "image/vnd.wap.wbmp",
        16 => "image/xbm",
        17 => "image/vnd.microsoft.icon",
        18 => "image/webp",
        19 => "image/avif",
        20 => "image/heif",
        21 => "image/svg+xml",
        _ => "application/octet-stream",
    };
    ret!(rv, Value::string(mime));
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

    let mut extension = Box::new(make_internal_function_ref(
        image_type_to_extension,
        2,
        1,
        0,
        vec!["image_type".into(), "include_dot".into()],
    ));
    extension.common.sig.param_type_hints = vec![ParamTypeHint::Int, ParamTypeHint::Bool];
    extension.common.sig.return_type_hint = ParamTypeHint::Union(vec![
        ParamTypeHint::String,
        ParamTypeHint::ClassName("false".into()),
    ]);
    extension.handler_validates_types = true;
    let pointer = &extension.common as *const FunctionCommon;
    eg.register_function("image_type_to_extension", pointer)
        .unwrap();
    eg.register_internal_function_reflection_metadata(
        pointer,
        vec![None, Some(Value::bool(true))],
        "standard",
    );
    functions.push(extension);

    let mut mime = Box::new(make_internal_function_ref(
        image_type_to_mime_type,
        1,
        1,
        0,
        vec!["image_type".into()],
    ));
    mime.common.sig.param_type_hints = vec![ParamTypeHint::Int];
    mime.common.sig.return_type_hint = ParamTypeHint::String;
    mime.handler_validates_types = true;
    let pointer = &mime.common as *const FunctionCommon;
    eg.register_function("image_type_to_mime_type", pointer)
        .unwrap();
    eg.register_internal_function_reflection_metadata(pointer, vec![None], "standard");
    functions.push(mime);
}
