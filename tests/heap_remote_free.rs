//! Runs alone in its own process: recycled pages go to a process-global pool,
//! so the page-count contract cannot be asserted next to concurrently
//! allocating tests.
use std::alloc::{GlobalAlloc, Layout};

use rphp::heap::{PhpHeap, page_of};

/// Blocks allocated by this thread and freed on other threads flow back
/// through the pages' remote lists and get reused here instead of forcing
/// fresh pages.
#[test]
fn cross_thread_frees_are_recovered_by_the_owner() {
    if std::env::var_os("RPHP_HEAP").is_some_and(|mode| mode == "system") {
        return;
    }
    let layout = Layout::from_size_align(40, 8).unwrap();
    // Producer/consumer ping-pong: blocks allocated here are freed on other
    // threads while this thread keeps allocating; contents must survive and
    // the remote frees must flow back into our pages.
    let mut pages_touched = std::collections::HashSet::new();
    for round in 0..20u8 {
        let blocks: Vec<usize> = (0..5000)
            .map(|i| {
                // SAFETY: test-owned block with a valid layout; freed exactly once.
                let ptr = unsafe { PhpHeap.alloc(layout) };
                // SAFETY: the block holds 40 bytes.
                unsafe { std::ptr::write_bytes(ptr, round ^ (i as u8), 40) };
                pages_touched.insert(page_of(ptr as usize));
                ptr as usize
            })
            .collect();
        std::thread::scope(|scope| {
            for chunk in blocks.chunks(1250) {
                scope.spawn(move || {
                    for block in chunk {
                        // SAFETY: test-owned block with a valid layout; freed exactly once.
                        unsafe {
                            let first = *(*block as *const u8);
                            let bytes = std::slice::from_raw_parts(*block as *const u8, 40);
                            assert!(bytes.iter().all(|byte| *byte == first));
                            PhpHeap.dealloc(*block as *mut u8, layout);
                        }
                    }
                });
            }
        });
    }
    // 100k blocks of 40 B would span ~62 pages if nothing came back.
    assert!(
        pages_touched.len() < 40,
        "remote frees were recycled: {} distinct pages",
        pages_touched.len()
    );
}
