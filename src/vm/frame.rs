use std::mem::size_of;

use crate::value::Value;
use crate::compiler::OpArray;
use super::function::{FunctionCommon, UserFunction};
use super::instruction::{Instruction, OpType};

/// Number of Value-sized slots for the ExecuteData header
pub const CALL_FRAME_SLOTS: usize =
    (size_of::<ExecuteData>() + size_of::<Value>() - 1) / size_of::<Value>();

/// Call frame — equivalent to zend_execute_data.
/// Allocated on VM stack, layout: [ExecuteData][CV0][CV1]...[TMPn]
#[repr(C)]
pub struct ExecuteData {
    pub opline: *const Instruction,
    pub call: *mut ExecuteData,
    pub return_value: *mut Value,
    /// Raw pointer to FunctionCommon header
    pub func: *const FunctionCommon,
    pub prev_execute_data: *mut ExecuteData,
    pub num_args: u32,
    pub num_cvs: u32,
    pub num_temps: u32,
    /// Per-frame flag: a return is pending after the current finally block completes.
    pub pending_return_after_finally: bool,
}

impl ExecuteData {
    /// Get compiled variable by index (CV slot)
    #[inline]
    pub unsafe fn cv(&self, idx: u32) -> &Value {
        let base = (self as *const Self as *const Value).add(CALL_FRAME_SLOTS);
        &*base.add(idx as usize)
    }

    /// Get mutable CV slot
    #[inline]
    pub unsafe fn cv_mut(&mut self, idx: u32) -> &mut Value {
        debug_assert!(
            idx < self.num_cvs,
            "cv_mut index {} out of bounds (num_cvs={})", idx, self.num_cvs
        );
        let base = (self as *mut Self as *mut Value).add(CALL_FRAME_SLOTS);
        &mut *base.add(idx as usize)
    }

    /// Get temporary variable by index
    #[inline]
    pub unsafe fn tmp(&self, idx: u32) -> &Value {
        let base = (self as *const Self as *const Value).add(CALL_FRAME_SLOTS);
        &*base.add(self.num_cvs as usize + idx as usize)
    }

    /// Get mutable temporary
    #[inline]
    pub unsafe fn tmp_mut(&mut self, idx: u32) -> &mut Value {
        let base = (self as *mut Self as *mut Value).add(CALL_FRAME_SLOTS);
        &mut *base.add(self.num_cvs as usize + idx as usize)
    }

    /// Resolve operand to Value reference based on operand type.
    /// Returns a raw pointer to avoid lifetime entanglement between
    /// frame (self) and op_array.
    #[inline]
    pub unsafe fn get_op_ptr(
        &self,
        operand: u32,
        op_type: OpType,
        op_array: &OpArray,
    ) -> *const Value {
        match op_type {
            OpType::Const => &op_array.literals()[operand as usize] as *const Value,
            OpType::Cv => self.cv(operand) as *const Value,
            OpType::Tmp | OpType::Var => self.tmp(operand) as *const Value,
            OpType::Unused => panic!("get_op on unused operand"),
        }
    }

    /// Resolve operand to mutable Value pointer
    #[inline]
    pub unsafe fn get_op_mut(&mut self, operand: u32, op_type: OpType) -> *mut Value {
        match op_type {
            OpType::Cv => self.cv_mut(operand) as *mut Value,
            OpType::Tmp | OpType::Var => self.tmp_mut(operand) as *mut Value,
            _ => panic!("get_op_mut on const/unused operand"),
        }
    }

    /// Get OpArray for user function frames.
    /// SAFETY: caller must know this frame is for a user function.
    #[inline]
    pub unsafe fn op_array(&self) -> &OpArray {
        // Go through raw pointer directly to avoid lifetime issues
        // with the Function wrapper (which is a local temporary).
        // self.func points to FunctionCommon which is at offset 0
        // of UserFunction (#[repr(C)]), so this cast is valid.
        let user = &*(self.func as *const UserFunction);
        &user.op_array
    }
}
