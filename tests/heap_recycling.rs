//! Runs alone in its own process: recycled pages go to a process-global pool
//! that concurrently allocating tests would drain.
use std::alloc::{GlobalAlloc, Layout};
use std::collections::HashSet;

use rphp::heap::{PhpHeap, page_of, pooled_pages};

/// Pages emptied by frees return to the global pool and serve the next
/// refills instead of fresh pages.
#[test]
fn empty_pages_return_to_the_pool_and_come_back() {
    if std::env::var_os("RPHP_HEAP").is_some_and(|mode| mode == "system") {
        return;
    }
    let layout = Layout::from_size_align(768, 8).unwrap();
    // 85 blocks per 64 KiB page: 2000 blocks span more than 20 pages.
    let blocks: Vec<*mut u8> = (0..2000)
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        .map(|_| unsafe { PhpHeap.alloc(layout) })
        .collect();
    let first_pages: HashSet<usize> = blocks
        .iter()
        .map(|block| page_of(*block as usize))
        .collect();
    assert!(
        first_pages.len() >= 20,
        "{} pages carved",
        first_pages.len()
    );
    let pooled_before = pooled_pages();
    for block in &blocks {
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        unsafe { PhpHeap.dealloc(*block, layout) };
    }
    let pooled = pooled_pages();
    assert!(
        pooled >= pooled_before + first_pages.len() - 2,
        "emptied pages were recycled: {pooled_before} -> {pooled}"
    );
    let again: Vec<*mut u8> = (0..2000)
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        .map(|_| unsafe { PhpHeap.alloc(layout) })
        .collect();
    let reused = again
        .iter()
        .filter(|block| first_pages.contains(&page_of(**block as usize)))
        .count();
    assert!(reused > 1500, "recycled pages served the refill: {reused}");
    for block in again {
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        unsafe { PhpHeap.dealloc(block, layout) };
    }
}
