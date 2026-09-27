const SMALL_PAGE_SIZE: usize = 1 << 16;
const SMALL_PAGE_MASK: usize = !(SMALL_PAGE_SIZE - 1);
const MEDIUM_PAGE_SIZE: usize = 1 << 20;
const MEDIUM_PAGE_MASK: usize = !(MEDIUM_PAGE_SIZE - 1);
/// The header in front of a large block: a multiple of 16 keeps the block
/// 16-byte aligned.
const PAGE_HEADER: usize = 128;
/// Free pages and free large blocks sit in stacks of 64 KiB-aligned entries
/// of the reservation. A large block's link word lives here, inside its
/// header; a page's lives in its header entry (`HEADER_LINK`). Either way it
/// is metadata only accessed atomically, never block memory a stale pop
/// could race.
const STACK_LINK: usize = PAGE_HEADER - 8;
const STACK_UNIT_SHIFT: u32 = 16;
/// Page headers live in an array at the start of the reservation: one
/// 128-byte entry per 64 KiB unit, a medium page using the entry of its
/// first unit. Headers inside the pages all sat at 64 KiB-aligned
/// addresses, that is in one L1 set (8 ways) and one or two L2 sets, so a
/// workload touching more than a handful of pages missed on every header.
/// Contiguous entries spread over all sets and share TLB entries.
const HEADER_SHIFT: u32 = 7;
const HEADER_SIZE: usize = 1 << HEADER_SHIFT;
/// A pooled page's stack link, in its header entry past `PageHeader`.
const HEADER_LINK: usize = HEADER_SIZE - 8;
/// The first block of a page starts at one of 16 offsets 64 bytes apart,
/// chosen by the page's address, so the first (under LIFO reuse often the
/// hottest) blocks of different pages do not share cache sets either. A
/// multiple of 16 keeps 16-byte classes aligned.
const COLOR_STEP: usize = 64;
const COLORS: usize = 16;
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
/// size itself, so the fast path needs no rounding. The tables are shared
/// by all allocation call sites.
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

/// Per-page bookkeeping, one entry of the header array per page. Fields
/// marked "owner" are touched only by the owning thread. The first cache
/// line holds what the fast paths use; the second what other threads write.
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
    /// Address of the page this header describes.
    start: usize,
}

const LISTING_NONE: u32 = 0;
const LISTING_AVAIL: u32 = 1;
const LISTING_CURRENT: u32 = 2;

const _: () = assert!(std::mem::size_of::<PageHeader>() <= HEADER_LINK);
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
