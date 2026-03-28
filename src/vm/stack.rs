use std::mem::size_of;

use crate::value::Value;
use crate::vm::stats;
use super::frame::{ExecuteData, CALL_FRAME_SLOTS};
use super::function::FunctionCommon;

const DEFAULT_STACK_PAGE_SIZE: usize = 256 * 1024; // 256 KB

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
        let page_size = DEFAULT_STACK_PAGE_SIZE;
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

    /// Allocate a call frame on the stack.
    #[inline]
    pub fn push_call_frame(
        &mut self,
        func: *const FunctionCommon,
        num_args: u32,
    ) -> *mut ExecuteData {
        let common = unsafe { &*func };
        let declared_cvs = common.frame.num_cvs as usize;
        let num_temps = common.frame.num_temps as usize;

        // Compute frame geometry: effective CV count and total slot count.
        // The common case is num_args <= declared CVs. The wider frame is
        // only needed for extra-arg error paths.
        let effective_cvs = if (num_args as usize) <= declared_cvs {
            declared_cvs
        } else {
            num_args as usize
        };
        let total_slots = CALL_FRAME_SLOTS + effective_cvs + num_temps;
        let needed = total_slots * size_of::<Value>();

        let available = unsafe { self.end.offset_from(self.top) } as usize * size_of::<Value>();
        if needed > available {
            self.extend(needed);
        }

        let frame = self.top as *mut ExecuteData;
        self.top = unsafe { self.top.add(total_slots) };

        // Initialize frame header.
        unsafe {
            (*frame).func = func;
            (*frame).opline = std::ptr::null();
            (*frame).call = std::ptr::null_mut();
            (*frame).return_value = std::ptr::null_mut();
            (*frame).prev_execute_data = std::ptr::null_mut();
            (*frame).num_args = num_args;
            (*frame).num_cvs = effective_cvs as u32;
            (*frame).num_temps = num_temps as u32;
            (*frame).pending_return_after_finally = false;
            (*frame).has_heap_slots = false;
            (*frame).named_args_used = false;
            (*frame).heap_bitmap = 0;
        }

        // Zero-init only CV slots beyond argument count. TMPs NOT zeroed.
        // Arg slots (0..num_args) are left uninitialized — written by SendVal before DoFcall.
        // CVs beyond args are set to Undef (zeroed) so BindDefaultParam can check for Undef.
        let zero_start = num_args as usize;
        let zero_end = effective_cvs;
        let zero_count = zero_end.saturating_sub(zero_start);

        stats::inc_push_call_frame(zero_count, zero_count * size_of::<Value>());

        if zero_count > 0 {
            let cv_base = unsafe {
                (frame as *mut u8).add((CALL_FRAME_SLOTS + zero_start) * size_of::<Value>())
            };
            unsafe { std::ptr::write_bytes(cv_base, 0, zero_count * size_of::<Value>()) };
            // Zeroed CVs are NOT marked in init_bitmap. They contain Undef (safe to read)
            // but are not "passed arguments". Named arg duplicate detection uses is_init
            // to distinguish "argument was provided" from "slot has default Undef".
        }

        frame
    }

    /// Pop call frame — reset stack top to frame start
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
            unsafe { std::alloc::dealloc(page as *mut u8, layout); }
            page = prev;
        }
    }
}
