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
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::ffi::CString;
use std::format;
use std::string::String;

/// Where every rank of a node maps its segment. Far from where Linux places the heap, the binary
/// (PIE, near 0x55..) and mmap's default range (below the stack, near 0x7f..).
pub(crate) const ADDR: usize = 0x2000_0000_0000;
/// The segment's size: address space, of which only touched pages take memory (M6: a builder or
/// an edit that does not fit gets a null pointer, which the allocation reports).
pub(crate) const LEN: usize = 16 << 30;
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

/// The table's end: where blocks start.
fn blocks_start(workers: u64) -> usize {
    (TABLE + 8 * (1 + 2 * workers as usize)).next_multiple_of(PAGE)
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
    // SAFETY: fd is ours; ftruncate only sizes the object (its pages are allocated when touched).
    if create && unsafe { libc::ftruncate(fd, LEN as libc::off_t) } != 0 {
        let e = std::io::Error::last_os_error();
        // SAFETY: closing our own descriptor.
        unsafe { libc::close(fd) };
        return Err(format!("ftruncate {name} to {LEN} bytes: {e}"));
    }
    // SAFETY: a fresh shared mapping at ADDR; MAP_FIXED_NOREPLACE fails rather than replace anything.
    let p = unsafe {
        libc::mmap(
            ADDR as *mut libc::c_void,
            LEN,
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
        return Err(format!("mmap of {LEN} bytes at {ADDR:#x}: {e}"));
    }
    if p as usize != ADDR {
        // A kernel before 4.17 ignores MAP_FIXED_NOREPLACE and may place the mapping elsewhere.
        // SAFETY: unmapping what this call mapped.
        unsafe { libc::munmap(p, LEN) };
        return Err(format!("the kernel mapped the segment at {:#x}, not {ADDR:#x} (needs Linux 4.17)", p as usize));
    }
    WORKERS.store(u64::from(workers), Release);
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

pub(crate) fn mapped() -> bool {
    MAPPED.load(Acquire)
}

pub(crate) fn contains(p: *mut u8) -> bool {
    mapped() && (ADDR..ADDR + LEN).contains(&(p as usize))
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
    if at + (1 << c) > LEN {
        // M6: nothing lands outside the domain. The null makes Rust abort the process; the message
        // says why first, without allocating (this is the allocator).
        const FULL: &[u8] = b"trame: the host's memory domain (16 GiB) is full: the states, the pool and what their builders and edits allocate do not fit\n";
        // SAFETY: writes FULL's bytes to stderr.
        let _ = unsafe { libc::write(2, FULL.as_ptr().cast(), FULL.len()) };
        return core::ptr::null_mut();
    }
    word(BUMP).store((at + (1 << c)) as u64, Release);
    (ADDR + at) as *mut u8
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
