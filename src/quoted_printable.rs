const HEX: &[u8; 16] = b"0123456789ABCDEF";
const MAX_CONTENT_COLUMNS: usize = 75;

pub fn encode(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut line_columns = 0_usize;
    let mut position = 0_usize;

    while position < input.len() {
        if input[position] == b'\r' && input.get(position + 1) == Some(&b'\n') {
            output.extend_from_slice(b"\r\n");
            line_columns = 0;
            position += 2;
            continue;
        }

        let byte = input[position];
        let encoded = must_encode(input, position);
        let projected_width = projected_character_width(byte, encoded);
        if projected_width != 0
            && line_columns.saturating_add(projected_width) > MAX_CONTENT_COLUMNS
        {
            output.extend_from_slice(b"=\r\n");
            line_columns = 0;
        }

        if encoded {
            output.push(b'=');
            output.push(HEX[usize::from(byte >> 4)]);
            output.push(HEX[usize::from(byte & 0x0f)]);
            line_columns = line_columns.saturating_add(3);
        } else {
            output.push(byte);
            line_columns = line_columns.saturating_add(1);
        }
        position += 1;
    }

    output
}

fn must_encode(input: &[u8], position: usize) -> bool {
    let byte = input[position];
    if byte == b' ' {
        return input.get(position + 1) == Some(&b'\r');
    }
    !matches!(byte, b'!'..=b'<' | b'>'..=b'~')
}

fn projected_character_width(byte: u8, encoded: bool) -> usize {
    if byte < 0x80 {
        return if encoded { 3 } else { 1 };
    }
    match byte {
        0x80..=0xdf => 6,
        0xe0..=0xef => 9,
        0xf0..=0xf4 => 12,
        _ => 0,
    }
}

pub fn decode(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut position = 0_usize;

    while position < input.len() {
        let byte = input[position];
        if byte == 0 {
            break;
        }
        if byte != b'=' {
            output.push(byte);
            position += 1;
            continue;
        }

        if let (Some(high), Some(low)) = (input.get(position + 1), input.get(position + 2))
            && let (Some(high), Some(low)) = (hex_value(*high), hex_value(*low))
        {
            output.push((high << 4) | low);
            position += 3;
            continue;
        }

        let mut after_whitespace = position + 1;
        while matches!(input.get(after_whitespace), Some(b' ' | b'\t')) {
            after_whitespace += 1;
        }
        match input.get(after_whitespace) {
            None | Some(0) => break,
            Some(b'\r') => {
                position = after_whitespace + 1;
                if input.get(position) == Some(&b'\n') {
                    position += 1;
                }
            }
            Some(b'\n') => position = after_whitespace + 1,
            Some(_) => {
                output.push(b'=');
                position += 1;
            }
        }
    }

    output
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    #[test]
    fn encode_preserves_php_printable_bytes_and_line_endings() {
        assert_eq!(encode(b"A =\t\r\nZ"), b"A =3D=09\r\nZ");
        assert_eq!(encode(b"A \r\nZ"), b"A=20\r\nZ");
        assert_eq!(encode(b"A \rZ"), b"A=20=0DZ");
        assert_eq!(encode(b"A\nZ\rQ"), b"A=0AZ=0DQ");
        assert_eq!(encode(b"\0\xff"), b"=00=FF");
    }

    #[test]
    fn encode_wraps_before_a_multibyte_character_boundary() {
        let mut input = b"prefix:".to_vec();
        input.extend_from_slice(&[b'A'; 65]);
        input.extend_from_slice(b"\xc4\x85Z");
        let mut expected = b"prefix:".to_vec();
        expected.extend_from_slice(&[b'A'; 65]);
        expected.extend_from_slice(b"=\r\n=C4=85Z");
        assert_eq!(encode(&input), expected);

        assert_eq!(
            encode(&[0; 26]),
            [b"=00".repeat(25), b"=\r\n=00".to_vec()].concat()
        );
    }

    #[test]
    fn decode_handles_escapes_soft_breaks_and_php_malformed_tails() {
        assert_eq!(decode(b"=00=7f=Af=ff"), b"\0\x7f\xaf\xff");
        assert_eq!(decode(b"a= \t\r\nb=\nc=\rd"), b"abcd");
        assert_eq!(decode(b"A=G==41=B"), b"A=G=A=B");
        assert_eq!(decode(b"before= \t"), b"before");
        assert_eq!(decode(b"A\0ignored"), b"A");
        assert_eq!(decode(b"A=\0ignored"), b"A");
    }
}
