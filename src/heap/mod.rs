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
//!   page header are a range compare and a mask, no block headers. Small
//!   classes (up to 1 KiB) live on 64 KiB pages in the lower half of the
//!   range, medium classes (up to 32 KiB) on 1 MiB pages in the third
//!   quarter, blocks up to 4 MiB in 64 KiB units in the last quarter. Larger
//!   or over-aligned blocks go to the system allocator, outside the range.
//! * The fast paths touch only the thread's heap block and one page header:
//!   a class without a page points at a read-only sentinel page that can
//!   neither pop nor bump (no null test), the thread caches the range bounds
//!   (no global load on free), and one signed decrement of the page's live
//!   count detects both "the page emptied" and "the page was full" on free.
//! * Cross-thread frees land on a per-page lock-free list; the page queues
//!   itself at its owner, which drains the queue when it needs blocks. The
//!   page state word carries the owner heap pointer plus a queued flag and a
//!   pusher count, so an owner can give a page up (thread exit, page
//!   recycling) without racing a remote free.
//! * Pages count their live blocks; a page that becomes empty and is not the
//!   current page returns to a global pool reusable by any class. Beyond a
//!   resident budget the body of a pooled page (or free large block) goes
//!   back to the OS; such cold entries are reused only after warm ones. The
//!   pools are version-tagged lock-free stacks, immune to ABA.
//! * A finished thread orphans its pages; refills adopt them per class.
//!
//! With the `php-heap-asm` feature on x86-64 the allocation and free fast
//! paths are hand-written leaf functions (`fast_asm`), entered by a tail call
//! with the thread's heap pointer; every miss tail-jumps with the argument
//! registers intact into the same Rust slow paths.
//!
//! Setting `RPHP_HEAP=system` in the environment routes everything to the
//! system allocator (A/B measurements without rebuilding).
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub struct PhpHeap;

const SMALL_PAGE_SIZE: usize = 1 << 16;
const SMALL_PAGE_MASK: usize = !(SMALL_PAGE_SIZE - 1);
const MEDIUM_PAGE_SIZE: usize = 1 << 20;
const MEDIUM_PAGE_MASK: usize = !(MEDIUM_PAGE_SIZE - 1);
/// First block offset inside a page, and the header in front of a large
/// block: a multiple of 16 keeps 16-byte alignment for every class whose
/// block size is a multiple of 16.
const PAGE_HEADER: usize = 128;
/// Free pages and free large blocks sit in stacks of 64 KiB-aligned entries
/// of the reservation. An entry's link word lives here, past `PageHeader`,
/// so writing a page header never touches a word a stale pop may read.
const STACK_LINK: usize = PAGE_HEADER - 8;
const STACK_UNIT_SHIFT: u32 = 16;
/// Largest size served by the table-driven small classes.
const MAX_SMALL: usize = 1024;
/// Largest size served by the size-class pages.
const MAX_POOL: usize = 32 * 1024;
/// Large blocks (above `MAX_POOL`) are carved from the large region in
/// multiples of `LARGE_UNIT`, each preceded by a `PAGE_HEADER`-sized header,
/// and recycled through one stack per unit count.
const LARGE_UNIT: usize = 1 << STACK_UNIT_SHIFT;
const LARGE_BUCKETS: usize = 64;
/// Largest block served from the large region (64 units); bigger or
/// over-aligned requests go to the system allocator.
const LARGE_MAX: usize = LARGE_BUCKETS * LARGE_UNIT - PAGE_HEADER;
/// Free large-block bytes kept resident before further ones return their
/// memory to the OS.
const LARGE_POOL_RESIDENT_BYTES: usize = 256 << 20;
const MAX_ALIGN: usize = 16;
/// Fully free pages kept resident per region before further ones return
/// their memory to the OS (64 MiB each). PHPStan frees and reallocates
/// hundreds of MB between phases; a 16 MiB budget re-faulted a quarter of
/// all pages (and splits transparent huge pages).
const SMALL_POOL_RESIDENT: usize = 1024;
const MEDIUM_POOL_RESIDENT: usize = 64;

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

/// Class index by size (0..=1024), for alignment up to 8 and for alignment
/// 16 (only classes whose block size is a multiple of 16). Indexed by the
/// size itself, so the fast path needs no rounding; statics, so the asm fast
/// path can address them.
static CLASS_BY_SIZE: [u8; MAX_SMALL + 1] = build_class_table(8);
static CLASS_BY_SIZE_16: [u8; MAX_SMALL + 1] = build_class_table(16);

const fn build_class_table(align: usize) -> [u8; MAX_SMALL + 1] {
    let mut table = [0u8; MAX_SMALL + 1];
    let mut size = 0;
    while size <= MAX_SMALL {
        let mut class = 0;
        while (CLASS_SIZES[class] as usize) < size || (CLASS_SIZES[class] as usize) % align != 0 {
            class += 1;
        }
        table[size] = class as u8;
        size += 1;
    }
    table
}

#[inline(always)]
fn class_for(size: usize, align: usize) -> Option<usize> {
    if align > MAX_ALIGN {
        return None;
    }
    if size <= MAX_SMALL {
        let table = if align > 8 {
            &CLASS_BY_SIZE_16
        } else {
            &CLASS_BY_SIZE
        };
        return Some(table[size] as usize);
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

/// `PageHeader::used` flag: the page is neither current nor listed as
/// available. It was full when it stopped being current; its next local
/// free relists it.
const RETIRED: u32 = 1 << 31;
/// Extra count a current page carries, so frees on it never reach zero. The
/// free fast path's single signed test (`used - 1 <= 0`) then fires only for
/// an available page that emptied and for a retired page.
const CURRENT_BIAS: u32 = 1;

/// Per-page bookkeeping at the start of every page. Fields marked "owner"
/// are touched only by the owning thread. The first cache line holds what
/// the fast paths use; the second what other threads write.
#[repr(C)]
struct PageHeader {
    /// LIFO of blocks freed by the owner (owner).
    local_free: Cell<usize>,
    /// Bump cursor for never-used blocks (owner).
    bump: Cell<usize>,
    /// End of the usable block area.
    end: usize,
    /// Live blocks (handed out and not yet back on `local_free`, including
    /// blocks waiting in `remote_free`), plus `CURRENT_BIAS` while current,
    /// or `RETIRED` while unlisted (owner).
    used: Cell<u32>,
    block_size: u32,
    /// The owning heap's address, zero while orphaned or pooled. Every free
    /// compares against it; it changes only with `owner_state`.
    owner: AtomicUsize,
    class: u32,
    /// 0 = not listed, 1 = in the owner's per-class available list,
    /// 2 = the owner's current page of its class (owner).
    listing: Cell<u32>,
    /// Doubly linked per-class list of other pages with room (owner); the
    /// forward link doubles as the orphan-list link.
    next_avail: Cell<*mut PageHeader>,
    prev_avail: Cell<*mut PageHeader>,
    owner_state: AtomicUsize,
    /// Lock-free stack of blocks freed by other threads.
    remote_free: AtomicUsize,
    /// Intrusive link in the owner's remote queue.
    next_queued: AtomicUsize,
    /// Doubly linked list of every page the owner holds (owner).
    next_page: Cell<*mut PageHeader>,
    prev_page: Cell<*mut PageHeader>,
}

const LISTING_NONE: u32 = 0;
const LISTING_AVAIL: u32 = 1;
const LISTING_CURRENT: u32 = 2;

const _: () = assert!(std::mem::size_of::<PageHeader>() <= STACK_LINK);
// Remote freers write only the second cache line.
const _: () = assert!(std::mem::offset_of!(PageHeader, owner_state) >= 64);
const _: () = assert!(std::mem::offset_of!(PageHeader, prev_avail) + 8 <= 64);

/// Stand-in current page for classes without one: no free block, and a bump
/// cursor past its end, so both fast paths fall through to the refill
/// without a null test. It is never written: the fast paths store only after
/// a successful pop or bump. Laid out like the leading `PageHeader` fields
/// (`local_free` 0, `bump` 1, `end` 0, `used`/`block_size` 0).
#[repr(C, align(64))]
struct SentinelPage([usize; 4]);
static SENTINEL_PAGE: SentinelPage = SentinelPage([0, 1, 0, 0]);
const _: () = assert!(std::mem::offset_of!(PageHeader, local_free) == 0);
const _: () = assert!(std::mem::offset_of!(PageHeader, bump) == 8);
const _: () = assert!(std::mem::offset_of!(PageHeader, end) == 16);
const _: () = assert!(std::mem::offset_of!(PageHeader, used) == 24);
const _: () = assert!(std::mem::offset_of!(PageHeader, block_size) == 28);

#[inline(always)]
fn sentinel() -> *mut PageHeader {
    (&SENTINEL_PAGE as *const SentinelPage).cast_mut().cast()
}

/// Queue of an owner's pages that received remote frees. Other threads write
/// it, so it gets a cache line of its own.
#[repr(C, align(64))]
struct RemoteQueue(AtomicUsize);

#[repr(C, align(64))]
struct ThreadHeap {
    /// The page each class currently allocates from, or the sentinel.
    current: [Cell<*mut PageHeader>; NUM_CLASSES],
    /// `RANGE.base` and the size of the small-page region, cached so the free
    /// fast path needs no global load. Zero until the thread takes its first
    /// page or frees into the pool, which sends every free to the cold path.
    base: Cell<usize>,
    small_limit: Cell<usize>,
    /// Other owned pages of the class with free blocks or bump space.
    avail: [Cell<*mut PageHeader>; NUM_CLASSES],
    /// Head of the doubly linked list of every page this thread owns.
    pages: Cell<*mut PageHeader>,
    /// 0 undecided, 1 pool, 2 system (also after the thread gave its pages
    /// up at exit).
    mode: Cell<u8>,
    remote: RemoteQueue,
}

const _: () = assert!(std::mem::align_of::<ThreadHeap>() > STATE_FLAGS);

impl ThreadHeap {
    /// The initial state of a thread's heap. On x86-64 Linux the TLS image
    /// in `heap_tls` spells out the same bytes.
    #[cfg_attr(
        all(target_arch = "x86_64", target_os = "linux", not(test)),
        allow(dead_code)
    )]
    const fn new() -> Self {
        const NO_PAGE_YET: Cell<*mut PageHeader> =
            Cell::new((&SENTINEL_PAGE as *const SentinelPage).cast_mut().cast());
        const NO_PAGE: Cell<*mut PageHeader> = Cell::new(std::ptr::null_mut());
        Self {
            current: [NO_PAGE_YET; NUM_CLASSES],
            base: Cell::new(0),
            small_limit: Cell::new(0),
            avail: [NO_PAGE; NUM_CLASSES],
            pages: NO_PAGE,
            mode: Cell::new(0),
            remote: RemoteQueue(AtomicUsize::new(0)),
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
        if !page.is_null() {
            let mut orphans = ORPHANS.lock().unwrap_or_else(|poison| poison.into_inner());
            while !page.is_null() {
                // SAFETY: pages in the owner's list are live pool pages; the
                // state word is cleared under the pusher protocol, so no
                // remote freer touches this heap afterwards.
                unsafe {
                    let next = (*page).next_page.get();
                    let header = &*page;
                    let state = detach(header);
                    let mut live = header.used.get() & !RETIRED;
                    if header.listing.get() == LISTING_CURRENT {
                        live -= CURRENT_BIAS;
                    }
                    header.used.set(live);
                    header.listing.set(LISTING_NONE);
                    if live == 0
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
            // Every pusher that had this heap's address has finished;
            // whatever sits in the queue is ours to drop.
            let _ = self.remote.0.swap(0, Ordering::AcqRel);
        }
        self.pages.set(std::ptr::null_mut());
        for class in 0..NUM_CLASSES {
            self.current[class].set(sentinel());
            self.avail[class].set(std::ptr::null_mut());
        }
    }
}

/// Touched once per thread before it takes its first page; its destructor
/// orphans the thread's pages. Keeping the destructor off the heap block
/// leaves the hot path with a plain TLS access.
struct ExitGuard;

impl Drop for ExitGuard {
    fn drop(&mut self) {
        heap().orphan_all();
    }
}

thread_local! {
    static EXIT_GUARD: ExitGuard = const { ExitGuard };
}

const _: () = assert!(std::mem::offset_of!(ThreadHeap, current) == 0);

/// The thread's heap block on x86-64 Linux: a TLS block of our own,
/// addressed with the local-exec model (thread pointer plus a link-time
/// offset). A `thread_local!` of this library crate is compiled for a
/// shared-library context: every access is a general-dynamic
/// `__tls_get_addr` call that the linker only later rewrites into the same
/// two instructions, so the compiler saved and shuffled argument registers
/// around each access in the allocator's entry points. The image holds the
/// sentinel in every `current` slot and zeros elsewhere, like
/// `ThreadHeap::new()`.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
mod heap_tls {
    use super::{NUM_CLASSES, SENTINEL_PAGE, ThreadHeap};

    std::arch::global_asm!(
        ".pushsection .tdata.__rphp_php_heap_tls,\"awT\",@progbits",
        ".p2align 6",
        ".globl __rphp_php_heap_tls",
        ".hidden __rphp_php_heap_tls",
        ".type __rphp_php_heap_tls,@object",
        ".size __rphp_php_heap_tls,{size}",
        "__rphp_php_heap_tls:",
        ".rept {classes}",
        ".quad {sentinel}",
        ".endr",
        ".zero {rest}",
        ".popsection",
        size = const std::mem::size_of::<ThreadHeap>(),
        classes = const NUM_CLASSES,
        rest = const std::mem::size_of::<ThreadHeap>() - NUM_CLASSES * 8,
        sentinel = sym SENTINEL_PAGE,
    );
}

/// The current thread's heap block. The block lives until the thread exits
/// (through all TLS destructors) and is only touched by its own thread;
/// remote frees go through atomics in page headers and the remote queue.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
#[inline(always)]
fn heap() -> &'static ThreadHeap {
    let ptr: usize;
    // SAFETY: `%fs:0` holds the thread pointer's own address (x86-64 TLS
    // ABI); adding the block's link-time offset reads no other memory,
    // writes nothing and leaves the flags alone.
    unsafe {
        std::arch::asm!(
            "mov {ptr}, qword ptr fs:0",
            "lea {ptr}, [{ptr} + __rphp_php_heap_tls@tpoff]",
            ptr = out(reg) ptr,
            options(pure, readonly, nostack, preserves_flags),
        );
    }
    // SAFETY: the block is initialized from its image when the thread
    // starts and stays valid for the thread's whole life.
    unsafe { &*(ptr as *const ThreadHeap) }
}

#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
thread_local! {
    /// No destructor: a plain static TLS block on the fast path. `EXIT_GUARD`
    /// carries the thread-exit cleanup.
    static HEAP: ThreadHeap = const { ThreadHeap::new() };
}

/// The current thread's heap block (portable variant). Taking the address
/// once keeps the fast paths free of the `LocalKey::with` closure.
#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
#[inline(always)]
fn heap() -> &'static ThreadHeap {
    let ptr = HEAP.with(|heap| heap as *const ThreadHeap);
    // SAFETY: a `const`-initialized thread-local without destructor is valid
    // for the whole thread lifetime, including other TLS destructors.
    unsafe { &*ptr }
}

/// Reserved address range: `[base, base + reserve)`; small pages below
/// `base + half`, medium pages below `base + large_start`, large blocks
/// above.
#[repr(C, align(64))]
struct Range {
    base: AtomicUsize,
    half: AtomicUsize,
    reserve: AtomicUsize,
    large_start: AtomicUsize,
}
static RANGE: Range = Range {
    base: AtomicUsize::new(0),
    half: AtomicUsize::new(0),
    reserve: AtomicUsize::new(0),
    large_start: AtomicUsize::new(0),
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

/// The link word of a stack entry.
///
/// # Safety
/// `entry` is a 64 KiB-aligned entry of the reservation.
#[inline]
unsafe fn stack_link(entry: usize) -> &'static AtomicUsize {
    // SAFETY: per the contract the word lies inside the mapped reservation,
    // and every bit pattern is a valid `AtomicUsize`.
    unsafe { &*((entry + STACK_LINK) as *const AtomicUsize) }
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
            let base = base as usize;
            let half = size / 2;
            let large_start = half + size / 4;
            match RANGE
                .base
                .compare_exchange(0, base, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {
                    RANGE.half.store(half, Ordering::Release);
                    RANGE.large_start.store(large_start, Ordering::Release);
                    SMALL_CURSOR.store(
                        (base + SMALL_PAGE_SIZE - 1) & SMALL_PAGE_MASK,
                        Ordering::Release,
                    );
                    MEDIUM_CURSOR.store(
                        (base + half + MEDIUM_PAGE_SIZE - 1) & MEDIUM_PAGE_MASK,
                        Ordering::Release,
                    );
                    LARGE_CURSOR.store(
                        (base + large_start + LARGE_UNIT - 1) & !(LARGE_UNIT - 1),
                        Ordering::Release,
                    );
                    #[cfg(feature = "php-heap-debug")]
                    debug_reserve_map(size);
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

/// Header of the page holding a small or medium pool pointer with the given
/// offset.
#[inline(always)]
fn page_header_at(ptr: usize, offset: usize) -> *mut PageHeader {
    let mask = if offset < RANGE.half.load(Ordering::Relaxed) {
        SMALL_PAGE_MASK
    } else {
        MEDIUM_PAGE_MASK
    };
    (ptr & mask) as *mut PageHeader
}

/// Header of the page holding a small or medium pool pointer.
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

/// Return an entry's body to the OS, keeping the OS page that holds its
/// header and stack link. `MADV_DONTNEED` needs an OS-page-aligned start
/// and the header is only 128 bytes, so the release starts at the next OS
/// page. Returns whether the kernel dropped the range.
///
/// # Safety
/// `[entry, entry + len)` is mapped private anonymous memory of the caller,
/// `entry + len` is OS-page aligned, and nothing past the first OS page
/// holds data anyone still needs.
unsafe fn reclaim_body(entry: usize, len: usize) -> bool {
    let start = (entry + PAGE_HEADER).next_multiple_of(os_page_size());
    let end = entry + len;
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
        if LARGE_WARM_BYTES.load(Ordering::Relaxed) >= LARGE_POOL_RESIDENT_BYTES
            && reclaim_body(entry, bytes)
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
    // SAFETY: the block is ours and `block_size` bytes long.
    unsafe {
        debug_mark_free(page, block)
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
    let queue = unsafe { &(*((state & !STATE_FLAGS) as *const ThreadHeap)).remote.0 };
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
            Ok(_) => {
                page.owner.store(0, Ordering::Relaxed);
                return state;
            }
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
            Ok(_) => {
                page.owner.store(0, Ordering::Relaxed);
                return true;
            }
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
    debug_assert!((page.used.get() & !RETIRED) as usize >= count);
    page.used.set(page.used.get().wrapping_sub(count as u32));
    count
}

/// Move every queued page's remote frees onto their local lists; pages that
/// became empty are recycled, full ones that got room relisted.
#[inline(never)]
fn drain_remote(heap: &ThreadHeap) {
    let mut page = heap.remote.0.swap(0, Ordering::AcqRel);
    while page != 0 {
        // SAFETY: queued pages are live pool pages this heap owns: a page
        // is only queued while its state carries this heap's address, and
        // ownership is never given up while `QUEUED` is set except at
        // thread exit, which drops the queue.
        unsafe {
            let header = &*(page as *const PageHeader);
            let next = header.next_queued.load(Ordering::Relaxed);
            header.owner_state.fetch_and(!QUEUED, Ordering::AcqRel);
            let absorbed = absorb_remote(header);
            let page_ptr = page as *mut PageHeader;
            match header.listing.get() {
                LISTING_AVAIL => {
                    if header.used.get() == 0 {
                        heap.remove_avail(page_ptr);
                        recycle_or_list(heap, page_ptr);
                    }
                }
                LISTING_NONE => {
                    if absorbed != 0 {
                        let live = header.used.get() & !RETIRED;
                        header.used.set(live);
                        if live == 0 {
                            recycle_or_list(heap, page_ptr);
                        } else {
                            heap.push_avail(page_ptr);
                        }
                    }
                }
                // Current: the bias keeps it.
                _ => {}
            }
            page = next;
        }
    }
}

/// Initialize `page` for `class` (untouched or recycled memory). The stack
/// link past the header is left alone.
///
/// # Safety
/// `page` is a page-aligned address inside the right region of the
/// reservation that no other thread references.
#[inline]
unsafe fn init_page(page: usize, class: usize) -> *mut PageHeader {
    let header = page as *mut PageHeader;
    let page_size = if class < NUM_SMALL_CLASSES {
        SMALL_PAGE_SIZE
    } else {
        MEDIUM_PAGE_SIZE
    };
    // Debug mode: the page's free bits belong to its previous layout.
    #[cfg(feature = "php-heap-debug")]
    debug_clear_range(page, page_size);
    // SAFETY: per the contract; every field is written before the page
    // becomes reachable.
    unsafe {
        header.write(PageHeader {
            local_free: Cell::new(0),
            bump: Cell::new(page + PAGE_HEADER),
            end: page + page_size,
            used: Cell::new(0),
            block_size: CLASS_SIZES[class],
            owner: AtomicUsize::new(0),
            class: class as u32,
            listing: Cell::new(LISTING_NONE),
            next_avail: Cell::new(std::ptr::null_mut()),
            prev_avail: Cell::new(std::ptr::null_mut()),
            owner_state: AtomicUsize::new(0),
            remote_free: AtomicUsize::new(0),
            next_queued: AtomicUsize::new(0),
            next_page: Cell::new(std::ptr::null_mut()),
            prev_page: Cell::new(std::ptr::null_mut()),
        });
    }
    header
}

/// A page for `class`: a resident pooled one, a reclaimed pooled one, or a
/// fresh one carved from the reservation. Null when the region is
/// exhausted. The reservation exists (the thread runs in pool mode).
#[cold]
fn new_page(class: usize) -> *mut PageHeader {
    let small = class < NUM_SMALL_CLASSES;
    let (warm, cold) = if small {
        (&SMALL_WARM, &SMALL_COLD)
    } else {
        (&MEDIUM_WARM, &MEDIUM_COLD)
    };
    let mut entry = warm.pop();
    if entry == 0 {
        entry = cold.pop();
    }
    if entry == 0 {
        let base = RANGE.base.load(Ordering::Acquire);
        entry = if small {
            carve(
                &SMALL_CURSOR,
                SMALL_PAGE_SIZE,
                base + RANGE.half.load(Ordering::Acquire),
            )
        } else {
            carve(
                &MEDIUM_CURSOR,
                MEDIUM_PAGE_SIZE,
                base + RANGE.large_start.load(Ordering::Acquire),
            )
        };
        if entry == 0 {
            return std::ptr::null_mut();
        }
    }
    // SAFETY: popped or carved, so exclusively ours.
    unsafe { init_page(entry, class) }
}

/// Put a detached, fully free page nobody references into its region's
/// pool. Beyond the resident budget its body goes back to the OS.
///
/// # Safety
/// `page` is a live pool page with no owner and no live blocks.
#[cold]
unsafe fn pool_page(page: *mut PageHeader) {
    let entry = page as usize;
    let (warm, cold, budget, page_size) = if is_small_page(entry) {
        (
            &SMALL_WARM,
            &SMALL_COLD,
            SMALL_POOL_RESIDENT,
            SMALL_PAGE_SIZE,
        )
    } else {
        (
            &MEDIUM_WARM,
            &MEDIUM_COLD,
            MEDIUM_POOL_RESIDENT,
            MEDIUM_PAGE_SIZE,
        )
    };
    // SAFETY: per the contract the page is ours alone and its body free.
    unsafe {
        if warm.count.load(Ordering::Relaxed) >= budget && reclaim_body(entry, page_size) {
            cold.push(entry);
        } else {
            warm.push(entry);
        }
    }
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

/// Recycle an empty page, or keep it listed as available when a remote
/// freer holds it queued.
///
/// # Safety
/// As for `recycle_page`.
#[cold]
unsafe fn recycle_or_list(heap: &ThreadHeap, page: *mut PageHeader) {
    // SAFETY: per the contract.
    unsafe {
        if !recycle_page(heap, page) {
            heap.push_avail(page);
        }
    }
}

/// Make `page` this heap's current page of its class (owner). The previous
/// current page is exhausted: it loses its bias and retires.
///
/// # Safety
/// `page` is a live page owned by (or being taken by) this thread, not
/// listed and not retired.
#[inline]
unsafe fn set_current(heap: &ThreadHeap, page: *mut PageHeader) {
    // SAFETY: per the contract; the previous current page is ours too.
    unsafe {
        let class = (*page).class as usize;
        let previous = heap.current.get_unchecked(class).replace(page);
        if previous != sentinel() {
            let live = (*previous).used.get() - CURRENT_BIAS;
            (*previous).listing.set(LISTING_NONE);
            if live == 0 {
                // Unreachable for an exhausted page, but a page without a
                // live block must not retire, or no free would relist it.
                (*previous).used.set(0);
                recycle_or_list(heap, previous);
            } else {
                (*previous).used.set(live | RETIRED);
            }
        }
        (*page).used.set((*page).used.get() + CURRENT_BIAS);
        (*page).listing.set(LISTING_CURRENT);
    }
}

/// Pop a block off `page`'s free list, or bump one (owner). `None` when the
/// page has neither; always `None` for the sentinel.
///
/// # Safety
/// `page` is a live page this thread owns, or the sentinel.
#[inline(always)]
unsafe fn page_alloc(page: *mut PageHeader) -> Option<*mut u8> {
    // SAFETY: per the contract; a free block keeps its successor in its
    // first word. The sentinel is only read: its list is empty and its bump
    // cursor lies past its end.
    unsafe {
        let head = (*page).local_free.get();
        if head != 0 {
            (*page).local_free.set(*(head as *const usize));
            (*page).used.set((*page).used.get() + 1);
            #[cfg(feature = "php-heap-debug")]
            debug_mark_taken(&*page, head, true);
            return Some(head as *mut u8);
        }
        let bump = (*page).bump.get();
        let next = bump + (*page).block_size as usize;
        if next <= (*page).end {
            (*page).bump.set(next);
            (*page).used.set((*page).used.get() + 1);
            #[cfg(feature = "php-heap-debug")]
            debug_mark_taken(&*page, bump, false);
            return Some(bump as *mut u8);
        }
    }
    None
}

/// Take an orphaned page of `class`, or null.
fn adopt_orphan(class: usize) -> *mut PageHeader {
    if ORPHAN_COUNT.load(Ordering::Relaxed) == 0 {
        return std::ptr::null_mut();
    }
    let mut orphans = ORPHANS.lock().unwrap_or_else(|poison| poison.into_inner());
    let page = orphans.0[class];
    if !page.is_null() {
        // SAFETY: orphan pages are live pool pages nobody owns.
        orphans.0[class] = unsafe { (*page).next_avail.get() };
        ORPHAN_COUNT.fetch_sub(1, Ordering::Relaxed);
    }
    page
}

/// Slow path: the current page of `class` has neither free blocks nor bump
/// space. Find another source and allocate from it. Returns null when the
/// reservation is exhausted.
#[cold]
#[inline(never)]
fn refill(heap: &ThreadHeap, class: usize) -> *mut u8 {
    // 1. Remote frees may have refilled pages of this class.
    if heap.remote.0.load(Ordering::Relaxed) != 0 {
        drain_remote(heap);
    }
    loop {
        // 2. The current page itself (after a drain it may have blocks again).
        // SAFETY: the current page is a live page this thread owns, or the
        // sentinel.
        if let Some(ptr) = unsafe { page_alloc(heap.current[class].get()) } {
            return ptr;
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
        // 4. An orphaned page of this class, else a pooled or fresh page.
        let mut page = adopt_orphan(class);
        if page.is_null() {
            page = new_page(class);
            if page.is_null() {
                return std::ptr::null_mut();
            }
        }
        // SAFETY: the page is exclusively ours: publish ownership, link it,
        // recover its remote frees, and let the loop allocate from it.
        unsafe {
            (*page).owner_state.store(heap.addr(), Ordering::Release);
            (*page).owner.store(heap.addr(), Ordering::Relaxed);
            heap.link_page(page);
            set_current(heap, page);
            absorb_remote(&*page);
        }
    }
}

/// First slow-path visit of a thread: decide the mode, and in pool mode
/// reserve the range, arm the exit guard and cache the range bounds.
#[cold]
#[inline(never)]
fn init_thread(heap: &ThreadHeap) -> u8 {
    // Allocations made while this runs (registering the exit guard may
    // allocate) take the system allocator.
    heap.mode.set(2);
    let mut mode = process_mode() as u8;
    // A thread that cannot clean up at exit never takes pages.
    if mode == 1 && !(ensure_reserved() && EXIT_GUARD.try_with(|_| ()).is_ok()) {
        mode = 2;
    }
    if mode == 1 {
        heap.base.set(RANGE.base.load(Ordering::Acquire));
        heap.small_limit.set(RANGE.half.load(Ordering::Acquire));
    }
    heap.mode.set(mode);
    mode
}

/// Cold continuation of the allocation fast path: the class has no current
/// page with room. Decides the mode on first use, refills the class, and
/// falls back to the system allocator when the pool is off or exhausted.
#[cold]
#[inline(never)]
fn alloc_slow(size: usize, align: usize, heap: &ThreadHeap, class: usize, zeroed: bool) -> *mut u8 {
    let mut mode = heap.mode.get();
    if mode == 0 {
        mode = init_thread(heap);
    }
    if mode == 1 {
        let ptr = refill(heap, class);
        if !ptr.is_null() {
            if zeroed {
                // SAFETY: the block holds at least `size` bytes.
                unsafe { std::ptr::write_bytes(ptr, 0, size) };
            }
            return ptr;
        }
    }
    // SAFETY: `size`/`align` come from a valid `Layout`.
    unsafe {
        let layout = Layout::from_size_align_unchecked(size, align);
        if zeroed {
            System.alloc_zeroed(layout)
        } else {
            System.alloc(layout)
        }
    }
}

/// Requests the small fast path does not take: 16-aligned small blocks,
/// medium blocks, large and over-aligned ones.
#[cold]
#[inline(never)]
fn alloc_other(size: usize, align: usize, heap: &ThreadHeap, zeroed: bool) -> *mut u8 {
    let Some(class) = class_for(size, align) else {
        // SAFETY: `size`/`align` come from a valid `Layout`.
        return unsafe { system_alloc(size, align, zeroed) };
    };
    // SAFETY: `class < NUM_CLASSES`; the current page is a live page this
    // thread owns, or the sentinel.
    let Some(ptr) = (unsafe { page_alloc(heap.current.get_unchecked(class).get()) }) else {
        return alloc_slow(size, align, heap, class, zeroed);
    };
    if zeroed {
        // SAFETY: the block holds at least `size` bytes.
        unsafe { std::ptr::write_bytes(ptr, 0, size) };
    }
    ptr
}

/// Owner-local free whose signed decrement fired: the page emptied (an
/// available page) or was retired (full when it stopped being current).
///
/// # Safety
/// `page` is a live page this thread owns; the freed block is already back
/// on its local list and counted off `used`.
#[cold]
#[inline(never)]
unsafe fn free_cold(page: *mut PageHeader, heap: &ThreadHeap) {
    // SAFETY: per the contract.
    unsafe {
        let used = (*page).used.get();
        if used & RETIRED != 0 {
            let live = used & !RETIRED;
            (*page).used.set(live);
            if live == 0 {
                recycle_or_list(heap, page);
            } else {
                heap.push_avail(page);
            }
        } else {
            // Current pages carry the bias, so this is an available page.
            debug_assert!(used == 0 && (*page).listing.get() == LISTING_AVAIL);
            heap.remove_avail(page);
            recycle_or_list(heap, page);
        }
    }
}

/// Free a pool block of `page`: onto the local list when this thread owns
/// the page, onto its remote list otherwise.
///
/// # Safety
/// `block` is a live block of the small or medium page `page`, freed once.
#[inline(always)]
unsafe fn pool_free_page(block: usize, page: *mut PageHeader, heap: &ThreadHeap) {
    // SAFETY: a pool pointer's page header is initialized.
    let header = unsafe { &*page };
    if header.owner.load(Ordering::Relaxed) != heap.addr() {
        // SAFETY: per the contract.
        return unsafe { remote_free(block, header) };
    }
    #[cfg(feature = "php-heap-debug")]
    // SAFETY: the block is ours and `block_size` bytes long.
    unsafe {
        debug_mark_free(header, block)
    };
    // SAFETY: the block is ours and free; its first word becomes the link.
    unsafe { *(block as *mut usize) = header.local_free.get() };
    header.local_free.set(block);
    let used = header.used.get().wrapping_sub(1);
    header.used.set(used);
    if (used as i32) <= 0 {
        // SAFETY: owner-only bookkeeping of a page that emptied or retired.
        unsafe { free_cold(page, heap) };
    }
}

/// Frees the small fast path does not take: medium and large pool blocks,
/// system blocks, and any free before the thread cached the range bounds.
///
/// # Safety
/// `ptr` was returned by this allocator for a layout of `size`/`align` and
/// is freed once.
#[cold]
#[inline(never)]
unsafe fn dealloc_other(ptr: *mut u8, size: usize, align: usize, heap: &ThreadHeap) {
    let reserve = RANGE.reserve.load(Ordering::Acquire);
    let offset = pool_offset(ptr as usize);
    if offset >= reserve {
        // SAFETY: outside the reservation: the block came from `System`.
        return unsafe { System.dealloc(ptr, Layout::from_size_align_unchecked(size, align)) };
    }
    if heap.base.get() == 0 {
        // Let this thread's next frees take the fast path.
        heap.base.set(RANGE.base.load(Ordering::Acquire));
        heap.small_limit.set(RANGE.half.load(Ordering::Acquire));
    }
    if is_large_offset(offset) {
        // SAFETY: large-region pointers come from `alloc_large`.
        return unsafe { free_large(ptr) };
    }
    let page = page_header_at(ptr as usize, offset);
    #[cfg(feature = "php-heap-debug")]
    // SAFETY: a pool pointer has an initialized page header.
    debug_check_free(unsafe { &*page }, ptr as usize, size, align);
    // SAFETY: per the contract.
    unsafe { pool_free_page(ptr as usize, page, heap) }
}

/// # Safety
/// `size`/`align` come from a valid `Layout`.
#[cold]
#[inline(never)]
unsafe fn system_alloc(size: usize, align: usize, zeroed: bool) -> *mut u8 {
    if align <= MAX_ALIGN && size <= LARGE_MAX && process_mode() == 1 {
        let ptr = alloc_large(size, zeroed);
        if !ptr.is_null() {
            return ptr;
        }
    }
    // SAFETY: forwarded verbatim.
    unsafe {
        let layout = Layout::from_size_align_unchecked(size, align);
        if zeroed {
            System.alloc_zeroed(layout)
        } else {
            System.alloc(layout)
        }
    }
}

/// Move a pool block into a block of another size.
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
            PhpHeap.dealloc(ptr, layout);
        }
        new_ptr
    }
}

/// `realloc` of anything but a small-page block.
///
/// # Safety
/// `GlobalAlloc::realloc` contract.
#[cold]
#[inline(never)]
unsafe fn realloc_other(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    let reserve = RANGE.reserve.load(Ordering::Acquire);
    let offset = pool_offset(ptr as usize);
    if offset >= reserve {
        // A system block stays with the system allocator; keeping origins
        // separate keeps `dealloc` a range check.
        // SAFETY: forwarded verbatim.
        return unsafe { System.realloc(ptr, layout, new_size) };
    }
    if is_large_offset(offset) {
        // SAFETY: large-region pointer from `alloc_large`.
        let capacity = unsafe { large_capacity(ptr) };
        // Keep the block while the new size still needs the large region
        // and uses more than half of it.
        if new_size > MAX_POOL && new_size <= capacity && new_size * 2 > capacity {
            return ptr;
        }
        // SAFETY: forwarded verbatim.
        return unsafe { realloc_copy(ptr, layout, new_size) };
    }
    let page = page_header_at(ptr as usize, offset);
    #[cfg(feature = "php-heap-debug")]
    // SAFETY: a pool pointer has an initialized page header.
    debug_check_free(
        unsafe { &*page },
        ptr as usize,
        layout.size(),
        layout.align(),
    );
    // SAFETY: a pool pointer's header records its class.
    if class_for(new_size, layout.align()) == Some(unsafe { (*page).class } as usize) {
        return ptr;
    }
    // SAFETY: forwarded verbatim.
    unsafe { realloc_copy(ptr, layout, new_size) }
}

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
    let page_base = page as *const PageHeader as usize;
    let misaligned = (block - page_base - PAGE_HEADER) % slot != 0;
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

/// Hand-written x86-64 fast paths (feature `php-heap-asm`; debug builds keep
/// the Rust paths and their checks).
///
/// Each is a leaf entered by a tail call from `GlobalAlloc` with the thread's
/// heap pointer in the next free argument register. A hit returns straight
/// to the allocator's caller; every miss tail-jumps with the argument
/// registers intact into an `extern "C"` Rust slow path. Nothing is saved or
/// restored and no result is re-tested by the compiler. Field offsets come
/// from `offset_of!`, so the Rust layout stays the single source of truth.
/// Hand-written x86-64 fast paths (feature `php-heap-asm`, Linux; debug
/// builds keep the Rust paths and their checks).
///
/// `__rust_alloc` and `__rust_dealloc` tail-call these leaves with their own
/// arguments. They reach the thread's heap block through `%fs` themselves.
/// A hit returns straight to the allocator's caller; every miss tail-jumps
/// with the argument registers intact into an `extern "C"` Rust slow path.
/// Nothing is saved or restored and no result is re-tested by the compiler.
/// Field offsets come from `offset_of!`, so the Rust layout stays the single
/// source of truth.
#[cfg(all(
    feature = "php-heap-asm",
    target_arch = "x86_64",
    target_os = "linux",
    not(feature = "php-heap-debug")
))]
mod fast_asm {
    use super::{
        CLASS_BY_SIZE, CLASS_BY_SIZE_16, MAX_ALIGN, MAX_SMALL, PageHeader, SMALL_PAGE_MASK,
        SMALL_PAGE_SIZE, ThreadHeap, alloc_other, alloc_slow, dealloc_other, free_cold, heap,
        remote_free,
    };
    use std::mem::offset_of;

    /// Small-class allocation: class by table, then pop the current page's
    /// free list or bump its cursor. The sentinel page misses both.
    ///
    /// # Safety
    /// `GlobalAlloc::alloc` contract for `size`/`align`.
    #[unsafe(naked)]
    pub(super) unsafe extern "C" fn alloc(size: usize, align: usize) -> *mut u8 {
        std::arch::naked_asm!(
            "cmp rsi, 8",
            "ja 4f",
            "lea rcx, [rip + {by_size}]",
            "cmp rdi, {max_small}",
            "ja {other}",
            "2:",
            "movzx edx, byte ptr [rcx + rdi]",
            "mov r8, qword ptr fs:[8*rdx + __rphp_php_heap_tls@tpoff]",
            "mov rax, qword ptr [r8 + {local_free}]",
            "test rax, rax",
            "jz 3f",
            "mov r9, qword ptr [rax]",
            "mov qword ptr [r8 + {local_free}], r9",
            "inc dword ptr [r8 + {used}]",
            "ret",
            "3:",
            "mov rax, qword ptr [r8 + {bump}]",
            "mov r9d, dword ptr [r8 + {block_size}]",
            "add r9, rax",
            "cmp r9, qword ptr [r8 + {end}]",
            "ja {refill}",
            "mov qword ptr [r8 + {bump}], r9",
            "inc dword ptr [r8 + {used}]",
            "ret",
            "4:",
            "cmp rsi, {max_align}",
            "ja {other}",
            "lea rcx, [rip + {by_size_16}]",
            "cmp rdi, {max_small}",
            "jbe 2b",
            "jmp {other}",
            by_size = sym CLASS_BY_SIZE,
            by_size_16 = sym CLASS_BY_SIZE_16,
            max_small = const MAX_SMALL,
            max_align = const MAX_ALIGN,
            local_free = const offset_of!(PageHeader, local_free),
            bump = const offset_of!(PageHeader, bump),
            end = const offset_of!(PageHeader, end),
            used = const offset_of!(PageHeader, used),
            block_size = const offset_of!(PageHeader, block_size),
            other = sym alloc_other_entry,
            refill = sym alloc_refill_entry,
        )
    }

    /// Free of a small-page block owned by this thread: push it onto the
    /// page's free list; a signed decrement of `used` sends emptied and
    /// retired pages to the cold path.
    ///
    /// # Safety
    /// `GlobalAlloc::dealloc` contract.
    #[unsafe(naked)]
    pub(super) unsafe extern "C" fn dealloc(ptr: *mut u8, size: usize, align: usize) {
        std::arch::naked_asm!(
            "mov rcx, qword ptr fs:0",
            "lea rcx, [rcx + __rphp_php_heap_tls@tpoff]",
            "mov rax, rdi",
            "sub rax, qword ptr [rcx + {base}]",
            "cmp rax, qword ptr [rcx + {small_limit}]",
            "jae {other}",
            "mov rax, rdi",
            "and rax, {page_mask}",
            "cmp qword ptr [rax + {owner}], rcx",
            "jne {remote}",
            "mov r8, qword ptr [rax + {local_free}]",
            "mov qword ptr [rdi], r8",
            "mov qword ptr [rax + {local_free}], rdi",
            "dec dword ptr [rax + {used}]",
            "jle {cold}",
            "ret",
            base = const offset_of!(ThreadHeap, base),
            small_limit = const offset_of!(ThreadHeap, small_limit),
            page_mask = const -(SMALL_PAGE_SIZE as i64),
            owner = const offset_of!(PageHeader, owner),
            local_free = const offset_of!(PageHeader, local_free),
            used = const offset_of!(PageHeader, used),
            other = sym dealloc_other_entry,
            remote = sym free_remote_entry,
            cold = sym free_cold_entry,
        )
    }

    /// # Safety
    /// Entered from `alloc` only, with its arguments.
    unsafe extern "C" fn alloc_other_entry(size: usize, align: usize) -> *mut u8 {
        alloc_other(size, align, heap(), false)
    }

    /// # Safety
    /// Entered from `alloc` only, with its arguments and the class it looked
    /// up.
    unsafe extern "C" fn alloc_refill_entry(size: usize, align: usize, class: usize) -> *mut u8 {
        alloc_slow(size, align, heap(), class, false)
    }

    /// # Safety
    /// Entered from `dealloc` only, with its arguments and the thread's heap.
    unsafe extern "C" fn dealloc_other_entry(
        ptr: *mut u8,
        size: usize,
        align: usize,
        heap: *const ThreadHeap,
    ) {
        // SAFETY: forwarded `dealloc` contract; `dealloc` passes the
        // calling thread's heap.
        unsafe { dealloc_other(ptr, size, align, &*heap) }
    }

    /// # Safety
    /// Entered from `dealloc` only, for a small-page block of another
    /// thread's page.
    unsafe extern "C" fn free_remote_entry(ptr: *mut u8) {
        let block = ptr as usize;
        // SAFETY: a live small-page pool block and its page header.
        unsafe { remote_free(block, &*((block & SMALL_PAGE_MASK) as *const PageHeader)) }
    }

    /// # Safety
    /// Entered from `dealloc` only, after it freed a block onto a page of
    /// this thread whose `used` went to zero or below.
    unsafe extern "C" fn free_cold_entry(
        ptr: *mut u8,
        _size: usize,
        _align: usize,
        heap: *const ThreadHeap,
    ) {
        let page = (ptr as usize & SMALL_PAGE_MASK) as *mut PageHeader;
        // SAFETY: the page is ours and the block already counted off.
        unsafe { free_cold(page, &*heap) }
    }
}

#[cfg(all(
    feature = "php-heap-asm",
    target_arch = "x86_64",
    target_os = "linux",
    not(feature = "php-heap-debug")
))]
#[inline(always)]
fn fast_alloc(size: usize, align: usize) -> *mut u8 {
    // SAFETY: `size`/`align` come from a valid `Layout`.
    unsafe { fast_asm::alloc(size, align) }
}

#[cfg(all(
    feature = "php-heap-asm",
    target_arch = "x86_64",
    target_os = "linux",
    not(feature = "php-heap-debug")
))]
/// # Safety
/// `GlobalAlloc::dealloc` contract.
#[inline(always)]
unsafe fn fast_dealloc(ptr: *mut u8, size: usize, align: usize) {
    // SAFETY: forwarded contract.
    unsafe { fast_asm::dealloc(ptr, size, align) }
}

#[cfg(not(all(
    feature = "php-heap-asm",
    target_arch = "x86_64",
    target_os = "linux",
    not(feature = "php-heap-debug")
)))]
#[inline(always)]
fn fast_alloc(size: usize, align: usize) -> *mut u8 {
    let heap = heap();
    if align > 8 || size > MAX_SMALL {
        return alloc_other(size, align, heap, false);
    }
    // SAFETY: `size <= MAX_SMALL` indexes the table, whose classes index
    // `current`.
    let class = unsafe { *CLASS_BY_SIZE.get_unchecked(size) } as usize;
    // SAFETY: the current page is a live page this thread owns, or the
    // sentinel.
    match unsafe { page_alloc(heap.current.get_unchecked(class).get()) } {
        Some(ptr) => ptr,
        None => alloc_slow(size, align, heap, class, false),
    }
}

#[cfg(not(all(
    feature = "php-heap-asm",
    target_arch = "x86_64",
    target_os = "linux",
    not(feature = "php-heap-debug")
)))]
/// # Safety
/// `GlobalAlloc::dealloc` contract.
#[inline(always)]
unsafe fn fast_dealloc(ptr: *mut u8, size: usize, align: usize) {
    let heap = heap();
    let block = ptr as usize;
    if block.wrapping_sub(heap.base.get()) < heap.small_limit.get() {
        let page = (block & SMALL_PAGE_MASK) as *mut PageHeader;
        #[cfg(feature = "php-heap-debug")]
        // SAFETY: a pool pointer has an initialized page header.
        debug_check_free(unsafe { &*page }, block, size, align);
        // SAFETY: a small-page pool block, freed once per the contract.
        return unsafe { pool_free_page(block, page, heap) };
    }
    // SAFETY: forwarded contract.
    unsafe { dealloc_other(ptr, size, align, heap) }
}

unsafe impl GlobalAlloc for PhpHeap {
    /// # Safety
    /// `GlobalAlloc::alloc` contract: `layout` has non-zero size.
    #[inline(always)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        fast_alloc(layout.size(), layout.align())
    }

    /// # Safety
    /// `GlobalAlloc::alloc_zeroed` contract.
    #[inline(always)]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let (size, align) = (layout.size(), layout.align());
        if align > 8 || size > MAX_SMALL {
            return alloc_other(size, align, heap(), true);
        }
        let ptr = fast_alloc(size, align);
        if !ptr.is_null() {
            // SAFETY: the block holds at least `size` bytes.
            unsafe { std::ptr::write_bytes(ptr, 0, size) };
        }
        ptr
    }

    /// # Safety
    /// `ptr` was returned by this allocator for `layout` and is freed once.
    #[inline(always)]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded contract.
        unsafe { fast_dealloc(ptr, layout.size(), layout.align()) }
    }

    /// # Safety
    /// `GlobalAlloc::realloc` contract: `ptr`/`layout` as for `dealloc`.
    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let heap = heap();
        let block = ptr as usize;
        if block.wrapping_sub(heap.base.get()) < heap.small_limit.get() {
            let page = (block & SMALL_PAGE_MASK) as *const PageHeader;
            #[cfg(feature = "php-heap-debug")]
            // SAFETY: a pool pointer has an initialized page header.
            debug_check_free(unsafe { &*page }, block, layout.size(), layout.align());
            // SAFETY: a small-page pool pointer: its header records the class.
            if class_for(new_size, layout.align()) == Some(unsafe { (*page).class } as usize) {
                return ptr;
            }
            // SAFETY: forwarded contract.
            return unsafe { realloc_copy(ptr, layout, new_size) };
        }
        // SAFETY: forwarded contract.
        unsafe { realloc_other(ptr, layout, new_size) }
    }
}

/// Pages currently waiting in the orphan pool (diagnostics).
pub fn orphaned_pages() -> usize {
    ORPHAN_COUNT.load(Ordering::Relaxed)
}

/// Fully free pages currently in the global pools, resident or reclaimed
/// (diagnostics).
pub fn pooled_pages() -> usize {
    [&SMALL_WARM, &SMALL_COLD, &MEDIUM_WARM, &MEDIUM_COLD]
        .iter()
        .map(|stack| stack.count.load(Ordering::Relaxed))
        .sum()
}

/// Bytes whose memory went back to the OS so far (diagnostics).
pub fn reclaimed_bytes() -> usize {
    RECLAIMED_BYTES.load(Ordering::Relaxed)
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
