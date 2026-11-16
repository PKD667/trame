// Routes between workers of two hosts (CONTRACT.md, F3, F5). Frames go rank to rank by two-sided MPI
// on `Host::cross`, a duplicate of the world, with no thread between: the experiment mpi_cross_host
// measured a relay thread at 49 to 50 us one way for 8 bytes against 30 us rank to rank, on two
// machines (HOSTS.md, Routes between hosts).
//
// A route is a lane. Its frames travel with MPI tag 2 lane, its credits with 2 lane + 1. The lane of
// a route is its position, by F4 tag, among the routes with the same sender and receiver, so the two
// hosts number it alike once they agree on their declarations (`connect`). MPI matches messages of one
// sender, tag and communicator in the order they were sent, which keeps T1.
//
// Receiver. It keeps `cap` receive buffers, each with a posted MPI_Irecv: buffer `i mod cap` holds
// frame i, for i from C, the frames consumed, to C + cap. Taking frame C copies it out, posts the
// receive of frame C + cap in its buffer, and sends C + 1 back as a credit.
//
// Sender. It knows C only from credits: `c`, the largest it received, is at most the receiver's C.
// Frame P is sent only when P - c < cap, so it always finds a posted receive, and a lossless send
// answers Full exactly when P - c = cap. That is INTERFACE.md's Full with U, the frames taken whose credit
// has not arrived (Release). A lossy send never waits: with no room, it keeps the frame as pending,
// replacing any older pending one (a Drop of a frame that is not the newest), and sends it once a
// credit makes room.
//
// Wake (S3). An agent of a worker with lanes never sleeps on the futex: it polls its lanes between
// checks of its epoch, as the experiment's mixed_flag mode did, which cost nothing measurable on the
// frames' latency. Every arrival of a frame or of a credit that a test observes moves the worker's
// epoch, whoever observed it, so a waiter that read the epoch before cannot miss it.
//
// Counts (detect.rs). Each end counts in the window, in its route's P and C words, the messages it
// sent and those it saw arrive, frames and credits alike. A credit counts as sent when it falls due,
// since it is sent at a later poll however long the end idles; a frame counts as arrived when a test
// observes it, at any slot, not only the head's, so a frame that waits behind an untaken head is
// counted. An end that has ended keeps testing its posted receives until the nonbarrier completes.
// Then a message in flight is exactly a difference of the sums of the two counts over every end.
//
// End. MPI_Finalize needs every message complete. `quiesce`, before it on every rank: complete this
// rank's sends, which always match a posted receive; then a nonbarrier, MPI_Ibarrier on `cross`,
// progressing all the while; once it completes no rank has a send outstanding, and the posted
// receives are cancelled. A pending lossy frame is dropped; frames sent to a worker that has ended are
// not taken (federation: a route is lossless while both ends run), though they are counted.

extern crate std;

use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering::SeqCst;
use std::boxed::Box;
use std::format;
use std::string::String;
use std::vec::Vec;

use mpi::ffi;
use mpi::traits::*;

use crate::launch::{Plan, Refused};
use crate::topology::{End, HERE};

use crate::backends::mpi::memory::layout::{Layout, RouteLayout};
use crate::backends::mpi::memory::shm::Window;
use crate::backends::mpi::transport::hosts::host;
use crate::backends::mpi::types::Answer;

fn ok(rc: i32, what: &str) {
    assert_eq!(rc, ffi::MPI_SUCCESS as i32, "trame: MPI_{what} failed on a route between hosts");
}

fn null() -> ffi::MPI_Request {
    // SAFETY: a constant of the MPI library (PROOFS.md, Assumptions: Foreign calls).
    unsafe { ffi::RSMPI_REQUEST_NULL }
}

/// Whether request `r` is complete; a null request is. Drives MPI's progress.
fn test(r: &mut ffi::MPI_Request) -> bool {
    let mut flag = 0;
    // SAFETY: `r` is a request of this process, or null (Foreign calls).
    ok(unsafe { ffi::MPI_Test(r, &mut flag, ffi::RSMPI_STATUS_IGNORE) }, "Test");
    flag != 0
}

fn wait(r: &mut ffi::MPI_Request) {
    // SAFETY: as `test` (Foreign calls).
    ok(unsafe { ffi::MPI_Wait(r, ffi::RSMPI_STATUS_IGNORE) }, "Wait");
}

/// Cancel a posted receive and complete it. One that already matched completes normally.
fn cancel(r: &mut ffi::MPI_Request) {
    if *r != null() {
        // SAFETY: `r` is a receive this process posted (Foreign calls).
        ok(unsafe { ffi::MPI_Cancel(r) }, "Cancel");
        wait(r);
    }
}

/// Post a receive of `n` bytes at `buf`. The buffer must stay put until the request completes.
fn irecv(buf: *mut u8, n: usize, peer: i32, tag: i32, comm: ffi::MPI_Comm, r: &mut ffi::MPI_Request) {
    // SAFETY: `buf` holds `n` bytes of a boxed slice of the lane, which lives until `quiesce` has
    // completed or cancelled every request on it (F5; Foreign calls).
    ok(unsafe { ffi::MPI_Irecv(buf.cast(), n as i32, ffi::RSMPI_UINT8_T, peer, tag, comm, r) }, "Irecv");
}

/// Send `n` bytes at `buf`. The buffer must stay unchanged until the request completes.
fn isend(buf: *const u8, n: usize, peer: i32, tag: i32, comm: ffi::MPI_Comm, r: &mut ffi::MPI_Request) {
    // SAFETY: as `irecv`, and the buffer is not written until the send completes: `put` waits on it
    // first (F5; Foreign calls).
    ok(unsafe { ffi::MPI_Isend(buf.cast(), n as i32, ffi::RSMPI_UINT8_T, peer, tag, comm, r) }, "Isend");
}

/// A count word of the window (`Counts` above): the route's P or C word, written by this end only.
struct Count(*const AtomicU64);

impl Count {
    fn at(win: &Window, off: usize) -> Count {
        Count(win.ptr(off, 8) as *const AtomicU64)
    }

    fn add(&self) {
        // SAFETY: an aligned word of the window, which stays mapped until MPI_Finalize, after the
        // lanes are closed (shm.rs; F5).
        unsafe { &*self.0 }.fetch_add(1, SeqCst);
    }
}

/// The sending end of a lane.
pub(crate) struct Out {
    comm: ffi::MPI_Comm,
    peer: i32,
    lane: i32,
    size: usize,
    cap: u64,
    lossy: bool,
    /// Frames sent, and the largest credit received.
    p: u64,
    c: u64,
    /// Frame i is sent from buffer i mod cap, by request i mod cap.
    bufs: Box<[u8]>,
    reqs: Box<[ffi::MPI_Request]>,
    credit: Box<u64>,
    credit_req: ffi::MPI_Request,
    /// A lossy frame waiting for room.
    pending: Option<Box<[u8]>>,
    spare: Box<[u8]>,
    /// Frames sent, credits arrived (`Counts`).
    sent: Count,
    got: Count,
}

/// The receiving end of a lane.
pub(crate) struct In {
    comm: ffi::MPI_Comm,
    peer: i32,
    lane: i32,
    size: usize,
    cap: u64,
    /// Frames consumed.
    c: u64,
    bufs: Box<[u8]>,
    reqs: Box<[ffi::MPI_Request]>,
    /// Whether the frame of buffer i has arrived (its request completed, now null).
    arrived: Box<[bool]>,
    credit: Box<u64>,
    credit_req: ffi::MPI_Request,
    /// A credit to send once the last one has gone.
    due: bool,
    /// Credits sent (from when they fall due), frames arrived (`Counts`).
    sent: Count,
    got: Count,
}

pub(crate) enum Lane {
    Out(Out),
    In(In),
}

/// The lanes of the endpoint of rank `me`, by route: one per route between hosts that it sends or
/// receives. Posts the receives. Allocates: called at launch, before any step.
pub(crate) fn lanes(lay: &Layout, me: u32, win: &Window) -> Vec<Option<Lane>> {
    let mut out = Vec::with_capacity(lay.routes.len());
    for r in &lay.routes {
        out.push(r.remote.filter(|_| r.sent_by(me) || r.received_by(me)).map(|x| lane(r, x, win)));
    }
    out
}

fn lane(r: &RouteLayout, x: crate::backends::mpi::memory::layout::Remote, win: &Window) -> Lane {
    let (sent, got) = (Count::at(win, r.base), Count::at(win, r.base + crate::backends::mpi::memory::layout::LINE));
    let comm = host().cross.as_raw();
    let n = r.size;
    let cap = r.cap as usize;
    let bufs = std::vec![0u8; n * cap].into_boxed_slice();
    let reqs = std::vec![null(); cap].into_boxed_slice();
    if x.outgoing {
        let mut o = Out {
            comm,
            peer: x.world,
            lane: x.lane,
            size: n,
            cap: r.cap,
            lossy: r.lossy,
            p: 0,
            c: 0,
            bufs,
            reqs,
            credit: Box::new(0),
            credit_req: null(),
            pending: None,
            spare: std::vec![0u8; n].into_boxed_slice(),
            sent,
            got,
        };
        o.post_credit();
        Lane::Out(o)
    } else {
        let mut i = In {
            comm,
            peer: x.world,
            lane: x.lane,
            size: n,
            cap: r.cap,
            c: 0,
            bufs,
            reqs,
            arrived: std::vec![false; cap].into_boxed_slice(),
            credit: Box::new(0),
            credit_req: null(),
            due: false,
            sent,
            got,
        };
        for s in 0..cap {
            i.post(s);
        }
        Lane::In(i)
    }
}

impl Out {
    fn post_credit(&mut self) {
        let at = (&mut *self.credit as *mut u64).cast::<u8>();
        irecv(at, 8, self.peer, 2 * self.lane + 1, self.comm, &mut self.credit_req);
    }

    /// Take every credit that has arrived. Answers whether room grew.
    pub(crate) fn poll(&mut self) -> bool {
        let mut grew = false;
        while test(&mut self.credit_req) {
            self.got.add();
            if *self.credit > self.c {
                self.c = *self.credit;
                grew = true;
            }
            self.post_credit();
        }
        if self.p - self.c < self.cap
            && let Some(f) = self.pending.take()
        {
            self.put(&f);
            self.spare = f;
        }
        grew
    }

    fn put(&mut self, msg: &[u8]) {
        let s = (self.p % self.cap) as usize;
        // Frame p - cap left this buffer, and its credit says it was received: the send completes.
        wait(&mut self.reqs[s]);
        let at = &mut self.bufs[s * self.size..(s + 1) * self.size];
        at.copy_from_slice(msg);
        isend(at.as_ptr(), self.size, self.peer, 2 * self.lane, self.comm, &mut self.reqs[s]);
        self.sent.add();
        self.p += 1;
    }

    /// Frames sent, and the largest credit received.
    pub(crate) fn counts(&self) -> (u64, u64) {
        (self.p, self.c)
    }

    /// Send `msg`. Answers Full only on a lossless lane with `cap` frames uncredited; the second
    /// value says whether room grew meanwhile (a credit arrived).
    pub(crate) fn send(&mut self, msg: &[u8]) -> (Answer, bool) {
        let grew = self.poll();
        if self.p - self.c < self.cap {
            self.put(msg);
            (Answer::Ok, grew)
        } else if self.lossy {
            // No allocation: the pending buffer and the spare swap.
            let mut f = self.pending.take().unwrap_or_else(|| core::mem::take(&mut self.spare));
            f.copy_from_slice(msg);
            self.pending = Some(f);
            (Answer::Ok, grew)
        } else {
            (Answer::Full, grew)
        }
    }

    /// Whether a send would find room; the second value as `send`'s.
    pub(crate) fn room(&mut self) -> (bool, bool) {
        let grew = self.poll();
        (self.lossy || self.p - self.c < self.cap, grew)
    }

    /// Complete every send; drop a pending frame. Answers whether all are complete.
    fn drain(&mut self) -> bool {
        self.pending = None;
        self.poll();
        self.reqs.iter_mut().fold(true, |all, r| test(r) && all)
    }

    fn close(&mut self) {
        cancel(&mut self.credit_req);
    }
}

impl In {
    fn post(&mut self, s: usize) {
        let at = self.bufs[s * self.size..].as_mut_ptr();
        irecv(at, self.size, self.peer, 2 * self.lane, self.comm, &mut self.reqs[s]);
    }

    /// Send the credit if one is due and the last has gone.
    fn credit(&mut self) {
        if self.due && test(&mut self.credit_req) {
            *self.credit = self.c;
            isend(
                (&*self.credit as *const u64).cast(),
                8,
                self.peer,
                2 * self.lane + 1,
                self.comm,
                &mut self.credit_req,
            );
            self.due = false;
        }
    }

    /// Frames consumed: the index of the head.
    pub(crate) fn consumed(&self) -> u64 {
        self.c
    }

    /// Whether frame C has arrived; the second value says whether it arrived just now.
    pub(crate) fn head(&mut self) -> (bool, bool) {
        self.credit();
        let s = (self.c % self.cap) as usize;
        if self.arrived[s] {
            return (true, false);
        }
        let now = test(&mut self.reqs[s]);
        self.arrived[s] = now;
        if now {
            self.got.add();
        }
        (now, now)
    }

    /// Observe every frame that has arrived, at any slot (`Counts`). Answers whether one did.
    fn observe(&mut self) -> bool {
        let mut any = false;
        for s in 0..self.reqs.len() {
            if !self.arrived[s] && test(&mut self.reqs[s]) {
                self.arrived[s] = true;
                self.got.add();
                any = true;
            }
        }
        any
    }

    /// Take frame C into `out` if it has arrived: its buffer gets the receive of frame C + cap, and a
    /// credit goes back. The second value as `head`'s.
    pub(crate) fn take(&mut self, out: &mut [u8]) -> (Option<u64>, bool) {
        let (here, now) = self.head();
        if !here {
            return (None, now);
        }
        let s = (self.c % self.cap) as usize;
        out[..self.size].copy_from_slice(&self.bufs[s * self.size..(s + 1) * self.size]);
        self.arrived[s] = false;
        self.post(s);
        let index = self.c;
        self.c += 1;
        if !self.due {
            self.sent.add();
        }
        self.due = true;
        self.credit();
        (Some(index), now)
    }

    /// Send the credit that is due, which counts as sent already; send no more after it; observe
    /// the frames that arrive (`Counts`). Answers whether the last credit is complete.
    fn drain(&mut self) -> bool {
        self.credit();
        self.observe();
        !self.due && test(&mut self.credit_req)
    }

    fn close(&mut self) {
        for s in 0..self.reqs.len() {
            if !self.arrived[s] {
                cancel(&mut self.reqs[s]);
            }
        }
    }
}

impl Lane {
    /// Progress: take credits, test the head. Answers whether anything arrived.
    pub(crate) fn poll(&mut self) -> bool {
        match self {
            Lane::Out(o) => o.poll(),
            Lane::In(i) => {
                let head = i.head().1;
                i.observe() || head
            }
        }
    }

    fn drain(&mut self) -> bool {
        match self {
            Lane::Out(o) => o.drain(),
            Lane::In(i) => i.drain(),
        }
    }

    fn close(&mut self) {
        match self {
            Lane::Out(o) => o.close(),
            Lane::In(i) => i.close(),
        }
    }
}

/// Before MPI_Finalize, on every rank of the world (see the module comment). `lanes` are this rank's.
pub(crate) fn quiesce(lanes: &mut [Option<Lane>]) {
    let idle = || std::thread::sleep(std::time::Duration::from_micros(20));
    let all = |lanes: &mut [Option<Lane>]| lanes.iter_mut().flatten().fold(true, |a, l| l.drain() && a);
    while !all(lanes) {
        idle();
    }
    let mut bar = null();
    // SAFETY: `cross` is a communicator of every rank of the world, and every rank calls this once,
    // in `finalize` (F5; Foreign calls).
    ok(unsafe { ffi::MPI_Ibarrier(host().cross.as_raw(), &mut bar) }, "Ibarrier");
    let mut answers = Vec::new();
    // A leader takes part in the detector's rounds until every leader has ended (detect.rs).
    if host().peers.is_some() {
        while crate::backends::mpi::roles::detect::round(true) != crate::backends::mpi::roles::detect::Verdict::Over {
            answer_connects(&mut answers);
            idle();
        }
    }
    while !test(&mut bar) {
        all(lanes);
        answer_connects(&mut answers);
        idle();
    }
    for (_, r) in &mut answers {
        wait(r);
    }
    for l in lanes.iter_mut().flatten() {
        l.close();
    }
}

/// On a leader that ends: answer the connect of every host still waiting for this one (F3). Such a
/// host declares routes with this one, and this one declared none back, or it would have answered in
/// its own `connect`: the answer is "no routes", and that host refuses its launch. A connect that
/// arrives before the end waits for it: a host learns of a one-sided declaration at the latest when
/// the other host ends, and no host waits for one it declares nothing with (F7). The replies stay
/// in `out` until they complete.
fn answer_connects(out: &mut Vec<(Vec<u8>, ffi::MPI_Request)>) {
    let Some(peers) = host().peers.as_ref() else { return };
    while let Some((msg, status)) = peers.any_process().immediate_matched_probe_with_tag(CONNECT_TAG) {
        let mut b = std::vec![0u8; status.count(u8::equivalent_datatype()) as usize];
        msg.matched_receive_into(&mut b[..]);
        out.push((encode(true, &[]), null()));
        let (reply, r) = out.last_mut().expect("just pushed");
        isend(reply.as_ptr(), reply.len(), status.source_rank(), CONNECT_TAG, peers.as_raw(), r);
    }
}

/// A route between hosts as both hosts declare it (F3): sender and receiver as (host, worker), its
/// F4 tag, size, capacity, kind, and the type's name for the message.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Port {
    from: (u32, u32),
    to: (u32, u32),
    wire: u64,
    size: u64,
    cap: u64,
    lossy: bool,
    name: String,
}

/// An end with its host: this host's id for an end here.
pub(crate) fn global(e: End, workers: u32, me: u32) -> (u32, u32) {
    match e {
        End::Worker(w) if w.1 == HERE => (me, w.0),
        End::Worker(w) => (w.1, w.0),
        End::Leader => (me, workers),
    }
}

fn ports(plan: &Plan, me: u32) -> Vec<Port> {
    let n = plan.workers.len() as u32;
    let mut out: Vec<Port> = plan
        .routes
        .iter()
        .map(|r| (r, global(r.from, n, me), global(r.to, n, me)))
        .filter(|(_, f, t)| f.0 != t.0)
        .map(|(r, from, to)| Port {
            from,
            to,
            wire: r.wire,
            size: r.size as u64,
            cap: r.cap as u64,
            lossy: r.lossy,
            name: r.name.into(),
        })
        .collect();
    out.sort();
    out
}

fn encode(ok: bool, ports: &[Port]) -> Vec<u8> {
    let mut b = std::vec![u8::from(ok)];
    for p in ports {
        for v in [p.from.0, p.from.1, p.to.0, p.to.1] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [p.wire, p.size, p.cap, p.name.len() as u64] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.push(u8::from(p.lossy));
        b.extend_from_slice(p.name.as_bytes());
    }
    b
}

fn decode(b: &[u8]) -> (bool, Vec<Port>) {
    let u32_at = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().expect("4 bytes"));
    let u64_at = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().expect("8 bytes"));
    let mut out = Vec::new();
    let mut i = 1;
    while i < b.len() {
        let len = u64_at(i + 40) as usize;
        out.push(Port {
            from: (u32_at(i), u32_at(i + 4)),
            to: (u32_at(i + 8), u32_at(i + 12)),
            wire: u64_at(i + 16),
            size: u64_at(i + 24),
            cap: u64_at(i + 32),
            lossy: b[i + 48] != 0,
            name: String::from_utf8_lossy(&b[i + 49..i + 49 + len]).into_owned(),
        });
        i += 49 + len;
    }
    (b[0] != 0, out)
}

fn kind(p: &Port) -> String {
    format!("({}, capacity {})", if p.lossy { "lossy" } else { "lossless" }, p.cap)
}

fn describe(p: &Port) -> String {
    format!(
        "{} from worker {} of host {} to worker {} of host {} {}",
        p.name,
        p.from.1,
        p.from.0,
        p.to.1,
        p.to.0,
        kind(p)
    )
}

/// The peer-channel tag of a connect message, apart from peer messages (hosts.rs `PEER_TAG`).
const CONNECT_TAG: i32 = 8;

/// F3, at launch. The leader sends to each host it shares a route with the routes it declares with
/// that host, and receives theirs; it checks that both sides declare the same routes and that the
/// peer was not refused. The verdict goes to every rank of the host, so the launch is refused on all
/// of them or none (L1). Only hosts that share routes wait for each other (F7): a host that declares
/// no route with this one is not asked, and a host that declares routes with a host that declares
/// none back waits for its answer, which comes at the latest when that host ends (`answer_connects`). `fine` is whether this host's own checks passed; a host that fails them
/// still answers, so its peers are refused too.
pub(crate) fn connect(plan: &Plan, fine: bool) -> Result<(), Refused> {
    let h = host();
    if h.count == 1 {
        return Ok(());
    }
    // The verdict: empty when the launch may go on, else why not. It travels behind one byte, since
    // MPI rejects the dangling pointer of an empty buffer.
    let mut verdict: Vec<u8> = std::vec![0];
    if let Some(peers) = h.peers.as_ref() {
        let me = h.id;
        let mine = ports(plan, me);
        let with = |p: &Port, k: u32| p.from.0 == k || p.to.0 == k;
        let mut others: Vec<u32> = mine.iter().flat_map(|p| [p.from.0, p.to.0]).filter(|&k| k != me).collect();
        others.sort_unstable();
        others.dedup();
        let outgoing: Vec<Vec<u8>> = others
            .iter()
            .map(|&k| encode(fine, &mine.iter().filter(|p| with(p, k)).cloned().collect::<Vec<_>>()))
            .collect();
        let mut sends: Vec<ffi::MPI_Request> = std::vec![null(); others.len()];
        for (i, &k) in others.iter().enumerate() {
            isend(outgoing[i].as_ptr(), outgoing[i].len(), k as i32, CONNECT_TAG, peers.as_raw(), &mut sends[i]);
        }
        for &k in &others {
            let (msg, status) = peers.process_at_rank(k as i32).matched_probe_with_tag(CONNECT_TAG);
            let mut b = std::vec![0u8; status.count(u8::equivalent_datatype()) as usize];
            msg.matched_receive_into(&mut b[..]);
            if verdict.len() > 1 {
                continue;
            }
            let (their_ok, theirs) = decode(&b);
            let ours: Vec<&Port> = mine.iter().filter(|p| with(p, k)).collect();
            let why = if !their_ok {
                Some(format!("host {k}, which shares routes with this host, refused its launch"))
            } else if let Some(p) = ours.iter().find(|p| !theirs.contains(p)) {
                let same = |q: &&Port| (q.from, q.to, q.wire) == (p.from, p.to, p.wire);
                Some(match theirs.iter().find(same) {
                    Some(q) => format!(
                        "this host declares the route of {}, host {k} {}; declare it alike on both",
                        describe(p),
                        kind(q)
                    ),
                    None => format!("host {k} does not declare the route of {}; declare it there too", describe(p)),
                })
            } else {
                theirs
                    .iter()
                    .find(|p| !ours.contains(p))
                    .map(|p| format!("host {k} declares the route of {}, which this host does not", describe(p)))
            };
            if let Some(why) = why {
                verdict.extend_from_slice(why.as_bytes());
            }
        }
        for r in &mut sends {
            wait(r);
        }
    }
    let root = h.comm.process_at_rank(h.comm.size() - 1);
    let mut n = verdict.len() as u64;
    root.broadcast_into(&mut n);
    verdict.resize(n as usize, 0);
    root.broadcast_into(&mut verdict[..]);
    if verdict.len() == 1 {
        Ok(())
    } else {
        Err(Refused::Backend { reason: String::from_utf8_lossy(&verdict[1..]).into_owned() })
    }
}
