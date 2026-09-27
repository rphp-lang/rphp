//! Focused allocator probe. Build with `cargo build --release --example heap_alloc`.
//! `lifo`, `mixed`, and `burst` call the process allocator with dynamic layouts;
//! `specialized` also exposes a constant PHP-sized layout to the optimizer.
use std::alloc::{GlobalAlloc, Layout, alloc, dealloc, handle_alloc_error};
use std::hint::black_box;
use std::time::Instant;

// Link the library that installs the selected process allocator.
use rphp as _;

#[cfg(feature = "php-heap")]
const DIRECT: rphp::heap::PhpHeap = rphp::heap::PhpHeap;
#[cfg(not(feature = "php-heap"))]
const DIRECT: std::alloc::System = std::alloc::System;

#[inline(never)]
fn run(kind: &str, count: usize) -> usize {
    let sizes = [1usize, 16, 24, 40, 48, 84, 104, 128, 152, 1024];
    let mut checksum = 0usize;
    if kind == "burst" {
        let layout = Layout::from_size_align(black_box(40), 8).unwrap();
        let mut pointers = [std::ptr::null_mut::<u8>(); 256];
        for round in 0..count / pointers.len() {
            for (index, slot) in pointers.iter_mut().enumerate() {
                // SAFETY: a non-zero layout; initialize before reading. Each
                // live pointer is saved exactly once and freed below.
                unsafe {
                    let ptr = alloc(layout);
                    if ptr.is_null() {
                        handle_alloc_error(layout);
                    }
                    ptr.write(round.wrapping_add(index) as u8);
                    *slot = black_box(ptr);
                }
            }
            for (index, ptr) in pointers.iter().enumerate().rev() {
                // SAFETY: the pointer belongs to this round and layout.
                unsafe {
                    let value = black_box(*ptr).read();
                    assert_eq!(value, round.wrapping_add(index) as u8);
                    checksum = checksum.wrapping_add(value as usize);
                    dealloc(*ptr, layout);
                }
            }
        }
    } else if kind == "specialized" {
        let layout = Layout::from_size_align(40, 8).unwrap();
        for index in 0..count {
            // SAFETY: a non-zero layout, initialized payload, one matching
            // deallocation. Direct calls let Rust specialize and inline.
            unsafe {
                let ptr = DIRECT.alloc(layout);
                if ptr.is_null() {
                    handle_alloc_error(layout);
                }
                ptr.write(index as u8);
                let value = black_box(ptr).read();
                assert_eq!(value, index as u8);
                checksum = checksum.wrapping_add(value as usize);
                DIRECT.dealloc(ptr, layout);
            }
        }
    } else {
        for index in 0..count {
            let size = if kind == "mixed" {
                sizes[index % sizes.len()]
            } else {
                40
            };
            let layout = Layout::from_size_align(black_box(size), 8).unwrap();
            // SAFETY: a non-zero layout, initialized payload, one matching
            // deallocation. black_box keeps the allocation observable.
            unsafe {
                let ptr = alloc(layout);
                if ptr.is_null() {
                    handle_alloc_error(layout);
                }
                ptr.write(index as u8);
                let value = black_box(ptr).read();
                assert_eq!(value, index as u8);
                checksum = checksum.wrapping_add(value as usize);
                dealloc(ptr, layout);
            }
        }
    }
    black_box(checksum)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 3, "usage: heap_alloc CASE ALLOCATIONS");
    let kind = args[1].as_str();
    assert!(matches!(kind, "lifo" | "mixed" | "burst" | "specialized"));
    let count: usize = args[2].parse().expect("positive allocation count");
    assert!(count > 0);
    // A burst always fills and drains a complete batch. Report its actual
    // allocation count so instruction slopes and checksums use the same unit.
    let count = if kind == "burst" {
        count.div_ceil(256).checked_mul(256).unwrap()
    } else {
        count
    };
    let start = Instant::now();
    let checksum = run(kind, count);
    println!("{kind} {count} {checksum} {}", start.elapsed().as_nanos());
}
