// The hosts of an MPI world (CONTRACT.md, Federation). A host is a shared-memory node: the ranks of
// `MPI_Comm_split_type(SHARED)`, cut into groups of TRAME_MPI_NODE_RANKS consecutive ranks for tests on
// one machine (nodes.rs). Hosts are numbered 0..count in order of their lowest world rank (F1). Each
// host launches on its own communicator, as a one-node backend: its own window, segment, leader (its
// last rank) and workers. A route between workers of two hosts goes rank to rank on `cross`, a
// duplicate of the world (transport/cross/). The leaders of the hosts form a communicator of their own, the
// peer channel (F6), ranked by host.
//
// The table is made at the first call, collectively over the world: every rank of the world calls
// `host` before its launch (backend.rs `agree`), so every rank reaches the splits.

extern crate std;

use std::boxed::Box;
use std::vec::Vec;

use mpi::ffi;
use mpi::point_to_point::Source;
use mpi::topology::{Color, Communicator, SimpleCommunicator};
use mpi::traits::*;

use crate::backends::mpi::memory::shm::Window;
use crate::backends::mpi::setup::state::{local, world};
use crate::backends::mpi::transport::nodes::{discover, node_ranks_setting, split_groups};

pub(crate) struct Host {
    /// This host's communicator: its launch's world.
    pub comm: SimpleCommunicator,
    /// This host's number, and the number of hosts.
    pub id: u32,
    pub count: u32,
    /// The leaders' communicator, rank = host, on the leader of each host only.
    pub peers: Option<SimpleCommunicator>,
    /// Peer messages received but not yet taken: (host, tag, bytes).
    pub pending: Vec<(u32, u64, Vec<u8>)>,
    /// Peer messages received and sent so far: for the leader's `wait` (leader.rs), and as messages
    /// in flight for the detector (detect.rs).
    pub arrivals: u64,
    pub sent: u64,
    /// The leader's deadlock detector, from its launch on (detect.rs).
    pub watch: Option<crate::backends::mpi::roles::detect::Watch>,
    /// Per host, its world ranks: worker k of host h is `members[h][k]`, its leader the last.
    pub members: Vec<Vec<i32>>,
    /// The world, duplicated for the frames and credits of routes between hosts (transport/cross/).
    pub cross: SimpleCommunicator,
    /// The host's gate: a window of one futex word that counts the `share`s the leader has reached, so the other
    /// processes of the host sleep until the leader comes instead of spinning in a collective.
    pub gate: Window,
    /// The `share`s this process has made.
    pub shares: u32,
    /// Peer messages posted and not yet complete: the request and the buffer it reads (`send`).
    pub outbox: Vec<(ffi::MPI_Request, Box<[u8]>)>,
}

/// One peer message tag for every peer message: the F4 tag travels in the first 8 bytes.
const PEER_TAG: i32 = 7;

/// This process's host, made at the first call (collective over the world, see the module comment).
pub(crate) fn host() -> &'static mut Host {
    if local().host.is_none() {
        let w = world();
        let setting = node_ranks_setting();
        let shared = w.split_shared(w.rank());
        let comm = match setting {
            Ok(Some(k)) => {
                if let Some(n) = k.ranks() {
                    assert_eq!(n, w.size(), "TRAME_MPI_NODE_RANKS names {n} ranks and the launch has {}", w.size());
                }
                let g = split_groups(w, &shared, &k).expect("a rank's own colour is never undefined");
                std::mem::forget(shared);
                g
            }
            _ => shared,
        };
        let nodes = discover(w, &comm);
        let leader = comm.rank() == comm.size() - 1;
        let colour = if leader { Color::with_value(0) } else { Color::undefined() };
        let peers = w.split_by_color_with_key(colour, nodes.me as i32);
        let cross = w.duplicate();
        // One word on the host's shared memory, made now, collectively, with the host's table.
        let gate = Window::allocate(&comm, 64, "gate".into()).expect("trame: the host's gate could not be allocated");
        local().host = Some(Host {
            comm,
            id: nodes.me,
            count: nodes.count,
            peers,
            pending: Vec::new(),
            arrivals: 0,
            sent: 0,
            watch: None,
            members: nodes.members,
            cross,
            gate,
            shares: 0,
            outbox: Vec::new(),
        });
    }
    local().host.as_mut().expect("the host was just set")
}

/// Send a peer message (F6): posted without waiting for the receiver (MPI_Isend), so two leaders that send each
/// other a message of any size at the same time do not deadlock, and `Full` is never answered. The message is kept
/// until MPI completes it: `drain` and `flush` test the posted ones.
pub(crate) fn send(to: u32, tag: u64, bytes: &[u8]) {
    let h = host();
    assert!(to != h.id && to < h.count, "trame: host {to} is not a peer of host {}", h.id);
    let peers = h.peers.as_ref().expect("trame: only a leader has peers");
    let mut msg = Vec::with_capacity(8 + bytes.len());
    msg.extend_from_slice(&tag.to_le_bytes());
    msg.extend_from_slice(bytes);
    let msg = msg.into_boxed_slice();
    let count = i32::try_from(msg.len()).expect("trame: a peer message is under 2 GiB");
    let mut request = unsafe { ffi::RSMPI_REQUEST_NULL };
    // SAFETY: `msg` is kept in `outbox` (a heap buffer that does not move) until the request completes; the
    // communicator is the leaders' and the call does not wait for the receiver.
    let rc = unsafe {
        ffi::MPI_Isend(
            msg.as_ptr() as *const _,
            count,
            ffi::RSMPI_UINT8_T,
            to as i32,
            PEER_TAG,
            peers.as_raw(),
            &mut request,
        )
    };
    assert_eq!(rc, ffi::MPI_SUCCESS as i32, "trame: MPI_Isend of a peer message failed");
    h.outbox.push((request, msg));
    h.sent += 1;
}

/// Forget the posted messages MPI has completed. Answers whether any is still posted.
fn settle(h: &mut Host) -> bool {
    h.outbox.retain_mut(|(request, _)| {
        let mut done = 0;
        // SAFETY: `request` was posted by `send` and is not yet complete; MPI sets it null when it is.
        let rc = unsafe { ffi::MPI_Test(request, &mut done, ffi::RSMPI_STATUS_IGNORE) };
        assert_eq!(rc, ffi::MPI_SUCCESS as i32, "trame: MPI_Test of a peer message failed");
        done == 0
    });
    !h.outbox.is_empty()
}

/// Wait until every message this leader sent has gone, receiving what arrives meanwhile (two leaders that flush
/// at once must not wait on each other). The end of a run calls it, so that no message is left posted.
pub(crate) fn flush() {
    while local().host.is_some() && settle(host()) {
        drain();
        core::hint::spin_loop();
    }
}

/// Move every peer message MPI holds for this leader into `pending`. Answers whether one came.
pub(crate) fn drain() -> bool {
    let h = host();
    settle(h);
    let Some(peers) = h.peers.as_ref() else { return false };
    let mut any = false;
    while let Some((msg, status)) = peers.any_process().immediate_matched_probe_with_tag(PEER_TAG) {
        keep(&mut h.pending, &mut h.arrivals, msg, status);
        any = true;
    }
    any
}

/// Wait until a peer message is held in `pending`: it returns at once when one is, else after the next arrives.
///
/// The wait is a poll of a non-blocking probe with a short sleep between polls, not a blocking probe: an MPI library
/// may spin in a blocking probe, and a leader that spins holds its core and, worse, does not return to its run loop,
/// so the workers' routes to it fill and the workers starve (a launch over two hosts stalls). A sleeping wait costs
/// the leader nothing and lets the run loop move. The fog of a sleep is 100 µs: far below any measurable round trip,
/// far above the cost of a probe.
pub(crate) fn wait() {
    use std::time::Duration;
    let h = host();
    if !h.pending.is_empty() {
        return;
    }
    let peers = h.peers.as_ref().expect("trame: only a leader has peers");
    loop {
        while let Some((msg, status)) = peers.any_process().immediate_matched_probe_with_tag(PEER_TAG) {
            keep(&mut h.pending, &mut h.arrivals, msg, status);
            return;
        }
        std::thread::sleep(Duration::from_micros(100));
    }
}

fn keep(
    pending: &mut Vec<(u32, u64, Vec<u8>)>,
    arrivals: &mut u64,
    msg: mpi::point_to_point::Message,
    status: mpi::point_to_point::Status,
) {
    let n = status.count(u8::equivalent_datatype()) as usize;
    let mut bytes = std::vec![0u8; n];
    msg.matched_receive_into(&mut bytes[..]);
    let tag = u64::from_le_bytes(bytes[..8].try_into().expect("a peer message starts with its tag"));
    pending.push((status.source_rank() as u32, tag, bytes[8..].to_vec()));
    *arrivals += 1;
}

/// Take the oldest pending peer message of tag `tag`, whatever its length; answers its host and bytes.
pub(crate) fn recv_bytes(tag: u64) -> Option<(u32, Vec<u8>)> {
    drain();
    let h = host();
    let i = h.pending.iter().position(|(_, t, _)| *t == tag)?;
    let (from, _, bytes) = h.pending.remove(i);
    Some((from, bytes))
}

/// Take the oldest pending peer message of tag `tag` into `out`; answers its host.
pub(crate) fn recv(tag: u64, out: &mut [u8]) -> Option<u32> {
    let (from, bytes) = recv_bytes(tag)?;
    assert_eq!(bytes.len(), out.len(), "trame: a peer message of this tag has another size");
    out.copy_from_slice(&bytes);
    Some(from)
}

/// `share` (backend.rs): the leader's bytes to every other process of the host. The others sleep on the gate's
/// futex until the leader reaches this call, then every process makes the broadcast, which is then short. Sleeping
/// here is for an event, the leader's arrival; a collective alone would spin in the MPI library.
pub(crate) fn share(bytes: &[u8]) -> Vec<u8> {
    use mpi::collective::Root;
    let h = host();
    h.shares = h.shares.wrapping_add(1);
    let n = h.shares;
    let leader = h.comm.rank() == h.comm.size() - 1;
    if leader {
        h.gate.word32(0).store(n, std::sync::atomic::Ordering::SeqCst);
        h.gate.wake(0);
    } else {
        loop {
            let seen = h.gate.word32(0).load(std::sync::atomic::Ordering::SeqCst);
            if seen == n || (seen.wrapping_sub(n) as i32) > 0 {
                break;
            }
            h.gate.wait_for(0, seen, None);
        }
    }
    let root = h.comm.process_at_rank(h.comm.size() - 1);
    let mut len = bytes.len() as u64;
    root.broadcast_into(&mut len);
    let mut out = if leader { bytes.to_vec() } else { std::vec![0u8; len as usize] };
    root.broadcast_into(&mut out[..]);
    out
}
