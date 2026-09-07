use super::{Value, ValueType};

#[test]
fn cleanup_classification_preserves_type_and_reference_ownership() {
    // Exercise metadata directly, without constructing a fake heap pointer or
    // dropping a Value whose storage does not match its tag.
    for tag in 0..=255 {
        for flags in [0, Value::OWNED_REFERENCE_FLAG, 1 << 17, 0xffff_ff00] {
            let expected = match tag {
                tag if tag == ValueType::String as u32
                    || tag == ValueType::Array as u32
                    || tag == ValueType::Object as u32
                    || tag == ValueType::Closure as u32 =>
                {
                    true
                }
                tag if tag == ValueType::Resource as u32 => cfg!(feature = "resource-lifetime"),
                tag if tag == ValueType::Reference as u32 => {
                    flags & Value::OWNED_REFERENCE_FLAG != 0
                }
                _ => false,
            };
            assert_eq!(
                Value::type_info_needs_cleanup(tag | flags),
                expected,
                "tag={tag}, flags={flags:#x}"
            );
        }
    }
}
