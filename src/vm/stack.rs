use std::mem::size_of;

use crate::value::Value;
use super::frame::{ExecuteData, CALL_FRAME_SLOTS};
use super::function::{FunctionCommon, FunctionType};

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
        let (num_cvs, num_temps) = match common.fn_type {
            FunctionType::User => {
                // Direct cast — avoids Function wrapper + dispatch overhead
                let user = unsafe { &*(func as *const super::function::UserFunction) };
                (user.op_array.num_cvs as usize, user.op_array.num_temps as usize)
            }
            _ => {
                let base = common.num_args as usize;
                let extra = if common.is_variadic { 1 } else { 0 };
                (base + extra, 0usize)
            }
        };
        // Allocate max(num_args, num_cvs) so that extra arguments
        // don't write past the frame before DoFcall validates the count.
        let effective_cvs = std::cmp::max(num_args as usize, num_cvs);
        let total_slots = CALL_FRAME_SLOTS + effective_cvs + num_temps;
        let needed = total_slots * size_of::<Value>();

        let available = unsafe { self.end.offset_from(self.top) } as usize * size_of::<Value>();
        if needed > available {
            self.extend(needed);
        }

        let frame = self.top as *mut ExecuteData;
        self.top = unsafe { self.top.add(total_slots) };

        // Initialize frame header
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
        }

        // Zero-init all CV+TMP slots to UNDEF — Value::undef() is all-zero bytes.
        // Using write_bytes is faster than per-slot loop for frames with many slots.
        let slot_count = effective_cvs + num_temps;
        if slot_count > 0 {
            let cv_base = unsafe { (frame as *mut u8).add(CALL_FRAME_SLOTS * size_of::<Value>()) };
            unsafe { std::ptr::write_bytes(cv_base, 0, slot_count * size_of::<Value>()) };
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
