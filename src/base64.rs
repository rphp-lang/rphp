/// Minimal base64 encode/decode — no external dependency.

const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub fn decode(input: &[u8], strict: bool) -> Option<Vec<u8>> {
    if !strict {
        return Some(decode_loose(input));
    }
    decode_strict(input)
}

fn decode_loose(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for byte in input {
        let val = match *byte {
            b'A'..=b'Z' => *byte - b'A',
            b'a'..=b'z' => *byte - b'a' + 26,
            b'0'..=b'9' => *byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        buf = (buf << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    out
}

fn decode_strict(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    let mut digits = 0_usize;
    let mut padding = 0_usize;
    let mut saw_padding = false;

    for byte in input {
        let val = match *byte {
            b'A'..=b'Z' => *byte - b'A',
            b'a'..=b'z' => *byte - b'a' + 26,
            b'0'..=b'9' => *byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'\n' | b'\r' | b' ' | b'\t' => continue,
            b'=' => {
                saw_padding = true;
                padding += 1;
                continue;
            }
            _ => return None,
        };
        if saw_padding {
            return None;
        }
        digits += 1;
        buf = (buf << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }

    let remainder = digits % 4;
    if remainder == 1 {
        return None;
    }
    if padding != 0 {
        let expected = match remainder {
            2 => 2,
            3 => 1,
            _ => return None,
        };
        if padding != expected {
            return None;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn loose_decode_ignores_every_non_alphabet_byte() {
        assert_eq!(
            decode(b"!Y\0\xffW\x80Jj==QQ", false),
            Some(b"abcA".to_vec())
        );
        assert_eq!(decode(b"!\0\xff", false), Some(Vec::new()));
        assert_eq!(decode(b"A", false), Some(Vec::new()));
    }

    #[test]
    fn strict_decode_validates_padding_and_php_whitespace() {
        assert_eq!(decode(b" YQ =\t=\r\n", true), Some(b"a".to_vec()));
        assert_eq!(decode(b"YQ", true), Some(b"a".to_vec()));
        assert_eq!(decode(b"YWI", true), Some(b"ab".to_vec()));
        assert_eq!(decode(b"A", true), None);
        assert_eq!(decode(b"YQ=", true), None);
        assert_eq!(decode(b"YQ===", true), None);
        assert_eq!(decode(b"YQ==A", true), None);
        assert_eq!(decode(b"YQ\x0b==", true), None);
    }
}
