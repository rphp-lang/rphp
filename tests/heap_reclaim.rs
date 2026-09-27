//! Runs alone in its own process: the resident budget of the page pool is
//! counted per process.
use std::alloc::{GlobalAlloc, Layout};

use rphp::heap::{PhpHeap, pooled_pages, reclaimed_bytes};

fn resident_bytes() -> usize {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
    let pages: usize = statm.split_whitespace().nth(1).unwrap().parse().unwrap();
    // SAFETY: sysconf only reads a system constant.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
    pages * page
}

/// Emptied pages beyond the 64 MiB resident budget give their memory back
/// to the OS (with an OS-page-aligned `madvise`, which the kernel accepts).
#[test]
fn freed_pages_beyond_the_budget_return_their_memory() {
    if std::env::var_os("RPHP_HEAP").is_some_and(|mode| mode == "system") {
        return;
    }
    let layout = Layout::from_size_align(768, 8).unwrap();
    // 96 MiB of 64 KiB pages: half again the resident budget.
    let count = (96 << 20) / 768;
    let blocks: Vec<usize> = (0..count)
        .map(|index| {
            // SAFETY: test-owned block with a valid layout; freed once below.
            let ptr = unsafe { PhpHeap.alloc(layout) };
            assert!(!ptr.is_null());
            // SAFETY: the block holds 768 bytes.
            unsafe { std::ptr::write_bytes(ptr, index as u8, 768) };
            ptr as usize
        })
        .collect();
    let reclaimed_before = reclaimed_bytes();
    let pooled_before = pooled_pages();
    let resident_full = resident_bytes();
    for block in blocks {
        // SAFETY: freed exactly once.
        unsafe { PhpHeap.dealloc(block as *mut u8, layout) };
    }
    let reclaimed = reclaimed_bytes() - reclaimed_before;
    let resident_after = resident_bytes();
    assert!(
        pooled_pages() >= pooled_before + 1400,
        "emptied pages went to the pool: {pooled_before} -> {}",
        pooled_pages()
    );
    assert!(
        reclaimed >= 24 << 20,
        "pages beyond the budget went back to the OS: {reclaimed} bytes"
    );
    assert!(
        resident_full.saturating_sub(resident_after) >= 16 << 20,
        "resident memory dropped: {resident_full} -> {resident_after}"
    );
    // The pages come back into use (resident ones first) and stay usable.
    let again: Vec<usize> = (0..count)
        .map(|index| {
            // SAFETY: test-owned block with a valid layout; freed once below.
            let ptr = unsafe { PhpHeap.alloc(layout) };
            // SAFETY: the block holds 768 bytes.
            unsafe { std::ptr::write_bytes(ptr, index as u8, 768) };
            ptr as usize
        })
        .collect();
    for (index, block) in again.into_iter().enumerate() {
        // SAFETY: a live block of 768 bytes, freed exactly once.
        unsafe {
            assert!(
                std::slice::from_raw_parts(block as *const u8, 768)
                    .iter()
                    .all(|byte| *byte == index as u8)
            );
            PhpHeap.dealloc(block as *mut u8, layout);
        }
    }
}
