//! `trame::Alloc`: the global allocator a trame program installs (CONTRACT.md, M1).
//!
//! A state and the pool are built by the leader and used in place by the worker's agents, which on
//! some backends run in another process or on a device. So every byte they own must lie in memory the
//! worker addresses at the same address: the worker's *domain*. The leader's builders
//! (`Topology::set`) and edits (`Run::edit`) run in *arena mode*, a flag of the calling thread; an
//! allocation made in arena mode asks the backend for domain memory. Outside arena mode every
//! allocation goes to the wrapped allocator, as if trame were not there. A block that lies in a domain
//! is always freed and resized there, whoever does it, so a state taken by the leader can be dropped or
//! grown by it.
//!
//! a backend whose workers share the leader's process answers `None` (their domain is the process heap), so there the wrapper only forwards.
//! The cost outside arena mode is one thread-local read and, on free, one range check.
//!
//! The launch checks that the wrapper is installed (`installed`), on every backend, so a program's
//! source is the same whichever backend its features select.

extern crate std;

use crate::backend::{Backend, Selected};
use core::alloc::{GlobalAlloc, Layout};
use core::cell::Cell;
use std::alloc::System;

/// The global allocator of a trame program, wrapping the allocator `A` the program would otherwise use:
///
/// ```ignore
/// #[global_allocator]
/// static A: trame::Alloc = trame::Alloc::new(std::alloc::System);
/// ```
///
/// A launch is refused (`Refused::Alloc`) when it is not installed.
pub struct Alloc<A = System> {
    inner: A,
}

impl<A> Alloc<A> {
    pub const fn new(inner: A) -> Self {
        Alloc { inner }
    }
}

std::thread_local! {
    /// Arena mode: this thread runs a builder or an edit.
    static ARENA: Cell<bool> = const { Cell::new(false) };
    /// Allocations this thread made in arena mode: the launch's probe reads it.
    static ARENA_ALLOCS: Cell<u64> = const { Cell::new(0) };
    /// Bytes this thread allocated in arena mode since `ARENA_BYTES` was last cleared.
    static ARENA_BYTES: Cell<u64> = const { Cell::new(0) };
}

/// Whether this thread is in arena mode. A thread whose locals are being destroyed is not: it runs
/// no builder and no edit.
fn arena_on() -> bool {
    ARENA.try_with(Cell::get).unwrap_or(false)
}

// SAFETY: every block comes from `inner` or from the backend's domain, and goes back to the one it
// came from: `domain_contains` tells them apart by address, and a domain never overlaps the heap
// (CONTRACT.md, M1).
unsafe impl<A: GlobalAlloc> GlobalAlloc for Alloc<A> {
    // SAFETY: `GlobalAlloc::alloc`'s contract, forwarded to `inner` or answered by the domain.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if arena_on() {
            let _ = ARENA_ALLOCS.try_with(|c| c.set(c.get() + 1));
            let _ = ARENA_BYTES.try_with(|c| c.set(c.get() + layout.size() as u64));
            if let Some(p) = Selected::domain_alloc(layout) {
                return p;
            }
        }
        // SAFETY: the caller's.
        unsafe { self.inner.alloc(layout) }
    }

    // SAFETY: `GlobalAlloc::alloc_zeroed`'s contract, as `alloc`.
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if arena_on() {
            let _ = ARENA_ALLOCS.try_with(|c| c.set(c.get() + 1));
            if let Some(p) = Selected::domain_alloc(layout) {
                if !p.is_null() {
                    // SAFETY: `p` is not null, so it has `layout.size()` writable bytes (M1).
                    unsafe { core::ptr::write_bytes(p, 0, layout.size()) };
                }
                return p;
            }
        }
        // SAFETY: the caller's.
        unsafe { self.inner.alloc_zeroed(layout) }
    }

    // SAFETY: `GlobalAlloc::dealloc`'s contract, forwarded to the allocator `p` came from.
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        if Selected::domain_contains(p) {
            // SAFETY: `p` came from the domain with `layout`.
            unsafe { Selected::domain_dealloc(p, layout) }
        } else {
            // SAFETY: `p` came from `inner` with `layout`.
            unsafe { self.inner.dealloc(p, layout) }
        }
    }

    // SAFETY: `GlobalAlloc::realloc`'s contract: a block moves between allocators only by `alloc`,
    // copy and `dealloc`.
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        // A domain block stays in the domain; a heap block moves to the domain in arena mode (a value
        // built before the builder and grown in it). Otherwise the heap's own resize.
        let domain = Selected::domain_contains(p);
        if !domain && !arena_on() {
            // SAFETY: the caller's.
            return unsafe { self.inner.realloc(p, layout, new) };
        }
        // SAFETY: `new` is non-zero and, rounded to the alignment, fits isize (the caller's promise).
        let to = unsafe { Layout::from_size_align_unchecked(new, layout.align()) };
        let q = if domain {
            Selected::domain_alloc(to).expect("trame: a domain block grows in its domain")
        } else {
            // SAFETY: as `alloc`.
            unsafe { self.alloc(to) }
        };
        if !q.is_null() {
            // SAFETY: both blocks hold at least min(old, new) bytes and do not overlap.
            unsafe { core::ptr::copy_nonoverlapping(p, q, layout.size().min(new)) };
            // SAFETY: `p` is freed once, where it came from.
            unsafe { self.dealloc(p, layout) };
        }
        q
    }
}

/// Run `f` in arena mode (the name `domain.rs`: `alloc` is the crate): what it allocates lands in the domain of the selected backend's launch.
/// Nested calls are allowed. A panic in `f` leaves arena mode.
/// The bytes this thread has allocated in arena mode since `arena_metered` last cleared the meter, or `None` outside
/// arena mode (no builder, no edit). `trame::system::left` reads it, so what a builder asks and what the refusal
/// counts are one number (INTERFACE.md 6, Definition (Memory of a host)).
pub(crate) fn arena_taken() -> Option<u64> {
    if arena_on() { Some(ARENA_BYTES.with(Cell::get)) } else { None }
}

/// Run `f` in arena mode and answer what it returned with the bytes it allocated. The launch uses it to hold each
/// builder to its worker's share (INTERFACE.md 6, Definition (Memory of a host)).
pub(crate) fn arena_metered<R>(f: impl FnOnce() -> R) -> (R, u64) {
    ARENA_BYTES.with(|c| c.set(0));
    let out = arena(f);
    (out, ARENA_BYTES.with(core::cell::Cell::get))
}

pub(crate) fn arena<R>(f: impl FnOnce() -> R) -> R {
    struct Leave(bool);
    impl Drop for Leave {
        fn drop(&mut self) {
            ARENA.with(|a| a.set(self.0));
        }
    }
    let _leave = Leave(ARENA.with(|a| a.replace(true)));
    f()
}

/// Whether `trame::Alloc` is the global allocator: an allocation in arena mode must pass through it.
pub(crate) fn installed() -> bool {
    let before = ARENA_ALLOCS.with(Cell::get);
    arena(|| drop(core::hint::black_box(std::boxed::Box::new(0u8))));
    ARENA_ALLOCS.with(Cell::get) > before
}

/// A builder's value, after the builder: when the backend's domain is shared by processes, every block the value
/// owns must be in the domain, or a process that adopts the value reads an address of the builder's heap. A `Vec`
/// built before the builder and moved into the state is the usual cause; one clone inside the builder moves it.
/// Panics naming the type. A backend whose heap is the domain accepts anything.
pub(crate) fn in_domain<T: crate::Plain>(v: T, what: &str) -> T {
    if !Selected::has_domain() {
        return v;
    }
    let mut outside = 0usize;
    let mut first = core::ptr::null::<u8>();
    let mut each = |p: *const u8| {
        if !Selected::domain_contains(p as *mut u8) {
            if outside == 0 {
                first = p;
            }
            outside += 1;
        }
        true
    };
    v.blocks(&mut each);
    assert!(
        outside == 0,
        "trame: the {what} `{}` owns a block outside the domain\n{outside} block(s), the first at {first:p}: a Vec, Box or String made before its builder ran keeps the heap of the process that made it, which no other process of the host can read. Make it inside the builder (`.clone()` there), and drop the original",
        core::any::type_name::<T>()
    );
    v
}
