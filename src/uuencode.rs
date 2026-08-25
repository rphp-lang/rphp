const INPUT_BYTES_PER_LINE: usize = 45;

pub fn encode(input: &[u8]) -> Vec<u8> {
    let line_count = input.len().div_ceil(INPUT_BYTES_PER_LINE);
    let mut output =
        Vec::with_capacity(input.len().saturating_mul(4).div_ceil(3) + line_count * 2 + 2);

    for line in input.chunks(INPUT_BYTES_PER_LINE) {
        output.push(six_bit_character(line.len() as u8));
        for group in line.chunks(3) {
            let first = group[0];
            let second = group.get(1).copied().unwrap_or(0);
            let third = group.get(2).copied().unwrap_or(0);
            output.push(six_bit_character(first >> 2));
            output.push(six_bit_character((first << 4 | second >> 4) & 0x3f));
            output.push(six_bit_character((second << 2 | third >> 6) & 0x3f));
            output.push(six_bit_character(third & 0x3f));
        }
        output.push(b'\n');
    }
    output.extend_from_slice(b"`\n");
    output
}

fn six_bit_character(value: u8) -> u8 {
    match value & 0x3f {
        0 => b'`',
        value => value + b' ',
    }
}

pub fn decode(input: &[u8]) -> Option<Vec<u8>> {
    if input.is_empty() {
        return None;
    }

    let mut output = Vec::with_capacity(input.len().saturating_mul(3) / 4);
    let mut position = 0_usize;
    loop {
        let line_length = usize::from(input[position].wrapping_sub(b' ') & 0x3f);
        position += 1;
        if line_length == 0 {
            return Some(output);
        }

        let encoded_length = line_length.div_ceil(3).checked_mul(4)?;
        let line_end = position.checked_add(encoded_length)?;
        let encoded = input.get(position..line_end)?;
        let mut remaining = line_length;
        for group in encoded.chunks_exact(4) {
            let first = group[0].wrapping_sub(b' ') & 0x3f;
            let second = group[1].wrapping_sub(b' ') & 0x3f;
            let third = group[2].wrapping_sub(b' ') & 0x3f;
            let fourth = group[3].wrapping_sub(b' ') & 0x3f;
            output.push(first << 2 | second >> 4);
            if remaining > 1 {
                output.push(second << 4 | third >> 2);
            }
            if remaining > 2 {
                output.push(third << 6 | fourth);
            }
            remaining = remaining.saturating_sub(3);
        }
        position = line_end;

        // PHP's decoder continues after a full or overlong data line. It
        // consumes exactly one delimiter byte; a short line is terminal and
        // any remaining bytes are ignored.
        if line_length < INPUT_BYTES_PER_LINE || position >= input.len() {
            return Some(output);
        }
        position += 1;
        if position >= input.len() {
            return Some(output);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    #[test]
    fn encode_uses_php_line_and_padding_contract() {
        assert_eq!(encode(b"Q"), b"!40``\n`\n");
        assert_eq!(encode(b"QR"), b"\"45(`\n`\n");
        assert_eq!(encode(b"QRS"), b"#45)3\n`\n");
        assert_eq!(encode(b"\0\x80\xff\r\n"), b"%`(#_#0H`\n`\n");
    }

    #[test]
    fn encode_chunks_at_45_bytes_and_roundtrips_binary_input() {
        let input = (0_u8..=255).collect::<Vec<_>>();
        let encoded = encode(&input);
        assert_eq!(encoded.iter().filter(|byte| **byte == b'\n').count(), 7);
        assert_eq!(decode(&encoded), Some(input));

        let boundary = vec![b'x'; 46];
        let encoded = encode(&boundary);
        assert_eq!(encoded[0], b'M');
        assert_eq!(encoded[61], b'\n');
        assert_eq!(encoded[62], b'!');
        assert_eq!(decode(&encoded), Some(boundary));
    }

    #[test]
    fn decode_matches_php_terminal_and_malformed_line_rules() {
        assert_eq!(decode(b"`"), Some(Vec::new()));
        assert_eq!(decode(b" \n"), Some(Vec::new()));
        assert_eq!(decode(b"!````\nignored"), Some(vec![0]));
        assert_eq!(decode(b"!\0```\n`\n"), Some(vec![0x80]));
        assert_eq!(decode(b"\"``"), None);
        assert_eq!(decode(b""), None);

        let mut full = b"M".to_vec();
        full.extend_from_slice(&[b'`'; 60]);
        full.push(b'\n');
        full.extend_from_slice(b"!````");
        assert_eq!(decode(&full), Some(vec![0; 46]));
    }
}
