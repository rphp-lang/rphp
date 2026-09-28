//! Runs alone in its own process: page counters are process-global, so the
//! adoption contract cannot be asserted next to concurrently allocating tests.
use std::alloc::{GlobalAlloc, Layout};

use rphp::heap::{PhpHeap, orphaned_pages, pages_in_use};

/// Blocks allocated by a thread that has since finished are freed by another
/// thread onto the orphaned pages' remote lists, and the next refill of that
/// class adopts those pages instead of carving new ones.
#[test]
fn orphaned_pages_are_adopted_and_their_blocks_reused() {
    if std::env::var_os("RPHP_HEAP").is_some_and(|mode| mode == "system") {
        return;
    }
    let layout = Layout::from_size_align(48, 16).unwrap();
    let blocks: Vec<usize> = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                (0..2000)
                    // SAFETY: test-owned block with a valid layout; freed exactly once.
                    .map(|_| unsafe { PhpHeap.alloc(layout) } as usize)
                    .collect::<Vec<_>>()
            })
            .join()
            .unwrap()
    });
    let orphans_before = orphaned_pages();
    assert!(
        orphans_before >= 2,
        "the finished thread orphaned its pages: {orphans_before}"
    );
    for block in &blocks {
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        unsafe { PhpHeap.dealloc(*block as *mut u8, layout) };
    }
    let pages_before = pages_in_use();
    let again: Vec<usize> = (0..2000)
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        .map(|_| unsafe { PhpHeap.alloc(layout) } as usize)
        .collect();
    // Fresh space of the current page is served first; once it runs out the
    // refill adopts an orphan instead of carving a new page, so the freed
    // blocks come back and the page count stays flat.
    let reused = again.iter().filter(|block| blocks.contains(block)).count();
    assert!(reused > 0, "adopted blocks reused: {reused}");
    assert!(
        pages_in_use() <= pages_before + 1,
        "no new pages while orphans exist"
    );
    assert!(orphaned_pages() < orphans_before);
    for block in again {
        // SAFETY: test-owned block with a valid layout; freed exactly once.
        unsafe { PhpHeap.dealloc(block as *mut u8, layout) };
    }
}
