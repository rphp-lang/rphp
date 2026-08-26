//! Clean-room logical-to-visual byte transformation for `hebrev()`.

use crate::runtime::ExecutorGlobals;
use crate::value::Value;
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

#[derive(Clone, Copy, Eq, PartialEq)]
enum Direction {
    Left,
    Right,
    Neutral,
    Previous,
}

#[inline]
fn byte_direction(byte: u8) -> Direction {
    match byte {
        0xe0..=0xfa | b'\r' => Direction::Right,
        b'-' | b'/' => Direction::Previous,
        b'\t' | b' '..=b'\'' | b'('..=b',' | b'.' | b':'..=b'@' | b'['..=b'`' | b'{'..=b'~' => {
            Direction::Neutral
        }
        _ => Direction::Left,
    }
}

#[inline]
fn mirrored_byte(byte: u8) -> u8 {
    match byte {
        b'(' => b')',
        b')' => b'(',
        b'/' => b'\\',
        b'<' => b'>',
        b'>' => b'<',
        b'[' => b']',
        b'\\' => b'/',
        b']' => b'[',
        b'{' => b'}',
        b'}' => b'{',
        byte => byte,
    }
}

fn visual_line(source: &[u8], source_offset: usize) -> Vec<u8> {
    let mut directions = source
        .iter()
        .copied()
        .map(byte_direction)
        .collect::<Vec<_>>();

    let mut previous = Direction::Left;
    for direction in &mut directions {
        match *direction {
            Direction::Previous => *direction = previous,
            Direction::Left | Direction::Right => previous = *direction,
            Direction::Neutral => {}
        }
    }

    let mut position = 0;
    while position < directions.len() {
        if directions[position] != Direction::Neutral {
            position += 1;
            continue;
        }
        let start = position;
        while position < directions.len() && directions[position] == Direction::Neutral {
            position += 1;
        }
        let left = start
            .checked_sub(1)
            .map_or(Direction::Left, |index| directions[index]);
        let right = directions
            .get(position)
            .copied()
            .unwrap_or(Direction::Right);
        let resolved = if left == Direction::Right || right == Direction::Right {
            Direction::Right
        } else {
            Direction::Left
        };
        directions[start..position].fill(resolved);
    }

    let mut blocks = Vec::new();
    let mut start = 0;
    while start < source.len() {
        let mut end = start + 1;
        while end < source.len() && directions[end] == directions[start] {
            end += 1;
        }
        let mut block = Vec::with_capacity(end - start);
        if directions[start] == Direction::Right {
            for index in (start..end).rev() {
                let byte = source[index];
                block.push(if source_offset + index == 0 {
                    byte
                } else {
                    mirrored_byte(byte)
                });
            }
        } else {
            block.extend_from_slice(&source[start..end]);
        }
        blocks.push(block);
        start = end;
    }
    let mut result = Vec::with_capacity(source.len());
    for block in blocks.into_iter().rev() {
        result.extend_from_slice(&block);
    }
    result
}

fn wrap_visual_line(line: &[u8], width: usize) -> Vec<u8> {
    if width == 0 || line.len() <= width {
        return line.to_vec();
    }

    let words = line
        .split(|byte| matches!(*byte, b' ' | b'\t'))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    if words.len() <= 1 {
        let chunk = width.saturating_add(1);
        let first = line.len() % chunk;
        let mut result = Vec::with_capacity(line.len());
        let mut end = line.len();
        while end > first.max(1) {
            let start = end.saturating_sub(chunk);
            result.extend_from_slice(&line[start..end]);
            end = start;
        }
        result.extend_from_slice(&line[..end]);
        return result;
    }

    let mut result = Vec::with_capacity(line.len().saturating_add(words.len()));
    let mut current: Vec<&[u8]> = Vec::new();
    let mut current_length = 0usize;
    for word in words.into_iter().rev() {
        let candidate = current_length
            .saturating_add(usize::from(!current.is_empty()))
            .saturating_add(word.len());
        if !current.is_empty() && candidate > width {
            for (index, part) in current.iter().rev().enumerate() {
                if index != 0 {
                    result.push(b' ');
                }
                result.extend_from_slice(part);
            }
            result.push(b'\n');
            current.clear();
            current_length = 0;
        }
        current_length = current_length
            .saturating_add(usize::from(!current.is_empty()))
            .saturating_add(word.len());
        current.push(word);
    }
    for (index, part) in current.iter().rev().enumerate() {
        if index != 0 {
            result.push(b' ');
        }
        result.extend_from_slice(part);
    }
    result
}

pub(super) fn hebrev_bytes(source: &[u8], max_chars_per_line: i64) -> Vec<u8> {
    let width = usize::try_from(max_chars_per_line).unwrap_or(0);
    let reverse_visual = max_chars_per_line < 0;
    let mut result = Vec::with_capacity(source.len());
    let mut start = 0;
    for position in 0..=source.len() {
        if position < source.len() && source[position] != b'\n' {
            continue;
        }
        let visual = visual_line(&source[start..position], start);
        let mut wrapped = visual;
        if position < source.len() && wrapped.first() == Some(&b'\r') {
            wrapped.remove(0);
            wrapped.push(b'\r');
        }
        if reverse_visual {
            wrapped.reverse();
            for byte in &mut wrapped {
                if matches!(*byte, b' ' | b'\t') {
                    *byte = b'\n';
                }
            }
        } else {
            wrapped = wrap_visual_line(&wrapped, width);
        }
        if !reverse_visual
            && position == source.len()
            && wrapped
                .first()
                .is_some_and(|byte| matches!(*byte, b' ' | b'\t' | b'\r'))
        {
            let leading = wrapped.remove(0);
            wrapped.push(if leading == b'\r' { b'\r' } else { b'\n' });
        }
        result.extend_from_slice(&wrapped);
        if position < source.len() {
            result.push(b'\n');
        }
        start = position + 1;
    }
    result
}

pub(super) fn fn_hebrev(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(string) = super::typed_internal_string_value_argument_expected(
        execute_data,
        eg,
        "hebrev",
        0,
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    let max_chars_per_line = if super::owned_argument(execute_data, 1).is_undef() {
        0
    } else {
        let Some(value) = super::typed_internal_int_argument(
            execute_data,
            eg,
            "hebrev",
            1,
            "max_chars_per_line",
        )?
        else {
            return Ok(());
        };
        value
    };
    let bytes = string.php_string_bytes().unwrap_or_default();
    let result = hebrev_bytes(&bytes, max_chars_per_line);
    super::write_return_value(
        return_pointer,
        super::php_byte_result(result, string.is_binary_string()),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::hebrev_bytes;

    #[test]
    fn reverses_directional_blocks_and_mirrors_punctuation() {
        assert_eq!(hebrev_bytes(b"AB\xe0\xe1CD", 0), b"CD\xe1\xe0AB");
        assert_eq!(hebrev_bytes(b"A.\xe0\xe1!B", 0), b"B!\xe1\xe0.A");
        assert_eq!(hebrev_bytes(b"\xe0(\xe1", 0), b"\xe1)\xe0");
        assert_eq!(hebrev_bytes(b"\xe0/A", 0), b"A\\\xe0");
    }

    #[test]
    fn keeps_non_hebrew_bytes_and_wraps_from_the_visual_right_edge() {
        assert_eq!(hebrev_bytes(b"abc", 0), b"abc");
        assert_eq!(hebrev_bytes(b"abc.", 0), b".abc");
        assert_eq!(hebrev_bytes(b"a b", 2), b"b\na");
        assert_eq!(hebrev_bytes(b"abcdefgh", 3), b"efghabcd");
        assert_eq!(hebrev_bytes(b"abc.", -1), b"cba.");
        assert_eq!(hebrev_bytes(b"a b", -5), b"b\na");
    }

    #[test]
    fn treats_utf8_and_binary_payloads_as_legacy_bytes() {
        assert_eq!(hebrev_bytes("שלום".as_bytes(), 0), "שלום".as_bytes());
        assert_eq!(hebrev_bytes(b"\0A\xffB", 0), b"\0A\xffB");
        assert_eq!(hebrev_bytes(b"A\r\nB", 0), b"A\r\nB");
    }
}
