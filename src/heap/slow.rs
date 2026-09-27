/// Cold continuation of the allocation fast path: the class has no current
/// page with room. Decides the mode on first use, refills the class, and
/// falls back to the system allocator when the pool is off or exhausted.
#[cold]
#[inline(never)]
fn alloc_slow(size: usize, align: usize, heap: &ThreadHeap, class: usize, zeroed: bool) -> *mut u8 {
    let mut mode = heap.mode.get();
    if mode == 0 {
        mode = init_thread(heap);
    }
    if mode == 1 {
        let ptr = refill(heap, class);
        if !ptr.is_null() {
            if zeroed {
                // SAFETY: the block holds at least `size` bytes.
                unsafe { std::ptr::write_bytes(ptr, 0, size) };
            }
            return ptr;
        }
    }
    // SAFETY: `size`/`align` come from a valid `Layout`.
    unsafe {
        let layout = Layout::from_size_align_unchecked(size, align);
        if zeroed {
            System.alloc_zeroed(layout)
        } else {
            System.alloc(layout)
        }
    }
}

/// Requests the small fast path does not take: 16-aligned small blocks,
/// medium blocks, large and over-aligned ones.
#[cold]
#[inline(never)]
fn alloc_other(size: usize, align: usize, heap: &ThreadHeap, zeroed: bool) -> *mut u8 {
    let Some(class) = class_for(size, align) else {
        // SAFETY: `size`/`align` come from a valid `Layout`.
        return unsafe { system_alloc(size, align, zeroed) };
    };
    // SAFETY: `class < NUM_CLASSES`; the current page is a live page this
    // thread owns, or the sentinel.
    let Some(ptr) = (unsafe { page_alloc(heap.current.get_unchecked(class).get()) }) else {
        return alloc_slow(size, align, heap, class, zeroed);
    };
    if zeroed {
        // SAFETY: the block holds at least `size` bytes.
        unsafe { std::ptr::write_bytes(ptr, 0, size) };
    }
    ptr
}

/// Owner-local free whose signed decrement fired: the page emptied (an
/// available page) or was retired (full when it stopped being current).
///
/// # Safety
/// `page` is a live page this thread owns; the freed block is already back
/// on its local list and counted off `used`.
#[cold]
#[inline(never)]
unsafe fn free_cold(page: *mut PageHeader, heap: &ThreadHeap) {
    // SAFETY: per the contract.
    unsafe {
        let used = (*page).used.get();
        if used & RETIRED != 0 {
            let live = used & !RETIRED;
            (*page).used.set(live);
            if live == 0 {
                recycle_or_list(heap, page);
            } else {
                heap.push_avail(page);
            }
        } else {
            // Current pages carry the bias, so this is an available page.
            debug_assert!(used == 0 && (*page).listing.get() == LISTING_AVAIL);
            heap.remove_avail(page);
            recycle_or_list(heap, page);
        }
    }
}

/// Free a pool block of `page`: onto the local list when this thread owns
/// the page, onto its remote list otherwise.
///
/// # Safety
/// `block` is a live block of the small or medium page `page`, freed once.
#[inline(always)]
unsafe fn pool_free_page(block: usize, page: *mut PageHeader, heap: &ThreadHeap) {
    // SAFETY: a pool pointer's page header is initialized.
    let header = unsafe { &*page };
    if header.owner.load(Ordering::Relaxed) != heap.addr() {
        // SAFETY: per the contract.
        return unsafe { remote_free(block, header) };
    }
    #[cfg(feature = "php-heap-debug")]
    // SAFETY: the block is ours and `block_size` bytes long.
    unsafe {
        debug_mark_free(header, block)
    };
    // SAFETY: the block is ours and free; its first word becomes the link.
    unsafe { *(block as *mut usize) = header.local_free.get() };
    header.local_free.set(block);
    let used = header.used.get().wrapping_sub(1);
    header.used.set(used);
    if (used as i32) <= 0 {
        // SAFETY: owner-only bookkeeping of a page that emptied or retired.
        unsafe { free_cold(page, heap) };
    }
}

/// Frees the small fast path does not take: medium and large pool blocks,
/// system blocks, and any free before the thread cached the range bounds.
///
/// # Safety
/// `ptr` was returned by this allocator for a layout of `size`/`align` and
/// is freed once.
#[cold]
#[inline(never)]
unsafe fn dealloc_other(ptr: *mut u8, size: usize, align: usize, heap: &ThreadHeap) {
    let reserve = RANGE.reserve.load(Ordering::Acquire);
    let offset = pool_offset(ptr as usize);
    if offset >= reserve {
        // SAFETY: outside the reservation: the block came from `System`.
        return unsafe { System.dealloc(ptr, Layout::from_size_align_unchecked(size, align)) };
    }
    if heap.base.get() == 0 {
        // Let this thread's next frees take the fast path.
        heap.base.set(RANGE.base.load(Ordering::Acquire));
        heap.small_limit.set(RANGE.half.load(Ordering::Acquire));
    }
    if is_large_offset(offset) {
        // SAFETY: large-region pointers come from `alloc_large`.
        return unsafe { free_large(ptr) };
    }
    let page = page_header_at(ptr as usize, offset);
    #[cfg(feature = "php-heap-debug")]
    // SAFETY: a pool pointer has an initialized page header.
    debug_check_free(unsafe { &*page }, ptr as usize, size, align);
    // SAFETY: per the contract.
    unsafe { pool_free_page(ptr as usize, page, heap) }
}

/// # Safety
/// `size`/`align` come from a valid `Layout`.
#[cold]
#[inline(never)]
unsafe fn system_alloc(size: usize, align: usize, zeroed: bool) -> *mut u8 {
    if align <= MAX_ALIGN && size <= LARGE_MAX && process_mode() == 1 {
        let ptr = alloc_large(size, zeroed);
        if !ptr.is_null() {
            return ptr;
        }
    }
    // SAFETY: forwarded verbatim.
    unsafe {
        let layout = Layout::from_size_align_unchecked(size, align);
        if zeroed {
            System.alloc_zeroed(layout)
        } else {
            System.alloc(layout)
        }
    }
}

/// Move a pool block into a block of another size.
///
/// # Safety
/// `GlobalAlloc::realloc` contract for a pool pointer.
#[cold]
#[inline(never)]
unsafe fn realloc_copy(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    // SAFETY: standard grow-by-copy; both layouts are valid.
    unsafe {
        let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
        let new_ptr = PhpHeap.alloc(new_layout);
        if !new_ptr.is_null() {
            std::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
            PhpHeap.dealloc(ptr, layout);
        }
        new_ptr
    }
}

/// `realloc` of anything but a small-page block.
///
/// # Safety
/// `GlobalAlloc::realloc` contract.
#[cold]
#[inline(never)]
unsafe fn realloc_other(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    let reserve = RANGE.reserve.load(Ordering::Acquire);
    let offset = pool_offset(ptr as usize);
    if offset >= reserve {
        // A system block stays with the system allocator; keeping origins
        // separate keeps `dealloc` a range check.
        // SAFETY: forwarded verbatim.
        return unsafe { System.realloc(ptr, layout, new_size) };
    }
    if is_large_offset(offset) {
        // SAFETY: large-region pointer from `alloc_large`.
        let capacity = unsafe { large_capacity(ptr) };
        // Keep the block while the new size still needs the large region
        // and uses more than half of it.
        if new_size > MAX_POOL && new_size <= capacity && new_size * 2 > capacity {
            return ptr;
        }
        // SAFETY: forwarded verbatim.
        return unsafe { realloc_copy(ptr, layout, new_size) };
    }
    let page = page_header_at(ptr as usize, offset);
    #[cfg(feature = "php-heap-debug")]
    // SAFETY: a pool pointer has an initialized page header.
    debug_check_free(
        unsafe { &*page },
        ptr as usize,
        layout.size(),
        layout.align(),
    );
    // SAFETY: a pool pointer's header records its class.
    if class_for(new_size, layout.align()) == Some(unsafe { (*page).class } as usize) {
        return ptr;
    }
    // SAFETY: forwarded verbatim.
    unsafe { realloc_copy(ptr, layout, new_size) }
}
