//! Clean-room HTML metadata extraction for `get_meta_tags()`.

use std::io;

use crate::runtime::ExecutorGlobals;
use crate::value::{ArrayKey, PhpArray, Value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::stream::PhpStream;

#[inline]
fn ascii_eq_ignore_case(left: &[u8], right: &[u8]) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[inline]
fn is_html_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

fn find_ascii_case_insensitive(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from > haystack.len().saturating_sub(needle.len()) {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| ascii_eq_ignore_case(window, needle))
        .map(|position| position + from)
}

fn tag_name_ends(bytes: &[u8], position: usize) -> bool {
    bytes
        .get(position)
        .is_none_or(|byte| is_html_space(*byte) || matches!(*byte, b'>' | b'/'))
}

fn next_tag(bytes: &[u8], needle: &[u8], mut from: usize) -> Option<usize> {
    while let Some(position) = find_ascii_case_insensitive(bytes, needle, from) {
        if tag_name_ends(bytes, position + needle.len()) {
            return Some(position);
        }
        from = position + 1;
    }
    None
}

fn normalize_meta_name(bytes: &[u8]) -> String {
    let normalized = bytes
        .iter()
        .map(|byte| match byte {
            b'A'..=b'Z' => *byte + (b'a' - b'A'),
            b' ' | b'$' | b'(' | b')' | b'*' | b'+' | b'.' | b'?' | b'[' | b'\\' | b']' | b'^' => {
                b'_'
            }
            byte => *byte,
        })
        .collect::<Vec<_>>();
    super::bytes_to_php_string(&normalized)
}

fn next_attribute(tag: &[u8], position: &mut usize) -> Option<(Vec<u8>, Vec<u8>, bool)> {
    while let Some(byte) = tag.get(*position) {
        if is_html_space(*byte) || *byte == b'<' {
            *position += 1;
        } else if *byte == b'=' {
            *position += 1;
            while tag.get(*position).is_some_and(|byte| !is_html_space(*byte)) {
                *position += 1;
            }
        } else {
            break;
        }
    }
    if *position >= tag.len() {
        return None;
    }

    let name_start = *position;
    while tag
        .get(*position)
        .is_some_and(|byte| !is_html_space(*byte) && !matches!(*byte, b'=' | b'<'))
    {
        *position += 1;
    }
    let name = tag[name_start..*position].to_vec();
    while tag.get(*position).is_some_and(|byte| is_html_space(*byte)) {
        *position += 1;
    }

    if tag.get(*position) != Some(&b'=') {
        return Some((name, Vec::new(), false));
    }
    *position += 1;
    let Some(first) = tag.get(*position).copied() else {
        return Some((name, Vec::new(), false));
    };
    if is_html_space(first) {
        return Some((name, Vec::new(), false));
    }

    let quoted = matches!(first, b'\'' | b'"');
    let quote = quoted.then_some(first);
    if quoted {
        *position += 1;
    }
    let value_start = *position;
    while let Some(byte) = tag.get(*position) {
        if quote.is_some_and(|quote| *byte == quote)
            || (quote.is_none() && (is_html_space(*byte) || *byte == b'='))
        {
            break;
        }
        *position += 1;
    }
    let mut value_end = *position;
    if quote.is_some() && *position < tag.len() {
        *position += 1;
    }
    if quote.is_none()
        && value_end == tag.len()
        && tag.get(value_end.wrapping_sub(1)) == Some(&b'/')
    {
        value_end -= 1;
    }
    Some((name, tag[value_start..value_end].to_vec(), true))
}

fn parse_meta_tag(tag: &[u8]) -> Option<(String, Vec<u8>)> {
    let mut position = 0;
    let mut name = None;
    let mut content = None;
    while let Some((attribute, value, has_value)) = next_attribute(tag, &mut position) {
        if has_value && ascii_eq_ignore_case(&attribute, b"name") {
            name = Some(value);
        } else if ascii_eq_ignore_case(&attribute, b"content") {
            content = Some(value);
        }
    }
    name.map(|name| (normalize_meta_name(&name), content.unwrap_or_default()))
}

fn set_meta_value(result: &mut PhpArray, name: String, content: Vec<u8>) {
    let value = super::php_byte_result(content, false);
    let key = crate::value::canonical_decimal_array_key(&name)
        .map_or_else(|| ArrayKey::String(name), ArrayKey::Int);
    result.set(key, value);
}

pub(super) fn parse_meta_tags(bytes: &[u8]) -> PhpArray {
    let mut result = PhpArray::new();
    let mut position = 0;
    loop {
        let meta = next_tag(bytes, b"<meta", position);
        let head_end = next_tag(bytes, b"</head", position);
        if head_end.is_some_and(|head_end| meta.is_none_or(|meta| head_end < meta)) {
            break;
        }
        let Some(meta) = meta else {
            break;
        };
        let body_start = meta + b"<meta".len();
        let Some(relative_end) = bytes[body_start..].iter().position(|byte| *byte == b'>') else {
            break;
        };
        let tag_end = body_start + relative_end;
        if let Some((name, content)) = parse_meta_tag(&bytes[body_start..tag_end]) {
            set_meta_value(&mut result, name, content);
        }
        // Advance only one byte past the opening marker. Malformed input may
        // contain a second `<meta` before this tag's `>`; PHP recovers that
        // nested marker rather than discarding it with the outer fragment.
        position = meta + 1;
    }
    result
}

fn local_file_uri_path(filename: &str) -> Option<&str> {
    let prefix = filename.get(..7)?;
    if !prefix.eq_ignore_ascii_case("file://") {
        return None;
    }
    let remainder = filename.get(7..)?;
    if remainder.starts_with('/') {
        return Some(remainder);
    }
    let authority = remainder.get(..9)?;
    let path = remainder.get(9..)?;
    (authority.eq_ignore_ascii_case("localhost") && path.starts_with('/')).then_some(path)
}

fn open_stream(filename: &str) -> io::Result<PhpStream> {
    if filename
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
    {
        return local_file_uri_path(filename)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "remote host file access not supported",
                )
            })
            .and_then(|path| PhpStream::open(path, "r"));
    }
    PhpStream::open(filename, "r")
}

fn read_stream(mut stream: PhpStream) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8 * 1024];
    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

#[cold]
pub(super) fn fn_get_meta_tags(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(filename) =
        super::typed_internal_string_argument(execute_data, eg, "get_meta_tags", 0, "filename")?
    else {
        return Ok(());
    };
    let use_include_path = if !super::owned_argument(execute_data, 1).is_undef() {
        let Some(value) = super::typed_internal_bool_argument(
            execute_data,
            eg,
            "get_meta_tags",
            1,
            "use_include_path",
        )?
        else {
            return Ok(());
        };
        value
    } else {
        false
    };

    if filename.is_empty() {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "Path must not be empty",
        ));
        return Ok(());
    }
    if filename.contains('\0') {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "get_meta_tags(): Argument #1 ($filename) must not contain any null bytes",
        ));
        return Ok(());
    }

    #[cfg(feature = "include-path")]
    let resolved = super::include_path::resolve_for_open(eg, &filename, use_include_path);
    #[cfg(not(feature = "include-path"))]
    let resolved = {
        let _ = use_include_path;
        filename.clone()
    };

    let bytes = match open_stream(&resolved) {
        Ok(stream) => {
            if stream.metadata().wrapper_type == "plainfile" {
                super::filesystem::clear_filesystem_stat_cache(eg);
            }
            read_stream(stream)
        }
        Err(error) => Err(error),
    };
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => {
            super::report_internal_diagnostic(
                eg,
                execute_data,
                2,
                "Warning",
                &format!(
                    "get_meta_tags({filename}): Failed to open stream: {}",
                    super::digest_file_error_reason(&error)
                ),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            super::write_return_value(return_pointer, Value::bool(false));
            return Ok(());
        }
    };
    super::write_return_value(return_pointer, Value::array(parse_meta_tags(&bytes)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_meta_tags;
    use crate::value::Value;

    fn text(array: &crate::value::PhpArray, key: &str) -> Option<String> {
        array
            .get_str(key)
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    #[test]
    fn extracts_normalized_names_and_stops_at_head_end() {
        let array = parse_meta_tags(
            br#"<META CONTENT="first" NAME="X.Y"><meta name=a content=one>
                </HEAD ><meta name=late content=no>"#,
        );
        assert_eq!(text(&array, "x_y").as_deref(), Some("first"));
        assert_eq!(text(&array, "a").as_deref(), Some("one"));
        assert!(array.get_str("late").is_none());
    }

    #[test]
    fn recovers_nested_markers_and_overwrites_duplicate_names() {
        let array = parse_meta_tags(
            br#"<meta name="ignored" content="first"
                <meta name="Key" content="one"><meta name=key content=two/>"#,
        );
        assert_eq!(array.len(), 1);
        assert_eq!(text(&array, "key").as_deref(), Some("two"));
    }

    #[test]
    fn keeps_php_attribute_token_boundaries() {
        let array = parse_meta_tags(
            br#"<meta name =x content=y><meta name=z content= spaced>
                <meta name="a b" content="c d"><metadata name=bad content=no>"#,
        );
        assert_eq!(text(&array, "x").as_deref(), Some("y"));
        assert_eq!(text(&array, "z").as_deref(), Some(""));
        assert_eq!(text(&array, "a_b").as_deref(), Some("c d"));
        assert!(array.get_str("bad").is_none());
        assert!(parse_meta_tags(b"<meta name= spaced content=x>").is_empty());
    }
}
