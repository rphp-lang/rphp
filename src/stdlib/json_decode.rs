//! Streaming JSON decode directly into canonical RPHP values.
//!
//! `serde_json::Value` is intentionally not an intermediate representation:
//! it would allocate a second recursive tree (including a BTreeMap for every
//! object) and then immediately walk and destroy that tree while constructing
//! the PHP result.

use std::borrow::Cow;
use std::cell::Cell;
use std::fmt;

use serde::de::{DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::value::{DynamicPropertyMap, PhpArray, PhpObject, Value, canonical_decimal_array_key};

#[derive(Clone, Copy)]
struct PhpValueSeed<'plan> {
    associative: bool,
    maximum_depth: u32,
    container_depth: u32,
    parser_stack_cost: u32,
    number_plan: Option<&'plan NumberPlan>,
    negative_zero_seen: Option<&'plan Cell<bool>>,
}

impl<'plan> PhpValueSeed<'plan> {
    #[inline(always)]
    const fn new(
        associative: bool,
        maximum_depth: u32,
        number_plan: Option<&'plan NumberPlan>,
        negative_zero_seen: Option<&'plan Cell<bool>>,
    ) -> Self {
        Self {
            associative,
            maximum_depth,
            container_depth: 0,
            parser_stack_cost: 0,
            number_plan,
            negative_zero_seen,
        }
    }

    #[inline(always)]
    fn planned_number<E>(self) -> Result<Option<Value>, E>
    where
        E: serde::de::Error,
    {
        self.number_plan.map(NumberPlan::next).transpose()
    }

    #[inline]
    fn enter_container<E>(self, parser_cost: u32) -> Result<Self, E>
    where
        E: serde::de::Error,
    {
        if self.container_depth.saturating_add(1) >= self.maximum_depth {
            return Err(E::custom(DEPTH_ERROR_MARKER));
        }
        let parser_stack_cost = self.parser_stack_cost.saturating_add(parser_cost);
        // PHP 8.5's JSON parser accepts 4,998 nested arrays, or 2,499
        // nested objects (an object consumes two parser stack entries), and
        // reports JSON_ERROR_SYNTAX before the native stack can overflow.
        if parser_stack_cost > 4_998 {
            return Err(E::custom(PARSER_STACK_ERROR_MARKER));
        }
        Ok(Self {
            associative: self.associative,
            maximum_depth: self.maximum_depth,
            container_depth: self.container_depth.saturating_add(1),
            parser_stack_cost,
            number_plan: self.number_plan,
            negative_zero_seen: self.negative_zero_seen,
        })
    }
}

impl<'de> DeserializeSeed<'de> for PhpValueSeed<'_> {
    type Value = Value;

    #[inline]
    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(PhpValueVisitor { seed: self })
    }
}

struct PhpValueVisitor<'plan> {
    seed: PhpValueSeed<'plan>,
}

#[inline(always)]
const fn is_deep_drop_checkpoint(depth: u32) -> bool {
    depth >= 256 && (depth - 256).is_multiple_of(512)
}

impl<'de> Visitor<'de> for PhpValueVisitor<'_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a valid JSON value")
    }

    #[inline]
    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Value::null())
    }

    #[inline]
    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Value::null())
    }

    #[inline]
    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Value::bool(value))
    }

    #[inline]
    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(self
            .seed
            .planned_number()?
            .unwrap_or_else(|| Value::long(value)))
    }

    #[inline]
    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(self
            .seed
            .planned_number()?
            .unwrap_or_else(|| match i64::try_from(value) {
                Ok(value) => Value::long(value),
                Err(_) => Value::double(value as f64),
            }))
    }

    #[inline]
    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if value == 0.0
            && value.is_sign_negative()
            && let Some(seen) = self.seed.negative_zero_seen
        {
            seen.set(true);
        }
        Ok(self
            .seed
            .planned_number()?
            .unwrap_or_else(|| Value::double(value)))
    }

    #[inline]
    fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E> {
        Ok(Value::string(value))
    }

    #[inline]
    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Value::string(value))
    }

    #[inline]
    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(Value::string(value))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let child = self.seed.enter_container::<A::Error>(1)?;
        let mut array = PhpArray::with_packed_capacity(sequence.size_hint().unwrap_or(0));
        while let Some(value) = sequence.next_element_seed(child)? {
            array.push(value);
        }
        let value = Value::array(array);
        if is_deep_drop_checkpoint(child.container_depth) {
            value.mark_deep_drop_stack_checkpoint();
        }
        Ok(value)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let child = self.seed.enter_container::<A::Error>(2)?;
        if self.seed.associative {
            let mut array = PhpArray::with_deferred_hash_capacity(map.size_hint().unwrap_or(0));
            while let Some(key) = map.next_key::<String>()? {
                let value = map.next_value_seed(child)?;
                if let Some(key) = canonical_decimal_array_key(&key) {
                    array.set_streamed_int(key, value);
                } else {
                    array.set_streamed_owned_str(key, value);
                }
            }
            let value = Value::array(array);
            if is_deep_drop_checkpoint(child.container_depth) {
                value.mark_deep_drop_stack_checkpoint();
            }
            Ok(value)
        } else {
            decode_object_map(map, child)
        }
    }
}

/// Keep object materialization out of the shared generic map-dispatch body.
/// Its storage policy may evolve independently without perturbing the hotter
/// associative-array branch's code layout and inlining decisions.
#[inline(never)]
fn decode_object_map<'de, A>(mut map: A, child: PhpValueSeed<'_>) -> Result<Value, A::Error>
where
    A: MapAccess<'de>,
{
    let mut properties = DynamicPropertyMap::with_capacity(map.size_hint().unwrap_or(0));
    while let Some(key) = map.next_key::<String>()? {
        // Zend decodes the member value before it attempts to install the
        // property. A malformed/depth-limited value therefore wins over the
        // later NUL-property-name diagnostic.
        let value = map.next_value_seed(child)?;
        if key.as_bytes().first() == Some(&0) {
            return Err(serde::de::Error::custom(INVALID_PROPERTY_ERROR_MARKER));
        }
        properties.insert_owned(key, value);
    }
    let value = Value::object(PhpObject::std_class_from_properties(properties));
    if is_deep_drop_checkpoint(child.container_depth) {
        value.mark_deep_drop_stack_checkpoint();
    }
    Ok(value)
}

const DEPTH_ERROR_MARKER: &str = "__rphp_json_depth__";
const PARSER_STACK_ERROR_MARKER: &str = "__rphp_json_parser_stack__";
const INVALID_PROPERTY_ERROR_MARKER: &str = "__rphp_json_invalid_property__";
const NUMBER_PLAN_ERROR_MARKER: &str = "__rphp_json_number_plan__";

const JSON_ERROR_DEPTH: i64 = 1;
const JSON_ERROR_STATE_MISMATCH: i64 = 2;
const JSON_ERROR_CONTROL_CHARACTER: i64 = 3;
const JSON_ERROR_SYNTAX: i64 = 4;
const JSON_ERROR_UTF8: i64 = 5;
const JSON_ERROR_INVALID_PROPERTY_NAME: i64 = 9;
const JSON_ERROR_UTF16: i64 = 10;

pub(super) const JSON_OBJECT_AS_ARRAY_FLAG: i64 = 1;
pub(super) const JSON_BIGINT_AS_STRING_FLAG: i64 = 2;
pub(super) const JSON_INVALID_UTF8_IGNORE_FLAG: i64 = 1_048_576;
pub(super) const JSON_INVALID_UTF8_SUBSTITUTE_FLAG: i64 = 2_097_152;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PhpJsonDecodeError {
    code: i64,
    incomplete: bool,
}

impl PhpJsonDecodeError {
    #[inline(always)]
    pub(super) const fn code(self) -> i64 {
        self.code
    }

    #[inline(always)]
    const fn new(code: i64) -> Self {
        Self {
            code,
            incomplete: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InvalidUtf8Policy {
    Reject,
    Ignore,
    Substitute,
}

#[derive(Default)]
struct JsonStringState {
    in_string: bool,
    escaped: bool,
    unicode_digits_remaining: u8,
}

impl JsonStringState {
    #[inline]
    fn update(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if !self.in_string {
                if byte == b'"' {
                    self.in_string = true;
                }
                continue;
            }
            if self.unicode_digits_remaining != 0 {
                self.unicode_digits_remaining -= 1;
                continue;
            }
            if self.escaped {
                self.escaped = false;
                if byte == b'u' {
                    self.unicode_digits_remaining = 4;
                }
            } else if byte == b'\\' {
                self.escaped = true;
            } else if byte == b'"' {
                self.in_string = false;
            }
        }
    }

    #[inline(always)]
    const fn invalid_byte_breaks_escape(&self) -> bool {
        self.escaped || self.unicode_digits_remaining != 0
    }
}

struct PreparedJsonInput<'input> {
    text: Cow<'input, str>,
    invalid_error: Option<PhpJsonDecodeError>,
}

/// A parser EOF normally means that the invalid byte interrupted a still-valid
/// container or string and therefore wins. PHP's lexer has two exceptions:
/// partial keywords and partial number lexemes are already syntax errors.
fn prefix_ends_incomplete_scalar_token(input: &str) -> bool {
    let input = input.trim_end_matches([' ', '\t', '\r', '\n']);
    let mut string_state = JsonStringState::default();
    string_state.update(input.as_bytes());
    if string_state.in_string {
        return false;
    }

    let bytes = input.as_bytes();
    let mut start = bytes.len();
    while start != 0
        && !matches!(
            bytes[start - 1],
            b' ' | b'\t' | b'\r' | b'\n' | b'[' | b']' | b'{' | b'}' | b',' | b':'
        )
    {
        start -= 1;
    }
    let token = &input[start..];
    if matches!(
        token,
        "t" | "tr" | "tru" | "f" | "fa" | "fal" | "fals" | "n" | "nu" | "nul"
    ) {
        return true;
    }
    let bytes = token.as_bytes();
    if bytes.is_empty() || (!bytes[0].is_ascii_digit() && bytes[0] != b'-') {
        return false;
    }
    let mut index = usize::from(bytes[0] == b'-');
    if index == bytes.len() {
        return true;
    }
    if bytes[index] == b'0' {
        index += 1;
    } else if bytes[index].is_ascii_digit() {
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
    } else {
        return false;
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let digits = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == digits {
            return true;
        }
    }
    if bytes
        .get(index)
        .is_some_and(|byte| matches!(byte, b'e' | b'E'))
    {
        index += 1;
        if bytes
            .get(index)
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
        {
            index += 1;
        }
        let digits = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == digits {
            return true;
        }
    }
    false
}

/// Return a PHP-visible parse or semantic error that occurs before an invalid
/// UTF-8 byte. This uses the same projection, depth and BIGINT path as the
/// final decode; unlike the old per-byte `IgnoredAny` probe it runs at most
/// once and does not misclassify valid overflow numbers.
fn prior_json_error_code(
    input: &str,
    associative: bool,
    maximum_depth: u32,
    bigint_as_string: bool,
) -> Option<i64> {
    let error = decode_php_text(input, associative, maximum_depth, bigint_as_string).err()?;
    (error.code == JSON_ERROR_UTF16
        || !error.incomplete
        || prefix_ends_incomplete_scalar_token(input))
    .then_some(error.code)
}

/// Repair invalid UTF-8 only while the offending bytes are inside a JSON
/// string. PHP's ignore/substitute flags do not turn invalid structural bytes
/// into a valid document.
fn json_input_with_utf8_policy(input: &[u8], policy: InvalidUtf8Policy) -> PreparedJsonInput<'_> {
    if let Ok(input) = std::str::from_utf8(input) {
        return PreparedJsonInput {
            text: Cow::Borrowed(input),
            invalid_error: None,
        };
    }

    let mut output = String::with_capacity(input.len());
    let mut remaining = input;
    let mut state = JsonStringState::default();
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                state.update(valid.as_bytes());
                output.push_str(valid);
                break;
            }
            Err(error) => {
                let valid_length = error.valid_up_to();
                let valid = std::str::from_utf8(&remaining[..valid_length])
                    .expect("from_utf8 valid prefix must remain valid");
                state.update(valid.as_bytes());
                output.push_str(valid);
                let invalid_error = if state.invalid_byte_breaks_escape() {
                    Some(PhpJsonDecodeError::new(JSON_ERROR_SYNTAX))
                } else if policy == InvalidUtf8Policy::Reject || !state.in_string {
                    Some(PhpJsonDecodeError::new(JSON_ERROR_UTF8))
                } else {
                    None
                };
                if let Some(invalid_error) = invalid_error {
                    return PreparedJsonInput {
                        text: Cow::Owned(output),
                        invalid_error: Some(invalid_error),
                    };
                }
                if policy == InvalidUtf8Policy::Substitute {
                    output.push('\u{fffd}');
                }
                let invalid_length = error
                    .error_len()
                    .unwrap_or_else(|| remaining.len().saturating_sub(valid_length));
                remaining = &remaining[valid_length + invalid_length..];
            }
        }
    }
    PreparedJsonInput {
        text: Cow::Owned(output),
        invalid_error: None,
    }
}

#[inline(always)]
fn is_json_number_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b' ' | b'\t' | b'\r' | b'\n' | b',' | b']' | b'}' | b':'
    )
}

fn is_valid_json_number(token: &str) -> bool {
    let bytes = token.as_bytes();
    let mut index = usize::from(bytes.first() == Some(&b'-'));
    if index >= bytes.len() {
        return false;
    }
    match bytes[index] {
        b'0' => index += 1,
        b'1'..=b'9' => {
            index += 1;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
        }
        _ => return false,
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let fraction_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == fraction_start {
            return false;
        }
    }
    if bytes
        .get(index)
        .is_some_and(|byte| matches!(byte, b'e' | b'E'))
    {
        index += 1;
        if bytes
            .get(index)
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
        {
            index += 1;
        }
        let exponent_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == exponent_start {
            return false;
        }
    }
    index == bytes.len()
}

fn php_number_from_lexeme(token: &str, bigint_as_string: bool) -> Value {
    if !token.bytes().any(|byte| matches!(byte, b'.' | b'e' | b'E')) {
        if let Ok(value) = token.parse::<i64>() {
            return Value::long(value);
        }
        if bigint_as_string {
            return Value::string(token);
        }
    }
    Value::double(token.parse::<f64>().unwrap_or(f64::NAN))
}

struct NumberPlan {
    values: Vec<Value>,
    cursor: Cell<usize>,
}

impl NumberPlan {
    fn next<E>(&self) -> Result<Value, E>
    where
        E: serde::de::Error,
    {
        let index = self.cursor.get();
        let Some(value) = self.values.get(index) else {
            return Err(E::custom(NUMBER_PLAN_ERROR_MARKER));
        };
        self.cursor.set(index + 1);
        Ok(value.clone())
    }
}

struct PreparedNumbers {
    input: String,
    plan: NumberPlan,
}

/// Replace syntactically complete numeric tokens with a cheap placeholder and
/// retain their PHP projections in encounter order. This cold path is used by
/// JSON_BIGINT_AS_STRING and as a retry when serde's finite-f64 parser rejects
/// an otherwise valid overflow such as `1e400`, which PHP represents as INF.
fn prepare_numbers(input: &str, bigint_as_string: bool) -> PreparedNumbers {
    let bytes = input.as_bytes();
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0usize;
    let mut copied_until = 0usize;
    let mut output = String::with_capacity(input.len());
    let mut values = Vec::new();

    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if in_string {
            if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            index += 1;
            continue;
        }
        if byte != b'-' && !byte.is_ascii_digit() {
            index += 1;
            continue;
        }

        let start = index;
        index += 1;
        while index < bytes.len() && !is_json_number_delimiter(bytes[index]) {
            index += 1;
        }
        let end = index;
        let token = &input[start..end];
        if is_valid_json_number(token) {
            output.push_str(&input[copied_until..start]);
            output.push('0');
            copied_until = end;
            values.push(php_number_from_lexeme(token, bigint_as_string));
        }
    }

    output.push_str(&input[copied_until..]);
    PreparedNumbers {
        input: output,
        plan: NumberPlan {
            values,
            cursor: Cell::new(0),
        },
    }
}

fn raw_control_error(input: &str) -> Option<i64> {
    let mut state = JsonStringState::default();
    for byte in input.bytes() {
        if byte <= 0x1f && (state.in_string || !matches!(byte, b' ' | b'\t' | b'\r' | b'\n')) {
            return Some(if state.invalid_byte_breaks_escape() {
                JSON_ERROR_SYNTAX
            } else {
                JSON_ERROR_CONTROL_CHARACTER
            });
        }
        state.update(std::slice::from_ref(&byte));
    }
    None
}

#[derive(Clone, Copy)]
enum ContainerExpectation {
    ArrayFirstOrEnd,
    ArrayValue,
    ArrayCommaOrEnd,
    ObjectFirstKeyOrEnd,
    ObjectKey,
    ObjectColon,
    ObjectValue,
    ObjectCommaOrEnd,
}

fn has_mismatched_container(input: &str) -> bool {
    let bytes = input.as_bytes();
    let mut stack = Vec::<(u8, ContainerExpectation)>::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut string_is_key = false;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if in_string {
            if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
                if let Some((_, expectation)) = stack.last_mut() {
                    if string_is_key {
                        *expectation = ContainerExpectation::ObjectColon;
                    } else {
                        match expectation {
                            ContainerExpectation::ArrayFirstOrEnd
                            | ContainerExpectation::ArrayValue => {
                                *expectation = ContainerExpectation::ArrayCommaOrEnd;
                            }
                            ContainerExpectation::ObjectValue => {
                                *expectation = ContainerExpectation::ObjectCommaOrEnd;
                            }
                            _ => {}
                        }
                    }
                }
            }
            index += 1;
            continue;
        }
        match byte {
            b'"' => {
                string_is_key = stack.last().is_some_and(|(_, expectation)| {
                    matches!(
                        expectation,
                        ContainerExpectation::ObjectFirstKeyOrEnd | ContainerExpectation::ObjectKey
                    )
                });
                in_string = true;
            }
            b'[' => stack.push((byte, ContainerExpectation::ArrayFirstOrEnd)),
            b'{' => stack.push((byte, ContainerExpectation::ObjectFirstKeyOrEnd)),
            b']' | b'}' => {
                let expected = if byte == b']' { b'[' } else { b'{' };
                let Some((open, expectation)) = stack.last().copied() else {
                    index += 1;
                    continue;
                };
                if open != expected {
                    return matches!(
                        expectation,
                        ContainerExpectation::ArrayFirstOrEnd
                            | ContainerExpectation::ArrayCommaOrEnd
                            | ContainerExpectation::ObjectFirstKeyOrEnd
                            | ContainerExpectation::ObjectCommaOrEnd
                    );
                }
                stack.pop();
                if let Some((_, parent)) = stack.last_mut() {
                    match parent {
                        ContainerExpectation::ArrayFirstOrEnd
                        | ContainerExpectation::ArrayValue => {
                            *parent = ContainerExpectation::ArrayCommaOrEnd;
                        }
                        ContainerExpectation::ObjectValue => {
                            *parent = ContainerExpectation::ObjectCommaOrEnd;
                        }
                        _ => {}
                    }
                }
            }
            b':' => {
                if let Some((_, ContainerExpectation::ObjectColon)) = stack.last_mut() {
                    stack.last_mut().unwrap().1 = ContainerExpectation::ObjectValue;
                }
            }
            b',' => {
                if let Some((_, expectation)) = stack.last_mut() {
                    if matches!(expectation, ContainerExpectation::ArrayCommaOrEnd) {
                        *expectation = ContainerExpectation::ArrayValue;
                    } else if matches!(expectation, ContainerExpectation::ObjectCommaOrEnd) {
                        *expectation = ContainerExpectation::ObjectKey;
                    }
                }
            }
            b' ' | b'\t' | b'\r' | b'\n' => {}
            _ => {
                let start = index;
                while index + 1 < bytes.len()
                    && !matches!(
                        bytes[index + 1],
                        b' ' | b'\t' | b'\r' | b'\n' | b',' | b']' | b'}' | b':'
                    )
                {
                    index += 1;
                }
                if index >= start {
                    if let Some((_, expectation)) = stack.last_mut() {
                        match expectation {
                            ContainerExpectation::ArrayFirstOrEnd
                            | ContainerExpectation::ArrayValue => {
                                *expectation = ContainerExpectation::ArrayCommaOrEnd;
                            }
                            ContainerExpectation::ObjectValue => {
                                *expectation = ContainerExpectation::ObjectCommaOrEnd;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        index += 1;
    }
    false
}

#[inline(always)]
fn hex_value(byte: u8) -> Option<u16> {
    match byte {
        b'0'..=b'9' => Some(u16::from(byte - b'0')),
        b'a'..=b'f' => Some(u16::from(byte - b'a' + 10)),
        b'A'..=b'F' => Some(u16::from(byte - b'A' + 10)),
        _ => None,
    }
}

fn unicode_escape(bytes: &[u8], start: usize) -> Option<u16> {
    if bytes.get(start..start + 2) != Some(b"\\u") {
        return None;
    }
    let mut value = 0u16;
    for &byte in bytes.get(start + 2..start + 6)? {
        value = value.checked_mul(16)?.checked_add(hex_value(byte)?)?;
    }
    Some(value)
}

fn has_unpaired_utf16_surrogate(input: &str) -> bool {
    let bytes = input.as_bytes();
    let mut in_string = false;
    let mut index = 0usize;
    while index < bytes.len() {
        if !in_string {
            if bytes[index] == b'"' {
                in_string = true;
            }
            index += 1;
            continue;
        }
        if bytes[index] == b'"' {
            in_string = false;
            index += 1;
            continue;
        }
        if bytes[index] != b'\\' {
            index += 1;
            continue;
        }
        if bytes.get(index + 1) != Some(&b'u') {
            index = index.saturating_add(2);
            continue;
        }
        let Some(code_unit) = unicode_escape(bytes, index) else {
            index += 1;
            continue;
        };
        if (0xd800..=0xdbff).contains(&code_unit) {
            let Some(low) = unicode_escape(bytes, index + 6) else {
                return true;
            };
            if !(0xdc00..=0xdfff).contains(&low) {
                return true;
            }
            index += 12;
        } else if (0xdc00..=0xdfff).contains(&code_unit) {
            return true;
        } else {
            index += 6;
        }
    }
    false
}

fn classify_syntax_error(input: &str) -> i64 {
    if let Some(code) = raw_control_error(input) {
        code
    } else if has_mismatched_container(input) {
        JSON_ERROR_STATE_MISMATCH
    } else if has_unpaired_utf16_surrogate(input) {
        JSON_ERROR_UTF16
    } else {
        JSON_ERROR_SYNTAX
    }
}

fn syntax_error_prefix<'input>(input: &'input str, error: &serde_json::Error) -> &'input str {
    let line = error.line().max(1);
    let column = error.column();
    let mut offset = 0usize;
    for _ in 1..line {
        let Some(newline) = input.as_bytes()[offset..]
            .iter()
            .position(|byte| *byte == b'\n')
        else {
            return input;
        };
        offset = offset.saturating_add(newline + 1);
    }
    let line_end = input.as_bytes()[offset..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(input.len(), |newline| offset + newline);
    let current_line = &input[offset..line_end];
    let mut column_end = column.min(current_line.len());
    while column_end < current_line.len() && !current_line.is_char_boundary(column_end) {
        column_end += 1;
    }
    &input[..offset + column_end]
}

#[inline]
fn classify_syntax_error_at(input: &str, error: &serde_json::Error) -> i64 {
    classify_syntax_error(syntax_error_prefix(input, error))
}

fn decode_php_text(
    input: &str,
    associative: bool,
    maximum_depth: u32,
    bigint_as_string: bool,
) -> Result<Value, PhpJsonDecodeError> {
    let decode = |source: &str,
                  number_plan: Option<&NumberPlan>,
                  negative_zero_seen: Option<&Cell<bool>>| {
        let mut deserializer = serde_json::Deserializer::from_str(source);
        deserializer.disable_recursion_limit();
        let result = PhpValueSeed::new(associative, maximum_depth, number_plan, negative_zero_seen)
            .deserialize(serde_stacker::Deserializer::new(&mut deserializer));
        result.and_then(|value| {
            deserializer.end()?;
            Ok(value)
        })
    };
    let map_error = |error: serde_json::Error, source: &str| {
        let message = error.to_string();
        let code = if message.contains(DEPTH_ERROR_MARKER) {
            JSON_ERROR_DEPTH
        } else if message.contains(PARSER_STACK_ERROR_MARKER) {
            JSON_ERROR_SYNTAX
        } else if message.contains(INVALID_PROPERTY_ERROR_MARKER) {
            JSON_ERROR_INVALID_PROPERTY_NAME
        } else {
            classify_syntax_error_at(source, &error)
        };
        PhpJsonDecodeError {
            code,
            incomplete: error.is_eof(),
        }
    };

    if bigint_as_string {
        let prepared = prepare_numbers(input, true);
        return decode(&prepared.input, Some(&prepared.plan), None)
            .map_err(|error| map_error(error, &prepared.input));
    }

    let negative_zero_seen = Cell::new(false);
    match decode(input, None, Some(&negative_zero_seen)) {
        Err(error) if error.to_string().contains("number out of range") => {
            let prepared = prepare_numbers(input, false);
            decode(&prepared.input, Some(&prepared.plan), None)
                .map_err(|error| map_error(error, &prepared.input))
        }
        Ok(_) if negative_zero_seen.get() => {
            let prepared = prepare_numbers(input, false);
            decode(&prepared.input, Some(&prepared.plan), None)
                .map_err(|error| map_error(error, &prepared.input))
        }
        result => result.map_err(|error| map_error(error, input)),
    }
}

pub(super) fn decode_php_bytes(
    input: &[u8],
    associative: bool,
    maximum_depth: u32,
    bigint_as_string: bool,
    utf8_policy: InvalidUtf8Policy,
) -> Result<Value, PhpJsonDecodeError> {
    let prepared = json_input_with_utf8_policy(input, utf8_policy);
    if let Some(invalid_error) = prepared.invalid_error {
        if let Some(code) =
            prior_json_error_code(&prepared.text, associative, maximum_depth, bigint_as_string)
        {
            return Err(PhpJsonDecodeError::new(code));
        }
        return Err(invalid_error);
    }
    decode_php_text(&prepared.text, associative, maximum_depth, bigint_as_string)
}

pub(super) fn decode_php_str_api(
    input: &str,
    associative: bool,
    maximum_depth: u32,
    flags: i64,
) -> Result<Value, PhpJsonDecodeError> {
    decode_php_text(
        input,
        associative,
        maximum_depth,
        flags & JSON_BIGINT_AS_STRING_FLAG != 0,
    )
}

pub(super) fn decode_php_api(
    input: &[u8],
    associative: bool,
    maximum_depth: u32,
    flags: i64,
) -> Result<Value, PhpJsonDecodeError> {
    let utf8_policy = if flags & JSON_INVALID_UTF8_SUBSTITUTE_FLAG != 0 {
        InvalidUtf8Policy::Substitute
    } else if flags & JSON_INVALID_UTF8_IGNORE_FLAG != 0 {
        InvalidUtf8Policy::Ignore
    } else {
        InvalidUtf8Policy::Reject
    };
    decode_php_bytes(
        input,
        associative,
        maximum_depth,
        flags & JSON_BIGINT_AS_STRING_FLAG != 0,
        utf8_policy,
    )
}

pub(super) fn decode_php_value(
    input: &str,
    associative: bool,
) -> Result<Value, PhpJsonDecodeError> {
    decode_php_text(input, associative, 512, false)
}

#[cfg(test)]
mod tests {
    use crate::value::Value;

    use super::{
        InvalidUtf8Policy, JSON_ERROR_CONTROL_CHARACTER, JSON_ERROR_DEPTH,
        JSON_ERROR_INVALID_PROPERTY_NAME, JSON_ERROR_STATE_MISMATCH, JSON_ERROR_SYNTAX,
        JSON_ERROR_UTF8, JSON_ERROR_UTF16, JSON_INVALID_UTF8_IGNORE_FLAG,
        JSON_INVALID_UTF8_SUBSTITUTE_FLAG, decode_php_api, decode_php_bytes, decode_php_value,
    };

    #[test]
    fn decodes_escaped_strings_and_unicode_surrogate_pairs() {
        let result = decode_php_value(
            r#"{"escaped":"line\nquote\"slash\\","unicode":"\uD83D\uDE00"}"#,
            true,
        );
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn enforces_php_container_depth_and_preserves_big_integer_lexemes() {
        assert_eq!(
            decode_php_bytes(b"[]", true, 1, false, InvalidUtf8Policy::Reject)
                .unwrap_err()
                .code(),
            JSON_ERROR_DEPTH
        );
        assert!(decode_php_bytes(b"[]", true, 2, false, InvalidUtf8Policy::Reject).is_ok());
        assert_eq!(
            decode_php_bytes(b"[[1]]", true, 2, false, InvalidUtf8Policy::Reject)
                .unwrap_err()
                .code(),
            JSON_ERROR_DEPTH
        );
        let decoded = decode_php_bytes(
            br#"[9223372036854775808,-9223372036854775809,999999999999999999999999999999]"#,
            true,
            4,
            true,
            InvalidUtf8Policy::Reject,
        )
        .unwrap();
        let values = decoded.as_array().unwrap();
        assert_eq!(
            values.get_int(0).and_then(Value::as_str),
            Some("9223372036854775808")
        );
        assert_eq!(
            values.get_int(1).and_then(Value::as_str),
            Some("-9223372036854775809")
        );
        assert_eq!(
            values.get_int(2).and_then(Value::as_str),
            Some("999999999999999999999999999999")
        );
    }

    #[test]
    fn retries_only_numeric_overflow_without_accepting_malformed_numbers() {
        let decoded = decode_php_bytes(
            br#"[1e400,-1e400,1e3,9223372036854775808]"#,
            true,
            8,
            false,
            InvalidUtf8Policy::Reject,
        )
        .unwrap();
        let values = decoded.as_array().unwrap();
        assert_eq!(
            values.get_int(0).and_then(Value::as_double),
            Some(f64::INFINITY)
        );
        assert_eq!(
            values.get_int(1).and_then(Value::as_double),
            Some(f64::NEG_INFINITY)
        );
        assert_eq!(values.get_int(2).and_then(Value::as_double), Some(1000.0));
        assert_eq!(
            values.get_int(3).and_then(Value::as_double),
            Some(9_223_372_036_854_776_000.0)
        );

        for malformed in [b"01".as_slice(), b"[1 2]", b"[1e]"] {
            assert_eq!(
                decode_php_bytes(malformed, true, 8, false, InvalidUtf8Policy::Reject)
                    .unwrap_err()
                    .code(),
                JSON_ERROR_SYNTAX
            );
        }
    }

    #[test]
    fn repairs_invalid_utf8_only_inside_json_strings() {
        let ignored =
            decode_php_bytes(b"\"A\xffB\"", false, 2, false, InvalidUtf8Policy::Ignore).unwrap();
        assert_eq!(ignored.as_str(), Some("AB"));
        let substituted = decode_php_bytes(
            b"\"A\xffB\"",
            false,
            2,
            false,
            InvalidUtf8Policy::Substitute,
        )
        .unwrap();
        assert_eq!(substituted.as_str(), Some("A\u{fffd}B"));
        assert_eq!(
            decode_php_bytes(b"[\xff]", true, 3, false, InvalidUtf8Policy::Ignore)
                .unwrap_err()
                .code(),
            JSON_ERROR_UTF8
        );
        assert_eq!(
            decode_php_api(
                b"\"A\xffB\"",
                false,
                2,
                JSON_INVALID_UTF8_IGNORE_FLAG | JSON_INVALID_UTF8_SUBSTITUTE_FLAG,
            )
            .unwrap()
            .as_str(),
            Some("A\u{fffd}B")
        );
        assert_eq!(
            decode_php_bytes(b"x\xff", false, 512, false, InvalidUtf8Policy::Reject)
                .unwrap_err()
                .code(),
            JSON_ERROR_SYNTAX
        );
    }

    #[test]
    fn classifies_php_visible_decode_failures() {
        for (input, expected) in [
            (b"[}".as_slice(), JSON_ERROR_STATE_MISMATCH),
            (b"[1,]".as_slice(), JSON_ERROR_SYNTAX),
            (br#"{"a"]"#.as_slice(), JSON_ERROR_SYNTAX),
            (b"\"a\x01b\"".as_slice(), JSON_ERROR_CONTROL_CHARACTER),
            (b"\"\\u12\x01\"".as_slice(), JSON_ERROR_SYNTAX),
            (b"\0".as_slice(), JSON_ERROR_CONTROL_CHARACTER),
            (br#""\uD800""#.as_slice(), JSON_ERROR_UTF16),
            (br#""a\"\uD800""#.as_slice(), JSON_ERROR_UTF16),
            (br#""a\"\uDC00""#.as_slice(), JSON_ERROR_UTF16),
        ] {
            assert_eq!(
                decode_php_bytes(input, false, 512, false, InvalidUtf8Policy::Reject)
                    .unwrap_err()
                    .code(),
                expected
            );
        }
        assert_eq!(
            decode_php_bytes(
                br#"{"\u0000x":1}"#,
                false,
                512,
                false,
                InvalidUtf8Policy::Reject,
            )
            .unwrap_err()
            .code(),
            JSON_ERROR_INVALID_PROPERTY_NAME
        );
        assert!(
            decode_php_bytes(
                br#"{"\u0000x":1}"#,
                true,
                512,
                false,
                InvalidUtf8Policy::Reject,
            )
            .is_ok()
        );
    }

    #[test]
    fn invalid_utf8_preserves_prior_php_parser_and_semantic_errors() {
        for (input, associative, depth, bigint, expected) in [
            (b"tru\xff".as_slice(), false, 512, false, JSON_ERROR_SYNTAX),
            (b"1e\xff".as_slice(), false, 512, false, JSON_ERROR_SYNTAX),
            (
                b"{\"\\u0000x\":1}\xff".as_slice(),
                false,
                512,
                false,
                JSON_ERROR_INVALID_PROPERTY_NAME,
            ),
            (b"[[0]]\xff".as_slice(), true, 2, false, JSON_ERROR_DEPTH),
            (
                b"\"\\uD800\"\xff".as_slice(),
                false,
                512,
                false,
                JSON_ERROR_UTF16,
            ),
            (
                b"\"\\uD800\xff\"".as_slice(),
                false,
                512,
                false,
                JSON_ERROR_UTF16,
            ),
            (
                b"\"\\uD800\\\xff\"".as_slice(),
                false,
                512,
                false,
                JSON_ERROR_UTF16,
            ),
            (
                b"\"\\uD800\\uDC\xff\"".as_slice(),
                false,
                512,
                false,
                JSON_ERROR_UTF16,
            ),
            (b"1e400\xff".as_slice(), false, 512, false, JSON_ERROR_UTF8),
            (
                b"9223372036854775808\xff".as_slice(),
                false,
                512,
                true,
                JSON_ERROR_UTF8,
            ),
        ] {
            assert_eq!(
                decode_php_bytes(input, associative, depth, bigint, InvalidUtf8Policy::Ignore,)
                    .unwrap_err()
                    .code(),
                expected,
                "input={input:?}",
            );
        }
    }

    #[test]
    fn syntax_error_prefix_respects_utf8_character_boundaries() {
        for input in ["руссиш", "[\"é\"x}"] {
            assert_eq!(
                decode_php_bytes(
                    input.as_bytes(),
                    false,
                    512,
                    false,
                    InvalidUtf8Policy::Reject,
                )
                .unwrap_err()
                .code(),
                JSON_ERROR_SYNTAX,
                "input={input:?}",
            );
        }
    }
}
