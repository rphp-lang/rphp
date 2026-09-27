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
    assert_eq!(class_for(1025, 8), None);
    assert_eq!(class_for(64, 32), None);
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
