use super::*;

/// Whether this thread serves small blocks from the pool (`RPHP_HEAP=system` off).
fn pool_enabled() -> bool {
    // The first allocation decides the mode; force it.
    let layout = Layout::from_size_align(8, 8).unwrap();
    // SAFETY: valid layout; freed immediately.
    unsafe {
        let probe = PhpHeap.alloc(layout);
        PhpHeap.dealloc(probe, layout);
    }
    heap().mode.get() == 1
}

#[test]
fn class_table_covers_every_small_size() {
    for size in 1..=MAX_SMALL {
        let class = class_for(size, 8).expect("small size has a class");
        assert!(
            CLASS_SIZES[class] as usize >= size,
            "size {size} class {class}"
        );
        if class > 0 {
            assert!((CLASS_SIZES[class - 1] as usize) < size);
        }
        let aligned = class_for(size, 16).expect("16-aligned class");
        assert_eq!(CLASS_SIZES[aligned] % 16, 0);
        assert!(CLASS_SIZES[aligned] as usize >= size);
    }
    for size in MAX_SMALL + 1..=MAX_POOL {
        let class = class_for(size, 16).expect("medium size has a class");
        assert!(
            class >= NUM_SMALL_CLASSES && class < NUM_CLASSES,
            "size {size}"
        );
        assert!(
            CLASS_SIZES[class] as usize >= size,
            "size {size} class {class}"
        );
        assert!(
            (CLASS_SIZES[class - 1] as usize) < size,
            "size {size} class {class}"
        );
    }
    assert_eq!(class_for(MAX_POOL + 1, 8), None);
    assert_eq!(class_for(64, 32), None);
}

#[test]
fn medium_blocks_live_on_medium_pages() {
    if !pool_enabled() {
        return;
    }
    for &size in &[1025usize, 1536, 4000, 8192, 20000, 32768] {
        let layout = Layout::from_size_align(size, 16).unwrap();
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        let ptr = unsafe { PhpHeap.alloc(layout) };
        assert!(in_pool(ptr as usize), "size {size} served from the pool");
        assert!(!is_small_page(ptr as usize), "size {size} on a medium page");
        assert_eq!(ptr as usize % 16, 0);
        // SAFETY: the block holds `size` bytes.
        unsafe {
            std::ptr::write_bytes(ptr, 0x5A, size);
            let class = (*page_header(ptr as usize)).class as usize;
            assert!(CLASS_SIZES[class] as usize >= size);
            assert_eq!(
                PhpHeap.realloc(ptr, layout, CLASS_SIZES[class] as usize),
                ptr
            );
            PhpHeap.dealloc(
                ptr,
                Layout::from_size_align(CLASS_SIZES[class] as usize, 16).unwrap(),
            );
        }
    }
    for &size in &[MAX_POOL + 1, 100_000, 1 << 20, LARGE_MAX] {
        let layout = Layout::from_size_align(size, 16).unwrap();
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        let large = unsafe { PhpHeap.alloc_zeroed(layout) };
        assert!(
            in_pool(large as usize),
            "size {size} lives in the large region"
        );
        assert!(is_large_offset(pool_offset(large as usize)));
        assert_eq!(large as usize % 16, 0);
        // SAFETY: the block holds `size` bytes.
        unsafe {
            assert!(
                std::slice::from_raw_parts(large, size)
                    .iter()
                    .all(|b| *b == 0)
            );
            std::ptr::write_bytes(large, 0x7B, size);
            // Growing within the rounded capacity keeps the block.
            let capacity = large_capacity(large);
            assert!(capacity >= size);
            assert_eq!(PhpHeap.realloc(large, layout, capacity), large);
            PhpHeap.dealloc(large, Layout::from_size_align(capacity, 16).unwrap());
            // The freed block is recycled for the next request of its size.
            let again = PhpHeap.alloc(layout);
            assert_eq!(again, large, "large block recycled");
            PhpHeap.dealloc(again, layout);
        }
    }
    let layout = Layout::from_size_align(LARGE_MAX + 1, 16).unwrap();
    // SAFETY: test-owned block with a valid layout; freed exactly once.
    let huge = unsafe { PhpHeap.alloc(layout) };
    assert!(!in_pool(huge as usize), "huge blocks come from the system");
    // SAFETY: test-owned block with a valid layout; freed exactly once.
    unsafe { PhpHeap.dealloc(huge, layout) };
    let layout = Layout::from_size_align(4096, 64).unwrap();
    // SAFETY: test-owned block with a valid layout; freed exactly once.
    let aligned = unsafe { PhpHeap.alloc(layout) };
    assert!(
        !in_pool(aligned as usize),
        "over-aligned blocks come from the system"
    );
    // SAFETY: test-owned block with a valid layout; freed exactly once.
    unsafe { PhpHeap.dealloc(aligned, layout) };
}

#[test]
fn pool_blocks_round_trip_and_reuse_lifo() {
    let heap = PhpHeap;
    let layout = Layout::from_size_align(40, 8).unwrap();
    // SAFETY: test-owned block with a valid layout; freed exactly once.
    let first = unsafe { heap.alloc(layout) };
    // SAFETY: test-owned block with a valid layout; freed exactly once.
    let second = unsafe { heap.alloc(layout) };
    assert!(!first.is_null() && !second.is_null() && first != second);
    if pool_enabled() {
        assert!(in_pool(first as usize) && in_pool(second as usize));
        assert_eq!(first as usize % 8, 0);
    }
    // SAFETY: test-owned block with a valid layout; freed exactly once.
    unsafe {
        std::ptr::write_bytes(first, 0xAB, 40);
        heap.dealloc(first, layout);
        let third = heap.alloc(layout);
        if pool_enabled() {
            assert_eq!(third, first, "LIFO reuse of the last freed block");
        }
        heap.dealloc(third, layout);
        heap.dealloc(second, layout);
    }
}

#[test]
fn zeroed_and_aligned_requests_hold_their_contracts() {
    let heap = PhpHeap;
    for &(size, align) in &[
        (1usize, 1usize),
        (24, 8),
        (48, 16),
        (152, 8),
        (1000, 16),
        (4096, 16),
        (64, 64),
    ] {
        let layout = Layout::from_size_align(size, align).unwrap();
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        let ptr = unsafe { heap.alloc_zeroed(layout) };
        assert!(!ptr.is_null());
        assert_eq!(ptr as usize % align, 0, "alignment {align} for size {size}");
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, size) };
        assert!(bytes.iter().all(|byte| *byte == 0));
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        unsafe { heap.dealloc(ptr, layout) };
    }
}

#[test]
fn realloc_keeps_the_block_inside_one_class_and_copies_across() {
    let heap = PhpHeap;
    let layout = Layout::from_size_align(20, 8).unwrap();
    // SAFETY: test-owned block with a valid layout; freed exactly once.
    let ptr = unsafe { heap.alloc(layout) };
    // SAFETY: test-owned block with a valid layout; freed exactly once.
    unsafe { std::ptr::copy_nonoverlapping(b"0123456789abcdefghij".as_ptr(), ptr, 20) };
    // SAFETY: test-owned block with a valid layout; freed exactly once.
    let same = unsafe { heap.realloc(ptr, layout, 24) };
    if pool_enabled() {
        assert_eq!(same, ptr);
    }
    // SAFETY: test-owned block with a valid layout; freed exactly once.
    let grown = unsafe { heap.realloc(same, Layout::from_size_align(24, 8).unwrap(), 300) };
    assert!(!grown.is_null());
    assert_eq!(
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        unsafe { std::slice::from_raw_parts(grown, 20) },
        b"0123456789abcdefghij"
    );
    // SAFETY: test-owned block with a valid layout; freed exactly once.
    unsafe { heap.dealloc(grown, Layout::from_size_align(300, 8).unwrap()) };
}

#[test]
fn stress_random_sizes_keep_contents_intact() {
    let heap = PhpHeap;
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut live: Vec<(*mut u8, Layout, u8)> = Vec::new();
    for round in 0..200_000u32 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let size = (state % 300) as usize + 1;
        let align = [1usize, 2, 4, 8, 16][(state >> 20) as usize % 5];
        let layout = Layout::from_size_align(size, align).unwrap();
        if live.len() < 500 || state & 1 == 0 {
            // SAFETY: test-owned block with a valid layout; freed exactly once.
            let ptr = unsafe { heap.alloc(layout) };
            assert!(!ptr.is_null());
            assert_eq!(ptr as usize % align, 0);
            let fill = (round & 0xFF) as u8;
            // SAFETY: test-owned block with a valid layout; freed exactly once.
            unsafe { std::ptr::write_bytes(ptr, fill, size) };
            live.push((ptr, layout, fill));
        } else {
            let index = (state >> 8) as usize % live.len();
            let (ptr, layout, fill) = live.swap_remove(index);
            // SAFETY: test-owned block with a valid layout; freed exactly once.
            let bytes = unsafe { std::slice::from_raw_parts(ptr, layout.size()) };
            assert!(
                bytes.iter().all(|byte| *byte == fill),
                "block content intact"
            );
            // SAFETY: test-owned block with a valid layout; freed exactly once.
            unsafe { heap.dealloc(ptr, layout) };
        }
    }
    for (ptr, layout, fill) in live {
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, layout.size()) };
        assert!(bytes.iter().all(|byte| *byte == fill));
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        unsafe { heap.dealloc(ptr, layout) };
    }
}

#[test]
fn sentinel_page_never_hands_out_a_block() {
    // SAFETY: the sentinel is readable, and a pop and a bump both miss it
    // without writing.
    assert!(unsafe { page_alloc(sentinel()) }.is_none());
    let fresh = ThreadHeap::new();
    assert!(fresh.current.iter().all(|page| page.get() == sentinel()));
}

/// The review's interleaving: a pop reads the head A and A's link B, then
/// stalls; another thread pops A and B, keeps B live and pushes A back. The
/// stalled pop must not install its stale link, or B would be handed out
/// twice.
#[test]
fn page_stack_rejects_a_stale_pop_after_the_entry_came_back() {
    if !pool_enabled() {
        return;
    }
    // Two entries of the reservation that nobody else references.
    let a_block = alloc_large(LARGE_UNIT, false);
    let b_block = alloc_large(LARGE_UNIT, false);
    assert!(!a_block.is_null() && !b_block.is_null() && a_block != b_block);
    let a = a_block as usize - PAGE_HEADER;
    let b = b_block as usize - PAGE_HEADER;
    let stack = PageStack::new();
    // SAFETY: both entries are exclusively ours and in no other stack.
    unsafe {
        stack.push(b);
        stack.push(a);
    }
    let stale_head = stack.head.load(Ordering::Acquire);
    // SAFETY: `a` is an entry of the reservation.
    let stale_next = unsafe { stack_link(a) }.load(Ordering::Relaxed);
    assert_eq!(stack.pop(), a);
    assert_eq!(stack.pop(), b);
    // SAFETY: `a` is ours again and out of the stack.
    unsafe { stack.push(a) };
    assert_eq!(
        stack.head.load(Ordering::Relaxed) & STACK_INDEX_MASK,
        stale_head & STACK_INDEX_MASK
    );
    let stale_swap = stack.head.compare_exchange(
        stale_head,
        PageStack::successor(stale_head, stale_next),
        Ordering::AcqRel,
        Ordering::Acquire,
    );
    assert!(stale_swap.is_err(), "the version tag rejects the stale pop");
    assert_eq!(stack.pop(), a);
    assert_eq!(stack.pop(), 0, "the live entry B is never handed out");
    assert_eq!(stack.count.load(Ordering::Relaxed), 0);
    // SAFETY: both blocks came from `alloc_large` and are freed once.
    unsafe {
        free_large(a_block);
        free_large(b_block);
    }
}

#[test]
fn page_stack_versions_survive_wraparound() {
    let head = STACK_INDEX_MASK & 5 | (usize::MAX & !STACK_INDEX_MASK);
    let next = PageStack::successor(head, 9);
    assert_eq!(next & STACK_INDEX_MASK, 9);
    assert_eq!(
        next & !STACK_INDEX_MASK,
        0,
        "the version wraps inside its bits"
    );
}

/// `MADV_DONTNEED` needs an OS-page-aligned start: the reclaim keeps the OS
/// page with the header and drops the rest of the entry.
#[test]
fn reclaim_drops_whole_os_pages_after_the_header() {
    let len = 4 * LARGE_UNIT;
    // SAFETY: a fresh private anonymous mapping, unmapped at the end; every
    // access stays inside it.
    unsafe {
        let map = libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );
        assert_ne!(map, libc::MAP_FAILED);
        let entry = (map as usize).next_multiple_of(LARGE_UNIT);
        let body = 2 * LARGE_UNIT;
        std::ptr::write_bytes(entry as *mut u8, 0xA5, body);
        let before = reclaimed_bytes();
        assert!(reclaim_body(entry, body), "the kernel accepts the range");
        let page = os_page_size();
        assert!(reclaimed_bytes() - before >= body - page);
        let mut residency = vec![0u8; body / page];
        assert_eq!(
            libc::mincore(entry as *mut libc::c_void, body, residency.as_mut_ptr()),
            0
        );
        assert_eq!(residency[0] & 1, 1, "the header page stays resident");
        assert!(
            residency[1..].iter().all(|state| state & 1 == 0),
            "the body went back to the OS"
        );
        assert_eq!(
            *(entry as *const u8),
            0xA5,
            "the header page keeps its bytes"
        );
        assert_eq!(
            *((entry + page) as *const u8),
            0,
            "the body reads back as zeros"
        );
        libc::munmap(map, len);
    }
}

/// Concurrent claims at the region limit never overlap: a failed claim must
/// not move the cursor under a successful one.
#[test]
fn carve_never_hands_out_overlapping_ranges() {
    let start = 1usize << 30;
    let unit = 4096;
    let cursor = AtomicUsize::new(start);
    let limit = start + 10 * unit;
    let claims: Vec<usize> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| carve(&cursor, 3 * unit, limit)))
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect()
    });
    let mut granted: Vec<usize> = claims.into_iter().filter(|&claim| claim != 0).collect();
    granted.sort_unstable();
    assert_eq!(granted, vec![start, start + 3 * unit, start + 6 * unit]);
    assert_eq!(cursor.load(Ordering::Relaxed), start + 9 * unit);
}

/// A page that filled up retires; its first free relists it, and once all
/// of its blocks are back it leaves the thread.
#[test]
fn full_pages_retire_relist_and_recycle() {
    if !pool_enabled() {
        return;
    }
    let layout = Layout::from_size_align(1024, 8).unwrap();
    // 63 blocks per page: 200 blocks fill three pages and start a fourth.
    let blocks: Vec<*mut u8> = (0..200)
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        .map(|_| unsafe { PhpHeap.alloc(layout) })
        .collect();
    let first_page = page_header(blocks[0] as usize);
    // SAFETY: a page this thread owns.
    unsafe {
        assert_eq!((*first_page).listing.get(), LISTING_NONE);
        assert_eq!((*first_page).used.get(), RETIRED | 63);
        PhpHeap.dealloc(blocks[0], layout);
        assert_eq!((*first_page).listing.get(), LISTING_AVAIL, "relisted");
        assert_eq!((*first_page).used.get(), 62);
        for block in &blocks[1..63] {
            PhpHeap.dealloc(*block, layout);
        }
        assert_ne!(
            (*first_page).owner.load(Ordering::Relaxed),
            heap().addr(),
            "the emptied page left the thread"
        );
        for block in &blocks[63..] {
            PhpHeap.dealloc(*block, layout);
        }
    }
}
