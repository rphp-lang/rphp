//! PHP-tailored small-object heap used as the process global allocator.
//!
//! Measured on a cold PHPStan run (24.6 M allocations): 99 % of blocks are
//! at most 1 KiB, 69 % at most 48 B, half of all blocks die before the second
//! following allocation, and 2.5 % are freed by a different thread than the
//! one that allocated them (the parser thread hands its AST to the main
//! thread). The heap therefore keeps one LIFO free list per size class per
//! thread (the block freed a moment ago is still in L1), carves 64 KiB pages
//! out of one reserved address range so ownership is a range compare, and
//! moves cross-thread frees onto a per-page atomic list drained by the owner.
//! Pages of a finished thread are adopted by the next thread that refills the
//! same class. Blocks above 1 KiB or with alignment above 16 go to the system
//! allocator, whose blocks lie outside the reserved range.
//!
//! Setting `RPHP_HEAP=system` in the environment routes everything to the
//! system allocator (A/B measurements without rebuilding).
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub struct PhpHeap;

const PAGE_SHIFT: usize = 16;
const PAGE_SIZE: usize = 1 << PAGE_SHIFT;
const PAGE_MASK: usize = !(PAGE_SIZE - 1);
/// First block offset inside a page: keeps 16-byte alignment for every class
/// whose block size is a multiple of 16.
const PAGE_HEADER: usize = 64;
const MAX_SMALL: usize = 1024;
const MAX_ALIGN: usize = 16;

/// Block sizes tailored to RPHP value layouts (`RcBox<String>` 40,
/// `Vec<Value>` of three 48, `RcBox<PhpArray>` 152, ...). Every size is a
/// multiple of 8; sizes that are multiples of 16 serve 16-aligned requests.
const CLASS_SIZES: [u32; NUM_CLASSES] = [
    8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 152, 168, 192, 256, 384, 512, 768, 1024,
];
const NUM_CLASSES: usize = 20;

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
    if size > MAX_SMALL || align > MAX_ALIGN {
        return None;
    }
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
    Some(class as usize)
}

/// Per-page bookkeeping at the start of every 64 KiB page.
#[repr(C)]
struct PageHeader {
    class: u32,
    /// Owning thread id; 0 once the owner finished (orphaned).
    owner: AtomicU32,
    /// Lock-free stack of blocks freed by other threads.
    remote_free: AtomicUsize,
    /// Bump cursor for never-used blocks (owner only).
    bump: Cell<usize>,
    /// End of the usable block area.
    end: usize,
    /// Next page of the same class in the owner's list (owner only).
    next_page: Cell<*mut PageHeader>,
    /// The owner's heap, so a remote free can queue this page for draining.
    owner_heap: AtomicUsize,
    /// Intrusive link in the owner's queue of pages holding remote frees.
    next_queued: AtomicUsize,
    /// Set while the page sits in that queue.
    queued: AtomicU32,
}

const _: () = assert!(std::mem::size_of::<PageHeader>() <= PAGE_HEADER);
// The asm fast path hard-codes these header offsets.
const _: () = assert!(std::mem::offset_of!(PageHeader, bump) == 16);
const _: () = assert!(std::mem::offset_of!(PageHeader, end) == 24);

struct ThreadHeap {
    tid: Cell<u32>,
    /// Per-thread copy of the process mode (0 undecided, 1 pool, 2 system) so
    /// the fast path reads it from the thread block it touches anyway.
    mode: Cell<u8>,
    /// Lock-free stack of owned pages that received remote frees.
    remote_pages: AtomicUsize,
    /// LIFO free list per class; the first word of a free block links the next.
    free: [Cell<usize>; NUM_CLASSES],
    /// Pages with unused bump space, one per class.
    current: [Cell<*mut PageHeader>; NUM_CLASSES],
    /// Every page this thread owns, per class.
    pages: [Cell<*mut PageHeader>; NUM_CLASSES],
}

impl ThreadHeap {
    const fn new() -> Self {
        const NO_BLOCK: Cell<usize> = Cell::new(0);
        const NO_PAGE: Cell<*mut PageHeader> = Cell::new(std::ptr::null_mut());
        Self {
            tid: Cell::new(0),
            mode: Cell::new(0),
            remote_pages: AtomicUsize::new(0),
            free: [NO_BLOCK; NUM_CLASSES],
            current: [NO_PAGE; NUM_CLASSES],
            pages: [NO_PAGE; NUM_CLASSES],
        }
    }

    #[inline(always)]
    fn tid(&self) -> u32 {
        let tid = self.tid.get();
        if tid != 0 {
            return tid;
        }
        let tid = NEXT_TID.fetch_add(1, Ordering::Relaxed);
        self.tid.set(tid);
        // Registers the thread-exit destructor that orphans our pages.
        EXIT_GUARD.with(|_| ());
        tid
    }
}

/// Touched once per thread on first use; its destructor orphans the thread's
/// pages. Keeping the destructor off `HEAP` itself leaves the hot path with a
/// plain thread-local access and no registration check.
struct ExitGuard;

impl Drop for ExitGuard {
    fn drop(&mut self) {
        HEAP.with(orphan_pages);
    }
}

thread_local! {
    static EXIT_GUARD: ExitGuard = const { ExitGuard };
}

#[cold]
fn orphan_pages(heap: &ThreadHeap) {
    heap.orphan_all();
}

impl ThreadHeap {
    fn orphan_all(&self) {
        // Hand every page to the orphan pool. Blocks still on this thread's
        // free lists are pushed onto their pages' remote lists so the adopter
        // recovers them; nothing is allocated here. Pages lose their heap
        // pointer before this heap's storage goes away.
        let _ = self.remote_pages.swap(0, Ordering::AcqRel);
        for class in 0..NUM_CLASSES {
            let mut block = self.free[class].get();
            while block != 0 {
                // SAFETY: free blocks are live pool blocks whose first word is
                // the next link; the page header lives at the page start.
                unsafe {
                    let next = *(block as *const usize);
                    let page = &*((block & PAGE_MASK) as *const PageHeader);
                    remote_push(page, block);
                    block = next;
                }
            }
            self.free[class].set(0);
            let mut page = self.pages[class].get();
            if page.is_null() {
                continue;
            }
            let mut orphans = ORPHANS.lock().unwrap_or_else(|poison| poison.into_inner());
            while !page.is_null() {
                // SAFETY: pages in the owner's list are live pool pages.
                let next = unsafe {
                    (*page).owner_heap.store(0, Ordering::Release);
                    (*page).owner.store(0, Ordering::Release);
                    (*page).next_page.get()
                };
                unsafe { (*page).next_page.set(orphans.0[class]) };
                orphans.0[class] = page;
                ORPHAN_COUNT.fetch_add(1, Ordering::Relaxed);
                page = next;
            }
            self.pages[class].set(std::ptr::null_mut());
            self.current[class].set(std::ptr::null_mut());
        }
    }
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

static NEXT_TID: AtomicU32 = AtomicU32::new(1);
/// Reserved address range: `[HEAP_BASE, HEAP_BASE + HEAP_RESERVE)`.
static HEAP_BASE: AtomicUsize = AtomicUsize::new(0);
static HEAP_RESERVE: AtomicUsize = AtomicUsize::new(0);
/// Next never-used page in the reservation.
static PAGE_CURSOR: AtomicUsize = AtomicUsize::new(0);
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
            // Align the page cursor to a page boundary inside the mapping.
            let first_page = (base + PAGE_SIZE - 1) & PAGE_MASK;
            match HEAP_BASE.compare_exchange(0, base, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => {
                    HEAP_RESERVE.store(size, Ordering::Release);
                    PAGE_CURSOR.store(first_page, Ordering::Release);
                    return true;
                }
                Err(_) => {
                    // SAFETY: another thread won the race; release ours.
                    unsafe { libc::munmap(base as *mut libc::c_void, size) };
                    return true;
                }
            }
        }
        size /= 2;
    }
    false
}

#[inline(always)]
fn in_pool(ptr: usize) -> bool {
    let base = HEAP_BASE.load(Ordering::Relaxed);
    base != 0 && ptr.wrapping_sub(base) < HEAP_RESERVE.load(Ordering::Relaxed)
}

/// # Safety
/// `block` must be a free block of `page` that no thread will touch again
/// until the owner drains the remote list; its first word is overwritten.
/// Push `block` onto the page's lock-free remote free list and, the first
/// time since the owner last drained it, queue the page for that owner.
#[inline]
unsafe fn remote_push(page: &PageHeader, block: usize) {
    let mut head = page.remote_free.load(Ordering::Acquire);
    loop {
        // SAFETY: caller passes a free pool block; its first word is ours.
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
    if page.queued.swap(1, Ordering::AcqRel) == 0 {
        let owner_heap = page.owner_heap.load(Ordering::Acquire);
        if owner_heap == 0 {
            // Orphaned: the adopter drains the page when it takes it.
            return;
        }
        // SAFETY: a heap outlives every page it owns until `Drop` clears
        // `owner_heap`; a push racing with that only leaves a stale entry.
        let queue = unsafe { &(*(owner_heap as *const ThreadHeap)).remote_pages };
        let mut head = queue.load(Ordering::Acquire);
        loop {
            page.next_queued.store(head, Ordering::Relaxed);
            match queue.compare_exchange_weak(
                head,
                page as *const PageHeader as usize,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(current) => head = current,
            }
        }
    }
}

/// # Safety
/// Same contract as `remote_push`.
#[cold]
#[inline(never)]
unsafe fn remote_push_cold(page: &PageHeader, block: usize) {
    // SAFETY: forwarded from `pool_free` with the same contract.
    unsafe { remote_push(page, block) }
}

/// Move every queued page's remote frees onto the owner's free lists.
#[inline(never)]
fn drain_remote(heap: &ThreadHeap) {
    let mut page = heap.remote_pages.swap(0, Ordering::AcqRel);
    while page != 0 {
        // SAFETY: queued pages are live pool pages this heap owns.
        let header = unsafe { &*(page as *const PageHeader) };
        let next = header.next_queued.load(Ordering::Relaxed);
        header.queued.store(0, Ordering::Release);
        let remote = header.remote_free.swap(0, Ordering::AcqRel);
        if remote != 0 {
            let class = header.class as usize;
            let mut tail = remote;
            // SAFETY: chain links are free pool blocks.
            unsafe {
                while *(tail as *const usize) != 0 {
                    tail = *(tail as *const usize);
                }
                *(tail as *mut usize) = heap.free[class].get();
            }
            heap.free[class].set(remote);
        }
        page = next;
    }
}

/// Carve a fresh page for `class` out of the reservation.
#[cold]
fn new_page(class: usize) -> *mut PageHeader {
    if HEAP_BASE.load(Ordering::Acquire) == 0 && !reserve_range() {
        return std::ptr::null_mut();
    }
    let page = PAGE_CURSOR.fetch_add(PAGE_SIZE, Ordering::AcqRel);
    let base = HEAP_BASE.load(Ordering::Acquire);
    if page + PAGE_SIZE > base + HEAP_RESERVE.load(Ordering::Acquire) {
        PAGE_CURSOR.fetch_sub(PAGE_SIZE, Ordering::AcqRel);
        return std::ptr::null_mut();
    }
    let header = page as *mut PageHeader;
    // SAFETY: the page lies inside the private mapping and is untouched
    // (zero) memory that only this thread has claimed.
    unsafe {
        header.write(PageHeader {
            class: class as u32,
            owner: AtomicU32::new(0),
            remote_free: AtomicUsize::new(0),
            bump: Cell::new(page + PAGE_HEADER),
            end: page + PAGE_SIZE,
            next_page: Cell::new(std::ptr::null_mut()),
            owner_heap: AtomicUsize::new(0),
            next_queued: AtomicUsize::new(0),
            queued: AtomicU32::new(0),
        });
    }
    header
}

/// Slow path: drain remote frees, bump from the current page, adopt an
/// orphaned page, or carve a new one. Returns null when memory is exhausted.
#[cold]
#[inline(never)]
fn refill(heap: &ThreadHeap, class: usize) -> *mut u8 {
    let tid = heap.tid();
    let size = CLASS_SIZES[class] as usize;
    // 1. Blocks other threads freed into our pages (only queued pages).
    if heap.remote_pages.load(Ordering::Relaxed) != 0 {
        drain_remote(heap);
        let head = heap.free[class].get();
        if head != 0 {
            // SAFETY: head is a free pool block with the next link in its first word.
            unsafe { heap.free[class].set(*(head as *const usize)) };
            return head as *mut u8;
        }
    }
    // 2. Adopt an orphaned page of this class before touching fresh memory:
    //    its blocks were freed recently and are the best candidates for reuse.
    let adopted = if ORPHAN_COUNT.load(Ordering::Relaxed) == 0 {
        std::ptr::null_mut()
    } else {
        let mut orphans = ORPHANS.lock().unwrap_or_else(|poison| poison.into_inner());
        let page = orphans.0[class];
        if !page.is_null() {
            // SAFETY: orphan pages are live pool pages nobody owns.
            orphans.0[class] = unsafe { (*page).next_page.get() };
            ORPHAN_COUNT.fetch_sub(1, Ordering::Relaxed);
        }
        page
    };
    let page = if !adopted.is_null() {
        adopted
    } else {
        // 3. Bump space on the current page.
        let current = heap.current[class].get();
        if !current.is_null() {
            // SAFETY: current is a live page we own.
            unsafe {
                let bump = (*current).bump.get();
                if bump + size <= (*current).end {
                    (*current).bump.set(bump + size);
                    return bump as *mut u8;
                }
            }
        }
        // 4. A fresh page.
        let page = new_page(class);
        if page.is_null() {
            return std::ptr::null_mut();
        }
        page
    };
    // SAFETY: the page is now exclusively ours: link it, take ownership,
    // recover its remote frees, and serve from its bump space or free list.
    unsafe {
        (*page)
            .owner_heap
            .store(heap as *const ThreadHeap as usize, Ordering::Release);
        (*page).owner.store(tid, Ordering::Release);
        (*page).queued.store(0, Ordering::Release);
        (*page).next_page.set(heap.pages[class].get());
        heap.pages[class].set(page);
        if (*page).bump.get() + size <= (*page).end || heap.current[class].get().is_null() {
            heap.current[class].set(page);
        }
        let remote = (*page).remote_free.swap(0, Ordering::AcqRel);
        if remote != 0 {
            heap.free[class].set(*(remote as *const usize));
            return remote as *mut u8;
        }
        let bump = (*page).bump.get();
        if bump + size <= (*page).end {
            (*page).bump.set(bump + size);
            return bump as *mut u8;
        }
    }
    // An adopted page without bump space or remote blocks: try again, now
    // with it linked in (its blocks may still arrive as remote frees later).
    refill(heap, class)
}

/// Free-list pop or bump for `class`, hand-scheduled for x86-64. Returns null
/// when the class needs the cold refill. The structure offsets are taken from
/// the Rust layout so the two implementations share every invariant; the
/// A/B between them (feature `php-heap-asm`) measures the effect of the
/// instruction selection alone.
#[cfg(all(feature = "php-heap-asm", target_arch = "x86_64"))]
#[inline(always)]
fn fast_alloc(heap: &ThreadHeap, class: usize) -> *mut u8 {
    debug_assert!(class < NUM_CLASSES);
    let free_base = heap.free.as_ptr() as usize;
    let current_base = heap.current.as_ptr() as usize;
    let size = CLASS_SIZES[class] as usize;
    let ptr: usize;
    // SAFETY: `free_base`/`current_base` point at this thread's heap arrays,
    // `class` indexes them in bounds, free blocks keep their successor in
    // the first word, and the current page (if any) is a live owned page
    // whose `bump`/`end` live at offsets 16/24 of its header.
    unsafe {
        std::arch::asm!(
            "mov {ptr}, qword ptr [{free} + {class} * 8]",
            "test {ptr}, {ptr}",
            "jz 2f",
            "mov {tmp}, qword ptr [{ptr}]",
            "mov qword ptr [{free} + {class} * 8], {tmp}",
            "jmp 3f",
            "2:",
            "mov {tmp}, qword ptr [{cur} + {class} * 8]",
            "test {tmp}, {tmp}",
            "jz 3f",
            "mov {ptr}, qword ptr [{tmp} + 16]",
            "lea {size}, [{ptr} + {size}]",
            "cmp {size}, qword ptr [{tmp} + 24]",
            "ja 4f",
            "mov qword ptr [{tmp} + 16], {size}",
            "jmp 3f",
            "4:",
            "xor {ptr:e}, {ptr:e}",
            "3:",
            ptr = out(reg) ptr,
            tmp = out(reg) _,
            free = in(reg) free_base,
            cur = in(reg) current_base,
            class = in(reg) class,
            size = inout(reg) size => _,
            options(nostack, preserves_flags),
        );
    }
    ptr as *mut u8
}

#[cfg(not(all(feature = "php-heap-asm", target_arch = "x86_64")))]
#[inline(always)]
fn fast_alloc(heap: &ThreadHeap, class: usize) -> *mut u8 {
    debug_assert!(class < NUM_CLASSES);
    // SAFETY: `class` comes from the class tables (< NUM_CLASSES); a free
    // pool block keeps its successor in its first word; the current page
    // is a live page this thread owns.
    unsafe {
        let free = heap.free.get_unchecked(class);
        let head = free.get();
        if head != 0 {
            free.set(*(head as *const usize));
            return head as *mut u8;
        }
        let current = heap.current.get_unchecked(class).get();
        if !current.is_null() {
            let bump = (*current).bump.get();
            let next = bump + *CLASS_SIZES.get_unchecked(class) as usize;
            if next <= (*current).end {
                (*current).bump.set(next);
                return bump as *mut u8;
            }
        }
    }
    std::ptr::null_mut()
}

/// Cold continuation of `pool_alloc`: refill the class, falling back to the
/// system allocator when the reservation is exhausted. A single tail-called
/// cold function keeps the hot path free of callee-saved registers.
#[cold]
#[inline(never)]
fn refill_or_system(heap: &ThreadHeap, class: usize, layout: Layout) -> *mut u8 {
    let ptr = refill(heap, class);
    if !ptr.is_null() {
        return ptr;
    }
    // SAFETY: forwarded verbatim.
    unsafe { System.alloc(layout) }
}

/// First allocation on this thread: decide the mode, then allocate.
#[cold]
#[inline(never)]
fn alloc_first_use(heap: &ThreadHeap, layout: Layout, zeroed: bool) -> *mut u8 {
    heap.mode.set(decide_mode() as u8);
    if zeroed {
        // SAFETY: forwarded verbatim.
        unsafe { PhpHeap.alloc_zeroed(layout) }
    } else {
        // SAFETY: forwarded verbatim.
        unsafe { PhpHeap.alloc(layout) }
    }
}

#[inline(always)]
fn pool_alloc(class: usize) -> *mut u8 {
    debug_assert!(class < NUM_CLASSES);
    let heap = heap();
    let ptr = fast_alloc(heap, class);
    if !ptr.is_null() {
        return ptr;
    }
    refill(heap, class)
}

/// # Safety
/// `ptr` must come from `pool_alloc` and not be freed twice.
#[inline(always)]
unsafe fn pool_free(ptr: *mut u8) {
    let block = ptr as usize;
    // SAFETY: the caller proved the pointer lies in the reservation, so its
    // page header is initialized.
    let page = unsafe { &*((block & PAGE_MASK) as *const PageHeader) };
    let class = page.class as usize;
    let heap = heap();
    let tid = heap.tid.get();
    if page.owner.load(Ordering::Relaxed) == tid && tid != 0 {
        // SAFETY: `class` was written by `new_page` (< NUM_CLASSES); the
        // block is ours and free; link it in front.
        unsafe {
            let free = heap.free.get_unchecked(class);
            *(block as *mut usize) = free.get();
            free.set(block);
        }
    } else {
        // SAFETY: `page` is a live pool page; the block is free.
        unsafe { remote_push_cold(page, block) };
    }
}

/// # Safety
/// Same contract as `GlobalAlloc::alloc`.
#[cold]
#[inline(never)]
unsafe fn system_alloc(layout: Layout) -> *mut u8 {
    // SAFETY: forwarded verbatim.
    unsafe { System.alloc(layout) }
}

/// # Safety
/// Same contract as `GlobalAlloc::alloc_zeroed`.
#[cold]
#[inline(never)]
unsafe fn system_alloc_zeroed(layout: Layout) -> *mut u8 {
    // SAFETY: forwarded verbatim.
    unsafe { System.alloc_zeroed(layout) }
}

/// # Safety
/// `ptr` must have been returned by `System` for `layout`.
#[cold]
#[inline(never)]
unsafe fn system_dealloc(ptr: *mut u8, layout: Layout) {
    // SAFETY: the block was obtained from `System`.
    unsafe { System.dealloc(ptr, layout) }
}

unsafe impl GlobalAlloc for PhpHeap {
    /// # Safety
    /// `GlobalAlloc::alloc` contract: `layout` has non-zero size.
    #[inline(always)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let heap = heap();
        let mode = heap.mode.get();
        if mode == 1 {
            if let Some(class) = class_for(layout.size(), layout.align()) {
                let ptr = fast_alloc(heap, class);
                if !ptr.is_null() {
                    return ptr;
                }
                return refill_or_system(heap, class, layout);
            }
            // SAFETY: forwarded verbatim.
            return unsafe { system_alloc(layout) };
        }
        if mode == 0 {
            return alloc_first_use(heap, layout, false);
        }
        // SAFETY: forwarded verbatim.
        unsafe { system_alloc(layout) }
    }

    /// # Safety
    /// `GlobalAlloc::alloc_zeroed` contract.
    #[inline(always)]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let heap = heap();
        let mode = heap.mode.get();
        if mode == 1 {
            if let Some(class) = class_for(layout.size(), layout.align()) {
                let ptr = pool_alloc(class);
                if !ptr.is_null() {
                    // SAFETY: the block holds at least `size` bytes.
                    unsafe { std::ptr::write_bytes(ptr, 0, layout.size()) };
                    return ptr;
                }
            }
            // SAFETY: forwarded verbatim.
            return unsafe { system_alloc_zeroed(layout) };
        }
        if mode == 0 {
            return alloc_first_use(heap, layout, true);
        }
        // SAFETY: forwarded verbatim.
        unsafe { system_alloc_zeroed(layout) }
    }

    /// # Safety
    /// `ptr` was returned by this allocator for `layout` and is freed once.
    #[inline(always)]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if in_pool(ptr as usize) {
            // SAFETY: pool pointers come from `pool_alloc`.
            unsafe { pool_free(ptr) };
        } else {
            // SAFETY: the block was obtained from `System`.
            unsafe { system_dealloc(ptr, layout) }
        }
    }

    /// # Safety
    /// `GlobalAlloc::realloc` contract: `ptr`/`layout` as for `dealloc`.
    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if in_pool(ptr as usize) {
            // SAFETY: pool pointer; its page header records the class.
            let class =
                unsafe { (*((ptr as usize & PAGE_MASK) as *const PageHeader)).class } as usize;
            if let Some(new_class) = class_for(new_size, layout.align())
                && new_class == class
            {
                return ptr;
            }
            // SAFETY: standard grow-by-copy; both layouts are valid.
            unsafe {
                let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
                let new_ptr = self.alloc(new_layout);
                if !new_ptr.is_null() {
                    std::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
                    pool_free(ptr);
                }
                return new_ptr;
            }
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

/// Live pool pages carved so far (diagnostics).
pub fn pages_in_use() -> usize {
    let base = HEAP_BASE.load(Ordering::Relaxed);
    if base == 0 {
        return 0;
    }
    (PAGE_CURSOR.load(Ordering::Relaxed) - ((base + PAGE_SIZE - 1) & PAGE_MASK)) / PAGE_SIZE
}

#[cfg(test)]
mod tests;
