// The leaders' deadlock detector (spec.md 5, Deadlock; CONTRACT.md F5). A worker is stuck when no
// step of it runs, its agents idle with no deadline, and nothing can move its epoch. Nothing can when
// every worker of every host is stuck or has a result, every leader waits, joins or has ended, and no
// message is in flight between hosts. The detector shows that state once it holds, and never before.
//
// Rounds. Each leader takes part in rounds while it waits (`Run::wait`), joins, or has ended
// (`quiesce`): never from its own code, which may still act. A round is one nonblocking allreduce
// of four sums over the leaders: the leaders whose snapshot is unstable, the messages sent between
// hosts, the messages arrived, and the leaders that have not ended. A snapshot is this host's words:
// per worker OUTCOME, RUN, ACTS, IDLE and EPOCH, the leader's epoch, the count words of every route
// between hosts (cross.rs, Counts), and the peer messages sent and received (hosts.rs). It is stable
// when every worker is stuck or has a result and the snapshot equals the one this leader gave the
// round before.
//
// Why a verdict is sound. Let round k + 1 give 0 unstable leaders and as many messages sent as
// arrived. Every leader read its round k snapshot before it posted round k, and its round k + 1
// snapshot after round k completed there, which is after every leader posted round k. So at T, the
// latest round k read, every leader is between its two reads. ACTS and EPOCH only grow (EPOCH up to
// its wrap, epoch.rs), and they are equal at both reads with RUN 0: no step ran on any worker between
// the reads, and no act moved any epoch. The counts only grow, and equal at both reads: at T the sums
// of sent and arrived are those of round k + 1, equal, so no message is in flight. At T nothing runs,
// nothing is in flight and nothing can wake: that is stuck, and it stays so. A stable leader also
// shows no act of its own, since every leader act moves a worker's epoch or a count.
//
// Not shown. A leader that never waits, joins or ends (a loop of its own) holds every round, so a
// deadlock elsewhere is not shown while it runs; nor is one while a leader edits. The detector polls:
// a waiting or joining leader wakes after a pause that doubles from PAUSE_MIN to PAUSE_MAX.

extern crate std;

use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering::SeqCst;
use std::boxed::Box;
use std::vec::Vec;

use mpi::ffi;
use mpi::traits::AsRaw;

use super::Transport;
use super::hosts::{self, host};
use super::layout::{DEADLOCK, K_ACTS, K_EPOCH, K_IDLE, K_OUTCOME, K_RUN, LEADER_EPOCH, LINE};

/// The pause of a waiting or joining leader between rounds, in nanoseconds.
const PAUSE_MIN: u64 = 20_000;
const PAUSE_MAX: u64 = 20_000_000;

/// What a round showed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Verdict {
    /// No verdict yet: something may still move, or the round is still in progress.
    Live,
    /// Every live leader's workers are stuck (the module comment).
    Stuck,
    /// Every leader has ended: the rounds are over.
    Over,
}

/// One leader's detector, in its host (hosts.rs).
pub(super) struct Watch {
    /// Addresses of the words of the snapshot: per worker the five K_ words, then the leader's epoch.
    words: Vec<usize>,
    /// Addresses of the count words of the routes between hosts: P (sent), then C (arrived).
    counts: Vec<(usize, usize)>,
    workers: usize,
    /// The snapshot given to the last round; empty before the first.
    last: Vec<u64>,
    round: Option<Round>,
    pause: u64,
}

/// A posted round: its buffers stay put until it completes.
struct Round {
    send: Box<[u64; 4]>,
    recv: Box<[u64; 4]>,
    req: ffi::MPI_Request,
}

/// At launch, on the leader: watch the words of `t`'s window.
pub(super) fn watch(t: &Transport) {
    let lay = t.lay();
    let at = |off: usize| t.win().ptr(off, 8) as usize;
    let mut words = Vec::new();
    for w in 0..lay.workers {
        for k in [K_OUTCOME, K_RUN, K_ACTS, K_IDLE, K_EPOCH] {
            words.push(at(lay.per_worker(k, w)));
        }
    }
    words.push(at(LEADER_EPOCH));
    let counts = lay.routes.iter().filter(|r| r.remote.is_some()).map(|r| (at(r.base), at(r.base + LINE))).collect();
    let workers = lay.workers as usize;
    host().watch = Some(Watch { words, counts, workers, last: Vec::new(), round: None, pause: PAUSE_MIN });
}

fn read(addr: usize) -> u64 {
    // SAFETY: an aligned word of the window, which stays mapped until MPI_Finalize (shm.rs), and every
    // round ends before it (`quiesce`).
    unsafe { &*(addr as *const AtomicU64) }.load(SeqCst)
}

impl Watch {
    /// This leader's part of a round: [unstable, sent, arrived, live].
    fn part(&mut self, ended: bool, peer_sent: u64, peer_got: u64) -> [u64; 4] {
        let snap: Vec<u64> = self.words.iter().map(|&a| read(a)).collect();
        // EPOCH is a u32 in its word; IDLE is EPOCH + 1 as the worker read it (worker.rs `mark_quiet`).
        let stuck = snap[..5 * self.workers].chunks(5).all(|w| w[0] != 0 || (w[1] == 0 && w[3] == (w[4] & 0xffff_ffff) + 1));
        let (mut sent, mut got) = (peer_sent, peer_got);
        for &(p, c) in &self.counts {
            sent += read(p);
            got += read(c);
        }
        let mut full = snap;
        full.extend([sent, got]);
        let stable = stuck && full == self.last;
        self.last = full;
        [u64::from(!stable), sent, got, u64::from(!ended)]
    }
}

fn verdict(r: &[u64; 4]) -> Verdict {
    if r[3] == 0 {
        Verdict::Over
    } else if r[0] == 0 && r[1] == r[2] {
        Verdict::Stuck
    } else {
        Verdict::Live
    }
}

/// Progress this leader's rounds once, without blocking. `ended`: this host's leader has ended. A
/// host whose leader never launched (a refusal) takes part with no workers.
pub(super) fn round(ended: bool) -> Verdict {
    hosts::drain();
    let h = host();
    let (peer_sent, peer_got, count) = (h.sent, h.arrivals, h.count);
    let w = h.watch.get_or_insert_with(|| Watch {
        words: Vec::new(),
        counts: Vec::new(),
        workers: 0,
        last: Vec::new(),
        round: None,
        pause: PAUSE_MIN,
    });
    if count == 1 {
        return verdict(&w.part(ended, peer_sent, peer_got));
    }
    if w.round.is_none() {
        let send = Box::new(w.part(ended, peer_sent, peer_got));
        let mut r = Round { send, recv: Box::new([0; 4]), req: null() };
        let peers = h.peers.as_ref().expect("trame: only a leader runs the detector").as_raw();
        // SAFETY: both buffers are boxed in the round, which is kept until the request completes;
        // `peers` holds every leader, and each takes part in every round in order (Foreign calls).
        let rc = unsafe {
            ffi::MPI_Iallreduce(
                r.send.as_ptr().cast(),
                r.recv.as_mut_ptr().cast(),
                4,
                ffi::RSMPI_UINT64_T,
                ffi::RSMPI_SUM,
                peers,
                &mut r.req,
            )
        };
        assert_eq!(rc, ffi::MPI_SUCCESS as i32, "trame: MPI_Iallreduce failed in the deadlock detector");
        w.round = Some(r);
    }
    let r = w.round.as_mut().expect("just posted");
    let mut flag = 0;
    // SAFETY: a request of this process (Foreign calls).
    let rc = unsafe { ffi::MPI_Test(&mut r.req, &mut flag, ffi::RSMPI_STATUS_IGNORE) };
    assert_eq!(rc, ffi::MPI_SUCCESS as i32, "trame: MPI_Test failed in the deadlock detector");
    if flag == 0 {
        return Verdict::Live;
    }
    let done = w.round.take().expect("the round just completed");
    verdict(&done.recv)
}

/// The pause before the next round, doubled each time up to PAUSE_MAX (the module comment).
pub(super) fn pause() -> u64 {
    let w = host().watch.as_mut().expect("trame: the leader watches from its launch on");
    let p = w.pause;
    w.pause = (p * 2).min(PAUSE_MAX);
    p
}

/// The leader joins and its workers are stuck: each ends with Deadlock at its next round (worker.rs).
pub(super) fn end_stuck(t: &Transport) {
    t.win().word(DEADLOCK).store(1, SeqCst);
    t.bump_all();
}

fn null() -> ffi::MPI_Request {
    // SAFETY: a constant of the MPI library (PROOFS.md, Assumptions: Foreign calls).
    unsafe { ffi::RSMPI_REQUEST_NULL }
}
