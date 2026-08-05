//! Streaming JSON decode directly into canonical RPHP values.
//!
//! `serde_json::Value` is intentionally not an intermediate representation:
//! it would allocate a second recursive tree (including a BTreeMap for every
//! object) and then immediately walk and destroy that tree while constructing
//! the PHP result.

use std::collections::HashMap;
use std::fmt;

use serde::de::{DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::value::{canonical_decimal_array_key, PhpArray, PhpObject, Value};

#[derive(Clone, Copy)]
struct PhpValueSeed {
    associative: bool,
}

impl PhpValueSeed {
    #[inline(always)]
    const fn new(associative: bool) -> Self {
        Self { associative }
    }
}

impl<'de> DeserializeSeed<'de> for PhpValueSeed {
    type Value = Value;

    #[inline]
    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(PhpValueVisitor {
            associative: self.associative,
        })
    }
}

struct PhpValueVisitor {
    associative: bool,
}

impl<'de> Visitor<'de> for PhpValueVisitor {
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
    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Value::long(value))
    }

    #[inline]
    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(match i64::try_from(value) {
            Ok(value) => Value::long(value),
            Err(_) => Value::double(value as f64),
        })
    }

    #[inline]
    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
        Ok(Value::double(value))
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
        let mut array = PhpArray::with_packed_capacity(sequence.size_hint().unwrap_or(0));
        while let Some(value) = sequence.next_element_seed(PhpValueSeed::new(self.associative))? {
            array.push(value);
        }
        Ok(Value::array(array))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        if self.associative {
            let mut array = PhpArray::with_hash_capacity(map.size_hint().unwrap_or(0));
            while let Some(key) = map.next_key::<String>()? {
                let value = map.next_value_seed(PhpValueSeed::new(true))?;
                if let Some(key) = canonical_decimal_array_key(&key) {
                    array.set_int(key, value);
                } else {
                    array.set_owned_str(key, value);
                }
            }
            Ok(Value::array(array))
        } else {
            let mut properties = HashMap::with_capacity(map.size_hint().unwrap_or(0));
            while let Some(key) = map.next_key::<String>()? {
                let value = map.next_value_seed(PhpValueSeed::new(false))?;
                properties.insert(key, value);
            }
            Ok(Value::object(PhpObject::dynamic(
                "stdClass".to_string(),
                0,
                properties,
            )))
        }
    }
}

pub(super) fn decode_php_value(input: &str, associative: bool) -> Result<Value, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(input);
    let value = PhpValueSeed::new(associative).deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::decode_php_value;

    #[test]
    fn decodes_escaped_strings_and_unicode_surrogate_pairs() {
        let result = decode_php_value(
            r#"{"escaped":"line\nquote\"slash\\","unicode":"\uD83D\uDE00"}"#,
            true,
        );
        assert!(result.is_ok(), "{result:?}");
    }
}
