// mpi_cross_host: a microbenchmark of the MPI backend's cross-machine transport. It is not part of
// the backend and shares no code with backends/mpi. Two ranks, rank 0 on machine A and rank 1 on
// machine B, send a frame back and forth; the latency is half the round trip.
//
// Why it exists. backends/mpi/NODES.md ("Replicas and relays") sends every frame between machines through
// a relay: the sender writes a shared-memory slot and wakes the relay's `out` thread, which MPI_Sends;
// the other machine's `in` thread blocks in MPI_Recv(ANY_SOURCE), copies the frame into a slot and wakes
// the receiver, which sleeps on a futex. The alternative is two-sided MPI from rank to rank, with the
// receiving rank spinning on a posted MPI_Irecv and no thread between. This program measures both on the
// same two machines, so the choice between them rests on a run and not on an argument.
//
// Modes (one per launch, `mpirun -n 2 mpi_cross_host <mode> [options]`):
//   direct        rank to rank. The receiver has an MPI_Irecv posted and spins on MPI_Test. MPI_THREAD_MULTIPLE.
//   direct_single the same under MPI_THREAD_SINGLE: what direct costs in MPI's own thread locks.
//   relay         the baseline: one out thread and one in thread per rank, slots and futexes, as above.
//   mixed_self    direct, and the rank also waits for a local waker: a second thread wakes it with an
//                 MPI_Isend to its own rank, received by a second posted MPI_Irecv; the rank spins on both
//                 with MPI_Testany.
//   mixed_flag    direct, and the rank alternates MPI_Test on the cross-machine Irecv with a load of an
//                 atomic flag that the second thread stores.
//   wake_futex    reference for the local wake alone: the receiver sleeps on a futex, as the relay's
//                 receiver does. A rank asleep on a futex cannot also be waiting in MPI.
// Phases. `rtt`: ping-pong of every size, latency = round trip / 2. `wake` (mixed_* and wake_futex): the
// second thread wakes the waiting rank and the latency is from just before the waker acts to the moment
// the receiver sees it. The wake phase runs on rank 1 while rank 0 waits in a barrier.
//
// Output: TSV on rank 0's stdout, one row per (mode, phase, size). Times are nanoseconds. Notes go to stderr.
// CPU is the process's user plus system time (getrusage RUSAGE_SELF, every thread) over the measured
// iterations, divided by them: per round trip, per rank. user/system split is the kernel's scaled estimate
// and only their sum is exact. Spinning ranks use a whole core by design; the column says what it costs.

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release, SeqCst};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize};
use std::thread;

use mpi::ffi;

const TAG_FRAME: i32 = 1;
const TAG_WAKE: i32 = 2;
const TAG_CPU: i32 = 90;

// ---- MPI, raw -------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Comm(ffi::MPI_Comm);
// SAFETY: a communicator handle is valid in every thread under MPI_THREAD_MULTIPLE.
unsafe impl Send for Comm {}
unsafe impl Sync for Comm {}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Req(ffi::MPI_Request);
// SAFETY: a request is used by one thread at a time here.
unsafe impl Send for Req {}

fn ok(rc: i32, what: &str) {
    assert_eq!(rc, ffi::MPI_SUCCESS as i32, "MPI_{what} failed");
}

impl Comm {
    fn world() -> Comm {
        Comm(unsafe { ffi::RSMPI_COMM_WORLD })
    }
    fn rank(self) -> i32 {
        let mut r = 0;
        ok(unsafe { ffi::MPI_Comm_rank(self.0, &mut r) }, "Comm_rank");
        r
    }
    fn dup(self) -> Comm {
        let mut c = self.0;
        ok(unsafe { ffi::MPI_Comm_dup(self.0, &mut c) }, "Comm_dup");
        Comm(c)
    }
    fn barrier(self) {
        ok(unsafe { ffi::MPI_Barrier(self.0) }, "Barrier");
    }
    fn send(self, dest: i32, tag: i32, buf: &[u8]) {
        let n = i32::try_from(buf.len()).expect("length fits i32");
        ok(unsafe { ffi::MPI_Send(buf.as_ptr() as *const c_void, n, ffi::RSMPI_UINT8_T, dest, tag, self.0) }, "Send");
    }
    fn recv(self, src: i32, tag: i32, buf: &mut [u8]) {
        let n = i32::try_from(buf.len()).expect("length fits i32");
        let rc = unsafe {
            ffi::MPI_Recv(buf.as_mut_ptr() as *mut c_void, n, ffi::RSMPI_UINT8_T, src, tag, self.0, ffi::RSMPI_STATUS_IGNORE)
        };
        ok(rc, "Recv");
    }
    fn isend(self, dest: i32, tag: i32, buf: &[u8]) -> Req {
        let n = i32::try_from(buf.len()).expect("length fits i32");
        let mut r = unsafe { ffi::RSMPI_REQUEST_NULL };
        ok(unsafe { ffi::MPI_Isend(buf.as_ptr() as *const c_void, n, ffi::RSMPI_UINT8_T, dest, tag, self.0, &mut r) }, "Isend");
        Req(r)
    }
    fn irecv(self, src: i32, tag: i32, buf: &mut [u8]) -> Req {
        let n = i32::try_from(buf.len()).expect("length fits i32");
        let mut r = unsafe { ffi::RSMPI_REQUEST_NULL };
        ok(unsafe { ffi::MPI_Irecv(buf.as_mut_ptr() as *mut c_void, n, ffi::RSMPI_UINT8_T, src, tag, self.0, &mut r) }, "Irecv");
        Req(r)
    }
    /// Block for one message from any rank, any tag. As backends/mpi/relay.rs `recv`: probe for the size
    /// and the sender, then receive that message. Only the in thread receives on its communicator.
    fn recv_any(self, buf: &mut Vec<u8>) -> usize {
        let mut st: ffi::MPI_Status = unsafe { std::mem::zeroed() };
        ok(unsafe { ffi::MPI_Probe(ffi::RSMPI_ANY_SOURCE, ffi::RSMPI_ANY_TAG, self.0, &mut st) }, "Probe");
        let mut n = 0;
        ok(unsafe { ffi::MPI_Get_count(&st, ffi::RSMPI_UINT8_T, &mut n) }, "Get_count");
        let n = n as usize;
        buf.resize(n, 0);
        let rc = unsafe {
            ffi::MPI_Recv(buf.as_mut_ptr() as *mut c_void, n as i32, ffi::RSMPI_UINT8_T, st.MPI_SOURCE, st.MPI_TAG, self.0, &mut st)
        };
        ok(rc, "Recv");
        n
    }
}

impl Req {
    fn null() -> Req {
        Req(unsafe { ffi::RSMPI_REQUEST_NULL })
    }
    fn test(&mut self) -> bool {
        let mut flag = 0;
        ok(unsafe { ffi::MPI_Test(&mut self.0, &mut flag, ffi::RSMPI_STATUS_IGNORE) }, "Test");
        flag != 0
    }
    fn wait(&mut self) {
        ok(unsafe { ffi::MPI_Wait(&mut self.0, ffi::RSMPI_STATUS_IGNORE) }, "Wait");
    }
}

/// MPI_Testany over two requests: the index of one that completed (it becomes null), if any did.
fn testany(a: &mut Req, b: &mut Req) -> Option<usize> {
    let mut reqs = [a.0, b.0];
    let (mut idx, mut flag) = (0, 0);
    ok(unsafe { ffi::MPI_Testany(2, reqs.as_mut_ptr(), &mut idx, &mut flag, ffi::RSMPI_STATUS_IGNORE) }, "Testany");
    a.0 = reqs[0];
    b.0 = reqs[1];
    (flag != 0 && idx >= 0).then_some(idx as usize)
}

// ---- clocks ---------------------------------------------------------------------------------------

/// CLOCK_MONOTONIC, nanoseconds. One machine's clock: only differences taken on one machine mean anything.
fn now() -> u64 {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t) };
    t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
}

#[derive(Clone, Copy, Default)]
struct Cpu {
    user: u64,
    sys: u64,
}

/// CPU time of this process (all its threads), nanoseconds, microsecond resolution.
fn cpu() -> Cpu {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
    let us = |t: libc::timeval| t.tv_sec as u64 * 1_000_000_000 + t.tv_usec as u64 * 1000;
    Cpu { user: us(ru.ru_utime), sys: us(ru.ru_stime) }
}

fn spin_for(ns: u64) {
    let end = now() + ns;
    while now() < end {
        std::hint::spin_loop();
    }
}

// ---- futex epoch (the sleep protocol of backends/mpi/epoch.rs) --------------------------------------

/// A word that moves on every act, and a count of sleepers. A bumper wakes the futex only when someone sleeps.
/// No lost wake-up: bump adds to `e` then loads `sleepers`; a sleeper adds to `sleepers` then reads `e` again
/// (all SeqCst), and the kernel sleeps only while `e` still equals what the sleeper saw.
struct Epoch {
    e: AtomicU32,
    sleepers: AtomicU32,
}

impl Epoch {
    fn new() -> Epoch {
        Epoch { e: AtomicU32::new(0), sleepers: AtomicU32::new(0) }
    }
    fn load(&self) -> u32 {
        self.e.load(SeqCst)
    }
    fn bump(&self) {
        self.e.fetch_add(1, SeqCst);
        if self.sleepers.load(SeqCst) != 0 {
            // FUTEX_WAKE without the private flag, as the backend's window futexes.
            unsafe { libc::syscall(libc::SYS_futex, self.e.as_ptr(), libc::FUTEX_WAKE, i32::MAX) };
        }
    }
    /// Sleep while the epoch equals `seen`. Returns when it differs.
    fn sleep(&self, seen: u32) {
        loop {
            if self.e.load(SeqCst) != seen {
                return;
            }
            self.sleepers.fetch_add(1, SeqCst);
            if self.e.load(SeqCst) != seen {
                self.sleepers.fetch_sub(1, SeqCst);
                return;
            }
            unsafe { libc::syscall(libc::SYS_futex, self.e.as_ptr(), libc::FUTEX_WAIT, seen, std::ptr::null::<libc::timespec>()) };
            self.sleepers.fetch_sub(1, SeqCst);
        }
    }
}

// ---- the relay's slots ------------------------------------------------------------------------------

/// One shared-memory slot with a single writer and a single reader, in lockstep (a ping-pong never has
/// two frames in flight in one direction, so the writer never overwrites a frame the reader has not
/// copied). `post` writes the frame, then moves `seq`, then bumps the epoch; `take` sleeps until `seq` moves.
struct Slot {
    seq: AtomicU64,
    len: AtomicUsize,
    /// The frame's bytes, `cap` of them, leaked: a slot lives as long as the program.
    data: *mut u8,
    wake: Epoch,
}
// SAFETY: the writer writes `data` before its release store of `seq`, the reader reads it after its acquire load.
unsafe impl Send for Slot {}
unsafe impl Sync for Slot {}

impl Slot {
    fn new(cap: usize) -> Slot {
        Slot { seq: AtomicU64::new(0), len: AtomicUsize::new(0), data: Vec::leak(vec![0u8; cap]).as_mut_ptr(), wake: Epoch::new() }
    }
    fn post(&self, src: &[u8]) {
        unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), self.data, src.len()) };
        self.len.store(src.len(), Relaxed);
        self.seq.fetch_add(1, Release);
        self.wake.bump();
    }
    /// Sleep on the futex until a frame newer than `seen` is posted; copy it into `dst`; answer (its seq, its length).
    fn take(&self, seen: u64, dst: &mut [u8]) -> (u64, usize) {
        loop {
            let e = self.wake.load();
            let s = self.seq.load(Acquire);
            if s > seen {
                let n = self.len.load(Relaxed);
                unsafe { std::ptr::copy_nonoverlapping(self.data, dst.as_mut_ptr(), n) };
                return (s, n);
            }
            self.wake.sleep(e);
        }
    }
}

/// The relay of one rank: an `out` thread that MPI_Sends what the agent posts, an `in` thread that blocks in
/// MPI_Recv(ANY_SOURCE) and posts what arrives. A zero-length frame ends both (sent to the peer by `out`).
struct Relay {
    out_slot: Arc<Slot>,
    in_slot: Arc<Slot>,
    threads: Vec<thread::JoinHandle<()>>,
    in_seen: u64,
}

impl Relay {
    fn start(comm: Comm, peer: i32, cap: usize) -> Relay {
        let out_slot = Arc::new(Slot::new(cap));
        let in_slot = Arc::new(Slot::new(cap));
        let (o, i) = (out_slot.clone(), in_slot.clone());
        let out = thread::Builder::new()
            .name("relay-out".into())
            .spawn(move || {
                let mut buf = vec![0u8; cap];
                let mut seen = 0;
                loop {
                    let (s, n) = o.take(seen, &mut buf);
                    seen = s;
                    comm.send(peer, 0, &buf[..n]);
                    if n == 0 {
                        return;
                    }
                }
            })
            .expect("out thread");
        let inn = thread::Builder::new()
            .name("relay-in".into())
            .spawn(move || {
                let mut buf = Vec::new();
                loop {
                    let n = comm.recv_any(&mut buf);
                    if n == 0 {
                        return;
                    }
                    i.post(&buf[..n]);
                }
            })
            .expect("in thread");
        Relay { out_slot, in_slot, threads: vec![out, inn], in_seen: 0 }
    }
    fn send(&self, frame: &[u8]) {
        self.out_slot.post(frame);
    }
    fn recv(&mut self, dst: &mut [u8]) -> usize {
        let (s, n) = self.in_slot.take(self.in_seen, dst);
        self.in_seen = s;
        n
    }
    fn stop(self) {
        self.out_slot.post(&[]);
        for t in self.threads {
            t.join().expect("relay thread");
        }
    }
}

// ---- measurement ------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Direct,
    DirectSingle,
    Relay,
    MixedSelf,
    MixedFlag,
    WakeFutex,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Direct => "direct",
            Mode::DirectSingle => "direct_single",
            Mode::Relay => "relay",
            Mode::MixedSelf => "mixed_self",
            Mode::MixedFlag => "mixed_flag",
            Mode::WakeFutex => "wake_futex",
        }
    }
}

struct Opts {
    mode: Mode,
    sizes: Vec<usize>,
    iters: usize,
    warmup: usize,
}

/// Median, 99th percentile (nearest rank), minimum, mean: in nanoseconds.
#[derive(Clone, Copy)]
struct Stats([f64; 4]);

/// What one rank knows of one row. Only the rank that timed has `stats` (rtt: rank 0, wake: rank 1); every rank
/// has its CPU over `cpu_iters` iterations. The two ranks produce the same rows in the same order.
struct Row {
    phase: &'static str,
    size: usize,
    iters: usize,
    stats: Option<Stats>,
    cpu: Cpu,
    cpu_iters: usize,
}

/// Summary of `v` / `div`.
fn summary(mut v: Vec<u64>, div: f64) -> Stats {
    assert!(!v.is_empty(), "no samples");
    v.sort_unstable();
    let n = v.len();
    let median = if n % 2 == 1 { v[n / 2] as f64 } else { (v[n / 2 - 1] + v[n / 2]) as f64 / 2.0 };
    let p99 = v[(n * 99).div_ceil(100) - 1] as f64;
    let mean = v.iter().map(|&x| x as f64).sum::<f64>() / n as f64;
    Stats([median / div, p99 / div, v[0] as f64 / div, mean / div])
}

fn diff(a: Cpu, b: Cpu) -> Cpu {
    Cpu { user: b.user - a.user, sys: b.sys - a.sys }
}

/// How the receiving rank also waits for a local waker, during the frame ping-pong. Nobody wakes it there;
/// the cost shown is that of having the extra wait source. The wake phase uses the same sources.
enum Local {
    None,
    /// A posted MPI_Irecv of one byte from this rank to itself, tested together with the frame's.
    SelfMsg { req: Req, buf: Box<[u8; 1]> },
    /// An atomic flag loaded between tests.
    Flag(Arc<AtomicU64>),
}

impl Local {
    fn repost(&mut self, comm: Comm, me: i32) {
        if let Local::SelfMsg { req, buf } = self {
            *req = comm.irecv(me, TAG_WAKE, &mut buf[..]);
        }
    }
    /// Spin until the frame request completes. A local wake here is a bug: nobody wakes this rank in the ping-pong.
    fn wait_frame(&mut self, cross: &mut Req) {
        match self {
            Local::None => while !cross.test() {},
            Local::SelfMsg { req, .. } => loop {
                match testany(cross, req) {
                    Some(0) => return,
                    Some(_) => panic!("a local wake in the frame phase"),
                    None => {}
                }
            },
            Local::Flag(f) => loop {
                if cross.test() {
                    return;
                }
                assert_eq!(f.load(Acquire), 0, "a local wake in the frame phase");
            },
        }
    }
}

fn put_counter(buf: &mut [u8], i: u64) {
    buf[..8].copy_from_slice(&i.to_le_bytes());
}

fn get_counter(buf: &[u8]) -> u64 {
    u64::from_le_bytes(buf[..8].try_into().unwrap())
}

/// Ping-pong of `size` bytes, `warmup + iters` round trips. Rank 0 times each round trip (the Irecv for the
/// reply is posted before the clock starts, and a posted Irecv is what a waiting agent has). The frame's
/// first 8 bytes are a counter that rank 1 echoes; a wrong echo fails the run. `relay` is Some in relay mode.
fn pingpong(comm: Comm, rank: i32, size: usize, o: &Opts, local: &mut Local, mut relay: Option<&mut Relay>) -> Row {
    let peer = 1 - rank;
    let total = o.warmup + o.iters;
    let mut sbuf = vec![0u8; size];
    let mut rbuf = vec![0u8; size];
    let mut samples = Vec::with_capacity(o.iters);
    let mut c0 = Cpu::default();
    let mut req = if rank == 1 && relay.is_none() { comm.irecv(peer, TAG_FRAME, &mut rbuf) } else { Req::null() };
    for i in 0..total {
        if i == o.warmup {
            c0 = cpu();
        }
        if rank == 0 {
            put_counter(&mut sbuf, i as u64);
            let t0;
            if let Some(r) = relay.as_deref_mut() {
                t0 = now();
                r.send(&sbuf);
                assert_eq!(r.recv(&mut rbuf), size);
            } else {
                req = comm.irecv(peer, TAG_FRAME, &mut rbuf);
                t0 = now();
                comm.send(peer, TAG_FRAME, &sbuf);
                local.wait_frame(&mut req);
            }
            let t1 = now();
            assert_eq!(get_counter(&rbuf), i as u64, "echo out of order");
            if i >= o.warmup {
                samples.push(t1 - t0);
            }
        } else {
            if let Some(r) = relay.as_deref_mut() {
                assert_eq!(r.recv(&mut rbuf), size);
            } else {
                local.wait_frame(&mut req);
            }
            let c = get_counter(&rbuf);
            assert_eq!(c, i as u64, "request out of order");
            put_counter(&mut sbuf, c);
            if let Some(r) = relay.as_deref_mut() {
                r.send(&sbuf);
            } else {
                // The next Irecv is posted before the reply leaves, so one is always posted when a frame arrives.
                if i + 1 < total {
                    req = comm.irecv(peer, TAG_FRAME, &mut rbuf);
                }
                comm.send(peer, TAG_FRAME, &sbuf);
            }
        }
    }
    let cpu_used = diff(c0, cpu());
    // The round trip is two one-way frames.
    let stats = (!samples.is_empty()).then(|| summary(samples, 2.0));
    Row { phase: "rtt", size, iters: o.iters, stats, cpu: cpu_used, cpu_iters: o.iters }
}

/// The wake phase of both ranks. Rank 1 is woken `warmup + iters` times by a second thread, each time after it has
/// seen the previous wake and a short random pause (5 to 25 us of spinning, so wakes do not lock to a periodic
/// event). The latency is `now()` at the receiver minus `now()` taken by the waker immediately before it acts; both
/// are this machine's CLOCK_MONOTONIC. For `MixedSelf` the act is MPI_Isend to this rank (completed by an
/// MPI_Wait after the clock is read), for `MixedFlag` a store, for `WakeFutex` an epoch bump that futex-wakes
/// a sleeping receiver. Rank 1's frame Irecv stays posted throughout (`MixedSelf`, `MixedFlag`); rank 0 sends the
/// frame that completes it after the phase. Rank 0 only waits in a barrier. The CPU of both ranks is taken over
/// the whole phase, warm-up included, and divided by the number of wakes: the pauses are in it, and so
/// is the waker thread's spinning.
fn wake_round(comm: Comm, rank: i32, kind: Mode, o: &Opts, local: &mut Local) -> Row {
    let n = o.warmup + o.iters;
    comm.barrier();
    if rank == 0 {
        let c0 = cpu();
        comm.barrier();
        let c = diff(c0, cpu());
        if kind != Mode::WakeFutex {
            comm.send(1, TAG_FRAME, &[0u8; 8]);
        }
        return Row { phase: "wake", size: 1, iters: o.iters, stats: None, cpu: c, cpu_iters: n };
    }
    let me = rank;
    let mut cross_buf = [0u8; 8];
    let mut cross = if kind == Mode::WakeFutex { Req::null() } else { comm.irecv(0, TAG_FRAME, &mut cross_buf) };
    let armed = Arc::new(AtomicU64::new(0));
    let flag = match local {
        Local::Flag(f) => f.clone(),
        _ => Arc::new(AtomicU64::new(0)),
    };
    let ep = Arc::new(Epoch::new());
    let (a, f, e) = (armed.clone(), flag.clone(), ep.clone());
    let c0 = cpu();
    let waker = thread::Builder::new()
        .name("waker".into())
        .spawn(move || {
            let mut t0s = Vec::with_capacity(n);
            let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
            let one = [1u8; 1];
            spin_for(2_000_000); // the receiver is waiting by now
            for k in 0..n as u64 {
                while a.load(Acquire) < k {
                    std::hint::spin_loop();
                }
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                spin_for(5_000 + x % 20_000);
                let t0 = now();
                match kind {
                    Mode::MixedSelf => {
                        let mut r = comm.isend(me, TAG_WAKE, &one);
                        t0s.push(t0);
                        r.wait();
                    }
                    Mode::MixedFlag => {
                        f.store(k + 1, Release);
                        t0s.push(t0);
                    }
                    _ => {
                        e.bump();
                        t0s.push(t0);
                    }
                }
            }
            t0s
        })
        .expect("waker thread");
    let mut seen_at = Vec::with_capacity(n);
    for k in 0..n as u64 {
        match (kind, &mut *local) {
            (Mode::MixedSelf, Local::SelfMsg { req, .. }) => loop {
                match testany(&mut cross, req) {
                    Some(1) => break,
                    Some(_) => panic!("a frame in the wake phase"),
                    None => {}
                }
            },
            (Mode::MixedFlag, Local::Flag(f)) => {
                while f.load(Acquire) <= k {
                    assert!(!cross.test(), "a frame in the wake phase");
                }
            }
            _ => loop {
                let s = ep.load();
                if u64::from(s) > k {
                    break;
                }
                ep.sleep(s);
            },
        }
        seen_at.push(now());
        // The wake Irecv is posted again before the waker may act again.
        local.repost(comm, me);
        armed.store(k + 1, Release);
    }
    let c = diff(c0, cpu());
    let t0s = waker.join().expect("waker");
    comm.barrier();
    if kind != Mode::WakeFutex {
        while !cross.test() {}
    }
    // The last repost of the wake Irecv is completed by a message to self.
    if let Local::SelfMsg { req, .. } = local {
        let mut s = comm.isend(me, TAG_WAKE, &[0u8]);
        req.wait();
        s.wait();
    }
    let lat: Vec<u64> = seen_at.iter().zip(&t0s).skip(o.warmup).map(|(s, t)| s - t).collect();
    Row { phase: "wake", size: 1, iters: o.iters, stats: Some(summary(lat, 1.0)), cpu: c, cpu_iters: n }
}

// ---- report -----------------------------------------------------------------------------------------

fn hostname() -> String {
    let mut b = [0 as libc::c_char; 256];
    unsafe { libc::gethostname(b.as_mut_ptr(), b.len()) };
    unsafe { std::ffi::CStr::from_ptr(b.as_ptr()) }.to_string_lossy().into_owned()
}

fn library_version() -> String {
    let mut b = vec![0 as libc::c_char; 8192];
    let mut n = 0;
    unsafe { ffi::MPI_Get_library_version(b.as_mut_ptr(), &mut n) };
    unsafe { std::ffi::CStr::from_ptr(b.as_ptr()) }.to_string_lossy().lines().next().unwrap_or("").to_string()
}

fn parse() -> Opts {
    let mut args = std::env::args().skip(1);
    let mode = match args.next().as_deref() {
        Some("direct") => Mode::Direct,
        Some("direct_single") => Mode::DirectSingle,
        Some("relay") => Mode::Relay,
        Some("mixed_self") => Mode::MixedSelf,
        Some("mixed_flag") => Mode::MixedFlag,
        Some("wake_futex") => Mode::WakeFutex,
        other => panic!("mode: direct, direct_single, relay, mixed_self, mixed_flag or wake_futex; got {other:?}"),
    };
    let mut o = Opts { mode, sizes: vec![8, 64, 1024, 65536], iters: 100_000, warmup: 10_000 };
    while let Some(a) = args.next() {
        let v = args.next().unwrap_or_else(|| panic!("{a} needs a value"));
        match a.as_str() {
            "--sizes" => o.sizes = v.split(',').map(|s| s.parse().expect("size")).collect(),
            "--iters" => o.iters = v.parse().expect("iters"),
            "--warmup" => o.warmup = v.parse().expect("warmup"),
            _ => panic!("unknown option {a}"),
        }
    }
    assert!(o.sizes.iter().all(|&s| s >= 8), "a frame carries an 8-byte counter");
    o
}

fn main() {
    let o = parse();
    let single = o.mode == Mode::DirectSingle;
    let (universe, got) = if single {
        (mpi::initialize().expect("MPI_Init"), mpi::Threading::Single)
    } else {
        mpi::initialize_with_threading(mpi::Threading::Multiple).expect("MPI_Init_thread")
    };
    let wanted = if single { mpi::Threading::Single } else { mpi::Threading::Multiple };
    assert!(got == wanted, "thread level {got:?}, wanted {wanted:?}");
    let world = Comm::world();
    let rank = world.rank();
    let mut size = 0;
    ok(unsafe { ffi::MPI_Comm_size(world.0, &mut size) }, "Comm_size");
    assert_eq!(size, 2, "two ranks: one on each machine");
    eprintln!("# rank {rank} host {} mode {} thread {got:?} {}", hostname(), o.mode.name(), library_version());

    let mut rows = Vec::new();
    world.barrier();
    match o.mode {
        Mode::Direct | Mode::DirectSingle | Mode::MixedSelf | Mode::MixedFlag => {
            let mut local = match o.mode {
                Mode::MixedSelf => {
                    let mut buf = Box::new([0u8; 1]);
                    let req = world.irecv(rank, TAG_WAKE, &mut buf[..]);
                    Local::SelfMsg { req, buf }
                }
                Mode::MixedFlag => Local::Flag(Arc::new(AtomicU64::new(0))),
                _ => Local::None,
            };
            for &s in &o.sizes {
                rows.push(pingpong(world, rank, s, &o, &mut local, None));
            }
            if matches!(o.mode, Mode::MixedSelf | Mode::MixedFlag) {
                rows.push(wake_round(world, rank, o.mode, &o, &mut local));
                // Rank 0's wake Irecv, posted above and never woken, is completed by a message to self.
                if let (Local::SelfMsg { req, .. }, 0) = (&mut local, rank) {
                    let mut s = world.isend(rank, TAG_WAKE, &[0u8]);
                    req.wait();
                    s.wait();
                }
            }
        }
        Mode::Relay => {
            // The relay has its own communicator, so that this program's barrier and final gather cannot meet
            // its ANY_SOURCE receive. The backend's relay uses the world; no other MPI call of this program
            // runs while the relay is up.
            let max = *o.sizes.iter().max().unwrap();
            let mut relay = Relay::start(world.dup(), 1 - rank, max);
            for &s in &o.sizes {
                rows.push(pingpong(world, rank, s, &o, &mut Local::None, Some(&mut relay)));
            }
            relay.stop();
        }
        Mode::WakeFutex => rows.push(wake_round(world, rank, o.mode, &o, &mut Local::None)),
    }
    gather(world, rank, &o, rows);
    drop(universe);
}

/// Rank 1 sends its rows (stats, CPU) to rank 0, after every other MPI act of the program, and rank 0 prints the
/// table: the stats from whichever rank timed, the CPU of each rank per iteration.
fn gather(world: Comm, rank: i32, o: &Opts, rows: Vec<Row>) {
    // Per row: has_stats, four stats, user, sys, cpu_iters.
    let pack = |r: &Row| {
        let s = r.stats.map_or([f64::NAN; 4], |s| s.0);
        [r.stats.is_some() as u8 as f64, s[0], s[1], s[2], s[3], r.cpu.user as f64, r.cpu.sys as f64, r.cpu_iters as f64]
    };
    let mine: Vec<[f64; 8]> = rows.iter().map(pack).collect();
    if rank == 1 {
        let bytes: Vec<u8> = mine.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        world.send(0, TAG_CPU, &bytes);
        return;
    }
    let mut bytes = vec![0u8; rows.len() * 64];
    world.recv(1, TAG_CPU, &mut bytes);
    let theirs: Vec<f64> = bytes.chunks(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect();
    println!("mode\tphase\tsize\titers\tmedian_ns\tp99_ns\tmin_ns\tmean_ns\tcpu0_ns\tcpu1_ns\tsys0_ns\tsys1_ns");
    for (r, t) in rows.iter().zip(theirs.chunks(8)) {
        let (a, b) = (pack(r), t);
        assert!((a[0] > 0.0) != (b[0] > 0.0), "exactly one rank timed row {} {}", r.phase, r.size);
        let s = if a[0] > 0.0 { &a[1..5] } else { &b[1..5] };
        let per = |c: f64, n: f64| c / n;
        println!(
            "{}\t{}\t{}\t{}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.0}\t{:.0}\t{:.0}\t{:.0}",
            o.mode.name(),
            r.phase,
            r.size,
            r.iters,
            s[0],
            s[1],
            s[2],
            s[3],
            per(a[5] + a[6], a[7]),
            per(b[5] + b[6], b[7]),
            per(a[6], a[7]),
            per(b[6], b[7]),
        );
    }
}
