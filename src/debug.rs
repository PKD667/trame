//! The debug dump (docs/debug.md): a compile-time, opt-in record of every trame primitive, metadata
//! only. The switch is `--cfg debug_dump`, off by default; every cfg that has to do with the dump
//! lives in this file. A call site is one line naming an op and its arguments, and carries no cfg of
//! its own: with the cfg off, `rec` is an empty `#[inline(always)]` body, so an optimized build
//! removes the call and the computation of its arguments.
//!
//! Recording is one `fetch_add` on the arena's cursor and a 32-byte store: no allocation, no lock,
//! no syscall, never blocking. When the arena fills up, records are only counted, and the dump ends
//! with one DROP record saying how many were dropped.
//!
//! The record layout, the op codes and the header are this module's and docs/debug.md's, and nobody
//! else's: the python readers (check/debug/read.py) are built from that file.

/// The ops the recorder takes (docs/debug.md has the table: what `a` and `b` mean per op). The table
/// is one piece: a build records the ops its call sites reach, and its backend decides which those
/// are, so a const is read in one build and not in another.
#[allow(dead_code)]
pub(crate) mod op {
    pub const STEP_START: u8 = 1;
    pub const STEP_END: u8 = 2;
    pub const SEND: u8 = 3;
    pub const SEND_FULL: u8 = 4;
    pub const RECV_HIT: u8 = 5;
    pub const RECV_MISS: u8 = 6;
    pub const WAIT: u8 = 7;
    pub const WAKE: u8 = 8;
    pub const EDIT: u8 = 9;
    pub const STOP: u8 = 10;
    pub const JOIN: u8 = 11;
    pub const PEER_SEND: u8 = 12;
    pub const PEER_DRAIN: u8 = 13;
    pub const PEER_WAIT: u8 = 14;
    pub const SHARE: u8 = 15;
    pub const RELAY_IN: u8 = 16;
    pub const RELAY_OUT: u8 = 17;
    pub const EPOCH_BUMP: u8 = 18;
    pub const DROP: u8 = 19;
    pub const MARK: u8 = 20;
}

/// The one entry point of the recorder: one line at a call site records (op, a, b). Arguments must
/// be plain values; anything that costs (a hash, a route lookup) belongs in a helper of this module,
/// called instead of this at that call site. Empty when `debug_dump` is off.
#[inline(always)]
pub(crate) fn rec(op: u8, a: u64, b: u64) {
    #[cfg(debug_dump)]
    imp::record(op, a, b);
    #[cfg(not(debug_dump))]
    {
        let _ = (op, a, b);
    }
}

/// Whether the dump is recording: the cheap check a hot call site makes before computing anything.
/// False when `debug_dump` is off. A helper of this module checks this itself; a call site that
/// computes more than plain values checks it first. A call site that computes only plain values
/// needs no check: `rec` is empty when the cfg is off.
#[allow(dead_code)] // the call sites that need it exist only in a debug_dump build
#[inline(always)]
pub(crate) fn enabled() -> bool {
    #[cfg(debug_dump)]
    {
        imp::live()
    }
    #[cfg(not(debug_dump))]
    {
        false
    }
}

#[cfg(not(debug_dump))]
mod imp {
    // The whole recorder is off: these exist so call sites compile, and do nothing.

    use crate::backend::Target;
    use crate::vocab::End;

    pub(crate) fn boot(_rank: u32, _host: u32) {}
    pub(crate) fn spawn_id() {}
    pub(crate) fn dump() {}
    #[inline(always)]
    pub(crate) fn check_requested() {}
    #[inline(always)]
    pub(crate) fn sent<_M>(_to: &End, _ty: core::any::TypeId, _answer: &Result<(), crate::io::Full>) {}
    #[inline(always)]
    pub(crate) fn got<_M>(_hit: bool) {}
    #[inline(always)]
    pub(crate) fn wait_room<_M>() {}
    #[inline(always)]
    pub(crate) fn wait_frame() {}
    #[inline(always)]
    pub(crate) fn waited(_waiting: bool, _frame: bool, _rooms: u64) {}
    pub(crate) fn edit(_target: &Target) {}
}

#[cfg(debug_dump)]
mod imp {
    // The recorder (docs/debug.md). Everything below is built once and then written to by `record`,
    // so the recording path holds no lock, allocates nothing and makes no call into the OS.

    extern crate std;

    use crate::backend::{Backend, Selected, Target};
    use crate::io::Message;
    use crate::vocab::End;
    use alloc::vec::Vec;
    use core::alloc::GlobalAlloc;
    use core::cell::Cell;
    use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU16, AtomicU32, AtomicU64, Ordering};
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock, PoisonError};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    /// One record (docs/debug.md): fixed 32 bytes, little-endian in the file. `t` is nanoseconds
    /// since this process's launch; the rest are the op's two words and the process's identity.
    #[repr(C)]
    pub(super) struct Record {
        t: u64,
        a: u64,
        b: u64,
        rank: u32,
        host: u16,
        agent: u8,
        op: u8,
    }

    /// The arena, one per process, or null when the process did not boot: `TRAME_DUMP_BYTES` bytes,
    /// 64-aligned, allocated with `std::alloc::System` directly (not the program's `trame::Alloc`,
    /// so the arena is invisible to the domain and to the memory share, CONTRACT.md M8, on purpose).
    static ARENA: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
    /// The byte offset of the next record slot. Keeps counting past `LIMIT`, so a full arena has
    /// `NEXT / 32 - LIMIT / 32` overflows.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    /// The first byte offset past the record slots. The arena's last 32 bytes are reserved for the
    /// DROP record of a full arena.
    static LIMIT: AtomicU64 = AtomicU64::new(0);
    /// Records counted after the arena filled.
    static DROPPED: AtomicU64 = AtomicU64::new(0);
    /// The recorder booted: the arena and the statics below are set.
    static LIVE: AtomicBool = AtomicBool::new(false);
    /// SIGUSR1 asked for a dump.
    static REQUEST: AtomicBool = AtomicBool::new(false);
    static RANK: AtomicU32 = AtomicU32::new(0);
    static HOST: AtomicU16 = AtomicU16::new(0);
    /// This process's launch, on the monotonic clock the records count from.
    static T0: OnceLock<Instant> = OnceLock::new();
    /// `T0` as wall-clock nanoseconds since the epoch: the launch id and the header's
    /// alignment field, so files of one run are merged on an approximate wall clock (docs/debug.md).
    static T0_WALL: AtomicU64 = AtomicU64::new(0);
    /// What `TRAME_DUMP_DIR` named, or unset when the process did not boot.
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    /// One dump at a time; dumping allocates, and is a cold path.
    static WRITE: Mutex<()> = Mutex::new(());
    /// The next agent id of a spawned thread; 0 is the main thread's, so the ids are dense from it.
    static NEXT_AGENT: AtomicU64 = AtomicU64::new(1);

    std::thread_local! {
        /// This thread's agent id (docs/debug.md): one byte per agent of the process.
        static AGENT: Cell<u8> = const { Cell::new(0) };
    }

    /// SIGUSR1 on the targets the backends run on (Linux).
    const SIGUSR1: i32 = 10;
    /// `signal(2)`'s error return.
    const SIG_ERR: usize = usize::MAX;

    // SAFETY: the four libc items below are declared exactly as glibc and POSIX define them (sigaction,
    // sigemptyset, signal and raise); nothing here calls them outside that contract (CONTRACT.md M1: no block
    // is shared, the arena is this process's own).
    unsafe extern "C" {
        /// The C library's `signal(2)`, linked by `std` on this target. The dump links nothing new.
        fn signal(signum: i32, handler: usize) -> usize;
    }

    /// The SIGUSR1 handler: one atomic store and nothing else, so it is async-signal-safe (no
    /// allocation, no syscall; docs/debug.md).
    // SAFETY: the handler runs with the C ABI and takes one int, as sigaction requires; it only stores into
    // atomics and never allocates (CONTRACT.md M4).
    unsafe extern "C" fn requested(_signum: i32) {
        REQUEST.store(true, Ordering::Relaxed);
    }

    #[inline(always)]
    pub(crate) fn live() -> bool {
        LIVE.load(Ordering::Acquire)
    }

    fn agent_id() -> u8 {
        AGENT.try_with(Cell::get).unwrap_or(0)
    }

    /// Nanoseconds since this process's launch (the monotonic clock: a vDSO read on Linux, no
    /// kernel entry).
    fn elapsed() -> u64 {
        T0.get().map_or(0, |t| u64::try_from(t.elapsed().as_nanos()).unwrap_or(u64::MAX))
    }

    /// The recording path (crate::debug::rec, under the cfg). A full arena counts the record and
    /// writes nothing; the count reaches the file as the DROP record of the last dump.
    #[inline(always)]
    pub(crate) fn record(op: u8, a: u64, b: u64) {
        if !live() {
            return;
        }
        let at = NEXT.fetch_add(32, Ordering::Relaxed) as usize;
        if at >= LIMIT.load(Ordering::Relaxed) as usize {
            DROPPED.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let r = Record {
            t: elapsed(),
            a,
            b,
            rank: RANK.load(Ordering::Relaxed),
            host: HOST.load(Ordering::Relaxed),
            agent: agent_id(),
            op,
        };
        // SAFETY: `at` is below `LIMIT`, so inside the arena, and this slot belongs to this call alone.
        // SAFETY: `at` came from a fetch_add that reserved this 32-byte slot, the arena is live until the process
        // ends, and `Record` is plain data (CONTRACT.md M3: the slot is written once, by its reserver).
        unsafe { core::ptr::write(ARENA.load(Ordering::Relaxed).add(at) as *mut Record, r) };
    }

    /// Boot the recorder: read the environment, allocate the arena, install the SIGUSR1 handler and
    /// start recording. Called once per process by the selected backend, at its launch. Without
    /// `TRAME_DUMP_DIR` the process records nothing: the dump is opt in twice, at compile time and
    /// at run time.
    pub(crate) fn boot(rank: u32, host: u32) {
        if LIVE.load(Ordering::Relaxed) {
            return;
        }
        let Some(dir) = std::env::var_os("TRAME_DUMP_DIR") else {
            return;
        };
        let bytes = bytes_setting();
        // The arena is never freed: the process ends with it.
        let Ok(layout) = core::alloc::Layout::from_size_align(bytes, 64) else { return };
        // SAFETY: `layout` has non-zero size (bytes >= 4096). Allocated with `System` directly, so
        // the program's `trame::Alloc` does not route it into a domain and it counts towards no
        // worker's share (CONTRACT.md M8; docs/debug.md).
        // SAFETY: the layout is the arena itself (a fixed size, an alignment that `Record` satisfies), the pointer
        // is checked for null, and the block is never freed, so it outlives every record written into it.
        // The arena is deliberately outside the domain and outside a worker's share (INTERFACE.md 6; docs/debug.md).
        let arena = unsafe { std::alloc::System.alloc(layout) };
        if arena.is_null() {
            return;
        }
        if DIR.set(PathBuf::from(dir)).is_err() {
            return;
        }
        let _ = T0.set(Instant::now());
        let wall =
            SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
        T0_WALL.store(wall, Ordering::Relaxed);
        RANK.store(rank, Ordering::Relaxed);
        HOST.store((host & 0xffff) as u16, Ordering::Relaxed);
        LIMIT.store(bytes as u64 - 32, Ordering::Relaxed);
        ARENA.store(arena, Ordering::Relaxed);
        // SAFETY: the handler is a plain extern "C" function; `signal` comes in with `std`.
        // SAFETY: `requested` has the signature `signal` takes and lives for the process's life.
        if unsafe { signal(SIGUSR1, requested as *const () as usize) } == SIG_ERR {
            // No dump on SIGUSR1 for this process; the triggers at join still work.
        }
        LIVE.store(true, Ordering::Release);
    }

    /// What `TRAME_DUMP_BYTES` named, or the default of 256 MiB; unreadable or absurd values take
    /// the default or the nearest sane bound.
    fn bytes_setting() -> usize {
        const DEFAULT: usize = 256 << 20;
        let from_env = std::env::var("TRAME_DUMP_BYTES").ok().and_then(|v| v.parse::<usize>().ok());
        from_env.unwrap_or(DEFAULT).clamp(4096, (isize::MAX as usize) - 64)
    }

    /// Give this spawned thread the next agent id; the main thread keeps 0. Later than the 255th
    /// thread of a process they share 255 (docs/debug.md).
    pub(crate) fn spawn_id() {
        let id = NEXT_AGENT.fetch_add(1, Ordering::Relaxed).min(255);
        let _ = AGENT.try_with(|c| c.set(id as u8));
    }

    /// Write `rank<rank>.dump` in `TRAME_DUMP_DIR`: the header (docs/debug.md), the records the
    /// arena holds, and, when records were dropped, one DROP record with their count. Full and
    /// partial dumps overwrite the same file, so the last dump of a process is what stays; the
    /// recorder keeps recording between dumps.
    pub(crate) fn dump() {
        if !LIVE.load(Ordering::Relaxed) {
            return;
        }
        let _w = WRITE.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(dir) = DIR.get() else {
            return;
        };
        let arena = ARENA.load(Ordering::Relaxed) as *const Record;
        let limit = LIMIT.load(Ordering::Relaxed) as usize;
        let mut count = (NEXT.load(Ordering::Relaxed) as usize / 32).min(limit / 32) as u64;
        let dropped = DROPPED.load(Ordering::Relaxed);
        if dropped > 0 {
            let r = Record {
                t: elapsed(),
                a: dropped,
                b: 0,
                rank: RANK.load(Ordering::Relaxed),
                host: HOST.load(Ordering::Relaxed),
                agent: u8::MAX,
                op: crate::debug::op::DROP,
            };
            // SAFETY: the arena's last slot, reserved by `LIMIT` and written by dumps alone.
            // SAFETY: this is the arena's last slot, reserved by construction for the terminal record.
            unsafe { core::ptr::write(arena.add(limit / 32) as *mut Record, r) };
            count += 1;
        }
        let mut out = Vec::with_capacity(52 + 32 * count as usize);
        out.extend_from_slice(b"TRAMEDMP");
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&HOST.load(Ordering::Relaxed).to_le_bytes());
        out.extend_from_slice(&(RANK.load(Ordering::Relaxed) as u16).to_le_bytes());
        out.extend_from_slice(&T0_WALL.load(Ordering::Relaxed).to_le_bytes());
        out.extend_from_slice(&(limit as u64 + 32).to_le_bytes());
        out.extend_from_slice(&32u32.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&T0_WALL.load(Ordering::Relaxed).to_le_bytes());
        for i in 0..count as usize {
            // SAFETY: slot `i` is inside the arena. A record another thread is writing now may be
            // caught mid-write (docs/debug.md, honesty).
            // SAFETY: every slot below the count was written by `rec` and never modified after.
            let r = unsafe { core::ptr::read(arena.add(i)) };
            out.extend_from_slice(&r.t.to_le_bytes());
            out.extend_from_slice(&r.a.to_le_bytes());
            out.extend_from_slice(&r.b.to_le_bytes());
            out.extend_from_slice(&r.rank.to_le_bytes());
            out.extend_from_slice(&r.host.to_le_bytes());
            out.extend_from_slice(&r.agent.to_le_bytes());
            out.extend_from_slice(&r.op.to_le_bytes());
        }
        let path = dir.join(alloc::format!("rank{}.dump", RANK.load(Ordering::Relaxed)));
        if let Err(e) = std::fs::write(&path, &out) {
            std::eprintln!("trame debug: {} not written: {e}", path.display());
        }
    }

    /// The agents' cheap poll (docs/debug.md): the SIGUSR1 handler only sets its flag, and an agent
    /// loop that passes here writes the dump when the flag is set. A plain load when it is not.
    #[inline(always)]
    pub(crate) fn check_requested() {
        if REQUEST.load(Ordering::Relaxed) && REQUEST.swap(false, Ordering::AcqRel) {
            dump();
        }
    }

    /// A worker's send, as the surface knows it (io.rs `send`): the tag of the message type `M` in
    /// hand, the route as `Backend::worker_route` numbers it (the same number `.room` waits on),
    /// and the answer. A full send records SEND_FULL, not SEND.
    #[inline(always)]
    pub(crate) fn sent<M: Message>(to: &End, ty: core::any::TypeId, answer: &Result<(), crate::io::Full>) {
        if !live() {
            return;
        }
        let rid = Selected::worker_route(*to, ty).map_or(u64::MAX, |r| r as u64);
        let op = if answer.is_ok() { crate::debug::op::SEND } else { crate::debug::op::SEND_FULL };
        record(op, crate::io::tag::<M>(), rid);
    }

    /// A worker's recv of a message of type `M`: a hit took a frame, and its incoming route is
    /// numbered by `Backend::worker_incoming` (the receive-side counterpart of `worker_route`); a
    /// miss found none.
    #[inline(always)]
    pub(crate) fn got<M: Message>(hit: bool) {
        if !live() {
            return;
        }
        if hit {
            let rid = Selected::worker_incoming(core::any::TypeId::of::<M>()).map_or(u64::MAX, |r| r as u64);
            record(crate::debug::op::RECV_HIT, crate::io::tag::<M>(), rid);
        } else {
            record(crate::debug::op::RECV_MISS, crate::io::tag::<M>(), 0);
        }
    }

    /// `.room::<M>()` on a wait: the step waits for room on a route of type `M` (status.rs).
    #[inline(always)]
    pub(crate) fn wait_room<M: Message>() {
        if !live() {
            return;
        }
        record(crate::debug::op::WAIT, 1, crate::io::tag::<M>());
    }

    /// `.frame()` on a wait: the step waits for a frame (status.rs).
    #[inline(always)]
    pub(crate) fn wait_frame() {
        if !live() {
            return;
        }
        record(crate::debug::op::WAIT, 2, 0);
    }

    /// A step's wait with no room and no frame: the wait(∅) the builders did not record (worker.rs
    /// `ended`; `status.rs` records the ones with a condition where they are built). `waiting` says
    /// the step actually ended in a wait: a progress or a done has an empty condition set too.
    #[inline(always)]
    pub(crate) fn waited(waiting: bool, frame: bool, rooms: u64) {
        if !live() || !waiting || frame || rooms != 0 {
            return;
        }
        record(crate::debug::op::WAIT, 0, 0);
    }

    /// A leader's edit of a worker (a=1, its index) or of the pool (a=2) (run.rs `edit`).
    pub(crate) fn edit(target: &Target) {
        if !live() {
            return;
        }
        match target {
            Target::Worker(w) => record(crate::debug::op::EDIT, 1, *w as u64),
            Target::Pool => record(crate::debug::op::EDIT, 2, 0),
        }
    }
}

pub(crate) use imp::*;
