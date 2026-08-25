use std::mem::size_of;

use super::function::{FunctionCommon, UserFunction};
use super::instruction::{Instruction, OpType};
use crate::compiler::OpArray;
use crate::value::Value;

/// Number of Value-sized slots for the ExecuteData header
pub const CALL_FRAME_SLOTS: usize =
    (size_of::<ExecuteData>() + size_of::<Value>() - 1) / size_of::<Value>();

const _: [(); 4] = [(); CALL_FRAME_SLOTS];

// ── HeapSlotIter ─────────────────────────────────────────────────────────────

/// Iterator over set bits in a u64 heap bitmap. Yields slot indices.
pub struct HeapSlotIter {
    bits: u64,
}

impl HeapSlotIter {
    #[inline(always)]
    pub fn new(bits: u64) -> Self {
        Self { bits }
    }
}

impl Iterator for HeapSlotIter {
    type Item = u32;

    #[inline(always)]
    fn next(&mut self) -> Option<u32> {
        if self.bits == 0 {
            return None;
        }
        let idx = self.bits.trailing_zeros();
        self.bits &= self.bits - 1; // clear lowest set bit
        Some(idx)
    }
}

// ── ExecuteData ──────────────────────────────────────────────────────────────

/// RPHP VM call frame.
/// Allocated on the VM stack, layout: [ExecuteData][CV0][CV1]...[TMPn]
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
    /// True if any SendNamed wrote to this call frame.
    /// Used by FastScalar DoFcall to skip the holes check on the hot path.
    /// Only named args can create holes in required params while num_args matches sig.num_args.
    pub named_args_used: bool,
    /// Compact argument-only activation owned by ExecutorGlobals::pending_call_stack.
    /// Such a call has no body CVs/TMPs until a failed scalar guard materializes
    /// the ordinary ABI frame on the main VM stack.
    pub call_kind_flags: u8,
    /// Per-slot heap bitmap: bit N = 1 means slot N currently holds an owned value
    /// (String, Array, Object, Resource, Closure) that needs cleanup or overwrite.
    /// Only valid for frames with <= 64 total slots (CVs + TMPs).
    /// For larger frames, falls back to has_heap_slots + full scan.
    /// Only maintained when has_heap_slots is true (scalar-only frames skip bitmap ops).
    pub heap_bitmap: u64,
}

impl ExecuteData {
    const EMBEDDED_LATE_STATIC_SHIFT: u32 = 32;
    const DEFERRED_SCALAR_CALL: u8 = 1;
    const ORIGINAL_CONSTRUCTOR_CALL: u8 = 1 << 1;
    const DETACHED_STRICT_CALL: u8 = 1 << 2;

    #[inline(always)]
    pub fn is_deferred_scalar_call(&self) -> bool {
        self.call_kind_flags & Self::DEFERRED_SCALAR_CALL != 0
    }

    #[inline(always)]
    pub fn set_deferred_scalar_call(&mut self, enabled: bool) {
        if enabled {
            self.call_kind_flags |= Self::DEFERRED_SCALAR_CALL;
        } else {
            self.call_kind_flags &= !Self::DEFERRED_SCALAR_CALL;
        }
    }

    /// Mark whether this is the exact constructor activation entered by
    /// `new`, rather than a later explicit `->__construct()` call. Frame-local
    /// metadata follows Fiber suspension without a sidecar allocation.
    #[inline(always)]
    pub fn set_original_constructor_call(&mut self, enabled: bool) {
        if enabled {
            self.call_kind_flags |= Self::ORIGINAL_CONSTRUCTOR_CALL;
        } else {
            self.call_kind_flags &= !Self::ORIGINAL_CONSTRUCTOR_CALL;
        }
    }

    #[inline(always)]
    pub fn is_original_constructor_call(&self) -> bool {
        self.call_kind_flags & Self::ORIGINAL_CONSTRUCTOR_CALL != 0
    }

    /// Preserve the caller file's `strict_types` bit on a detached internal
    /// activation whose physical predecessor must remain null.
    #[inline(always)]
    pub fn set_detached_strict_call(&mut self, enabled: bool) {
        if enabled {
            self.call_kind_flags |= Self::DETACHED_STRICT_CALL;
        } else {
            self.call_kind_flags &= !Self::DETACHED_STRICT_CALL;
        }
    }

    #[inline(always)]
    pub fn is_detached_strict_call(&self) -> bool {
        self.call_kind_flags & Self::DETACHED_STRICT_CALL != 0
    }

    /// Recover a late-called class stored in the unused half of the heap
    /// bitmap for compact frames. Slot ownership uses only the low half when
    /// a frame has at most 32 CV/TMP slots.
    #[inline(always)]
    pub unsafe fn embedded_late_static_class_id(&self) -> u32 {
        if self.num_cvs + self.num_temps <= Self::EMBEDDED_LATE_STATIC_SHIFT {
            (self.heap_bitmap >> Self::EMBEDDED_LATE_STATIC_SHIFT) as u32
        } else {
            0
        }
    }

    /// Store a late-called class without growing ExecuteData. Wide frames
    /// return false and use the existing sparse ExecutorGlobals sidecar.
    #[inline(always)]
    pub unsafe fn try_set_embedded_late_static_class_id(&mut self, class_id: u32) -> bool {
        if self.num_cvs + self.num_temps > Self::EMBEDDED_LATE_STATIC_SHIFT {
            return false;
        }
        const LOW_SLOT_MASK: u64 = u32::MAX as u64;
        self.heap_bitmap = (self.heap_bitmap & LOW_SLOT_MASK)
            | ((class_id as u64) << Self::EMBEDDED_LATE_STATIC_SHIFT);
        true
    }

    /// Heap ownership bits with frame-local late-static metadata removed.
    #[inline(always)]
    pub unsafe fn owned_heap_bitmap(&self) -> u64 {
        if self.num_cvs + self.num_temps <= Self::EMBEDDED_LATE_STATIC_SHIFT {
            self.heap_bitmap & u32::MAX as u64
        } else {
            self.heap_bitmap
        }
    }

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
            "cv_mut index {} out of bounds (num_cvs={})",
            idx,
            self.num_cvs
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
                if (*ptr).is_reference() {
                    (*ptr).as_ref_ptr() as *const Value
                } else {
                    ptr
                }
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
                if (*ptr).is_reference() {
                    (*ptr).as_ref_ptr()
                } else {
                    ptr
                }
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
