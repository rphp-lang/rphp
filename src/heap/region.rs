/// Reserved address range: `[base, base + reserve)`, `base` 64 KiB aligned.
/// The header array fills `[base, base + small_start)`, small pages follow
/// below `base + half`, medium pages below `base + large_start`, large
/// blocks above.
#[repr(C, align(64))]
struct Range {
    base: AtomicUsize,
    half: AtomicUsize,
    reserve: AtomicUsize,
    large_start: AtomicUsize,
    small_start: AtomicUsize,
}
static RANGE: Range = Range {
    base: AtomicUsize::new(0),
    half: AtomicUsize::new(0),
    reserve: AtomicUsize::new(0),
    large_start: AtomicUsize::new(0),
    small_start: AtomicUsize::new(0),
};
/// Next never-used address per region.
static SMALL_CURSOR: AtomicUsize = AtomicUsize::new(0);
static MEDIUM_CURSOR: AtomicUsize = AtomicUsize::new(0);
static LARGE_CURSOR: AtomicUsize = AtomicUsize::new(0);

/// Lock-free LIFO of free 64 KiB-aligned entries of the reservation (free
/// pages, free large blocks), linked through the word at `STACK_LINK`.
///
/// The head packs the top entry's unit index (plus one; zero means empty)
/// with a version that every successful update increments. A pop that read
/// the head and the top entry's link, then lost the race to threads that
/// popped that entry and pushed it back, finds a different version and
/// retries instead of installing its stale link (the ABA problem). Entries
/// stay mapped for the life of the process and their link is only accessed
/// atomically, so the stale read itself is harmless.
#[repr(C, align(64))]
struct PageStack {
    head: AtomicUsize,
    /// Entries in the stack: incremented before a push and decremented after
    /// a pop, so it never underflows.
    count: AtomicUsize,
}

const STACK_INDEX_BITS: u32 = 24;
const STACK_INDEX_MASK: usize = (1 << STACK_INDEX_BITS) - 1;
const STACK_VERSION: usize = 1 << STACK_INDEX_BITS;

impl PageStack {
    const fn new() -> Self {
        Self {
            head: AtomicUsize::new(0),
            count: AtomicUsize::new(0),
        }
    }

    /// The head that replaces `head` with top index `index`; the version
    /// part (above the index bits) always moves on.
    #[inline]
    fn successor(head: usize, index: usize) -> usize {
        ((head & !STACK_INDEX_MASK).wrapping_add(STACK_VERSION)) | (index & STACK_INDEX_MASK)
    }

    /// # Safety
    /// `entry` is a 64 KiB-aligned entry of the reservation that the caller
    /// owns exclusively and that sits in no stack.
    unsafe fn push(&self, entry: usize) {
        let index = ((entry - RANGE.base.load(Ordering::Relaxed)) >> STACK_UNIT_SHIFT) + 1;
        debug_assert!(entry & (LARGE_UNIT - 1) == 0 && index <= STACK_INDEX_MASK);
        // SAFETY: per the contract the entry is mapped.
        let link = unsafe { stack_link(entry) };
        self.count.fetch_add(1, Ordering::Relaxed);
        let mut head = self.head.load(Ordering::Relaxed);
        loop {
            link.store(head & STACK_INDEX_MASK, Ordering::Relaxed);
            match self.head.compare_exchange_weak(
                head,
                Self::successor(head, index),
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(current) => head = current,
            }
        }
    }

    /// Take the top entry; 0 when the stack is empty.
    fn pop(&self) -> usize {
        let base = RANGE.base.load(Ordering::Relaxed);
        let mut head = self.head.load(Ordering::Acquire);
        loop {
            let index = head & STACK_INDEX_MASK;
            if index == 0 {
                return 0;
            }
            let entry = base + ((index - 1) << STACK_UNIT_SHIFT);
            // SAFETY: entries stay mapped for the life of the process and the
            // link is only accessed atomically. If another thread took the
            // entry since `head` was read, the value may be stale; the
            // versioned compare-exchange then fails.
            let next = unsafe { stack_link(entry) }.load(Ordering::Relaxed);
            match self.head.compare_exchange_weak(
                head,
                Self::successor(head, next),
                Ordering::Acquire,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.count.fetch_sub(1, Ordering::Relaxed);
                    return entry;
                }
                Err(current) => head = current,
            }
        }
    }
}

/// The link word of a stack entry: in the page's header entry, or in the
/// large block's header.
///
/// # Safety
/// `entry` is a 64 KiB-aligned entry of the reservation.
#[inline]
unsafe fn stack_link(entry: usize) -> &'static AtomicUsize {
    let link = if pool_offset(entry) < RANGE.large_start.load(Ordering::Relaxed) {
        header_of(entry) as usize + HEADER_LINK
    } else {
        entry + STACK_LINK
    };
    // SAFETY: per the contract the word lies inside the mapped reservation,
    // and every bit pattern is a valid `AtomicUsize`.
    unsafe { &*(link as *const AtomicUsize) }
}

/// Fully free pages per region: resident ones, and ones whose body went
/// back to the OS (reused last, since touching them faults again).
static SMALL_WARM: PageStack = PageStack::new();
static SMALL_COLD: PageStack = PageStack::new();
static MEDIUM_WARM: PageStack = PageStack::new();
static MEDIUM_COLD: PageStack = PageStack::new();
/// Free large blocks per unit count, split the same way.
static LARGE_WARM: [PageStack; LARGE_BUCKETS] = [const { PageStack::new() }; LARGE_BUCKETS];
static LARGE_COLD: [PageStack; LARGE_BUCKETS] = [const { PageStack::new() }; LARGE_BUCKETS];
/// Bytes in `LARGE_WARM`: added before a push, subtracted after a pop.
static LARGE_WARM_BYTES: AtomicUsize = AtomicUsize::new(0);
/// Bytes handed back to the OS so far (diagnostics).
static RECLAIMED_BYTES: AtomicUsize = AtomicUsize::new(0);

/// Pages whose owner thread finished, per class.
struct OrphanPages([*mut PageHeader; NUM_CLASSES]);
// SAFETY: orphan pages are only dereferenced under the mutex or after
// adoption by exactly one thread.
unsafe impl Send for OrphanPages {}
static ORPHANS: Mutex<OrphanPages> = Mutex::new(OrphanPages([std::ptr::null_mut(); NUM_CLASSES]));
/// Number of orphaned pages; lets refills skip the mutex when there are none.
static ORPHAN_COUNT: AtomicUsize = AtomicUsize::new(0);
/// 0 = undecided, 1 = pool enabled, 2 = system allocator only.
static MODE: AtomicU32 = AtomicU32::new(0);

unsafe impl Sync for PhpHeap {}

#[cold]
fn decide_mode() -> u32 {
    // SAFETY: getenv reads process environment without allocating.
    let system_only = unsafe {
        let value = libc::getenv(c"RPHP_HEAP".as_ptr());
        !value.is_null() && libc::strcmp(value, c"system".as_ptr()) == 0
    };
    let mode = if system_only { 2 } else { 1 };
    let _ = MODE.compare_exchange(0, mode, Ordering::AcqRel, Ordering::Acquire);
    MODE.load(Ordering::Acquire)
}

#[inline]
fn process_mode() -> u32 {
    match MODE.load(Ordering::Acquire) {
        0 => decide_mode(),
        mode => mode,
    }
}

#[cold]
fn reserve_range() -> bool {
    let mut size: usize = 64 << 30;
    while size >= (256 << 20) {
        // SAFETY: anonymous private mapping; MAP_NORESERVE keeps the range
        // virtual until touched.
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                -1,
                0,
            )
        };
        if base != libc::MAP_FAILED {
            // Transparent huge pages for the whole reservation (best effort;
            // the kernel honours it only in THP "always"/"madvise" modes):
            // pages are carved contiguously, so first touches fault in 2 MiB
            // extents instead of 4 KiB ones and the TLB covers 512 times
            // more heap per entry. `RPHP_HEAP_THP=0` disables it for A/B.
            // SAFETY: getenv reads process environment without allocating;
            // madvise on our own fresh mapping.
            unsafe {
                let setting = libc::getenv(c"RPHP_HEAP_THP".as_ptr());
                if setting.is_null() || *setting != b'0' as libc::c_char {
                    libc::madvise(base, size, libc::MADV_HUGEPAGE);
                }
            }
            let raw = base as usize;
            let base = (raw + LARGE_UNIT - 1) & !(LARGE_UNIT - 1);
            let reserve = (raw + size - base) & !(LARGE_UNIT - 1);
            let half = (reserve / 2) & MEDIUM_PAGE_MASK;
            let large_start = half + ((reserve / 4) & !(LARGE_UNIT - 1));
            // One header entry per 64 KiB unit below the large region.
            let small_start = ((large_start >> STACK_UNIT_SHIFT) << HEADER_SHIFT)
                .next_multiple_of(SMALL_PAGE_SIZE);
            match RANGE
                .base
                .compare_exchange(0, base, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {
                    RANGE.half.store(half, Ordering::Release);
                    RANGE.large_start.store(large_start, Ordering::Release);
                    RANGE.small_start.store(small_start, Ordering::Release);
                    SMALL_CURSOR.store(base + small_start, Ordering::Release);
                    MEDIUM_CURSOR.store(
                        (base + half + MEDIUM_PAGE_SIZE - 1) & MEDIUM_PAGE_MASK,
                        Ordering::Release,
                    );
                    LARGE_CURSOR.store(base + large_start, Ordering::Release);
                    #[cfg(feature = "php-heap-debug")]
                    debug_reserve_map(reserve);
                    RANGE.reserve.store(reserve, Ordering::Release);
                    return true;
                }
                Err(_) => {
                    // SAFETY: another thread won the race; release ours.
                    unsafe { libc::munmap(raw as *mut libc::c_void, size) };
                    // Wait for the winner to publish the cursors.
                    while RANGE.reserve.load(Ordering::Acquire) == 0 {
                        std::hint::spin_loop();
                    }
                    return true;
                }
            }
        }
        size /= 2;
    }
    false
}

#[inline]
fn ensure_reserved() -> bool {
    RANGE.reserve.load(Ordering::Acquire) != 0 || reserve_range()
}

/// Claim `bytes` of never-used address space from `cursor` below `limit`;
/// 0 when the region is exhausted. A compare-exchange loop, so a failed
/// claim never moves the cursor: undoing an overshooting `fetch_add` with a
/// `fetch_sub` would let a concurrent claim land on a range another thread
/// then gets again.
fn carve(cursor: &AtomicUsize, bytes: usize, limit: usize) -> usize {
    let mut current = cursor.load(Ordering::Relaxed);
    loop {
        if current + bytes > limit {
            return 0;
        }
        match cursor.compare_exchange_weak(
            current,
            current + bytes,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return current,
            Err(actual) => current = actual,
        }
    }
}

/// Offset of a pointer inside the reservation, or at least `reserve` when
/// it lies outside.
#[inline(always)]
fn pool_offset(ptr: usize) -> usize {
    ptr.wrapping_sub(RANGE.base.load(Ordering::Relaxed))
}

#[cfg(test)]
fn in_pool(ptr: usize) -> bool {
    pool_offset(ptr) < RANGE.reserve.load(Ordering::Relaxed)
}

/// Header entry of the page starting at `page`.
#[inline(always)]
fn header_of(page: usize) -> *mut PageHeader {
    let base = RANGE.base.load(Ordering::Relaxed);
    (base + (((page - base) >> STACK_UNIT_SHIFT) << HEADER_SHIFT)) as *mut PageHeader
}

/// Header entry of the small page holding `offset` (from the base).
#[inline(always)]
fn small_header(base: usize, offset: usize) -> *mut PageHeader {
    (base + ((offset >> STACK_UNIT_SHIFT) << HEADER_SHIFT)) as *mut PageHeader
}

/// Start of the page holding a small or medium pool pointer with the given
/// offset.
#[inline(always)]
fn page_start_at(ptr: usize, offset: usize) -> usize {
    let mask = if offset < RANGE.half.load(Ordering::Relaxed) {
        SMALL_PAGE_MASK
    } else {
        MEDIUM_PAGE_MASK
    };
    ptr & mask
}

/// Header of the page holding a small or medium pool pointer with the given
/// offset.
#[inline(always)]
fn page_header_at(ptr: usize, offset: usize) -> *mut PageHeader {
    header_of(page_start_at(ptr, offset))
}

/// Header of the page holding a small or medium pool pointer.
#[cfg(test)]
fn page_header(ptr: usize) -> *mut PageHeader {
    page_header_at(ptr, pool_offset(ptr))
}

/// Address of the page holding a pool pointer (diagnostics; only meaningful
/// for small and medium pool pointers).
#[inline(always)]
pub fn page_of(ptr: usize) -> usize {
    page_start_at(ptr, pool_offset(ptr))
}

/// Offset of a page's first block: 64-byte steps chosen by the page address.
#[inline]
fn page_color(page: usize, page_size: usize) -> usize {
    ((page / page_size) % COLORS) * COLOR_STEP
}

#[inline(always)]
fn is_small_page(page: usize) -> bool {
    pool_offset(page) < RANGE.half.load(Ordering::Relaxed)
}

/// Whether a pool offset lies in the large region.
#[inline(always)]
fn is_large_offset(offset: usize) -> bool {
    offset >= RANGE.large_start.load(Ordering::Relaxed)
}

/// The OS page size (4 KiB on x86-64 Linux; 16 or 64 KiB on some AArch64
/// kernels).
fn os_page_size() -> usize {
    static CACHED: AtomicUsize = AtomicUsize::new(0);
    let cached = CACHED.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }
    // SAFETY: sysconf only reads a system constant.
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let size = if size > 0 { size as usize } else { 4096 };
    CACHED.store(size, Ordering::Relaxed);
    size
}

/// Return `[start, end)` to the OS. `MADV_DONTNEED` needs OS-page-aligned
/// bounds (a 128-byte header offset made the kernel reject every call with
/// `EINVAL`); callers keep whatever metadata must stay resident outside the
/// range. Returns whether the kernel dropped the range.
///
/// # Safety
/// `[start, end)` is mapped private anonymous memory of the caller whose
/// contents nobody needs any more; both bounds are OS-page aligned.
unsafe fn reclaim(start: usize, end: usize) -> bool {
    if start >= end {
        return false;
    }
    // SAFETY: per the contract.
    let dropped =
        unsafe { libc::madvise(start as *mut libc::c_void, end - start, libc::MADV_DONTNEED) } == 0;
    if dropped {
        RECLAIMED_BYTES.fetch_add(end - start, Ordering::Relaxed);
    }
    dropped
}

/// Header in front of every large block: its length in `LARGE_UNIT`s. It
/// shares the first OS page with the stack link, which never goes back to
/// the OS.
#[repr(C)]
struct LargeHeader {
    units: u32,
}

/// A block of at least `size` bytes from the large region, or null when the
/// region is exhausted.
#[cold]
#[inline(never)]
fn alloc_large(size: usize, zeroed: bool) -> *mut u8 {
    if !ensure_reserved() {
        return std::ptr::null_mut();
    }
    let units = (size + PAGE_HEADER).div_ceil(LARGE_UNIT);
    debug_assert!((1..=LARGE_BUCKETS).contains(&units));
    let bytes = units * LARGE_UNIT;
    let warm = LARGE_WARM[units - 1].pop();
    if warm != 0 {
        LARGE_WARM_BYTES.fetch_sub(bytes, Ordering::Relaxed);
        let block = (warm + PAGE_HEADER) as *mut u8;
        #[cfg(feature = "php-heap-debug")]
        debug_large(block as usize, false);
        if zeroed {
            // SAFETY: the block spans `bytes - PAGE_HEADER >= size` bytes.
            unsafe { std::ptr::write_bytes(block, 0, size) };
        }
        return block;
    }
    let cold = LARGE_COLD[units - 1].pop();
    if cold != 0 {
        let block = (cold + PAGE_HEADER) as *mut u8;
        #[cfg(feature = "php-heap-debug")]
        debug_large(block as usize, false);
        if zeroed {
            // Only the first OS page kept its bytes; the kernel refills the
            // rest of a reclaimed body with zeros on first touch.
            let stale = (os_page_size() - PAGE_HEADER).min(size);
            // SAFETY: the block spans at least `size` bytes.
            unsafe { std::ptr::write_bytes(block, 0, stale) };
        }
        return block;
    }
    let limit = RANGE.base.load(Ordering::Acquire) + RANGE.reserve.load(Ordering::Acquire);
    let header = carve(&LARGE_CURSOR, bytes, limit);
    if header == 0 {
        return std::ptr::null_mut();
    }
    // SAFETY: untouched (zero) memory of the reservation, claimed by us.
    unsafe {
        (header as *mut LargeHeader).write(LargeHeader {
            units: units as u32,
        })
    };
    (header + PAGE_HEADER) as *mut u8
}

/// Return a large block to its unit bucket; beyond the resident budget its
/// body goes back to the OS first.
///
/// # Safety
/// `ptr` was returned by `alloc_large` and is freed once.
#[cold]
#[inline(never)]
unsafe fn free_large(ptr: *mut u8) {
    #[cfg(feature = "php-heap-debug")]
    debug_large(ptr as usize, true);
    let entry = ptr as usize - PAGE_HEADER;
    // SAFETY: per the contract, the header precedes the block.
    let units = unsafe { (*(entry as *const LargeHeader)).units } as usize;
    let bytes = units * LARGE_UNIT;
    // SAFETY: the block is free and ours; entries are 64 KiB aligned and
    // `bytes` is a multiple of 64 KiB.
    unsafe {
        // The first OS page keeps the header and the stack link.
        if LARGE_WARM_BYTES.load(Ordering::Relaxed) >= LARGE_POOL_RESIDENT_BYTES
            && reclaim(
                (entry + PAGE_HEADER).next_multiple_of(os_page_size()),
                entry + bytes,
            )
        {
            LARGE_COLD[units - 1].push(entry);
        } else {
            LARGE_WARM_BYTES.fetch_add(bytes, Ordering::Relaxed);
            LARGE_WARM[units - 1].push(entry);
        }
    }
}

/// Usable size of a large block.
///
/// # Safety
/// `ptr` was returned by `alloc_large`.
#[inline]
unsafe fn large_capacity(ptr: *mut u8) -> usize {
    // SAFETY: per the contract.
    let units = unsafe { (*((ptr as usize - PAGE_HEADER) as *const LargeHeader)).units } as usize;
    units * LARGE_UNIT - PAGE_HEADER
}
