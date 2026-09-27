//! Runs alone in its own process: several threads hammer the global page
//! pools and the large-block stacks at once and check every block's bytes
//! before freeing it, so a stack that hands one entry out twice (ABA), a
//! page recycled while still in use, or two overlapping carves show up as
//! corrupted data.
use std::alloc::{GlobalAlloc, Layout};
use std::sync::mpsc;

use rphp::heap::PhpHeap;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// A block size drawn like a PHP workload: mostly small, some medium, a few
/// large ones that live in the large region's stacks.
fn draw_layout(rng: &mut Rng) -> Layout {
    let pick = rng.next();
    let size = match pick % 32 {
        0 | 1 => 32 * 1024 + 1 + (pick >> 8) as usize % (300 * 1024),
        2..=5 => 1025 + (pick >> 8) as usize % (31 * 1024),
        _ => 1 + (pick >> 8) as usize % 1024,
    };
    let align = if pick & (1 << 40) != 0 { 16 } else { 8 };
    Layout::from_size_align(size, align).unwrap()
}

fn fill(ptr: *mut u8, layout: Layout, tag: u8) {
    // SAFETY: the block holds `layout.size()` bytes.
    unsafe { std::ptr::write_bytes(ptr, tag, layout.size()) };
}

fn check_and_free(ptr: *mut u8, layout: Layout, tag: u8) {
    // SAFETY: a live block of `layout.size()` bytes, freed exactly once.
    unsafe {
        let bytes = std::slice::from_raw_parts(ptr, layout.size());
        assert!(
            bytes.iter().all(|byte| *byte == tag),
            "block of {} bytes changed while live",
            layout.size()
        );
        PhpHeap.dealloc(ptr, layout);
    }
}

fn churn(seed: u64, rounds: usize) {
    let mut rng = Rng(seed | 1);
    let mut live: Vec<(usize, Layout, u8)> = Vec::new();
    for round in 0..rounds {
        let pick = rng.next();
        if live.len() < 48 || pick & 1 == 0 {
            let layout = draw_layout(&mut rng);
            // SAFETY: valid non-zero layout.
            let ptr = unsafe { PhpHeap.alloc(layout) };
            assert!(!ptr.is_null());
            assert_eq!(ptr as usize % layout.align(), 0);
            let tag = (round as u8) ^ (seed as u8) | 1;
            fill(ptr, layout, tag);
            live.push((ptr as usize, layout, tag));
        } else {
            let index = (pick >> 8) as usize % live.len();
            let (ptr, layout, tag) = live.swap_remove(index);
            check_and_free(ptr as *mut u8, layout, tag);
        }
    }
    for (ptr, layout, tag) in live {
        check_and_free(ptr as *mut u8, layout, tag);
    }
}

#[test]
fn concurrent_churn_keeps_every_block_intact() {
    std::thread::scope(|scope| {
        for thread in 0..8u64 {
            scope.spawn(move || churn(0x9E37_79B9_7F4A_7C15 ^ (thread << 32), 30_000));
        }
    });
}

/// Producers allocate and fill, consumers on other threads check and free:
/// every free is remote, and producers keep reusing the pages the consumers
/// hand back.
#[test]
fn blocks_freed_on_other_threads_stay_intact() {
    let (sender, receiver) = mpsc::sync_channel::<Vec<(usize, Layout, u8)>>(16);
    let receiver = std::sync::Mutex::new(receiver);
    std::thread::scope(|scope| {
        for producer in 0..4u64 {
            let sender = sender.clone();
            scope.spawn(move || {
                let mut rng = Rng(0xD1B5_4A32_D192_ED03 ^ producer);
                for batch in 0..200usize {
                    let blocks = (0..64)
                        .map(|index| {
                            let layout = draw_layout(&mut rng);
                            // SAFETY: valid non-zero layout.
                            let ptr = unsafe { PhpHeap.alloc(layout) };
                            assert!(!ptr.is_null());
                            let tag = (batch as u8) ^ (index as u8) | 1;
                            fill(ptr, layout, tag);
                            (ptr as usize, layout, tag)
                        })
                        .collect();
                    sender.send(blocks).unwrap();
                }
            });
        }
        drop(sender);
        for _ in 0..4 {
            let receiver = &receiver;
            scope.spawn(move || {
                loop {
                    let batch = match receiver.lock().unwrap().recv() {
                        Ok(batch) => batch,
                        Err(_) => break,
                    };
                    for (ptr, layout, tag) in batch {
                        check_and_free(ptr as *mut u8, layout, tag);
                    }
                }
            });
        }
    });
}

/// Short-lived threads (like the parser's helper threads) leave orphaned
/// pages behind; the next threads adopt them while the main thread frees
/// the blocks the dead threads allocated.
#[test]
fn orphaned_pages_survive_generations_of_threads() {
    let mut survivors: Vec<(usize, Layout, u8)> = Vec::new();
    for generation in 0..40u64 {
        let batch: Vec<(usize, Layout, u8)> = std::thread::scope(|scope| {
            scope
                .spawn(move || {
                    let mut rng = Rng(0xA076_1D64_78BD_642F ^ generation);
                    let mut kept = Vec::new();
                    for index in 0..2_000usize {
                        let layout = draw_layout(&mut rng);
                        // SAFETY: valid non-zero layout.
                        let ptr = unsafe { PhpHeap.alloc(layout) };
                        assert!(!ptr.is_null());
                        let tag = (generation as u8) ^ (index as u8) | 1;
                        fill(ptr, layout, tag);
                        if index % 3 == 0 {
                            kept.push((ptr as usize, layout, tag));
                        } else {
                            check_and_free(ptr, layout, tag);
                        }
                    }
                    kept
                })
                .join()
                .unwrap()
        });
        // Free the previous generation's blocks remotely while this one's
        // pages sit orphaned.
        for (ptr, layout, tag) in survivors.drain(..) {
            check_and_free(ptr as *mut u8, layout, tag);
        }
        survivors = batch;
    }
    for (ptr, layout, tag) in survivors {
        check_and_free(ptr as *mut u8, layout, tag);
    }
}
