use std::mem::size_of;

use crate::value::Value;
use crate::compiler::OpArray;
use super::function::{FunctionCommon, UserFunction};
use super::instruction::{Instruction, OpType};

/// Number of Value-sized slots for the ExecuteData header
pub const CALL_FRAME_SLOTS: usize =
    (size_of::<ExecuteData>() + size_of::<Value>() - 1) / size_of::<Value>();

// ── SlotState ────────────────────────────────────────────────────────────────

/// Slot-state tracking for frame slots (CV + TMP).
///
/// Two bitmaps: `init` (which slots are valid) and `heap` (which hold heap values).
/// Invariant: `heap ⊆ init`.
///
/// Covers up to 64 slots (u64). Frames with > 64 slots use fallback
/// (checked by caller via `num_cvs + num_temps > 64`).
#[derive(Clone, Copy)]
pub struct SlotState {
    pub init: u64,
    pub heap: u64,
}

impl SlotState {
    pub const EMPTY: SlotState = SlotState { init: 0, heap: 0 };

    /// Set init bits for range [start..end).
    #[inline(always)]
    pub fn mark_init_range(&mut self, start: u32, end: u32) {
        if start >= end { return; }
        let mask = if end >= 64 { !0u64 << start } else { ((1u64 << end) - 1) & !((1u64 << start) - 1) };
        self.init |= mask;
    }

    #[inline(always)]
    pub fn is_init(&self, idx: u32) -> bool {
        self.init & (1u64 << idx) != 0
    }

    #[inline(always)]
    pub fn is_heap(&self, idx: u32) -> bool {
        self.heap & (1u64 << idx) != 0
    }

    #[inline(always)]
    pub fn has_any_heap(&self) -> bool {
        self.heap != 0
    }

    /// Iterator over indices of heap slots.
    #[inline(always)]
    pub fn heap_iter(&self) -> HeapSlotIter {
        HeapSlotIter { bits: self.heap }
    }
}

/// Iterator over set bits in heap bitmap. Yields slot indices.
pub struct HeapSlotIter {
    bits: u64,
}

impl Iterator for HeapSlotIter {
    type Item = u32;

    #[inline(always)]
    fn next(&mut self) -> Option<u32> {
        if self.bits == 0 { return None; }
        let idx = self.bits.trailing_zeros();
        self.bits &= self.bits - 1; // clear lowest set bit
        Some(idx)
    }
}

// ── ExecuteData ──────────────────────────────────────────────────────────────

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
    /// Runtime over-approximation: true when any frame slot may hold a heap value.
    /// Monotonically true — once set, never cleared within frame lifetime.
    /// Used by cleanup to fast-skip scan and by frame_slot_set to skip drop.
    pub has_heap_slots: bool,
}

impl ExecuteData {
    /// Pointer to slot[idx] — unified accessor for both CVs and TMPs.
    /// idx is absolute slot offset: CV if idx < num_cvs, TMP if idx >= num_cvs.
    #[inline(always)]
    pub unsafe fn slot_ptr(&self, idx: u32) -> *mut Value {
        let base = (self as *const Self as *mut Value).add(CALL_FRAME_SLOTS);
        base.add(idx as usize)
    }

    /// Reference to slot[idx] — absolute offset from slot base.
    /// Use for resolved Tmp operands (after resolve_tmp_offsets).
    #[inline(always)]
    pub unsafe fn slot(&self, idx: u32) -> &Value {
        let base = (self as *const Self as *const Value).add(CALL_FRAME_SLOTS);
        &*base.add(idx as usize)
    }

    /// Mutable reference to slot[idx] — absolute offset from slot base.
    #[inline(always)]
    pub unsafe fn slot_mut(&mut self, idx: u32) -> &mut Value {
        let base = (self as *mut Self as *mut Value).add(CALL_FRAME_SLOTS);
        &mut *base.add(idx as usize)
    }

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
    /// For CV operands: follows references (returns pointer to target).
    #[inline]
    pub unsafe fn get_op_ptr(
        &self,
        operand: u32,
        op_type: OpType,
        op_array: &OpArray,
    ) -> *const Value {
        match op_type {
            OpType::Const => &op_array.literals()[operand as usize] as *const Value,
            OpType::Cv => {
                let ptr = self.cv(operand) as *const Value;
                if (*ptr).is_reference() { (*ptr).as_ref_ptr() as *const Value } else { ptr }
            }
            // Tmp/Var operands already contain absolute slot offset (num_cvs + tmp_idx)
            // after resolve_tmp_offsets(), so just index from slot base.
            OpType::Tmp | OpType::Var => {
                let base = (self as *const Self as *const Value).add(CALL_FRAME_SLOTS);
                base.add(operand as usize)
            }
            OpType::Unused => panic!("get_op on unused operand"),
        }
    }

    /// Resolve operand to mutable Value pointer.
    /// For CV operands: follows references (returns pointer to target).
    #[inline]
    pub unsafe fn get_op_mut(&mut self, operand: u32, op_type: OpType) -> *mut Value {
        match op_type {
            OpType::Cv => {
                let ptr = self.cv_mut(operand) as *mut Value;
                if (*ptr).is_reference() { (*ptr).as_ref_ptr() } else { ptr }
            }
            // Tmp/Var operands already contain absolute slot offset.
            OpType::Tmp | OpType::Var => {
                let base = (self as *mut Self as *mut Value).add(CALL_FRAME_SLOTS);
                base.add(operand as usize)
            }
            _ => panic!("get_op_mut on const/unused operand"),
        }
    }

    /// Get OpArray for user function frames.
    /// SAFETY: caller must know this frame is for a user function.
    #[inline]
    pub unsafe fn op_array(&self) -> &OpArray {
        let user = &*(self.func as *const UserFunction);
        &user.op_array
    }
}
