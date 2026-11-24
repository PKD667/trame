// The machine segment: the domain of this backend's workers (CONTRACT.md, M1). One POSIX shared-memory
// object per node of a launch, mapped at the same address, ADDR, in every rank of the node, so a
// pointer the builder wrote is the same pointer in the worker's process. The leader builds the states
// of the workers on its node, and the pool, there; the host rank of another node builds that node's
// (setup/adopt.rs). A state is then used in place: nothing is encoded, and `take`, `error` and `edit` read and
// write the same bytes the worker's steps do.
//
// Linux only, as the backend is: MAP_FIXED_NOREPLACE (Linux 4.17) maps at ADDR or fails, never over
// another mapping; the launch is refused on every rank if one rank cannot (open.rs).
//
// Layout (offsets from ADDR):
//   0        LOCK   u32, the allocator's lock (a spin lock: allocation happens in builders, edits and
//                   at a worker's end, never in a step, D5)
//   8        BUMP   u64, the offset of the first never-allocated byte
//   64       FREE   u64 per size class: the address of the first free block of that class, 0 if none
//   PAGE     TABLE  u64 slots: the pool's block, then each worker's state block, then each worker's
//                   trace (0 until written)
//   after    blocks
//
// Allocation is by power-of-two size class with one free list per class: a block of class c holds
// 2^c bytes and is aligned to min(2^c, PAGE), or to its layout's alignment if larger. Freed blocks are
// reused by their class, never split or merged. That wastes up to half of a block and never returns
// memory, which is enough for builders and edits; it is not a general allocator.
//
// The object is unlinked once every rank of the node has mapped it, so no run leaves it in /dev/shm.
// The mapping is never unmapped: a state the leader took lives there until the process ends.

extern crate std;

use core::alloc::Layout;
use core::sync::atomic::Ordering::{Acquire, Release};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize};
use std::ffi::CString;
use std::format;
use std::string::String;

/// Where every rank of a node maps its segment. Far from where Linux places the heap, the binary
/// (PIE, near 0x55..) and mmap's default range (below the stack, near 0x7f..).
pub(crate) const ADDR: usize = 0x2000_0000_0000;
/// The segment's default size, when the host's memory cannot be read: address space, of which only touched pages
/// take memory (M6: a builder or an edit that does not fit gets a null pointer, which the allocation reports).
const DEFAULT: usize = 16 << 30;
/// The segment's size for this launch, fixed when it is mapped: the host's memory at the launch less the sixteenth
/// the launch keeps and with room for every worker's grace, so that a builder that spends its whole share
/// (`trame::system::left`, INTERFACE.md 6, Definition (Memory of a host)) fits the domain by construction. Address
/// space, not memory: pages are taken when touched.
static LEN: AtomicUsize = AtomicUsize::new(DEFAULT);

/// The segment's size for this launch.
pub(crate) fn len() -> usize {
    LEN.load(Acquire)
}

/// The size the node's creating rank gives the segment: the host's available memory, less the sixteenth the launch
/// keeps, plus trame's grace for every worker, page-rounded and at least 1 GiB. `DEFAULT` when the memory cannot be
/// read. The same reading and the same rule as the workers' share (`system::share`), so the domain holds it.
fn target(workers: u32) -> usize {
    let Some(available) = crate::backends::supply::available() else { return DEFAULT };
    let room = available - available / 16 + u64::from(workers) * crate::system::GRACE;
    let room = room.clamp(1 << 30, 1 << 40);
    usize::try_from(room).unwrap_or(DEFAULT).next_multiple_of(PAGE)
}
const PAGE: usize = 4096;
const LOCK: usize = 0;
const BUMP: usize = 8;
const FREE: usize = 64;
const CLASSES: usize = 48;
const TABLE: usize = PAGE;

/// Set once this process has mapped the segment; before that, the heap is the domain (the probe of
/// domain.rs runs before the launch maps it).
static MAPPED: AtomicBool = AtomicBool::new(false);
/// The number of workers, which places the table's slots.
static WORKERS: AtomicU64 = AtomicU64::new(0);

fn word(off: usize) -> &'static AtomicU64 {
    // SAFETY: the segment is mapped (callers check MAPPED) and `off` is an aligned header offset.
    unsafe { &*((ADDR + off) as *const AtomicU64) }
}

fn lock() -> &'static AtomicU32 {
    // SAFETY: as `word`.
    unsafe { &*((ADDR + LOCK) as *const AtomicU32) }
}

/// The table's end: where blocks start. Slots: the pool, the workers' states, the workers' traces, then the launch's
/// memory share and the bytes of a builder that took more than it (INTERFACE.md 6, Definition (Memory of a host)).
fn blocks_start(workers: u64) -> usize {
    (TABLE + 8 * (3 + 2 * workers as usize)).next_multiple_of(PAGE)
}

/// Map this node's segment `name` at ADDR. The node's host rank creates it (`create`), every rank of
/// the node maps it, and the host initialises the header before anyone allocates. Answers why not.
pub(crate) fn map(name: &str, create: bool, workers: u32) -> Result<(), String> {
    let c = CString::new(name).map_err(|_| format!("segment name {name} holds a NUL"))?;
    let flags = if create { libc::O_CREAT | libc::O_EXCL | libc::O_RDWR } else { libc::O_RDWR };
    // SAFETY: a valid C string; the descriptor is closed below.
    let fd = unsafe { libc::shm_open(c.as_ptr(), flags, 0o600) };
    if fd < 0 {
        return Err(format!("shm_open {name}: {}", std::io::Error::last_os_error()));
    }
    // The creator sizes the object from the host's memory; every other rank maps the size it finds, so all of a
    // node's ranks hold one object of one size whatever they read at their own moment.
    let mut len = DEFAULT;
    if create {
        len = target(workers);
        // SAFETY: fd is ours; ftruncate only sizes the object (its pages are allocated when touched).
        if unsafe { libc::ftruncate(fd, len as libc::off_t) } != 0 {
            let e = std::io::Error::last_os_error();
            // SAFETY: closing our own descriptor.
            unsafe { libc::close(fd) };
            return Err(format!("ftruncate {name} to {len} bytes: {e}"));
        }
    } else {
        // SAFETY: fd is ours; a zeroed stat is a valid argument.
        let mut st: libc::stat = unsafe { core::mem::zeroed() };
        // SAFETY: as above.
        if unsafe { libc::fstat(fd, &mut st) } == 0 && st.st_size > 0 {
            len = st.st_size as usize;
        }
    }
    // SAFETY: a fresh shared mapping at ADDR; MAP_FIXED_NOREPLACE fails rather than replace anything.
    let p = unsafe {
        libc::mmap(
            ADDR as *mut libc::c_void,
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED | libc::MAP_FIXED_NOREPLACE,
            fd,
            0,
        )
    };
    let e = std::io::Error::last_os_error();
    // SAFETY: the mapping keeps the object; the descriptor is no longer needed.
    unsafe { libc::close(fd) };
    if p == libc::MAP_FAILED {
        return Err(format!("mmap of {len} bytes at {ADDR:#x}: {e}"));
    }
    if p as usize != ADDR {
        // A kernel before 4.17 ignores MAP_FIXED_NOREPLACE and may place the mapping elsewhere.
        // SAFETY: unmapping what this call mapped.
        unsafe { libc::munmap(p, len) };
        return Err(format!("the kernel mapped the segment at {:#x}, not {ADDR:#x} (needs Linux 4.17)", p as usize));
    }
    WORKERS.store(u64::from(workers), Release);
    LEN.store(len, Release);
    if create {
        // The object is new, so zero: LOCK free, the table empty.
        word(BUMP).store(blocks_start(u64::from(workers)) as u64, Release);
    }
    MAPPED.store(true, Release);
    Ok(())
}

/// Remove the object's name: every rank of the node has mapped it (open.rs).
pub(crate) fn unlink(name: &str) {
    if let Ok(c) = CString::new(name) {
        // SAFETY: a valid C string. A name already gone is not an error here.
        unsafe { libc::shm_unlink(c.as_ptr()) };
    }
}

/// Table slots: the pool's block, worker `w`'s state block, worker `w`'s trace.
pub(crate) fn pool_slot() -> &'static AtomicU64 {
    word(TABLE)
}

pub(crate) fn state_slot(w: u32) -> &'static AtomicU64 {
    word(TABLE + 8 * (1 + w as usize))
}

pub(crate) fn trace_slot(w: u32) -> &'static AtomicU64 {
    word(TABLE + 8 * (1 + WORKERS.load(Acquire) as usize + w as usize))
}

/// The launch's memory share for a worker of this node, and the bytes of a builder that went past it (0: none): what
/// the node's builder fixed, read by every other rank of the node after the build barrier.
pub(crate) fn share_slot() -> &'static AtomicU64 {
    word(TABLE + 8 * (1 + 2 * WORKERS.load(Acquire) as usize))
}

pub(crate) fn over_slot() -> &'static AtomicU64 {
    word(TABLE + 8 * (2 + 2 * WORKERS.load(Acquire) as usize))
}

pub(crate) fn mapped() -> bool {
    MAPPED.load(Acquire)
}

pub(crate) fn contains(p: *mut u8) -> bool {
    mapped() && (ADDR..ADDR + len()).contains(&(p as usize))
}

fn class(l: Layout) -> usize {
    l.size().max(l.align()).max(16).next_power_of_two().trailing_zeros() as usize
}

struct Held;
impl Held {
    fn take() -> Held {
        while lock().compare_exchange_weak(0, 1, Acquire, Acquire).is_err() {
            std::thread::yield_now();
        }
        Held
    }
}
impl Drop for Held {
    fn drop(&mut self) {
        lock().store(0, Release);
    }
}

/// A block for `l`, or null when the segment is full (M6).
pub(crate) fn alloc(l: Layout) -> *mut u8 {
    let c = class(l);
    assert!(c < CLASSES, "trame: a block of {} bytes is larger than any segment", l.size());
    let _held = Held::take();
    let head = word(FREE + 8 * c);
    let p = head.load(Acquire);
    if p != 0 {
        // SAFETY: a free block holds the address of the next free block of its class.
        head.store(unsafe { *(p as *const u64) }, Release);
        return p as *mut u8;
    }
    let align = (1usize << c).min(PAGE).max(l.align());
    let at = (word(BUMP).load(Acquire) as usize).next_multiple_of(align);
    if at + (1 << c) > len() {
        // M6: nothing lands outside the domain. The null makes Rust abort the process; the message
        // says why first, without allocating (this is the allocator).
        // The numbers without formatting (this is the allocator): "trame: the host's memory domain of N GiB is
        // full: the states, the pool and what their builders and edits allocate do not fit\n".
        let mut buf = [0u8; 160];
        let mut n = copy(&mut buf, 0, b"trame: the host's memory domain of ");
        n = copy(&mut buf, n, &gi_gib(len()));
        n = copy(&mut buf, n, b" is full: the states, the pool and what their builders and edits allocate do not fit\n");
        // SAFETY: writes the message's bytes to stderr.
        let _ = unsafe { libc::write(2, buf.as_ptr().cast(), n) };
        return core::ptr::null_mut();
    }
    word(BUMP).store((at + (1 << c)) as u64, Release);
    (ADDR + at) as *mut u8
}

/// Copy `src` into `buf` at `at`; answers the new end.
fn copy(buf: &mut [u8], at: usize, src: &[u8]) -> usize {
    let end = (at + src.len()).min(buf.len());
    buf[at..end].copy_from_slice(&src[..end - at]);
    end
}

/// `n` as "N.N" GiB, in ASCII, without allocating.
fn gi_gib(n: usize) -> [u8; 12] {
    let tenths = (n as u64 * 10) >> 30;
    let (whole, frac) = (tenths / 10, tenths % 10);
    let mut digits = [0u8; 20];
    let mut len = 0;
    let mut w = whole;
    loop {
        digits[len] = b'0' + (w % 10) as u8;
        len += 1;
        w /= 10;
        if w == 0 {
            break;
        }
    }
    let mut out = [b' '; 12];
    for i in 0..len {
        out[i] = digits[len - 1 - i];
    }
    out[len] = b'.';
    out[len + 1] = b'0' + frac as u8;
    out
}

/// Give back a block `alloc` answered for `l`.
pub(crate) fn free(p: *mut u8, l: Layout) {
    let c = class(l);
    let _held = Held::take();
    let head = word(FREE + 8 * c);
    // SAFETY: the block is ours again and holds at least 16 bytes.
    unsafe { *(p as *mut u64) = head.load(Acquire) };
    head.store(p as u64, Release);
}
