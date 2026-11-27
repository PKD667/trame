// F3 at launch: the hosts compare the routes they share before any lane opens, and a leader that ends answers
// the connects still waiting for it. The messages go on the peer channel (hosts.rs).

extern crate std;

use std::format;
use std::string::String;
use std::vec::Vec;

use mpi::ffi;
use mpi::traits::*;

use crate::launch::{Plan, Refused};
use crate::vocab::{End, HERE};

use crate::backends::mpi::transport::cross::calls::{isend, null, wait};
use crate::backends::mpi::transport::hosts::host;

/// On a leader that ends: answer the connect of every host still waiting for this one (F3). Such a
/// host declares routes with this one, and this one declared none back, or it would have answered in
/// its own `connect`: the answer is "no routes", and that host refuses its launch. A connect that
/// arrives before the end waits for it: a host learns of a one-sided declaration at the latest when
/// the other host ends, and no host waits for one it declares nothing with (F7). The replies stay
/// in `out` until they complete.
pub(super) fn answer_connects(out: &mut Vec<(Vec<u8>, ffi::MPI_Request)>) {
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
