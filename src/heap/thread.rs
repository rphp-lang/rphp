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

thread_local! {
    /// No destructor: the constant initializer permits direct TLS access.
    /// `EXIT_GUARD` carries the thread-exit cleanup separately.
    static HEAP: ThreadHeap = const { ThreadHeap::new() };
}

/// The current thread's heap block. Only its owner touches non-atomic
/// fields; remote frees use the page headers and the atomic remote queue.
#[inline(always)]
fn heap() -> &'static ThreadHeap {
    let ptr = HEAP.with(|heap| heap as *const ThreadHeap);
    // SAFETY: a const-initialized thread-local without a destructor remains
    // valid through the thread's TLS destructors. This private reference is
    // only used on its owning thread; ThreadHeap is not Sync.
    unsafe { &*ptr }
}
