#[cfg(feature = "php-heap-debug")]
const POISON: u8 = 0xDE;

/// Debug mode: one bit per 8-byte granule of the reservation, set while the
/// block starting there is free. A double free finds its bit already set;
/// a free list that hands out a block whose bit is clear was corrupted.
/// Unlike a marker value stored in the block, the bitmap cannot be forged
/// by callers copying uninitialized bytes around (a `Vec` of `Option`s
/// copies whole elements, payload garbage included, and that garbage may
/// hold whatever the allocator last left in a register). Reserved lazily,
/// like the heap, and touched only where blocks live.
#[cfg(feature = "php-heap-debug")]
static DEBUG_FREE_MAP: AtomicUsize = AtomicUsize::new(0);

/// Debug mode: reserve the free bitmap for a reservation of `size` bytes.
#[cfg(feature = "php-heap-debug")]
#[cold]
fn debug_reserve_map(size: usize) {
    // SAFETY: anonymous private mapping; untouched parts stay virtual.
    let map = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size / 64,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
            -1,
            0,
        )
    };
    if map != libc::MAP_FAILED {
        DEBUG_FREE_MAP.store(map as usize, Ordering::Release);
    }
}

/// Debug mode: the bitmap word and bit of the block at `block`.
#[cfg(feature = "php-heap-debug")]
fn debug_bit(block: usize) -> Option<(&'static std::sync::atomic::AtomicU64, u64)> {
    let map = DEBUG_FREE_MAP.load(Ordering::Acquire);
    if map == 0 {
        return None;
    }
    let granule = pool_offset(block) >> 3;
    // SAFETY: the map covers the whole reservation at one bit per granule;
    // any bit pattern is a valid `AtomicU64`.
    let word = unsafe { &*((map + (granule >> 6) * 8) as *const std::sync::atomic::AtomicU64) };
    Some((word, 1u64 << (granule & 63)))
}

/// Debug mode: forget the free bits of `[entry, entry + len)`.
#[cfg(feature = "php-heap-debug")]
fn debug_clear_range(entry: usize, len: usize) {
    let map = DEBUG_FREE_MAP.load(Ordering::Acquire);
    if map == 0 {
        return;
    }
    // Entries are 64 KiB aligned: whole bitmap words.
    let first = map + (pool_offset(entry) >> 3) / 8;
    // SAFETY: inside the bitmap, which covers the reservation.
    unsafe { std::ptr::write_bytes(first as *mut u8, 0, len / 64) };
}

/// Debug mode: `block` (of `page`) is being freed. Abort on a double free;
/// poison everything past the link word.
///
/// # Safety
/// `block` is a slot of `page`.
#[cfg(feature = "php-heap-debug")]
unsafe fn debug_mark_free(page: &PageHeader, block: usize) {
    if let Some((word, bit)) = debug_bit(block)
        && word.fetch_or(bit, Ordering::AcqRel) & bit != 0
    {
        // SAFETY: per the contract.
        unsafe { debug_report_double_free(page, block) };
    }
    let size = page.block_size as usize;
    if size > 8 {
        // SAFETY: per the contract the slot holds `size` bytes.
        unsafe { std::ptr::write_bytes((block + 8) as *mut u8, POISON, size - 8) };
    }
}

/// Debug mode: `block` leaves `page`. One popped off a free list must be
/// marked free and still carry its poison past the link word (else it was
/// written after the free); a bumped one must not be marked free.
#[cfg(feature = "php-heap-debug")]
fn debug_mark_taken(page: &PageHeader, block: usize, from_free_list: bool) {
    if let Some((word, bit)) = debug_bit(block) {
        let was_free = word.fetch_and(!bit, Ordering::AcqRel) & bit != 0;
        if was_free != from_free_list {
            debug_fail(if from_free_list {
                b"rphp heap: a free list held a block that was not free\n"
            } else {
                b"rphp heap: a never-used block was marked free\n"
            });
        }
    }
    let size = page.block_size as usize;
    if from_free_list && size > 8 {
        // SAFETY: the slot holds `size` bytes.
        let body = unsafe { std::slice::from_raw_parts((block + 8) as *const u8, size - 8) };
        if body.iter().any(|byte| *byte != POISON) {
            debug_fail(b"rphp heap: a free block was written after its free\n");
        }
    }
}

/// Debug mode: large blocks keep a free bit at their start.
#[cfg(feature = "php-heap-debug")]
fn debug_large(block: usize, freeing: bool) {
    if let Some((word, bit)) = debug_bit(block) {
        let was_free = if freeing {
            word.fetch_or(bit, Ordering::AcqRel)
        } else {
            word.fetch_and(!bit, Ordering::AcqRel)
        } & bit
            != 0;
        if was_free == freeing {
            debug_fail(if freeing {
                b"rphp heap: double free of a large block\n"
            } else {
                b"rphp heap: a large block handed out twice\n"
            });
        }
    }
}

/// Debug mode: a block being freed must start a slot of its page and the
/// caller's layout must map to the page's class.
#[cfg(feature = "php-heap-debug")]
fn debug_check_free(page: &PageHeader, block: usize, size: usize, align: usize) {
    let class = page.class as usize;
    let slot = page.block_size as usize;
    let page_size = if class < NUM_SMALL_CLASSES {
        SMALL_PAGE_SIZE
    } else {
        MEDIUM_PAGE_SIZE
    };
    let first = page.start + page_color(page.start, page_size);
    let misaligned = block < first || (block - first) % slot != 0;
    let wrong_class = class_for(size, align) != Some(class);
    if misaligned || wrong_class {
        let mut buffer = [0u8; 256];
        let mut len = 0;
        let mut put = |text: &[u8]| {
            let n = text.len().min(buffer.len() - len);
            buffer[len..len + n].copy_from_slice(&text[..n]);
            len += n;
        };
        put(if misaligned {
            b"rphp heap: free of a non-slot pointer"
        } else {
            b"rphp heap: free with a layout of another class"
        });
        put(b" class=");
        put(&[b'0' + (class / 10) as u8, b'0' + (class % 10) as u8]);
        put(b" size=");
        let mut digits = [0u8; 20];
        let mut n = size;
        let mut i = 20;
        loop {
            i -= 1;
            digits[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        put(&digits[i..]);
        put(b"\n");
        // SAFETY: writing a stack buffer to stderr.
        unsafe {
            libc::write(2, buffer.as_ptr().cast(), len);
            libc::abort()
        }
    }
}

/// Debug mode: print a fixed message and abort.
#[cfg(feature = "php-heap-debug")]
#[cold]
fn debug_fail(message: &[u8]) -> ! {
    // SAFETY: writing a static buffer to stderr.
    unsafe {
        libc::write(2, message.as_ptr().cast(), message.len());
        libc::abort()
    }
}

/// Debug mode: describe the offending block without allocating, then abort.
///
/// # Safety
/// `block` is a slot of `page`, whose free lists hold only slots of `page`.
#[cfg(feature = "php-heap-debug")]
#[cold]
unsafe fn debug_report_double_free(page: &PageHeader, block: usize) -> ! {
    let mut buffer = [0u8; 256];
    let mut len = 0;
    let mut put = |text: &[u8]| {
        let n = text.len().min(buffer.len() - len);
        buffer[len..len + n].copy_from_slice(&text[..n]);
        len += n;
    };
    let hex = |mut value: usize, put: &mut dyn FnMut(&[u8])| {
        let mut digits = [0u8; 16];
        for digit in digits.iter_mut().rev() {
            *digit = b"0123456789abcdef"[value & 15];
            value >>= 4;
        }
        put(&digits);
    };
    let listed_in = |mut cursor: usize| {
        let mut steps = 0;
        while cursor != 0 && steps < 100_000 {
            if cursor == block {
                return true;
            }
            // SAFETY: free-list links are blocks of this page.
            cursor = unsafe { *(cursor as *const usize) };
            steps += 1;
        }
        false
    };
    let local = listed_in(page.local_free.get());
    let remote = listed_in(page.remote_free.load(Ordering::Acquire));
    put(b"rphp heap: double free detected block=0x");
    hex(block, &mut put);
    put(b" class=");
    hex(page.class as usize, &mut put);
    put(b" used=");
    hex(page.used.get() as usize, &mut put);
    put(b" listing=");
    hex(page.listing.get() as usize, &mut put);
    put(if local {
        b" in-local-list"
    } else {
        b" not-in-local-list"
    });
    put(if remote {
        b" in-remote-list"
    } else {
        b" not-in-remote-list"
    });
    put(b" owner=0x");
    hex(page.owner.load(Ordering::Relaxed), &mut put);
    put(b" heap=0x");
    hex(heap().addr(), &mut put);
    put(b"\n");
    // SAFETY: writing a stack buffer to stderr.
    unsafe {
        libc::write(2, buffer.as_ptr().cast(), len);
        libc::abort()
    }
}
