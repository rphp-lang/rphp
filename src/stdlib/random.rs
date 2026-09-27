//! Built-in Random extension declarations admitted by the PHP 8.5 contract.

use std::collections::HashMap;
use std::rc::Rc;

use crate::compiler::compile::{ClassDef, PropertyDefinition};
use crate::compiler::make_internal_function;
use crate::parser::Visibility;
use crate::runtime::ExecutorGlobals;
use crate::value::{ObjectLayout, PhpArray, PhpObject, Value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{FunctionCommon, InternalFunction, ParamTypeHint};

const INTERVAL_BOUNDARY: &str = "Random\\IntervalBoundary";
const INTERVAL_BOUNDARY_CASES: [&str; 4] = ["ClosedOpen", "ClosedClosed", "OpenClosed", "OpenOpen"];

const MT_STATE_SIZE: usize = 624;
const MT_PERIOD: usize = 397;
const MT_MATRIX: u32 = 0x9908_b0df;

/// Request-local MT19937 state shared by PHP's legacy random functions.
///
/// PHP retains the historical `MT_RAND_PHP` twist variant for compatibility,
/// while every other mode selects the standard recurrence. Range projection
/// consumes the engine's full 32-bit words; the zero-argument `mt_rand()` API
/// applies its separate 31-bit presentation afterwards.
pub(crate) struct Mt19937State {
    words: [u32; MT_STATE_SIZE],
    next: usize,
    legacy: bool,
}

impl Mt19937State {
    pub(crate) fn seeded(seed: u32, legacy: bool) -> Self {
        let mut words = [0; MT_STATE_SIZE];
        words[0] = seed;
        for index in 1..MT_STATE_SIZE {
            let previous = words[index - 1];
            words[index] = 1_812_433_253u32
                .wrapping_mul(previous ^ (previous >> 30))
                .wrapping_add(index as u32);
        }
        Self {
            words,
            next: MT_STATE_SIZE,
            legacy,
        }
    }

    fn reload(&mut self) {
        for index in 0..MT_STATE_SIZE {
            let current = self.words[index];
            let following = self.words[(index + 1) % MT_STATE_SIZE];
            let mixed = (current & 0x8000_0000) | (following & 0x7fff_ffff);
            let parity = if self.legacy { current } else { following } & 1;
            self.words[index] = self.words[(index + MT_PERIOD) % MT_STATE_SIZE]
                ^ (mixed >> 1)
                ^ if parity != 0 { MT_MATRIX } else { 0 };
        }
        self.next = 0;
    }

    pub(crate) fn next_u32(&mut self) -> u32 {
        if self.next == MT_STATE_SIZE {
            self.reload();
        }
        let mut value = self.words[self.next];
        self.next += 1;
        value ^= value >> 11;
        value ^= (value << 7) & 0x9d2c_5680;
        value ^= (value << 15) & 0xefc6_0000;
        value ^ (value >> 18)
    }

    pub(crate) fn next_i31(&mut self) -> i64 {
        i64::from(self.next_u32() >> 1)
    }

    pub(crate) fn uses_legacy_twist(&self) -> bool {
        self.legacy
    }

    /// The deprecated PHP variant retains its historical multiply-and-scale
    /// range mapping. It intentionally consumes only the presented 31-bit
    /// value even for a 64-bit interval, which is why the API is biased.
    pub(crate) fn legacy_offset(&mut self, width: u128) -> u128 {
        (self.next_i31() as u128 * width) >> 31
    }

    fn next_u64(&mut self) -> u64 {
        let low = u64::from(self.next_u32());
        let high = u64::from(self.next_u32());
        low | (high << 32)
    }

    /// Draw an unbiased offset in `0..width`, consuming one 32-bit word when
    /// possible and two words in PHP's low-then-high order for wider ranges.
    pub(crate) fn offset(&mut self, width: u128) -> u128 {
        debug_assert!(width > 0 && width <= (1u128 << 64));
        let bits = if width <= 1u128 << 32 { 32 } else { 64 };
        let sample_space = 1u128 << bits;
        let accepted = sample_space - sample_space % width;
        loop {
            let sample = if bits == 32 {
                u128::from(self.next_u32())
            } else {
                u128::from(self.next_u64())
            };
            if sample < accepted {
                return sample % width;
            }
        }
    }

    pub(crate) fn index(&mut self, upper_exclusive: usize) -> usize {
        debug_assert!(upper_exclusive > 0);
        self.offset(upper_exclusive as u128) as usize
    }
}

fn unit_enum_case(class: &str, name: &str) -> PropertyDefinition {
    let mut properties = HashMap::with_capacity(1);
    properties.insert("name".to_string(), Value::string(name));
    PropertyDefinition::new(
        name.to_string(),
        Some(Value::object(PhpObject::dynamic(
            class.to_string(),
            0,
            properties,
        ))),
        Visibility::Public,
        class.to_string(),
    )
}

fn interval_boundary_cases(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let definition = eg
        .find_class(INTERVAL_BOUNDARY)
        .expect("registered Random\\IntervalBoundary enum is available");
    let class_id = definition.class_id;
    let count = definition.static_properties.len();
    let mut cases = PhpArray::with_packed_capacity(count);
    for index in 0..count {
        let storage_slot = eg
            .static_property_storage_slot(class_id, index)
            .expect("registered enum case owns a static storage slot");
        let value = eg
            .static_property_value(storage_slot)
            .expect("registered enum case storage remains live")
            .clone();
        cases.push(value);
    }
    super::write_return_value(rv, Value::array(cases));
    Ok(())
}

pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let cases = INTERVAL_BOUNDARY_CASES
        .into_iter()
        .map(|name| unit_enum_case(INTERVAL_BOUNDARY, name))
        .collect();
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: INTERVAL_BOUNDARY.to_string(),
        source_file: None,
        declaration_line: 0,
        end_line: 0,
        doc_comment: None,
        parent: None,
        implements: vec!["UnitEnum".to_string()],
        is_interface: false,
        is_abstract: false,
        // PHP's internal enum omits the ReflectionClass final modifier. The
        // enum-parent guard still rejects every attempt to extend it.
        is_final: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        is_trait: false,
        is_enum: true,
        uses: Vec::new(),
        trait_aliases: Vec::new(),
        trait_precedences: Vec::new(),
        properties: vec![PropertyDefinition::declared(
            "name".to_string(),
            None,
            Visibility::Public,
            INTERVAL_BOUNDARY.to_string(),
            ParamTypeHint::String,
            true,
            false,
        )],
        static_properties: cases,
        constants: Vec::new(),
        property_layout: Rc::new(ObjectLayout::empty()),
        property_defaults: Rc::from([]),
        readonly_props: vec!["name".to_string()],
        methods: Vec::new(),
        abstract_methods: Vec::new(),
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .expect("Random\\IntervalBoundary registration is unique");

    let mut cases_method = Box::new(make_internal_function(
        interval_boundary_cases,
        0,
        0,
        Vec::new(),
    ));
    cases_method.common.sig.return_type_hint = ParamTypeHint::Array;
    let cases_pointer = &cases_method.common as *const FunctionCommon;
    eg.insert_function_entry("random\\intervalboundary::cases".to_string(), cases_pointer);
    eg.method_declaring_class
        .insert(cases_pointer, INTERVAL_BOUNDARY.into());

    vec![cases_method]
}

#[cfg(test)]
mod tests {
    use super::Mt19937State;

    #[test]
    fn mt19937_matches_php_standard_and_legacy_sequences() {
        let mut standard = Mt19937State::seeded(1234, false);
        assert_eq!(
            (0..6).map(|_| standard.next_i31()).collect::<Vec<_>>(),
            [
                411_284_887,
                1_068_724_585,
                1_335_968_403,
                1_756_294_682,
                940_013_158,
                1_314_500_282,
            ]
        );

        let mut legacy = Mt19937State::seeded(1234, true);
        assert_eq!(
            (0..5).map(|_| legacy.next_i31()).collect::<Vec<_>>(),
            [
                1_741_177_057,
                1_068_724_585,
                1_335_968_403,
                400_890_732,
                1_196_196_624,
            ]
        );
    }

    #[test]
    fn range_projection_uses_full_words_and_low_then_high_wide_draws() {
        let mut state = Mt19937State::seeded(1234, false);
        assert_eq!(
            (0..8).map(|_| state.offset(11)).collect::<Vec<_>>(),
            [5, 0, 0, 10, 3, 3, 9, 2]
        );

        let mut wide = Mt19937State::seeded(1, false);
        let offset = wide.offset(1u128 << 64);
        assert_eq!(offset, 0xff47_80eb_6ac1_f425);

        let mut legacy = Mt19937State::seeded(0, true);
        assert_eq!(legacy.legacy_offset(1_000_000_000), 448_865_905);
        assert_eq!(legacy.legacy_offset(1_000), 592);
    }
}
