//! PHP-tailored heap used as the process global allocator.
//!
//! Measured on a cold PHPStan run (24.6 M allocations): 99 % of blocks are
//! at most 1 KiB, 69 % at most 48 B, half of all blocks die before the second
//! following allocation, and 2.5 % are freed by a different thread than the
//! one that allocated them (the parser thread hands its AST to the main
//! thread). The heap is built around those facts:
//!
//! * Size classes tailored to RPHP's value layouts; every class allocates from
//!   one *current page* per thread. A page's own free list is served before
//!   its bump area, so a block freed a moment ago (still in L1) goes out
//!   next and consecutive allocations of a class share one page.
//! * Pages are carved from one reserved address range: ownership and the
//!   page header are a range compare and a mask, no block headers. Small
//!   classes (up to 1 KiB) live on 64 KiB pages in the lower half of the
//!   range, medium classes (up to 32 KiB) on 1 MiB pages in the third
//!   quarter, blocks up to 4 MiB in 64 KiB units in the last quarter. Larger
//!   or over-aligned blocks go to the system allocator, outside the range.
//! * The fast paths touch only the thread's heap block and one page header:
//!   a class without a page points at a read-only sentinel page that can
//!   neither pop nor bump (no null test), the thread caches the range bounds
//!   (no global load on free), and one signed decrement of the page's live
//!   count detects both "the page emptied" and "the page was full" on free.
//! * Cross-thread frees land on a per-page lock-free list; the page queues
//!   itself at its owner, which drains the queue when it needs blocks. The
//!   page state word carries the owner heap pointer plus a queued flag and a
//!   pusher count, so an owner can give a page up (thread exit, page
//!   recycling) without racing a remote free.
//! * Pages count their live blocks; a page that becomes empty and is not the
//!   current page returns to a global pool reusable by any class. Beyond a
//!   resident budget the body of a pooled page (or free large block) goes
//!   back to the OS; such cold entries are reused only after warm ones. The
//!   pools are version-tagged lock-free stacks, immune to ABA.
//! * A finished thread orphans its pages; refills adopt them per class.
//!
//! Setting `RPHP_HEAP=system` in the environment routes everything to the
//! system allocator (A/B measurements without rebuilding).
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub struct PhpHeap;

// These private implementation parts share a lexical module deliberately:
// extracting responsibilities must not add call boundaries or change the
// optimizer's view of layout, ownership, and the allocation fast paths.
include!("layout.rs");

include!("thread.rs");

include!("region.rs");

include!("remote.rs");

include!("pages.rs");

include!("slow.rs");

include!("debug.rs");

#[inline(always)]
fn fast_alloc(size: usize, align: usize) -> *mut u8 {
    let heap = heap();
    if align > 8 || size > MAX_SMALL {
        return alloc_other(size, align, heap, false);
    }
    // SAFETY: `size <= MAX_SMALL` indexes the table, whose classes index
    // `current`.
    let class = unsafe { *CLASS_BY_SIZE.get_unchecked(size) } as usize;
    // SAFETY: the current page is a live page this thread owns, or the
    // sentinel.
    match unsafe { page_alloc(heap.current.get_unchecked(class).get()) } {
        Some(ptr) => ptr,
        None => alloc_slow(size, align, heap, class, false),
    }
}

/// # Safety
/// `GlobalAlloc::dealloc` contract.
#[inline(always)]
unsafe fn fast_dealloc(ptr: *mut u8, size: usize, align: usize) {
    let heap = heap();
    let block = ptr as usize;
    let offset = block.wrapping_sub(heap.base.get());
    if offset < heap.small_limit.get() {
        let page = small_header(heap.base.get(), offset);
        #[cfg(feature = "php-heap-debug")]
        // SAFETY: a pool pointer has an initialized page header.
        debug_check_free(unsafe { &*page }, block, size, align);
        // SAFETY: a small-page pool block, freed once per the contract.
        return unsafe { pool_free_page(block, page, heap) };
    }
    // SAFETY: forwarded contract.
    unsafe { dealloc_other(ptr, size, align, heap) }
}

unsafe impl GlobalAlloc for PhpHeap {
    /// # Safety
    /// `GlobalAlloc::alloc` contract: `layout` has non-zero size.
    #[inline(always)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        fast_alloc(layout.size(), layout.align())
    }

    /// # Safety
    /// `GlobalAlloc::alloc_zeroed` contract.
    #[inline(always)]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let (size, align) = (layout.size(), layout.align());
        if align > 8 || size > MAX_SMALL {
            return alloc_other(size, align, heap(), true);
        }
        let ptr = fast_alloc(size, align);
        if !ptr.is_null() {
            // SAFETY: the block holds at least `size` bytes.
            unsafe { std::ptr::write_bytes(ptr, 0, size) };
        }
        ptr
    }

    /// # Safety
    /// `ptr` was returned by this allocator for `layout` and is freed once.
    #[inline(always)]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded contract.
        unsafe { fast_dealloc(ptr, layout.size(), layout.align()) }
    }

    /// # Safety
    /// `GlobalAlloc::realloc` contract: `ptr`/`layout` as for `dealloc`.
    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let heap = heap();
        let block = ptr as usize;
        let offset = block.wrapping_sub(heap.base.get());
        if offset < heap.small_limit.get() {
            let page = small_header(heap.base.get(), offset);
            #[cfg(feature = "php-heap-debug")]
            // SAFETY: a pool pointer has an initialized page header.
            debug_check_free(unsafe { &*page }, block, layout.size(), layout.align());
            // SAFETY: a small-page pool pointer: its header records the class.
            if class_for(new_size, layout.align()) == Some(unsafe { (*page).class } as usize) {
                return ptr;
            }
            // SAFETY: forwarded contract.
            return unsafe { realloc_copy(ptr, layout, new_size) };
        }
        // SAFETY: forwarded contract.
        unsafe { realloc_other(ptr, layout, new_size) }
    }
}

/// Pages currently waiting in the orphan pool (diagnostics).
pub fn orphaned_pages() -> usize {
    ORPHAN_COUNT.load(Ordering::Relaxed)
}

/// Fully free pages currently in the global pools, resident or reclaimed
/// (diagnostics).
pub fn pooled_pages() -> usize {
    [&SMALL_WARM, &SMALL_COLD, &MEDIUM_WARM, &MEDIUM_COLD]
        .iter()
        .map(|stack| stack.count.load(Ordering::Relaxed))
        .sum()
}

/// Bytes whose memory went back to the OS so far (diagnostics).
pub fn reclaimed_bytes() -> usize {
    RECLAIMED_BYTES.load(Ordering::Relaxed)
}

/// Pool pages carved out of the reservation so far (diagnostics).
pub fn pages_in_use() -> usize {
    let base = RANGE.base.load(Ordering::Relaxed);
    if RANGE.reserve.load(Ordering::Relaxed) == 0 {
        return 0;
    }
    let small = (SMALL_CURSOR.load(Ordering::Relaxed)
        - (base + RANGE.small_start.load(Ordering::Relaxed)))
        / SMALL_PAGE_SIZE;
    let medium_start =
        (base + RANGE.half.load(Ordering::Relaxed) + MEDIUM_PAGE_SIZE - 1) & MEDIUM_PAGE_MASK;
    let medium = (MEDIUM_CURSOR.load(Ordering::Relaxed) - medium_start) / MEDIUM_PAGE_SIZE;
    small + medium
}

#[cfg(test)]
mod tests;
