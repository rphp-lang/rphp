pub fn count_chars(input: &[u8]) -> [u64; 256] {
    let mut counts = [0u64; 256];
    for byte in input {
        counts[usize::from(*byte)] += 1;
    }
    counts
}

pub fn quotemeta(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    for byte in input {
        if matches!(
            byte,
            b'.' | b'\\' | b'+' | b'*' | b'?' | b'[' | b'^' | b']' | b'(' | b'$' | b')'
        ) {
            output.push(b'\\');
        }
        output.push(*byte);
    }
    output
}

pub fn str_rot13(input: &[u8]) -> Vec<u8> {
    input
        .iter()
        .map(|byte| match byte {
            b'a'..=b'm' | b'A'..=b'M' => byte + 13,
            b'n'..=b'z' | b'N'..=b'Z' => byte - 13,
            _ => *byte,
        })
        .collect()
}

fn ascii_upper(byte: u8) -> u8 {
    byte.to_ascii_uppercase()
}

fn soundex_code(byte: u8) -> Option<u8> {
    match ascii_upper(byte) {
        b'B' | b'F' | b'P' | b'V' => Some(b'1'),
        b'C' | b'G' | b'J' | b'K' | b'Q' | b'S' | b'X' | b'Z' => Some(b'2'),
        b'D' | b'T' => Some(b'3'),
        b'L' => Some(b'4'),
        b'M' | b'N' => Some(b'5'),
        b'R' => Some(b'6'),
        _ => None,
    }
}

pub fn soundex(input: &[u8]) -> [u8; 4] {
    let Some((first_position, first)) = input
        .iter()
        .copied()
        .enumerate()
        .find(|(_, byte)| byte.is_ascii_alphabetic())
    else {
        return *b"0000";
    };

    let mut output = [b'0'; 4];
    output[0] = ascii_upper(first);
    let mut output_length = 1usize;
    let mut previous = soundex_code(first);
    for byte in &input[first_position + 1..] {
        let upper = ascii_upper(*byte);
        if let Some(code) = soundex_code(upper) {
            if previous != Some(code) {
                output[output_length] = code;
                output_length += 1;
                if output_length == output.len() {
                    break;
                }
            }
            previous = Some(code);
        } else if upper.is_ascii_alphabetic() {
            previous = None;
        }
    }
    output
}

fn is_vowel(byte: u8) -> bool {
    matches!(byte, b'A' | b'E' | b'I' | b'O' | b'U')
}

fn is_front_vowel(byte: u8) -> bool {
    matches!(byte, b'E' | b'I' | b'Y')
}

fn is_letter(byte: u8) -> bool {
    byte.is_ascii_uppercase()
}

fn metaphone_push(output: &mut Vec<u8>, byte: u8, phonemes: &mut usize, limit: usize) {
    if *phonemes < limit {
        output.push(byte);
        *phonemes += 1;
    }
}

pub fn metaphone(input: &[u8], max_phonemes: usize) -> Vec<u8> {
    let input = input.split(|byte| *byte == 0).next().unwrap_or_default();
    let letters: Vec<u8> = input.iter().map(|byte| ascii_upper(*byte)).collect();
    let limit = if max_phonemes == 0 {
        usize::MAX
    } else {
        max_phonemes
    };
    let mut output = Vec::with_capacity(letters.len().min(limit));
    let mut phonemes = 0usize;
    let Some(mut position) = letters.iter().position(|byte| is_letter(*byte)) else {
        return output;
    };

    let current = letters[position];
    let next = letters.get(position + 1).copied().unwrap_or(0);
    match (current, next) {
        (b'A', b'E') => {
            metaphone_push(&mut output, b'E', &mut phonemes, limit);
            position += 2;
        }
        (b'G' | b'K' | b'P', b'N') => {
            metaphone_push(&mut output, b'N', &mut phonemes, limit);
            position += 2;
        }
        (b'W', b'R') => {
            metaphone_push(&mut output, b'R', &mut phonemes, limit);
            position += 2;
        }
        (b'W', b'H') => {
            metaphone_push(&mut output, b'W', &mut phonemes, limit);
            position += 2;
        }
        (b'X', _) => {
            metaphone_push(&mut output, b'S', &mut phonemes, limit);
            position += 1;
        }
        (byte, _) if is_vowel(byte) => {
            metaphone_push(&mut output, byte, &mut phonemes, limit);
            position += 1;
        }
        _ => {}
    }

    while position < letters.len() && phonemes < limit {
        let current = letters[position];
        if !is_letter(current) {
            position += 1;
            continue;
        }
        let previous = position
            .checked_sub(1)
            .and_then(|index| letters.get(index))
            .copied()
            .unwrap_or(0);
        let next = letters.get(position + 1).copied().unwrap_or(0);
        let after_next = letters.get(position + 2).copied().unwrap_or(0);

        if current == previous
            && current != b'C'
            && !(current == b'D' && next == b'G' && is_front_vowel(after_next))
        {
            position += 1;
            continue;
        }

        match current {
            b'B' => {
                if previous != b'M' {
                    metaphone_push(&mut output, b'B', &mut phonemes, limit);
                }
            }
            b'C' => {
                if next == b'I' && after_next == b'A' {
                    metaphone_push(&mut output, b'X', &mut phonemes, limit);
                    position += 2;
                } else if previous == b'S' && is_front_vowel(next) {
                } else if is_front_vowel(next) {
                    metaphone_push(&mut output, b'S', &mut phonemes, limit);
                } else if next == b'H' {
                    metaphone_push(&mut output, b'X', &mut phonemes, limit);
                    position += 1;
                } else {
                    metaphone_push(&mut output, b'K', &mut phonemes, limit);
                }
            }
            b'D' => {
                if next == b'G' && is_front_vowel(after_next) {
                    metaphone_push(&mut output, b'J', &mut phonemes, limit);
                } else {
                    metaphone_push(&mut output, b'T', &mut phonemes, limit);
                }
            }
            b'F' | b'J' | b'L' | b'M' | b'N' | b'R' => {
                metaphone_push(&mut output, current, &mut phonemes, limit);
            }
            b'G' => {
                if next == b'H' {
                    metaphone_push(&mut output, b'F', &mut phonemes, limit);
                    position += 1;
                } else if next == b'N'
                    && (position + 2 == letters.len()
                        || (after_next == b'E'
                            && letters.get(position + 3).copied() == Some(b'D')
                            && position + 4 == letters.len()))
                {
                } else if previous == b'D' && is_front_vowel(next) {
                } else if is_front_vowel(next) {
                    metaphone_push(&mut output, b'J', &mut phonemes, limit);
                } else {
                    metaphone_push(&mut output, b'K', &mut phonemes, limit);
                }
            }
            b'H' => {
                if !matches!(previous, b'C' | b'S' | b'P' | b'T' | b'G') && is_vowel(next) {
                    metaphone_push(&mut output, b'H', &mut phonemes, limit);
                }
            }
            b'K' => {
                if previous != b'C' {
                    metaphone_push(&mut output, b'K', &mut phonemes, limit);
                }
            }
            b'P' => metaphone_push(
                &mut output,
                if next == b'H' { b'F' } else { b'P' },
                &mut phonemes,
                limit,
            ),
            b'Q' => metaphone_push(&mut output, b'K', &mut phonemes, limit),
            b'S' => {
                if next == b'H' || (next == b'I' && matches!(after_next, b'O' | b'A')) {
                    metaphone_push(&mut output, b'X', &mut phonemes, limit);
                } else {
                    metaphone_push(&mut output, b'S', &mut phonemes, limit);
                }
            }
            b'T' => {
                if next == b'I' && matches!(after_next, b'O' | b'A') {
                    metaphone_push(&mut output, b'X', &mut phonemes, limit);
                } else if next == b'H' {
                    metaphone_push(&mut output, b'0', &mut phonemes, limit);
                    position += 1;
                } else if !(next == b'C' && after_next == b'H') {
                    metaphone_push(&mut output, b'T', &mut phonemes, limit);
                }
            }
            b'V' => metaphone_push(&mut output, b'F', &mut phonemes, limit),
            b'W' | b'Y' => {
                if is_vowel(next) {
                    metaphone_push(&mut output, current, &mut phonemes, limit);
                }
            }
            b'X' => {
                if phonemes < limit {
                    output.extend_from_slice(b"KS");
                    phonemes += 2;
                }
            }
            b'Z' => metaphone_push(&mut output, b'S', &mut phonemes, limit),
            _ => {}
        }
        position += 1;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{count_chars, metaphone, quotemeta, soundex, str_rot13};

    #[test]
    fn byte_transforms_preserve_non_ascii_and_escape_only_metacharacters() {
        assert_eq!(quotemeta(b"A.\\+?\0\xff"), b"A\\.\\\\\\+\\?\0\xff");
        assert_eq!(str_rot13(b"Az-Nm\0\xff"), b"Nm-Az\0\xff");
        let counts = count_chars(b"A\0A\xff");
        assert_eq!((counts[0], counts[65], counts[255]), (1, 2, 1));
    }

    #[test]
    fn soundex_matches_php_examples_and_ignores_non_letters() {
        assert_eq!(soundex(b"Robert"), *b"R163");
        assert_eq!(soundex(b"Rupert"), *b"R163");
        assert_eq!(soundex(b"Ashcraft"), *b"A226");
        assert_eq!(soundex(b"\0-123"), *b"0000");
    }

    #[test]
    fn metaphone_handles_initial_and_contextual_rules_with_limits() {
        assert_eq!(metaphone(b"knight", 0), b"NFT");
        assert_eq!(metaphone(b"xylophone", 0), b"SLFN");
        assert_eq!(metaphone(b"thistle", 2), b"0S");
        assert_eq!(metaphone(b"A\0Robert", 0), b"A");
    }

    #[test]
    fn metaphone_matches_php_edge_ordering_and_atomic_x_limits() {
        assert_eq!(metaphone(b"AMBER", 0), b"AMR");
        assert_eq!(metaphone(b"CYA", 0), b"SY");
        assert_eq!(metaphone(b"SCIA", 0), b"SX");
        assert_eq!(metaphone(b"DDGE", 0), b"TJ");
        assert_eq!(metaphone(b"DGYA", 0), b"JY");
        assert_eq!(metaphone(b"ghost", 0), b"FST");
        assert_eq!(metaphone(b"laugh", 0), b"LF");
        assert_eq!(metaphone(b"AXA", 2), b"AKS");
    }
}
