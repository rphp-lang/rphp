//! The debug heap's detectors, exercised in child processes (a detection
//! aborts the process). Only built with `php-heap-debug`.
#![cfg(feature = "php-heap-debug")]

use std::alloc::{GlobalAlloc, Layout};
use std::process::Command;

use rphp::heap::PhpHeap;

const CHILD: &str = "RPHP_HEAP_DEBUG_CHILD";

/// Run `test` of this binary again in a child with `CHILD=case`; return its
/// stderr and whether it failed.
fn run_child(test: &str, case: &str) -> (bool, String) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--test-threads=1", "--nocapture"])
        .env(CHILD, case)
        .output()
        .unwrap();
    (
        !output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn a_double_free_aborts_with_a_report() {
    if std::env::var_os(CHILD).is_some_and(|case| case == "double-free") {
        let layout = Layout::from_size_align(64, 8).unwrap();
        // SAFETY: deliberately freed twice; the detector aborts first.
        unsafe {
            let block = PhpHeap.alloc(layout);
            PhpHeap.dealloc(block, layout);
            PhpHeap.dealloc(block, layout);
        }
        return;
    }
    if std::env::var_os("RPHP_HEAP").is_some_and(|mode| mode == "system") {
        return;
    }
    let (failed, stderr) = run_child("a_double_free_aborts_with_a_report", "double-free");
    assert!(failed, "the child survived a double free");
    assert!(stderr.contains("double free detected"), "{stderr}");
}

#[test]
fn a_write_after_free_is_caught_when_the_block_comes_back() {
    if std::env::var_os(CHILD).is_some_and(|case| case == "write-after-free") {
        let layout = Layout::from_size_align(64, 8).unwrap();
        // SAFETY: deliberately writes a freed block (still mapped, owned by
        // the heap) so the next allocation of it trips the poison check.
        unsafe {
            let block = PhpHeap.alloc(layout);
            PhpHeap.dealloc(block, layout);
            std::ptr::write_volatile(block.add(40), 0x55);
            let _ = PhpHeap.alloc(layout);
        }
        return;
    }
    if std::env::var_os("RPHP_HEAP").is_some_and(|mode| mode == "system") {
        return;
    }
    let (failed, stderr) = run_child(
        "a_write_after_free_is_caught_when_the_block_comes_back",
        "write-after-free",
    );
    assert!(failed, "the child survived a write after free");
    assert!(stderr.contains("written after its free"), "{stderr}");
}

/// Copies of uninitialized bytes (whole `Option` elements) must never look
/// like frees: the free state lives in a bitmap, not in the blocks.
#[test]
fn copying_uninitialized_payloads_is_not_a_double_free() {
    #[derive(Clone, Copy)]
    struct Pair {
        _start: usize,
        _end: usize,
    }
    for round in 0..2_000usize {
        let mut source: Vec<Option<Pair>> = Vec::with_capacity(3);
        source.push(None);
        source.push(None);
        source.push(Some(Pair {
            _start: round,
            _end: round + 1,
        }));
        let copies: Vec<Vec<Option<Pair>>> = (0..8).map(|_| source.clone()).collect();
        drop(source);
        drop(copies);
    }
}
