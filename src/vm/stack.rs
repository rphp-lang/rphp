use std::mem::size_of;

use super::frame::{CALL_FRAME_SLOTS, ExecuteData};
use super::function::FunctionCommon;
use crate::value::Value;
use crate::vm::stats;

const DEFAULT_STACK_PAGE_SIZE: usize = 256 * 1024; // 256 KB
const PENDING_STACK_PAGE_SIZE: usize = 16 * 1024;

/// VM stack page. Later pages remain available for reuse after an unwind.
#[repr(C, align(16))]
struct VmStackPage {
    prev: *mut VmStackPage,
    next: *mut VmStackPage,
    // The charge already owns the immutable page size, including before a
    // request budget is installed. Avoid duplicating it in the page header.
    allocation: crate::request_memory::Allocation,
    // data follows after header
}

// Both the first frame and the page end must lie on the Value-slot grid for
// the inlined capacity calculation below.
const _: () = assert!(size_of::<VmStackPage>().is_multiple_of(size_of::<Value>()));

/// VM stack — bump allocator for call frames.
/// Grows by allocating new pages when needed.
pub struct VmStack {
    top: *mut Value,
    end: *mut Value,
    current_page: *mut VmStackPage,
    page_size: usize,
}

impl VmStack {
    /// Initial request stacks exist before execution installs the budget.
    /// Adopt them on entry; subsequent pages charge before allocating.
    pub(crate) fn account_request(&mut self) {
        let mut page = self.current_page;
        // SAFETY: this stack exclusively owns its live, acyclic page chain.
        unsafe {
            while !(*page).prev.is_null() {
                page = (*page).prev;
            }
            while let Some(header) = page.as_mut() {
                header.allocation.grow_to(header.allocation.bytes());
                page = header.next;
            }
        }
    }

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
                call_kind_flags: 1,
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
                call_kind_flags: 0,
                heap_bitmap: 0,
            });
        }

        // Zero-init CV slots beyond argument count. Small-frame TMPs are
        // protected by the heap bitmap and may retain arbitrary stack bytes;
        // large frames retain initialized TMPs for tail and cold-path scans.
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

    /// Pop a call frame, restoring its page bounds before the next push.
    #[inline(always)]
    pub fn pop_call_frame(&mut self, frame: *mut ExecuteData) {
        // One unsigned range test handles addresses below and above this
        // allocation. A preceding page wraps past page_len; ordinary returns
        // keep a single conditional branch before resetting the bump pointer.
        let page_base = self.current_page as usize;
        let page_len = self.end as usize - page_base;
        if (frame as usize).wrapping_sub(page_base) >= page_len {
            self.rewind_to_page(frame);
        }
        self.top = frame as *mut Value;
    }

    #[cold]
    #[inline(never)]
    fn rewind_to_page(&mut self, frame: *mut ExecuteData) {
        // SAFETY: callers retire a frame belonging to this stack in LIFO
        // order. Every preceding page remains allocated; compare addresses
        // before deriving top/end pointers from the matching allocation.
        unsafe {
            let mut page = (*self.current_page).prev;
            while let Some(header) = page.as_ref() {
                let start = page as usize + size_of::<VmStackPage>();
                let end = page as usize + header.allocation.bytes();
                if (start..end).contains(&(frame as usize)) {
                    self.current_page = page;
                    self.end = (page as *mut u8).add(header.allocation.bytes()).cast();
                    return;
                }
                page = header.prev;
            }
        }
        panic!("call frame does not belong to a preceding VM stack page");
    }

    // Page growth is rare even for call-heavy programs. Keep selection and
    // allocation outside the inlined frame push and the main VM dispatcher.
    #[cold]
    #[inline(never)]
    fn extend(&mut self, needed: usize) {
        let required = needed
            .checked_add(size_of::<VmStackPage>())
            .expect("VM stack page size overflow");
        // SAFETY: pages after current_page contain only retired frames.
        // Reuse a sufficiently large page, or append one initialized by
        // alloc_page. Both links stay within this stack's acyclic chain.
        unsafe {
            let mut previous = self.current_page;
            let mut page = (*previous).next;
            while !page.is_null() && (*page).allocation.bytes() < required {
                previous = page;
                page = (*page).next;
            }
            if page.is_null() {
                page = Self::alloc_page(self.page_size.max(required));
                (*page).prev = previous;
                (*previous).next = page;
            }
            self.current_page = page;
            self.top = (page as *mut u8).add(size_of::<VmStackPage>()).cast();
            self.end = (page as *mut u8).add((*page).allocation.bytes()).cast();
        }
    }

    fn alloc_page(size: usize) -> *mut VmStackPage {
        let allocation = crate::request_memory::Allocation::new(size);
        let layout = std::alloc::Layout::from_size_align(size, 4096).unwrap();
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            panic!("VM stack allocation failed");
        }
        unsafe {
            (ptr as *mut VmStackPage).write(VmStackPage {
                prev: std::ptr::null_mut(),
                next: std::ptr::null_mut(),
                allocation,
            });
        }
        ptr as *mut VmStackPage
    }
}

impl Drop for VmStack {
    fn drop(&mut self) {
        let mut page = self.current_page;
        // SAFETY: the entire bidirectional chain is exclusively owned by this
        // stack, including reusable pages after current_page. Read each next
        // link while its header is live and release every allocation once.
        unsafe {
            while !(*page).prev.is_null() {
                page = (*page).prev;
            }
            while !page.is_null() {
                let next = (*page).next;
                let allocation_size = (*page).allocation.bytes();
                let layout = std::alloc::Layout::from_size_align(allocation_size, 4096).unwrap();
                std::ptr::drop_in_place(page);
                std::alloc::dealloc(page as *mut u8, layout);
                page = next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::ExecutorGlobals;
    use crate::vm::execute::VmError;
    use std::ptr::null_mut;

    fn empty(_: *mut ExecuteData, _: *mut Value, _: &mut ExecutorGlobals) -> Result<(), VmError> {
        Ok(())
    }

    #[test]
    fn cross_page_pop_restores_bounds_and_reuses_full_and_pending_pages() {
        let function = crate::compiler::make_internal_function(empty, 0, 0, vec![]);
        for deferred in [false, true] {
            let mut stack = VmStack::with_page_size(256);
            let first_page = stack.current_page;
            let first_end = stack.end;
            let mut retained_child = null_mut();
            for round in 0..20 {
                let mut push = |args, parent| {
                    if deferred {
                        stack.push_deferred_scalar_call(
                            &function.common,
                            args,
                            args,
                            parent,
                            null_mut(),
                        )
                    } else {
                        stack.push_call_frame(&function.common, args, args, parent, null_mut())
                    }
                };
                let outer = push(0, null_mut());
                let child = push(64, outer);
                assert_ne!(stack.current_page, first_page);
                if round == 0 {
                    retained_child = child;
                }
                assert_eq!(child, retained_child, "retired pages must be reused");
                stack.pop_call_frame(child);
                stack.pop_call_frame(outer);
                assert_eq!(stack.current_page, first_page);
                assert_eq!(stack.end, first_end);
                assert_eq!(stack.top, outer.cast());
            }
        }
    }

    #[test]
    fn oversized_reuse_skips_small_pages_and_can_rewind_past_them() {
        let function = crate::compiler::make_internal_function(empty, 0, 0, vec![]);
        let mut stack = VmStack::with_page_size(256);
        let first_page = stack.current_page;
        let outer = stack.push_call_frame(&function.common, 0, 0, null_mut(), null_mut());
        let middle = stack.push_call_frame(&function.common, 64, 64, outer, null_mut());
        let large = stack.push_call_frame(&function.common, 256, 256, middle, null_mut());
        stack.pop_call_frame(large);
        stack.pop_call_frame(middle);
        stack.pop_call_frame(outer);
        let outer = stack.push_call_frame(&function.common, 0, 0, null_mut(), null_mut());
        let reused = stack.push_call_frame(&function.common, 256, 256, outer, null_mut());
        assert_eq!(reused, large);
        stack.pop_call_frame(reused);
        stack.pop_call_frame(outer);
        assert_eq!(stack.current_page, first_page);
    }

    #[test]
    fn retained_pages_keep_their_size_when_adopting_a_request_budget() {
        let function = crate::compiler::make_internal_function(empty, 0, 0, vec![]);
        let mut stack = VmStack::with_page_size(256);
        let outer = stack.push_call_frame(&function.common, 0, 0, null_mut(), null_mut());
        let child = stack.push_call_frame(&function.common, 64, 64, outer, null_mut());
        let expected =
            256 + size_of::<VmStackPage>() + (CALL_FRAME_SLOTS + 64) * size_of::<Value>();
        stack.pop_call_frame(child);
        stack.pop_call_frame(outer);

        let budget = crate::request_memory::Budget::default();
        let _scope = budget.enter();
        stack.account_request();
        assert_eq!(budget.usage(), expected);
        let outer = stack.push_call_frame(&function.common, 0, 0, null_mut(), null_mut());
        let reused = stack.push_call_frame(&function.common, 64, 64, outer, null_mut());
        assert_eq!(child, reused);
        stack.account_request();
        assert_eq!(budget.usage(), expected, "page adoption must be idempotent");
        stack.pop_call_frame(reused);
        stack.pop_call_frame(outer);
        drop(stack);
        assert_eq!(
            budget.usage(),
            0,
            "release retained pages as well as active ones"
        );
    }
}
