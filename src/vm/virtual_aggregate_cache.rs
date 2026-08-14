use crate::compiler::OpArray;
use crate::compiler::compile::ClassDef;
use crate::value::Value;
use crate::vm::function::{
    FunctionCommon, ObjectArrayFunctionPlan, ObjectArrayLongCall, ObjectLongFunctionPlan,
    UserFunction,
};
use crate::vm::instruction::Instruction;

pub(crate) const RESOLVED_VIRTUAL_AGGREGATE_CACHE_SLOTS: usize = 4;

#[derive(Clone, Copy)]
pub(crate) struct ResolvedObjectArrayCall {
    pub(crate) operation: *const ObjectArrayLongCall,
    pub(crate) receiver: *const Value,
    pub(crate) receiver_identity: usize,
    pub(crate) target: *const FunctionCommon,
    pub(crate) callee: *const UserFunction,
    pub(crate) plan: *const ObjectLongFunctionPlan,
    pub(crate) declaring_class: Option<*const str>,
}

impl ResolvedObjectArrayCall {
    pub(crate) const EMPTY: Self = Self {
        operation: std::ptr::null(),
        receiver: std::ptr::null(),
        receiver_identity: 0,
        target: std::ptr::null(),
        callee: std::ptr::null(),
        plan: std::ptr::null(),
        declaring_class: None,
    };
}

#[derive(Clone, Copy)]
pub(crate) struct ResolvedVirtualAggregateCacheEntry {
    pub(crate) site: *const Instruction,
    pub(crate) caller_op_array: *const OpArray,
    pub(crate) class_id: u32,
    pub(crate) class_def: *const ClassDef,
    pub(crate) constructor_target: *const FunctionCommon,
    pub(crate) constructor_declaring_class: Option<*const str>,
    pub(crate) constructor_argument_count: u8,
    pub(crate) assignment_cache_ips: [u16; 8],
    pub(crate) assignment_slots: [usize; 8],
    pub(crate) assignment_arguments: [u8; 8],
    pub(crate) assignment_count: u8,
    pub(crate) property_slots: [usize; 8],
    pub(crate) property_arguments: [u8; 8],
    pub(crate) property_count: u8,
    pub(crate) method_ip: u32,
    pub(crate) method_class_id: u32,
    pub(crate) method_receiver_identity: usize,
    pub(crate) method_target: *const FunctionCommon,
    pub(crate) method_declaring_class: Option<*const str>,
    pub(crate) method_user: *const UserFunction,
    pub(crate) method_plan: *const ObjectArrayFunctionPlan,
    pub(crate) method_argument_count: u8,
    pub(crate) virtual_argument: u8,
    pub(crate) nested_calls: [ResolvedObjectArrayCall; 8],
    pub(crate) nested_call_count: u8,
    pub(crate) consumer_entries: [u8; 4],
    pub(crate) consumer_accumulators: [u16; 4],
    pub(crate) consumer_count: u8,
    pub(crate) trailing_entry: u8,
    pub(crate) trailing_result: u16,
    pub(crate) next_ip: u32,
}

impl ResolvedVirtualAggregateCacheEntry {
    pub(crate) const EMPTY: Self = Self {
        site: std::ptr::null(),
        caller_op_array: std::ptr::null(),
        class_id: 0,
        class_def: std::ptr::null(),
        constructor_target: std::ptr::null(),
        constructor_declaring_class: None,
        constructor_argument_count: 0,
        assignment_cache_ips: [0; 8],
        assignment_slots: [usize::MAX; 8],
        assignment_arguments: [0; 8],
        assignment_count: 0,
        property_slots: [usize::MAX; 8],
        property_arguments: [0; 8],
        property_count: 0,
        method_ip: 0,
        method_class_id: 0,
        method_receiver_identity: 0,
        method_target: std::ptr::null(),
        method_declaring_class: None,
        method_user: std::ptr::null(),
        method_plan: std::ptr::null(),
        method_argument_count: 0,
        virtual_argument: u8::MAX,
        nested_calls: [ResolvedObjectArrayCall::EMPTY; 8],
        nested_call_count: 0,
        consumer_entries: [u8::MAX; 4],
        consumer_accumulators: [0; 4],
        consumer_count: 0,
        trailing_entry: u8::MAX,
        trailing_result: 0,
        next_ip: 0,
    };
}

#[cfg(test)]
mod tests {
    use super::{RESOLVED_VIRTUAL_AGGREGATE_CACHE_SLOTS, ResolvedVirtualAggregateCacheEntry};

    #[test]
    fn resolved_virtual_aggregate_cache_is_fixed_and_bounded() {
        assert!(RESOLVED_VIRTUAL_AGGREGATE_CACHE_SLOTS.is_power_of_two());
        let bytes = std::mem::size_of::<ResolvedVirtualAggregateCacheEntry>()
            * RESOLVED_VIRTUAL_AGGREGATE_CACHE_SLOTS;
        assert!(bytes <= 4096, "request-local cache grew to {bytes} bytes");
    }
}
