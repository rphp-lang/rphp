/// Push `block` onto the page's lock-free remote free list and, unless the
/// page is already queued or ownerless, queue the page at its owner.
///
/// # Safety
/// `block` must be a live block of `page` that no thread will touch again
/// until the owner drains the remote list; its first word is overwritten.
#[cold]
#[inline(never)]
unsafe fn remote_free(block: usize, page: &PageHeader) {
    #[cfg(feature = "php-heap-debug")]
    // SAFETY: the block is ours and `block_size` bytes long.
    unsafe {
        debug_mark_free(page, block)
    };
    let mut head = page.remote_free.load(Ordering::Acquire);
    loop {
        // SAFETY: caller passes a live pool block; its first word is ours.
        unsafe { *(block as *mut usize) = head };
        match page.remote_free.compare_exchange_weak(
            head,
            block,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(current) => head = current,
        }
    }
    let mut state = page.owner_state.load(Ordering::Acquire);
    loop {
        if state & QUEUED != 0 || state & !STATE_FLAGS == 0 {
            // Already queued, or orphaned/pooled: the owner or adopter picks
            // the block up when it drains the page.
            return;
        }
        if state & PUSHER_MASK == PUSHER_MASK {
            std::hint::spin_loop();
            state = page.owner_state.load(Ordering::Acquire);
            continue;
        }
        match page.owner_state.compare_exchange_weak(
            state,
            (state | QUEUED) + PUSHER_UNIT,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(current) => state = current,
        }
    }
    // SAFETY: the pusher count keeps the owner from giving the page (and
    // thereby its heap address) up until we decrement it below.
    let queue = unsafe { &(*((state & !STATE_FLAGS) as *const ThreadHeap)).remote.0 };
    let mut head = queue.load(Ordering::Acquire);
    loop {
        page.next_queued.store(head, Ordering::Relaxed);
        match queue.compare_exchange_weak(
            head,
            page as *const PageHeader as usize,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(current) => head = current,
        }
    }
    page.owner_state.fetch_sub(PUSHER_UNIT, Ordering::AcqRel);
}

/// Clear the page's owner, waiting for in-flight remote pushers. Returns the
/// state before clearing (owner). Never fails: with `QUEUED` set the page may
/// still be referenced from the owner's queue, which callers must tolerate
/// (orphaning drops the queue; recycling refuses via `try_detach`).
///
/// # Safety
/// `page` is a live page owned by the calling thread.
#[inline]
unsafe fn detach(page: &PageHeader) -> usize {
    let mut state = page.owner_state.load(Ordering::Acquire);
    loop {
        if state & PUSHER_MASK != 0 {
            std::hint::spin_loop();
            state = page.owner_state.load(Ordering::Acquire);
            continue;
        }
        match page
            .owner_state
            .compare_exchange_weak(state, 0, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                page.owner.store(0, Ordering::Relaxed);
                return state;
            }
            Err(current) => state = current,
        }
    }
}

/// Like `detach`, but refuses (returns false) while the page is queued at
/// this heap, so a stale queue entry never outlives the ownership.
///
/// # Safety
/// `page` is a live page owned by the calling thread.
#[inline]
unsafe fn try_detach(page: &PageHeader) -> bool {
    let mut state = page.owner_state.load(Ordering::Acquire);
    loop {
        if state & QUEUED != 0 {
            return false;
        }
        if state & PUSHER_MASK != 0 {
            std::hint::spin_loop();
            state = page.owner_state.load(Ordering::Acquire);
            continue;
        }
        match page
            .owner_state
            .compare_exchange_weak(state, 0, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                page.owner.store(0, Ordering::Relaxed);
                return true;
            }
            Err(current) => state = current,
        }
    }
}

/// Move a remote chain onto the page's local list, fixing `used` (owner).
/// Returns the number of blocks moved.
///
/// # Safety
/// `page` is a live page owned by the calling thread.
#[inline]
unsafe fn absorb_remote(page: &PageHeader) -> usize {
    let remote = page.remote_free.swap(0, Ordering::AcqRel);
    if remote == 0 {
        return 0;
    }
    let mut count = 1usize;
    let mut tail = remote;
    // SAFETY: chain links are free pool blocks of this page.
    unsafe {
        while *(tail as *const usize) != 0 {
            tail = *(tail as *const usize);
            count += 1;
        }
        *(tail as *mut usize) = page.local_free.get();
    }
    page.local_free.set(remote);
    debug_assert!((page.used.get() & !RETIRED) as usize >= count);
    page.used.set(page.used.get().wrapping_sub(count as u32));
    count
}

/// Move every queued page's remote frees onto their local lists; pages that
/// became empty are recycled, full ones that got room relisted.
#[inline(never)]
fn drain_remote(heap: &ThreadHeap) {
    let mut page = heap.remote.0.swap(0, Ordering::AcqRel);
    while page != 0 {
        // SAFETY: queued pages are live pool pages this heap owns: a page
        // is only queued while its state carries this heap's address, and
        // ownership is never given up while `QUEUED` is set except at
        // thread exit, which drops the queue.
        unsafe {
            let header = &*(page as *const PageHeader);
            let next = header.next_queued.load(Ordering::Relaxed);
            header.owner_state.fetch_and(!QUEUED, Ordering::AcqRel);
            let absorbed = absorb_remote(header);
            let page_ptr = page as *mut PageHeader;
            match header.listing.get() {
                LISTING_AVAIL => {
                    if header.used.get() == 0 {
                        heap.remove_avail(page_ptr);
                        recycle_or_list(heap, page_ptr);
                    }
                }
                LISTING_NONE => {
                    if absorbed != 0 {
                        let live = header.used.get() & !RETIRED;
                        header.used.set(live);
                        if live == 0 {
                            recycle_or_list(heap, page_ptr);
                        } else {
                            heap.push_avail(page_ptr);
                        }
                    }
                }
                // Current: the bias keeps it.
                _ => {}
            }
            page = next;
        }
    }
}
