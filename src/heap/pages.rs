/// Initialize the header entry of `page` for `class`. The stack link past
/// the header struct is left alone.
///
/// # Safety
/// `page` is a page-aligned address inside the right region of the
/// reservation that no other thread references.
#[inline]
unsafe fn init_page(page: usize, class: usize) -> *mut PageHeader {
    let header = header_of(page);
    let page_size = if class < NUM_SMALL_CLASSES {
        SMALL_PAGE_SIZE
    } else {
        MEDIUM_PAGE_SIZE
    };
    // Debug mode: the page's free bits belong to its previous layout.
    #[cfg(feature = "php-heap-debug")]
    debug_clear_range(page, page_size);
    // SAFETY: per the contract the header entry is ours; every field is
    // written before the page becomes reachable.
    unsafe {
        header.write(PageHeader {
            local_free: Cell::new(0),
            bump: Cell::new(page + page_color(page, page_size)),
            end: page + page_size,
            used: Cell::new(0),
            block_size: CLASS_SIZES[class],
            owner: AtomicUsize::new(0),
            class: class as u32,
            listing: Cell::new(LISTING_NONE),
            next_avail: Cell::new(std::ptr::null_mut()),
            prev_avail: Cell::new(std::ptr::null_mut()),
            owner_state: AtomicUsize::new(0),
            remote_free: AtomicUsize::new(0),
            next_queued: AtomicUsize::new(0),
            next_page: Cell::new(std::ptr::null_mut()),
            prev_page: Cell::new(std::ptr::null_mut()),
            start: page,
        });
    }
    header
}

/// A page for `class`: a resident pooled one, a reclaimed pooled one, or a
/// fresh one carved from the reservation. Null when the region is
/// exhausted. The reservation exists (the thread runs in pool mode).
#[cold]
fn new_page(class: usize) -> *mut PageHeader {
    let small = class < NUM_SMALL_CLASSES;
    let (warm, cold) = if small {
        (&SMALL_WARM, &SMALL_COLD)
    } else {
        (&MEDIUM_WARM, &MEDIUM_COLD)
    };
    let mut entry = warm.pop();
    if entry == 0 {
        entry = cold.pop();
    }
    if entry == 0 {
        let base = RANGE.base.load(Ordering::Acquire);
        entry = if small {
            carve(
                &SMALL_CURSOR,
                SMALL_PAGE_SIZE,
                base + RANGE.half.load(Ordering::Acquire),
            )
        } else {
            carve(
                &MEDIUM_CURSOR,
                MEDIUM_PAGE_SIZE,
                base + RANGE.large_start.load(Ordering::Acquire),
            )
        };
        if entry == 0 {
            return std::ptr::null_mut();
        }
    }
    // SAFETY: popped or carved, so exclusively ours.
    unsafe { init_page(entry, class) }
}

/// Put a detached, fully free page nobody references into its region's
/// pool. Beyond the resident budget its memory goes back to the OS (the
/// header entry, with the stack link, stays).
///
/// # Safety
/// `page` is the header of a live pool page with no owner and no live
/// blocks.
#[cold]
unsafe fn pool_page(page: *mut PageHeader) {
    // SAFETY: per the contract.
    let entry = unsafe { (*page).start };
    let (warm, cold, budget, page_size) = if is_small_page(entry) {
        (
            &SMALL_WARM,
            &SMALL_COLD,
            SMALL_POOL_RESIDENT,
            SMALL_PAGE_SIZE,
        )
    } else {
        (
            &MEDIUM_WARM,
            &MEDIUM_COLD,
            MEDIUM_POOL_RESIDENT,
            MEDIUM_PAGE_SIZE,
        )
    };
    // SAFETY: per the contract the page is ours alone and holds nothing.
    unsafe {
        if warm.count.load(Ordering::Relaxed) >= budget && reclaim(entry, entry + page_size) {
            cold.push(entry);
        } else {
            warm.push(entry);
        }
    }
}

/// Give an empty, unlisted, non-current page back to the global pool.
/// Returns false (leaving the page owned) when a remote freer has queued it
/// in the meantime; the next drain retries.
///
/// # Safety
/// `page` is a live page owned by this thread with `used == 0` and
/// `listing == LISTING_NONE`.
#[cold]
#[inline(never)]
unsafe fn recycle_page(heap: &ThreadHeap, page: *mut PageHeader) -> bool {
    // SAFETY: per the contract.
    unsafe {
        if !try_detach(&*page) {
            return false;
        }
        heap.unlink_page(page);
        pool_page(page);
        true
    }
}

/// Recycle an empty page, or keep it listed as available when a remote
/// freer holds it queued.
///
/// # Safety
/// As for `recycle_page`.
#[cold]
unsafe fn recycle_or_list(heap: &ThreadHeap, page: *mut PageHeader) {
    // SAFETY: per the contract.
    unsafe {
        if !recycle_page(heap, page) {
            heap.push_avail(page);
        }
    }
}

/// Make `page` this heap's current page of its class (owner). The previous
/// current page is exhausted: it loses its bias and retires.
///
/// # Safety
/// `page` is a live page owned by (or being taken by) this thread, not
/// listed and not retired.
#[inline]
unsafe fn set_current(heap: &ThreadHeap, page: *mut PageHeader) {
    // SAFETY: per the contract; the previous current page is ours too.
    unsafe {
        let class = (*page).class as usize;
        let previous = heap.current.get_unchecked(class).replace(page);
        if previous != sentinel() {
            let live = (*previous).used.get() - CURRENT_BIAS;
            (*previous).listing.set(LISTING_NONE);
            if live == 0 {
                // Unreachable for an exhausted page, but a page without a
                // live block must not retire, or no free would relist it.
                (*previous).used.set(0);
                recycle_or_list(heap, previous);
            } else {
                (*previous).used.set(live | RETIRED);
            }
        }
        (*page).used.set((*page).used.get() + CURRENT_BIAS);
        (*page).listing.set(LISTING_CURRENT);
    }
}

/// Pop a block off `page`'s free list, or bump one (owner). `None` when the
/// page has neither; always `None` for the sentinel.
///
/// # Safety
/// `page` is a live page this thread owns, or the sentinel.
#[inline(always)]
unsafe fn page_alloc(page: *mut PageHeader) -> Option<*mut u8> {
    // SAFETY: per the contract; a free block keeps its successor in its
    // first word. The sentinel is only read: its list is empty and its bump
    // cursor lies past its end.
    unsafe {
        let head = (*page).local_free.get();
        if head != 0 {
            (*page).local_free.set(*(head as *const usize));
            (*page).used.set((*page).used.get() + 1);
            #[cfg(feature = "php-heap-debug")]
            debug_mark_taken(&*page, head, true);
            return Some(head as *mut u8);
        }
        let bump = (*page).bump.get();
        let next = bump + (*page).block_size as usize;
        if next <= (*page).end {
            (*page).bump.set(next);
            (*page).used.set((*page).used.get() + 1);
            #[cfg(feature = "php-heap-debug")]
            debug_mark_taken(&*page, bump, false);
            return Some(bump as *mut u8);
        }
    }
    None
}

/// Take an orphaned page of `class`, or null.
fn adopt_orphan(class: usize) -> *mut PageHeader {
    if ORPHAN_COUNT.load(Ordering::Relaxed) == 0 {
        return std::ptr::null_mut();
    }
    let mut orphans = ORPHANS.lock().unwrap_or_else(|poison| poison.into_inner());
    let page = orphans.0[class];
    if !page.is_null() {
        // SAFETY: orphan pages are live pool pages nobody owns.
        orphans.0[class] = unsafe { (*page).next_avail.get() };
        ORPHAN_COUNT.fetch_sub(1, Ordering::Relaxed);
    }
    page
}

/// Slow path: the current page of `class` has neither free blocks nor bump
/// space. Find another source and allocate from it. Returns null when the
/// reservation is exhausted.
#[cold]
#[inline(never)]
fn refill(heap: &ThreadHeap, class: usize) -> *mut u8 {
    // 1. Remote frees may have refilled pages of this class.
    if heap.remote.0.load(Ordering::Relaxed) != 0 {
        drain_remote(heap);
    }
    loop {
        // 2. The current page itself (after a drain it may have blocks again).
        // SAFETY: the current page is a live page this thread owns, or the
        // sentinel.
        if let Some(ptr) = unsafe { page_alloc(heap.current[class].get()) } {
            return ptr;
        }
        // 3. Another owned page with room.
        let avail = heap.avail[class].get();
        if !avail.is_null() {
            // SAFETY: pages in the avail list are live pages we own.
            unsafe {
                heap.remove_avail(avail);
                set_current(heap, avail);
            }
            continue;
        }
        // 4. An orphaned page of this class, else a pooled or fresh page.
        let mut page = adopt_orphan(class);
        if page.is_null() {
            page = new_page(class);
            if page.is_null() {
                return std::ptr::null_mut();
            }
        }
        // SAFETY: the page is exclusively ours: publish ownership, link it,
        // recover its remote frees, and let the loop allocate from it.
        unsafe {
            (*page).owner_state.store(heap.addr(), Ordering::Release);
            (*page).owner.store(heap.addr(), Ordering::Relaxed);
            heap.link_page(page);
            set_current(heap, page);
            absorb_remote(&*page);
        }
    }
}

/// First slow-path visit of a thread: decide the mode, and in pool mode
/// reserve the range, arm the exit guard and cache the range bounds.
#[cold]
#[inline(never)]
fn init_thread(heap: &ThreadHeap) -> u8 {
    // Allocations made while this runs (registering the exit guard may
    // allocate) take the system allocator.
    heap.mode.set(2);
    let mut mode = process_mode() as u8;
    // A thread that cannot clean up at exit never takes pages.
    if mode == 1 && !(ensure_reserved() && EXIT_GUARD.try_with(|_| ()).is_ok()) {
        mode = 2;
    }
    if mode == 1 {
        heap.base.set(RANGE.base.load(Ordering::Acquire));
        heap.small_limit.set(RANGE.half.load(Ordering::Acquire));
    }
    heap.mode.set(mode);
    mode
}
