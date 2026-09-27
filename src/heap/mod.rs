//! PHP-tailored heap used as the process global allocator.
//!
//! Measured on a cold PHPStan run (24.6 M allocations): 99 % of blocks are
//! at most 1 KiB, 69 % at most 48 B, half of all blocks die before the second
//! following allocation, and 2.5 % are freed by a different thread than the
//! one that allocated them (the parser thread hands its AST to the main
//! thread). The heap is built around those facts:
//!
//! * Size classes tailored to RPHP's value layouts; every class allocates from
//!   one *current page* per thread. A page's own free list is served before
//!   its bump area, so a block freed a moment ago (still in L1) goes out
//!   next and consecutive allocations of a class share one page.
//! * Pages are carved from one reserved address range: ownership and the
//!   page header are a range compare and a mask, no block headers.
//! * Small classes (up to 1 KiB) live on 64 KiB pages in the lower half of the
//!   range, medium classes (up to 32 KiB) on 1 MiB pages in the upper half.
//!   Larger or over-aligned blocks go to the system allocator, whose blocks
//!   lie outside the range.
//! * Cross-thread frees land on a per-page lock-free list; the page queues
//!   itself at its owner, which drains the queue when it needs blocks. The
//!   page state word carries the owner heap pointer plus a queued flag and a
//!   pusher count, so an owner can give a page up (thread exit, page
//!   recycling) without racing a remote free.
//! * Pages count their live blocks; a page that becomes empty and is not the
//!   current page returns to a global pool reusable by any class, and beyond
//!   a resident budget its memory goes back to the OS.
//! * A finished thread orphans its pages; refills adopt them per class.
//!
//! Setting `RPHP_HEAP=system` in the environment routes everything to the
//! system allocator (A/B measurements without rebuilding).
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ptr::NonNull;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub struct PhpHeap;

const SMALL_PAGE_SHIFT: usize = 16;
const SMALL_PAGE_SIZE: usize = 1 << SMALL_PAGE_SHIFT;
const SMALL_PAGE_MASK: usize = !(SMALL_PAGE_SIZE - 1);
const MEDIUM_PAGE_SHIFT: usize = 20;
const MEDIUM_PAGE_SIZE: usize = 1 << MEDIUM_PAGE_SHIFT;
const MEDIUM_PAGE_MASK: usize = !(MEDIUM_PAGE_SIZE - 1);
/// First block offset inside a page: a multiple of 16 keeps 16-byte
/// alignment for every class whose block size is a multiple of 16.
const PAGE_HEADER: usize = 128;
/// Largest size served by the table-driven small classes.
const MAX_SMALL: usize = 1024;
/// Largest size served from the pool at all.
const MAX_POOL: usize = 32 * 1024;
const MAX_ALIGN: usize = 16;
/// Fully free pages kept resident per region before `madvise` returns the
/// memory of further ones to the OS (16 MiB each).
const SMALL_POOL_RESIDENT: usize = 256;
const MEDIUM_POOL_RESIDENT: usize = 16;

/// Block sizes. Small classes follow RPHP value layouts (`RcBox<String>` 40,
/// `Vec<Value>` of three 48, `RcBox<PhpArray>` 152, ...); medium classes
/// sit at 1.25/1.5/1.75/2 times each power of two. Every size is a multiple
/// of 8; sizes that are multiples of 16 serve 16-aligned requests.
const CLASS_SIZES: [u32; NUM_CLASSES] = [
    8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 152, 168, 192, 256, 384, 512, 768, 1024, 1280,
    1536, 1792, 2048, 2560, 3072, 3584, 4096, 5120, 6144, 7168, 8192, 10240, 12288, 14336, 16384,
    20480, 24576, 28672, 32768,
];
const NUM_SMALL_CLASSES: usize = 20;
const NUM_CLASSES: usize = 40;

/// Class index by `(size + 7) / 8` for sizes up to 1024; the second table
/// only admits classes whose block size is a multiple of 16, for 16-aligned
/// requests.
const CLASS_BY_OCTETS: [u8; MAX_SMALL / 8 + 1] = build_class_table(8);
const CLASS_BY_OCTETS_16: [u8; MAX_SMALL / 8 + 1] = build_class_table(16);

const fn build_class_table(align: usize) -> [u8; MAX_SMALL / 8 + 1] {
    let mut table = [0u8; MAX_SMALL / 8 + 1];
    let mut octets = 0;
    while octets <= MAX_SMALL / 8 {
        let size = octets * 8;
        let mut class = 0;
        while (CLASS_SIZES[class] as usize) < size || (CLASS_SIZES[class] as usize) % align != 0 {
            class += 1;
        }
        table[octets] = class as u8;
        octets += 1;
    }
    table
}

#[inline(always)]
fn class_for(size: usize, align: usize) -> Option<usize> {
    if align > MAX_ALIGN {
        return None;
    }
    if size <= MAX_SMALL {
        let octets = (size + 7) >> 3;
        // SAFETY: `octets <= MAX_SMALL / 8` after the size check; both tables
        // hold `MAX_SMALL / 8 + 1` entries.
        let class = unsafe {
            if align == 16 {
                *CLASS_BY_OCTETS_16.get_unchecked(octets)
            } else {
                *CLASS_BY_OCTETS.get_unchecked(octets)
            }
        };
        return Some(class as usize);
    }
    if size > MAX_POOL {
        return None;
    }
    // Four classes per power of two: 1.25x, 1.5x, 1.75x, 2x.
    let n = size - 1;
    let log = (usize::BITS - 1 - n.leading_zeros()) as usize;
    Some(NUM_SMALL_CLASSES + (log - 10) * 4 + ((n >> (log - 2)) & 3))
}

/// Owner-state word of a page: the owning heap's address (64-byte aligned)
/// in the high bits, `QUEUED` while the page sits in the owner's remote
/// queue, and a count of remote freers currently queueing it. Zero means
/// orphaned or pooled.
const QUEUED: usize = 1;
const PUSHER_UNIT: usize = 2;
const PUSHER_MASK: usize = 0x3E;
const STATE_FLAGS: usize = 0x3F;

/// Per-page bookkeeping at the start of every page. Fields marked "owner"
/// are touched only by the owning thread; the rest are shared. The owner's
/// hot fields lead so the fast paths touch one cache line.
#[repr(C)]
struct PageHeader {
    /// LIFO of blocks freed by the owner (owner).
    local_free: Cell<usize>,
    /// Bump cursor for never-used blocks (owner).
    bump: Cell<usize>,
    /// End of the usable block area.
    end: usize,
    /// Blocks currently handed out from this page (owner).
    used: Cell<u32>,
    class: u32,
    owner_state: AtomicUsize,
    /// Lock-free stack of blocks freed by other threads.
    remote_free: AtomicUsize,
    /// Intrusive link in the owner's remote queue, or in the global pool of
    /// free pages.
    next_queued: AtomicUsize,
    /// 0 = not listed, 1 = in the owner's per-class available list,
    /// 2 = the owner's current page of its class (owner).
    listing: Cell<u32>,
    capacity: u32,
    /// Doubly linked list of every page the owner holds (owner).
    next_page: Cell<*mut PageHeader>,
    prev_page: Cell<*mut PageHeader>,
    /// Doubly linked per-class list of other pages with room (owner); the
    /// forward link doubles as the orphan-list link.
    next_avail: Cell<*mut PageHeader>,
    prev_avail: Cell<*mut PageHeader>,
}

const LISTING_NONE: u32 = 0;
const LISTING_AVAIL: u32 = 1;
const LISTING_CURRENT: u32 = 2;

const _: () = assert!(std::mem::size_of::<PageHeader>() <= PAGE_HEADER);
// The asm fast path hard-codes these header offsets.
const _: () = assert!(std::mem::offset_of!(PageHeader, local_free) == 0);
const _: () = assert!(std::mem::offset_of!(PageHeader, bump) == 8);
const _: () = assert!(std::mem::offset_of!(PageHeader, end) == 16);
const _: () = assert!(std::mem::offset_of!(PageHeader, used) == 24);

#[repr(C, align(64))]
struct ThreadHeap {
    /// The page each class currently allocates from.
    current: [Cell<*mut PageHeader>; NUM_CLASSES],
    /// Other owned pages of the class with free blocks or bump space.
    avail: [Cell<*mut PageHeader>; NUM_CLASSES],
    /// Head of the doubly linked list of every page this thread owns.
    pages: Cell<*mut PageHeader>,
    /// Per-thread copy of the process mode (0 undecided, 1 pool, 2 system,
    /// also 2 after the thread orphaned its pages).
    mode: Cell<u8>,
    /// Lock-free stack of owned pages that received remote frees.
    remote_pages: AtomicUsize,
}

const _: () = assert!(std::mem::align_of::<ThreadHeap>() > STATE_FLAGS);

impl ThreadHeap {
    const fn new() -> Self {
        const NO_PAGE: Cell<*mut PageHeader> = Cell::new(std::ptr::null_mut());
        Self {
            current: [NO_PAGE; NUM_CLASSES],
            avail: [NO_PAGE; NUM_CLASSES],
            pages: NO_PAGE,
            mode: Cell::new(0),
            remote_pages: AtomicUsize::new(0),
        }
    }

    #[inline(always)]
    fn addr(&self) -> usize {
        self as *const ThreadHeap as usize
    }

    /// Link `page` into the owned-pages list (owner).
    ///
    /// # Safety
    /// `page` is a live page this thread now owns and is not linked yet.
    #[inline]
    unsafe fn link_page(&self, page: *mut PageHeader) {
        let head = self.pages.get();
        if head.is_null() {
            // First page of this thread: arm the exit cleanup. `try_with`
            // stays silent if the thread is already tearing down its TLS.
            let _ = EXIT_GUARD.try_with(|_| ());
        }
        // SAFETY: per the contract; `head` is a live owned page or null.
        unsafe {
            (*page).prev_page.set(std::ptr::null_mut());
            (*page).next_page.set(head);
            if !head.is_null() {
                (*head).prev_page.set(page);
            }
        }
        self.pages.set(page);
    }

    /// Unlink `page` from the owned-pages list (owner).
    ///
    /// # Safety
    /// `page` is linked in this thread's list.
    #[inline]
    unsafe fn unlink_page(&self, page: *mut PageHeader) {
        // SAFETY: per the contract; neighbours are live owned pages.
        unsafe {
            let prev = (*page).prev_page.get();
            let next = (*page).next_page.get();
            if prev.is_null() {
                self.pages.set(next);
            } else {
                (*prev).next_page.set(next);
            }
            if !next.is_null() {
                (*next).prev_page.set(prev);
            }
        }
    }

    /// Put `page` (not current, not listed) at the head of its class's
    /// available list (owner).
    ///
    /// # Safety
    /// `page` is a live owned page with `listing == LISTING_NONE`.
    #[inline]
    unsafe fn push_avail(&self, page: *mut PageHeader) {
        // SAFETY: per the contract; the class is a valid index.
        unsafe {
            let slot = self.avail.get_unchecked((*page).class as usize);
            let head = slot.get();
            (*page).listing.set(LISTING_AVAIL);
            (*page).prev_avail.set(std::ptr::null_mut());
            (*page).next_avail.set(head);
            if !head.is_null() {
                (*head).prev_avail.set(page);
            }
            slot.set(page);
        }
    }

    /// Remove `page` from its class's available list (owner).
    ///
    /// # Safety
    /// `page` is a live owned page with `listing == LISTING_AVAIL`.
    #[inline]
    unsafe fn remove_avail(&self, page: *mut PageHeader) {
        // SAFETY: per the contract; neighbours are live owned pages.
        unsafe {
            let prev = (*page).prev_avail.get();
            let next = (*page).next_avail.get();
            if prev.is_null() {
                self.avail.get_unchecked((*page).class as usize).set(next);
            } else {
                (*prev).next_avail.set(next);
            }
            if !next.is_null() {
                (*next).prev_avail.set(prev);
            }
            (*page).listing.set(LISTING_NONE);
        }
    }

    /// Give up every page: fully free ones go to the global pool, the rest
    /// to the orphan list. Nothing is allocated here. Afterwards the thread
    /// allocates from the system allocator.
    fn orphan_all(&self) {
        self.mode.set(2);
        let mut page = self.pages.get();
        if page.is_null() {
            return;
        }
        let mut orphans = ORPHANS.lock().unwrap_or_else(|poison| poison.into_inner());
        while !page.is_null() {
            // SAFETY: pages in the owner's list are live pool pages; the
            // state word is cleared under the pusher protocol, so no remote
            // freer touches this heap afterwards.
            unsafe {
                let next = (*page).next_page.get();
                let header = &*page;
                let state = detach(header);
                header.listing.set(LISTING_NONE);
                if header.used.get() == 0
                    && state & QUEUED == 0
                    && header.remote_free.load(Ordering::Acquire) == 0
                {
                    pool_page(page);
                } else {
                    let class = header.class as usize;
                    header.next_avail.set(orphans.0[class]);
                    orphans.0[class] = page;
                    ORPHAN_COUNT.fetch_add(1, Ordering::Relaxed);
                }
                page = next;
            }
        }
        // Every pusher that had this heap's address has finished; whatever
        // sits in the queue is ours to drop.
        let _ = self.remote_pages.swap(0, Ordering::AcqRel);
        self.pages.set(std::ptr::null_mut());
        for class in 0..NUM_CLASSES {
            self.current[class].set(std::ptr::null_mut());
            self.avail[class].set(std::ptr::null_mut());
        }
    }
}

/// Touched once per thread when it takes its first page; its destructor
/// orphans the thread's pages. Keeping the destructor off `HEAP` itself
/// leaves the hot path with a plain thread-local access.
struct ExitGuard;

impl Drop for ExitGuard {
    fn drop(&mut self) {
        HEAP.with(|heap| heap.orphan_all());
    }
}

thread_local! {
    static EXIT_GUARD: ExitGuard = const { ExitGuard };
}

thread_local! {
    /// No destructor: a plain static TLS block on the fast path. `EXIT_GUARD`
    /// carries the thread-exit cleanup.
    static HEAP: ThreadHeap = const { ThreadHeap::new() };
}

/// The current thread's heap block. Taking the address once keeps the fast
/// paths free of the `LocalKey::with` closure, which stopped inlining as the
/// body grew. The block lives until the thread exits and is only touched by
/// its own thread (remote frees go through atomics in page headers).
#[inline(always)]
fn heap() -> &'static ThreadHeap {
    let ptr = HEAP.with(|heap| heap as *const ThreadHeap);
    // SAFETY: a `const`-initialized thread-local without destructor is valid
    // for the whole thread lifetime, including other TLS destructors.
    unsafe { &*ptr }
}

/// Reserved address range: `[base, base + reserve)`; small pages below
/// `base + half`, medium pages above. One struct so the fast paths address
/// one static (one GOT load without LTO) and one cache line.
#[repr(C, align(64))]
struct Range {
    base: AtomicUsize,
    half: AtomicUsize,
    reserve: AtomicUsize,
}
static RANGE: Range = Range {
    base: AtomicUsize::new(0),
    half: AtomicUsize::new(0),
    reserve: AtomicUsize::new(0),
};
/// Next never-used page per region.
static SMALL_CURSOR: AtomicUsize = AtomicUsize::new(0);
static MEDIUM_CURSOR: AtomicUsize = AtomicUsize::new(0);
/// Lock-free stacks (via `next_queued`) of fully free pages per region.
static SMALL_POOL: AtomicUsize = AtomicUsize::new(0);
static MEDIUM_POOL: AtomicUsize = AtomicUsize::new(0);
static SMALL_POOL_COUNT: AtomicUsize = AtomicUsize::new(0);
static MEDIUM_POOL_COUNT: AtomicUsize = AtomicUsize::new(0);
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
            let base = base as usize;
            let half = size / 2;
            match RANGE
                .base
                .compare_exchange(0, base, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {
                    RANGE.half.store(half, Ordering::Release);
                    SMALL_CURSOR.store(
                        (base + SMALL_PAGE_SIZE - 1) & SMALL_PAGE_MASK,
                        Ordering::Release,
                    );
                    MEDIUM_CURSOR.store(
                        (base + half + MEDIUM_PAGE_SIZE - 1) & MEDIUM_PAGE_MASK,
                        Ordering::Release,
                    );
                    RANGE.reserve.store(size, Ordering::Release);
                    return true;
                }
                Err(_) => {
                    // SAFETY: another thread won the race; release ours.
                    unsafe { libc::munmap(base as *mut libc::c_void, size) };
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

/// Header of the page holding a pool pointer with the given offset.
#[inline(always)]
fn page_header_at(ptr: usize, offset: usize) -> *mut PageHeader {
    let mask = if offset < RANGE.half.load(Ordering::Relaxed) {
        SMALL_PAGE_MASK
    } else {
        MEDIUM_PAGE_MASK
    };
    (ptr & mask) as *mut PageHeader
}

/// Header of the page holding a pool pointer.
#[inline(always)]
fn page_header(ptr: usize) -> *mut PageHeader {
    page_header_at(ptr, pool_offset(ptr))
}

/// Address of the page holding a pool pointer (diagnostics; only meaningful
/// for pointers inside the reservation).
#[inline(always)]
pub fn page_of(ptr: usize) -> usize {
    page_header(ptr) as usize
}

#[inline(always)]
fn is_small_page(page: usize) -> bool {
    pool_offset(page) < RANGE.half.load(Ordering::Relaxed)
}

/// Push `block` onto the page's lock-free remote free list and, unless the
/// page is already queued or ownerless, queue the page at its owner.
///
/// # Safety
/// `block` must be a live block of `page` that no thread will touch again
/// until the owner drains the remote list; its first word is overwritten.
#[cold]
#[inline(never)]
unsafe fn remote_free(block: usize, page: &PageHeader) {
    #[cfg(feature = "php-heap-debug")]
    // SAFETY: the block is ours and at least `class size` bytes long.
    unsafe {
        debug_poison(page, block)
    };
    let mut head = page.remote_free.load(Ordering::Acquire);
    loop {
        // SAFETY: caller passes a live pool block; its first word is ours.
        unsafe { *(block as *mut usize) = head };
        match page.remote_free.compare_exchange_weak(
            head,
            block,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(current) => head = current,
        }
    }
    let mut state = page.owner_state.load(Ordering::Acquire);
    loop {
        if state & QUEUED != 0 || state & !STATE_FLAGS == 0 {
            // Already queued, or orphaned/pooled: the owner or adopter picks
            // the block up when it drains the page.
            return;
        }
        if state & PUSHER_MASK == PUSHER_MASK {
            std::hint::spin_loop();
            state = page.owner_state.load(Ordering::Acquire);
            continue;
        }
        match page.owner_state.compare_exchange_weak(
            state,
            (state | QUEUED) + PUSHER_UNIT,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(current) => state = current,
        }
    }
    // SAFETY: the pusher count keeps the owner from giving the page (and
    // thereby its heap address) up until we decrement it below.
    let queue = unsafe { &(*((state & !STATE_FLAGS) as *const ThreadHeap)).remote_pages };
    let mut head = queue.load(Ordering::Acquire);
    loop {
        page.next_queued.store(head, Ordering::Relaxed);
        match queue.compare_exchange_weak(
            head,
            page as *const PageHeader as usize,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(current) => head = current,
        }
    }
    page.owner_state.fetch_sub(PUSHER_UNIT, Ordering::AcqRel);
}

/// Clear the page's owner, waiting for in-flight remote pushers. Returns the
/// state before clearing (owner). Never fails: with `QUEUED` set the page may
/// still be referenced from the owner's queue, which callers must tolerate
/// (orphaning drops the queue; recycling refuses via `try_detach`).
///
/// # Safety
/// `page` is a live page owned by the calling thread.
#[inline]
unsafe fn detach(page: &PageHeader) -> usize {
    let mut state = page.owner_state.load(Ordering::Acquire);
    loop {
        if state & PUSHER_MASK != 0 {
            std::hint::spin_loop();
            state = page.owner_state.load(Ordering::Acquire);
            continue;
        }
        match page
            .owner_state
            .compare_exchange_weak(state, 0, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => return state,
            Err(current) => state = current,
        }
    }
}

/// Like `detach`, but refuses (returns false) while the page is queued at
/// this heap, so a stale queue entry never outlives the ownership.
///
/// # Safety
/// `page` is a live page owned by the calling thread.
#[inline]
unsafe fn try_detach(page: &PageHeader) -> bool {
    let mut state = page.owner_state.load(Ordering::Acquire);
    loop {
        if state & QUEUED != 0 {
            return false;
        }
        if state & PUSHER_MASK != 0 {
            std::hint::spin_loop();
            state = page.owner_state.load(Ordering::Acquire);
            continue;
        }
        match page
            .owner_state
            .compare_exchange_weak(state, 0, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => return true,
            Err(current) => state = current,
        }
    }
}

/// Move a remote chain onto the page's local list, fixing `used` (owner).
/// Returns the number of blocks moved.
///
/// # Safety
/// `page` is a live page owned by the calling thread.
#[inline]
unsafe fn absorb_remote(page: &PageHeader) -> usize {
    let remote = page.remote_free.swap(0, Ordering::AcqRel);
    if remote == 0 {
        return 0;
    }
    let mut count = 1usize;
    let mut tail = remote;
    // SAFETY: chain links are free pool blocks of this page.
    unsafe {
        while *(tail as *const usize) != 0 {
            tail = *(tail as *const usize);
            count += 1;
        }
        *(tail as *mut usize) = page.local_free.get();
    }
    page.local_free.set(remote);
    debug_assert!(page.used.get() as usize >= count);
    page.used.set(page.used.get().wrapping_sub(count as u32));
    count
}

/// Move every queued page's remote frees onto their local lists; pages that
/// became empty are recycled, the others made available.
#[inline(never)]
fn drain_remote(heap: &ThreadHeap) {
    let mut page = heap.remote_pages.swap(0, Ordering::AcqRel);
    while page != 0 {
        // SAFETY: queued pages are live pool pages this heap owns: a page
        // is only queued while its state carries this heap's address, and
        // ownership is never given up while `QUEUED` is set except at
        // thread exit, which drops the queue.
        unsafe {
            let header = &*(page as *const PageHeader);
            let next = header.next_queued.load(Ordering::Relaxed);
            header.owner_state.fetch_and(!QUEUED, Ordering::AcqRel);
            if absorb_remote(header) != 0 && header.listing.get() != LISTING_CURRENT {
                let page = page as *mut PageHeader;
                if header.used.get() == 0 {
                    if header.listing.get() == LISTING_AVAIL {
                        heap.remove_avail(page);
                    }
                    if !recycle_page(heap, page) {
                        heap.push_avail(page);
                    }
                } else if header.listing.get() == LISTING_NONE {
                    heap.push_avail(page);
                }
            }
            page = next;
        }
    }
}

/// Initialize `page` for `class` (untouched or recycled memory).
///
/// # Safety
/// `page` is a page-aligned address inside the right region of the
/// reservation that no other thread references.
#[inline]
unsafe fn init_page(page: usize, class: usize) -> *mut PageHeader {
    let header = page as *mut PageHeader;
    let size = CLASS_SIZES[class] as usize;
    let page_size = if class < NUM_SMALL_CLASSES {
        SMALL_PAGE_SIZE
    } else {
        MEDIUM_PAGE_SIZE
    };
    // SAFETY: per the contract; every field is written before the page
    // becomes reachable.
    unsafe {
        header.write(PageHeader {
            local_free: Cell::new(0),
            bump: Cell::new(page + PAGE_HEADER),
            end: page + page_size,
            used: Cell::new(0),
            class: class as u32,
            owner_state: AtomicUsize::new(0),
            remote_free: AtomicUsize::new(0),
            next_queued: AtomicUsize::new(0),
            listing: Cell::new(LISTING_NONE),
            capacity: ((page_size - PAGE_HEADER) / size) as u32,
            next_page: Cell::new(std::ptr::null_mut()),
            prev_page: Cell::new(std::ptr::null_mut()),
            next_avail: Cell::new(std::ptr::null_mut()),
            prev_avail: Cell::new(std::ptr::null_mut()),
        });
    }
    header
}

/// Pop a page from a region's pool of fully free pages.
fn pop_pool(pool: &AtomicUsize, count: &AtomicUsize) -> usize {
    let mut head = pool.load(Ordering::Acquire);
    while head != 0 {
        // SAFETY: pooled pages are live pool pages nobody owns; `next_queued`
        // links the pool.
        let next = unsafe {
            (*(head as *const PageHeader))
                .next_queued
                .load(Ordering::Relaxed)
        };
        match pool.compare_exchange_weak(head, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => {
                count.fetch_sub(1, Ordering::Relaxed);
                return head;
            }
            Err(current) => head = current,
        }
    }
    0
}

/// A page for `class`: recycled from the pool, or carved fresh from the
/// reservation. Null when the reservation is exhausted.
#[cold]
fn new_page(class: usize) -> *mut PageHeader {
    if RANGE.reserve.load(Ordering::Acquire) == 0 && !reserve_range() {
        return std::ptr::null_mut();
    }
    let small = class < NUM_SMALL_CLASSES;
    let recycled = if small {
        pop_pool(&SMALL_POOL, &SMALL_POOL_COUNT)
    } else {
        pop_pool(&MEDIUM_POOL, &MEDIUM_POOL_COUNT)
    };
    if recycled != 0 {
        // SAFETY: exclusively ours now.
        return unsafe { init_page(recycled, class) };
    }
    let base = RANGE.base.load(Ordering::Acquire);
    let (cursor, page_size, limit) = if small {
        (
            &SMALL_CURSOR,
            SMALL_PAGE_SIZE,
            base + RANGE.half.load(Ordering::Acquire),
        )
    } else {
        (
            &MEDIUM_CURSOR,
            MEDIUM_PAGE_SIZE,
            base + RANGE.reserve.load(Ordering::Acquire),
        )
    };
    let page = cursor.fetch_add(page_size, Ordering::AcqRel);
    if page + page_size > limit {
        cursor.fetch_sub(page_size, Ordering::AcqRel);
        return std::ptr::null_mut();
    }
    // SAFETY: an untouched page of the reservation, claimed by this thread.
    unsafe { init_page(page, class) }
}

/// Put a detached, fully free page nobody references into its region's
/// pool. Beyond the resident budget its block area goes back to the OS.
///
/// # Safety
/// `page` is a live pool page with no owner and no live blocks.
#[cold]
unsafe fn pool_page(page: *mut PageHeader) {
    let address = page as usize;
    let (pool, count, budget, page_size) = if is_small_page(address) {
        (
            &SMALL_POOL,
            &SMALL_POOL_COUNT,
            SMALL_POOL_RESIDENT,
            SMALL_PAGE_SIZE,
        )
    } else {
        (
            &MEDIUM_POOL,
            &MEDIUM_POOL_COUNT,
            MEDIUM_POOL_RESIDENT,
            MEDIUM_PAGE_SIZE,
        )
    };
    if count.load(Ordering::Relaxed) >= budget {
        // SAFETY: the block area of our own page; the header stays mapped
        // and is rewritten on reuse.
        unsafe {
            libc::madvise(
                (address + PAGE_HEADER) as *mut libc::c_void,
                page_size - PAGE_HEADER,
                libc::MADV_DONTNEED,
            );
        }
    }
    // SAFETY: per the contract, the header is ours to relink.
    unsafe {
        let mut head = pool.load(Ordering::Acquire);
        loop {
            (*page).next_queued.store(head, Ordering::Relaxed);
            match pool.compare_exchange_weak(head, address, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => break,
                Err(current) => head = current,
            }
        }
    }
    count.fetch_add(1, Ordering::Relaxed);
}

/// Give an empty, unlisted, non-current page back to the global pool.
/// Returns false (leaving the page owned) when a remote freer has queued it
/// in the meantime; the next drain retries.
///
/// # Safety
/// `page` is a live page owned by this thread with `used == 0` and
/// `listing == LISTING_NONE`.
#[cold]
#[inline(never)]
unsafe fn recycle_page(heap: &ThreadHeap, page: *mut PageHeader) -> bool {
    // SAFETY: per the contract.
    unsafe {
        if !try_detach(&*page) {
            return false;
        }
        heap.unlink_page(page);
        pool_page(page);
        true
    }
}

/// Make `page` this heap's current page of its class (owner).
///
/// # Safety
/// `page` is a live page owned by (or being taken by) this thread and is
/// not listed as available.
#[inline]
unsafe fn set_current(heap: &ThreadHeap, page: *mut PageHeader) {
    // SAFETY: per the contract.
    unsafe {
        let class = (*page).class as usize;
        let previous = heap.current.get_unchecked(class).replace(page);
        if !previous.is_null() {
            // The exhausted previous page comes back through its frees.
            (*previous).listing.set(LISTING_NONE);
        }
        (*page).listing.set(LISTING_CURRENT);
    }
}

/// Slow path: the current page of `class` has neither free blocks nor bump
/// space. Find another source and allocate from it. Returns null when the
/// reservation is exhausted.
#[cold]
#[inline(never)]
fn refill(heap: &ThreadHeap, class: usize) -> *mut u8 {
    let size = CLASS_SIZES[class] as usize;
    // 1. Remote frees may have refilled pages of this class.
    if heap.remote_pages.load(Ordering::Relaxed) != 0 {
        drain_remote(heap);
    }
    loop {
        // 2. The current page itself (after a drain it may have blocks again).
        let current = heap.current[class].get();
        if !current.is_null() {
            // SAFETY: a live page we own.
            unsafe {
                let head = (*current).local_free.get();
                if head != 0 {
                    (*current).local_free.set(*(head as *const usize));
                    (*current).used.set((*current).used.get() + 1);
                    return head as *mut u8;
                }
                let bump = (*current).bump.get();
                if bump + size <= (*current).end {
                    (*current).bump.set(bump + size);
                    (*current).used.set((*current).used.get() + 1);
                    return bump as *mut u8;
                }
            }
        }
        // 3. Another owned page with room.
        let avail = heap.avail[class].get();
        if !avail.is_null() {
            // SAFETY: pages in the avail list are live pages we own.
            unsafe {
                heap.remove_avail(avail);
                set_current(heap, avail);
            }
            continue;
        }
        // 4. Adopt an orphaned page of this class.
        let adopted = if ORPHAN_COUNT.load(Ordering::Relaxed) == 0 {
            std::ptr::null_mut()
        } else {
            let mut orphans = ORPHANS.lock().unwrap_or_else(|poison| poison.into_inner());
            let page = orphans.0[class];
            if !page.is_null() {
                // SAFETY: orphan pages are live pool pages nobody owns.
                orphans.0[class] = unsafe { (*page).next_avail.get() };
                ORPHAN_COUNT.fetch_sub(1, Ordering::Relaxed);
            }
            page
        };
        let page = if !adopted.is_null() {
            adopted
        } else {
            // 5. A recycled or fresh page.
            let page = new_page(class);
            if page.is_null() {
                return std::ptr::null_mut();
            }
            page
        };
        // SAFETY: the page is exclusively ours: publish ownership, link it,
        // recover its remote frees, and let the loop allocate from it.
        unsafe {
            (*page).owner_state.store(heap.addr(), Ordering::Release);
            heap.link_page(page);
            set_current(heap, page);
            absorb_remote(&*page);
        }
    }
}

/// Free-list pop or bump on the current page of `class`, hand-scheduled for
/// x86-64. Returns null when the class needs the cold refill. The header
/// offsets are pinned by the asserts next to `PageHeader`, so both
/// implementations share every invariant; the A/B between them (feature
/// `php-heap-asm`) measures the instruction selection alone.
#[cfg(all(feature = "php-heap-asm", target_arch = "x86_64"))]
#[inline(always)]
fn fast_alloc(heap: &ThreadHeap, class: usize) -> Option<NonNull<u8>> {
    debug_assert!(class < NUM_CLASSES);
    // SAFETY: `class` indexes `current` in bounds; the current page (if any)
    // is a live owned page whose `local_free`/`bump`/`end`/`used` live at
    // offsets 0/8/16/24 of its header; free blocks keep their successor in
    // the first word.
    let page = unsafe { heap.current.get_unchecked(class).get() } as usize;
    if page == 0 {
        return None;
    }
    // SAFETY: `class < NUM_CLASSES`.
    let size = unsafe { *CLASS_SIZES.get_unchecked(class) } as usize;
    let ptr: usize;
    // SAFETY: see above; only the page header and the popped block are
    // written.
    unsafe {
        std::arch::asm!(
            "mov {ptr}, qword ptr [{page}]",
            "test {ptr}, {ptr}",
            "jz 2f",
            "mov {tmp}, qword ptr [{ptr}]",
            "mov qword ptr [{page}], {tmp}",
            "jmp 3f",
            "2:",
            "mov {ptr}, qword ptr [{page} + 8]",
            "lea {tmp}, [{ptr} + {size}]",
            "cmp {tmp}, qword ptr [{page} + 16]",
            "ja 4f",
            "mov qword ptr [{page} + 8], {tmp}",
            "3:",
            "inc dword ptr [{page} + 24]",
            "jmp 5f",
            "4:",
            "xor {ptr:e}, {ptr:e}",
            "5:",
            ptr = out(reg) ptr,
            tmp = out(reg) _,
            page = in(reg) page,
            size = in(reg) size,
            options(nostack),
        );
    }
    NonNull::new(ptr as *mut u8)
}

#[cfg(not(all(feature = "php-heap-asm", target_arch = "x86_64")))]
#[inline(always)]
fn fast_alloc(heap: &ThreadHeap, class: usize) -> Option<NonNull<u8>> {
    debug_assert!(class < NUM_CLASSES);
    // SAFETY: `class` comes from `class_for` (< NUM_CLASSES); the current
    // page is a live page this thread owns; a free block keeps its successor
    // in its first word; blocks never sit at address zero.
    unsafe {
        let page = heap.current.get_unchecked(class).get();
        if page.is_null() {
            return None;
        }
        let head = (*page).local_free.get();
        if head != 0 {
            (*page).local_free.set(*(head as *const usize));
            (*page).used.set((*page).used.get() + 1);
            return Some(NonNull::new_unchecked(head as *mut u8));
        }
        let bump = (*page).bump.get();
        let next = bump + *CLASS_SIZES.get_unchecked(class) as usize;
        if next <= (*page).end {
            (*page).bump.set(next);
            (*page).used.set((*page).used.get() + 1);
            return Some(NonNull::new_unchecked(bump as *mut u8));
        }
    }
    None
}

/// Cold continuation of the allocation fast path: the class has no current
/// page with room. Decides the mode on first use, refills the class, and
/// falls back to the system allocator when the pool is off or exhausted. A
/// single tail-called cold function keeps the hot path free of callee-saved
/// registers.
#[cold]
#[inline(never)]
fn alloc_slow(size: usize, align: usize, class: usize, heap: &ThreadHeap, zeroed: bool) -> *mut u8 {
    // SAFETY: `size`/`align` come from a valid `Layout`.
    let layout = unsafe { Layout::from_size_align_unchecked(size, align) };
    let mut mode = heap.mode.get();
    if mode == 0 {
        mode = decide_mode() as u8;
        heap.mode.set(mode);
    }
    if mode == 1 {
        let ptr = refill(heap, class);
        if !ptr.is_null() {
            debug_unpoison(ptr, class);
            if zeroed {
                // SAFETY: the block holds at least `size` bytes.
                unsafe { std::ptr::write_bytes(ptr, 0, layout.size()) };
            }
            return ptr;
        }
    }
    // SAFETY: forwarded verbatim.
    unsafe {
        if zeroed {
            System.alloc_zeroed(layout)
        } else {
            System.alloc(layout)
        }
    }
}

/// Owner free that emptied a page other than the current one: recycle it.
///
/// # Safety
/// `block` lies on a live page this thread owns with `used == 0` and
/// `listing != LISTING_CURRENT`.
#[cold]
#[inline(never)]
unsafe fn free_emptied_page(block: usize, heap: &ThreadHeap) {
    // SAFETY: per the contract.
    unsafe {
        let page = page_header(block);
        if (*page).listing.get() == LISTING_AVAIL {
            heap.remove_avail(page);
        }
        if !recycle_page(heap, page) {
            heap.push_avail(page);
        }
    }
}

/// Owner free on a page that is neither current nor listed: list it.
///
/// # Safety
/// `block` lies on a live page this thread owns with `listing == LISTING_NONE`.
#[cold]
#[inline(never)]
unsafe fn free_relist_page(block: usize, heap: &ThreadHeap) {
    // SAFETY: per the contract.
    unsafe { heap.push_avail(page_header(block)) }
}

#[cfg(feature = "php-heap-debug")]
const POISON: u8 = 0xDE;
/// Marker in the second word of a freed block; not a repeated byte, so a
/// block filled with one value never looks freed.
#[cfg(feature = "php-heap-debug")]
const FREED_MARK: u64 = 0xF2EE_D0B1_0C4B_A5ED;

/// Debug mode: clear the poison marker of a block being handed out so a
/// later free of it is not mistaken for a double free.
#[cfg(feature = "php-heap-debug")]
#[inline(always)]
fn debug_unpoison(ptr: *mut u8, class: usize) {
    if CLASS_SIZES[class] >= 16 {
        // SAFETY: the block holds at least 16 bytes.
        unsafe { *(ptr as *mut u64).add(1) = 0 };
    }
}

#[cfg(not(feature = "php-heap-debug"))]
#[inline(always)]
fn debug_unpoison(_ptr: *mut u8, _class: usize) {}

/// Debug mode: poison a freed block beyond its link word and abort on a
/// double free (a block whose second word already carries the poison).
///
/// # Safety
/// `block` is a block of `page`.
#[cfg(feature = "php-heap-debug")]
#[inline]
unsafe fn debug_poison(page: &PageHeader, block: usize) {
    let size = CLASS_SIZES[page.class as usize] as usize;
    if size < 16 {
        return;
    }
    // SAFETY: per the contract.
    unsafe {
        let words = block as *mut u64;
        if *words.add(1) == FREED_MARK {
            libc::write(2, b"rphp heap: double free detected\n".as_ptr().cast(), 32);
            libc::abort();
        }
        std::ptr::write_bytes((block + 8) as *mut u8, POISON, size - 8);
        *words.add(1) = FREED_MARK;
    }
}

/// # Safety
/// `ptr` must come from the pool (`offset` is its `pool_offset`) and not be
/// freed twice.
#[inline(always)]
unsafe fn pool_free(ptr: *mut u8, offset: usize) {
    let block = ptr as usize;
    // SAFETY: the caller proved the pointer lies in the reservation, so its
    // page header is initialized.
    let page = unsafe { &*page_header_at(block, offset) };
    let heap = heap();
    if page.owner_state.load(Ordering::Relaxed) & !STATE_FLAGS != heap.addr() {
        // SAFETY: `page` is a live pool page; the block is live.
        return unsafe { remote_free(block, page) };
    }
    #[cfg(feature = "php-heap-debug")]
    // SAFETY: the block is ours and at least `class size` bytes long.
    unsafe {
        debug_poison(page, block)
    };
    // SAFETY: the block is ours and free; link it in front of the page's
    // local list.
    unsafe {
        *(block as *mut usize) = page.local_free.get();
    }
    page.local_free.set(block);
    let used = page.used.get().wrapping_sub(1);
    page.used.set(used);
    let listing = page.listing.get();
    if used == 0 {
        if listing != LISTING_CURRENT {
            // SAFETY: owner-only handling of an emptied page.
            unsafe { free_emptied_page(block, heap) };
        }
    } else if listing == LISTING_NONE {
        // SAFETY: owner-only list update.
        unsafe { free_relist_page(block, heap) };
    }
}

/// # Safety
/// `size`/`align` come from a valid `Layout` (`GlobalAlloc::alloc` contract).
/// Scalar arguments keep the fast path's registers in place for the tail
/// call.
#[cold]
#[inline(never)]
unsafe fn system_alloc(size: usize, align: usize) -> *mut u8 {
    // SAFETY: forwarded verbatim.
    unsafe { System.alloc(Layout::from_size_align_unchecked(size, align)) }
}

/// # Safety
/// `size`/`align` come from a valid `Layout`.
#[cold]
#[inline(never)]
unsafe fn system_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
    // SAFETY: forwarded verbatim.
    unsafe { System.alloc_zeroed(Layout::from_size_align_unchecked(size, align)) }
}

/// # Safety
/// `ptr` must have been returned by `System` for a layout of `size`/`align`.
#[cold]
#[inline(never)]
unsafe fn system_dealloc(ptr: *mut u8, size: usize, align: usize) {
    // SAFETY: the block was obtained from `System`.
    unsafe { System.dealloc(ptr, Layout::from_size_align_unchecked(size, align)) }
}

/// Grow or shrink a pool block by copying into a fresh block.
///
/// # Safety
/// `GlobalAlloc::realloc` contract for a pool pointer.
#[cold]
#[inline(never)]
unsafe fn realloc_copy(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    // SAFETY: standard grow-by-copy; both layouts are valid.
    unsafe {
        let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
        let new_ptr = PhpHeap.alloc(new_layout);
        if !new_ptr.is_null() {
            std::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
            pool_free(ptr, pool_offset(ptr as usize));
        }
        new_ptr
    }
}

/// The allocation fast path with direct returns: a free-list pop or a bump
/// on the current page of `class`, else the cold `alloc_slow`.
macro_rules! pool_alloc_or {
    ($heap:ident, $class:ident, $slow:expr) => {{
        let Some(ptr) = fast_alloc($heap, $class) else {
            return $slow;
        };
        let ptr = ptr.as_ptr();
        debug_unpoison(ptr, $class);
        ptr
    }};
}

unsafe impl GlobalAlloc for PhpHeap {
    /// # Safety
    /// `GlobalAlloc::alloc` contract: `layout` has non-zero size.
    #[inline(always)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if let Some(class) = class_for(layout.size(), layout.align()) {
            // With the pool off (or undecided) every current page is null,
            // so the slow path sorts the mode out; the hot path never reads
            // it.
            let heap = heap();
            return pool_alloc_or!(
                heap,
                class,
                alloc_slow(layout.size(), layout.align(), class, heap, false)
            );
        }
        // SAFETY: forwarded verbatim.
        unsafe { system_alloc(layout.size(), layout.align()) }
    }

    /// # Safety
    /// `GlobalAlloc::alloc_zeroed` contract.
    #[inline(always)]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if let Some(class) = class_for(layout.size(), layout.align()) {
            let heap = heap();
            let ptr = pool_alloc_or!(
                heap,
                class,
                alloc_slow(layout.size(), layout.align(), class, heap, true)
            );
            // SAFETY: the block holds at least `size` bytes.
            unsafe { std::ptr::write_bytes(ptr, 0, layout.size()) };
            return ptr;
        }
        // SAFETY: forwarded verbatim.
        unsafe { system_alloc_zeroed(layout.size(), layout.align()) }
    }

    /// # Safety
    /// `ptr` was returned by this allocator for `layout` and is freed once.
    #[inline(always)]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let offset = pool_offset(ptr as usize);
        if offset < RANGE.reserve.load(Ordering::Relaxed) {
            // SAFETY: pool pointers come from the pool paths of `alloc`.
            unsafe { pool_free(ptr, offset) };
        } else {
            // SAFETY: the block was obtained from `System`.
            unsafe { system_dealloc(ptr, layout.size(), layout.align()) }
        }
    }

    /// # Safety
    /// `GlobalAlloc::realloc` contract: `ptr`/`layout` as for `dealloc`.
    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let offset = pool_offset(ptr as usize);
        if offset < RANGE.reserve.load(Ordering::Relaxed) {
            // SAFETY: pool pointer; its page header records the class.
            let class = unsafe { (*page_header_at(ptr as usize, offset)).class } as usize;
            if let Some(new_class) = class_for(new_size, layout.align())
                && new_class == class
            {
                return ptr;
            }
            // SAFETY: forwarded verbatim.
            return unsafe { realloc_copy(ptr, layout, new_size) };
        }
        // A system block stays with the system allocator unless it now fits
        // the pool; keeping origins separate keeps `dealloc` a range check.
        // SAFETY: forwarded verbatim.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// Pages currently waiting in the orphan pool (diagnostics).
pub fn orphaned_pages() -> usize {
    ORPHAN_COUNT.load(Ordering::Relaxed)
}

/// Fully free pages currently in the global pools (diagnostics).
pub fn pooled_pages() -> usize {
    SMALL_POOL_COUNT.load(Ordering::Relaxed) + MEDIUM_POOL_COUNT.load(Ordering::Relaxed)
}

/// Pool pages carved out of the reservation so far (diagnostics).
pub fn pages_in_use() -> usize {
    let base = RANGE.base.load(Ordering::Relaxed);
    if RANGE.reserve.load(Ordering::Relaxed) == 0 {
        return 0;
    }
    let small = (SMALL_CURSOR.load(Ordering::Relaxed)
        - ((base + SMALL_PAGE_SIZE - 1) & SMALL_PAGE_MASK))
        / SMALL_PAGE_SIZE;
    let medium_start =
        (base + RANGE.half.load(Ordering::Relaxed) + MEDIUM_PAGE_SIZE - 1) & MEDIUM_PAGE_MASK;
    let medium = (MEDIUM_CURSOR.load(Ordering::Relaxed) - medium_start) / MEDIUM_PAGE_SIZE;
    small + medium
}

#[cfg(test)]
mod tests;
