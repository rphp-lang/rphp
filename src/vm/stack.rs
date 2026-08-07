use std::mem::size_of;

use super::frame::{CALL_FRAME_SLOTS, ExecuteData};
use super::function::FunctionCommon;
use crate::value::Value;
use crate::vm::stats;

const DEFAULT_STACK_PAGE_SIZE: usize = 256 * 1024; // 256 KB
const PENDING_STACK_PAGE_SIZE: usize = 16 * 1024;

/// VM stack page — linked list of pages
struct VmStackPage {
    prev: *mut VmStackPage,
    // data follows after header
}

/// VM stack — bump allocator for call frames.
/// Grows by allocating new pages when needed.
pub struct VmStack {
    top: *mut Value,
    end: *mut Value,
    current_page: *mut VmStackPage,
    page_size: usize,
}

impl VmStack {
    pub fn new() -> Self {
        Self::with_page_size(DEFAULT_STACK_PAGE_SIZE)
    }

    /// Smaller bump stack used by compact argument-only call activations.
    pub fn new_pending() -> Self {
        Self::with_page_size(PENDING_STACK_PAGE_SIZE)
    }

    fn with_page_size(page_size: usize) -> Self {
        let page = Self::alloc_page(page_size);

        let top = unsafe { (page as *mut u8).add(size_of::<VmStackPage>()) as *mut Value };
        let end = unsafe { (page as *mut u8).add(page_size) as *mut Value };

        Self {
            top,
            end,
            current_page: page,
            page_size,
        }
    }

    /// Allocate only the ExecuteData header and already-declared argument slots.
    /// The function body is never entered through this activation; DoFcall either
    /// evaluates its scalar plan or materializes a full frame on the main stack.
    #[inline(always)]
    pub fn push_deferred_scalar_call(
        &mut self,
        func: *const FunctionCommon,
        storage_num_args: u32,
        public_num_args: u32,
        prev_execute_data: *mut ExecuteData,
        pending_call: *mut ExecuteData,
    ) -> *mut ExecuteData {
        let total_slots = CALL_FRAME_SLOTS + storage_num_args as usize;
        let needed = total_slots * size_of::<Value>();
        let available = unsafe { self.end.offset_from(self.top) } as usize * size_of::<Value>();
        if needed > available {
            self.extend(needed);
        }

        let frame = self.top as *mut ExecuteData;
        self.top = unsafe { self.top.add(total_slots) };
        unsafe {
            frame.write(ExecuteData {
                opline: std::ptr::null(),
                call: pending_call,
                return_value: std::ptr::null_mut(),
                func,
                prev_execute_data,
                num_args: public_num_args,
                num_cvs: storage_num_args,
                num_temps: 0,
                pending_return_after_finally: false,
                has_heap_slots: false,
                named_args_used: false,
                deferred_scalar_call: true,
                heap_bitmap: 0,
            });
        }
        frame
    }

    /// Allocate a call frame on the stack.
    #[inline(always)]
    pub fn push_call_frame(
        &mut self,
        func: *const FunctionCommon,
        storage_num_args: u32,
        public_num_args: u32,
        prev_execute_data: *mut ExecuteData,
        pending_call: *mut ExecuteData,
    ) -> *mut ExecuteData {
        let common = unsafe { &*func };
        let declared_cvs = common.frame.num_cvs as usize;
        let num_temps = common.frame.num_temps as usize;

        // Compute frame geometry: effective CV count and total slot count.
        // The common case is storage_num_args <= declared CVs. The wider
        // frame is only needed for extra-arg error paths.
        let effective_cvs = if (storage_num_args as usize) <= declared_cvs {
            declared_cvs
        } else {
            storage_num_args as usize
        };
        let total_slots = CALL_FRAME_SLOTS + effective_cvs + num_temps;
        let needed = total_slots * size_of::<Value>();

        let available = unsafe { self.end.offset_from(self.top) } as usize * size_of::<Value>();
        if needed > available {
            self.extend(needed);
        }

        let frame = self.top as *mut ExecuteData;
        self.top = unsafe { self.top.add(total_slots) };

        // Initialize every header field with its final value. Keeping storage
        // geometry separate from public arity handles hidden method `$this`
        // and closure captures without fixing up the header after allocation.
        unsafe {
            frame.write(ExecuteData {
                opline: std::ptr::null(),
                call: pending_call,
                return_value: std::ptr::null_mut(),
                func,
                prev_execute_data,
                num_args: public_num_args,
                num_cvs: effective_cvs as u32,
                num_temps: num_temps as u32,
                pending_return_after_finally: false,
                has_heap_slots: false,
                named_args_used: false,
                deferred_scalar_call: false,
                heap_bitmap: 0,
            });
        }

        // Zero-init CV slots beyond argument count. Small-frame TMPs are
        // protected by the heap bitmap and may retain arbitrary stack bytes;
        // large frames have no per-slot bitmap, so initialize their TMPs too.
        // Arg-storage slots (0..storage_num_args) are left uninitialized —
        // written by SendVal or hidden-value binding before DoFcall.
        // CVs beyond args are set to Undef (zeroed) so BindDefaultParam can check for Undef.
        let zero_start = storage_num_args as usize;
        let zero_end = effective_cvs;
        let zero_count = zero_end.saturating_sub(zero_start);

        let temp_zero_count = if effective_cvs + num_temps > 64 {
            num_temps
        } else {
            0
        };
        let initialized_count = zero_count + temp_zero_count;
        stats::inc_push_call_frame(initialized_count, initialized_count * size_of::<Value>());

        if zero_count > 0 {
            let cv_base = unsafe {
                (frame as *mut u8).add((CALL_FRAME_SLOTS + zero_start) * size_of::<Value>())
            };
            unsafe { std::ptr::write_bytes(cv_base, 0, zero_count * size_of::<Value>()) };
            // Zeroed CVs are NOT marked in init_bitmap. They contain Undef (safe to read)
            // but are not "passed arguments". Named arg duplicate detection uses is_init
            // to distinguish "argument was provided" from "slot has default Undef".
        }

        if temp_zero_count > 0 {
            let tmp_base = unsafe {
                (frame as *mut u8).add((CALL_FRAME_SLOTS + effective_cvs) * size_of::<Value>())
            };
            unsafe { std::ptr::write_bytes(tmp_base, 0, temp_zero_count * size_of::<Value>()) };
        }

        frame
    }

    /// Pop call frame — reset stack top to frame start
    #[inline(always)]
    pub fn pop_call_frame(&mut self, frame: *mut ExecuteData) {
        self.top = frame as *mut Value;
    }

    fn extend(&mut self, _needed: usize) {
        let page = Self::alloc_page(self.page_size);
        unsafe {
            (*(page)).prev = self.current_page;
        }
        self.current_page = page;
        self.top = unsafe { (page as *mut u8).add(size_of::<VmStackPage>()) as *mut Value };
        self.end = unsafe { (page as *mut u8).add(self.page_size) as *mut Value };
    }

    fn alloc_page(size: usize) -> *mut VmStackPage {
        let layout = std::alloc::Layout::from_size_align(size, 4096).unwrap();
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            panic!("VM stack allocation failed");
        }
        unsafe {
            (*(ptr as *mut VmStackPage)).prev = std::ptr::null_mut();
        }
        ptr as *mut VmStackPage
    }
}

impl Drop for VmStack {
    fn drop(&mut self) {
        let mut page = self.current_page;
        while !page.is_null() {
            let prev = unsafe { (*page).prev };
            let layout = std::alloc::Layout::from_size_align(self.page_size, 4096).unwrap();
            unsafe {
                std::alloc::dealloc(page as *mut u8, layout);
            }
            page = prev;
        }
    }
}
