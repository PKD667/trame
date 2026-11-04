// The launch checks of the transport, the layout they yield, and the exact encoding that ranks compare.
// Pure: the same topology gives the same answer on every rank, so no rank ever needs another's.

extern crate std;

use std::collections::BTreeSet;
use std::format;
use std::string::String;
use std::vec::Vec;

use super::layout::{PER_WORKER, LINE, Layout, RouteLayout, SLOT_HEAD, Topology, WORKER_LINES};
use super::refusal::Refusal;

fn refuse(why: impl Into<String>) -> Refusal {
    Refusal::Topology(why.into())
}

/// Check the topology and compute its layout.
pub(crate) fn plan(topo: &Topology) -> Result<Layout, Refusal> {
    if topo.workers == 0 {
        return Err(refuse("no workers"));
    }
    let mut seen = BTreeSet::new();
    let header = PER_WORKER + WORKER_LINES * LINE * topo.workers as usize;
    let mut at = header;
    let mut routes = Vec::with_capacity(topo.routes.len());
    for (rank, r) in topo.routes.iter().enumerate() {
        // The end on this host must be a worker of the launch; on another host, a worker there (cross.rs).
        let here = match r.remote {
            None => r.from <= topo.workers && r.to <= topo.workers,
            Some(x) => (if x.outgoing { r.from } else { r.to }) < topo.workers,
        };
        if !here {
            return Err(refuse(format!("route {rank} names an agent outside the launch")));
        }
        if !seen.insert((r.from, r.to, r.tag, r.remote.map(|x| (x.host, x.outgoing)))) {
            return Err(refuse(format!("two routes with sender {}, receiver {}, tag {}", r.from, r.to, r.tag)));
        }
        if r.cap == 0 {
            return Err(refuse(format!("route {rank} has capacity 0")));
        }
        let stride = r
            .size
            .div_ceil(8)
            .checked_mul(8)
            .and_then(|b| b.checked_add(SLOT_HEAD))
            .ok_or_else(|| refuse("slot size overflows"))?;
        // A route to another host has no ring here: its frames land in the receiver's buffers (cross.rs).
        let slots = if r.remote.is_some() { 0 } else { r.cap as usize };
        let body = slots.checked_mul(stride).ok_or_else(|| refuse("route size overflows"))?;
        let base = at;
        at = base
            .checked_add(2 * LINE)
            .and_then(|b| b.checked_add(body))
            .ok_or_else(|| refuse("window size overflows"))?;
        routes.push(RouteLayout {
            from: r.from,
            to: r.to,
            size: r.size,
            cap: u64::from(r.cap),
            lossy: r.lossy,
            base,
            stride,
            remote: r.remote,
        });
    }
    Ok(Layout { workers: topo.workers, routes, total: at })
}

/// The topology as bytes, exactly: two topologies give the same bytes only if they are equal. Ranks
/// compare these bytes (`agree` in open.rs), so no hash can let inconsistent ranks through.
pub fn encode(topo: &Topology) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&topo.workers.to_le_bytes());
    out.extend_from_slice(&(topo.routes.len() as u64).to_le_bytes());
    for r in &topo.routes {
        out.extend_from_slice(&r.from.to_le_bytes());
        out.extend_from_slice(&r.to.to_le_bytes());
        out.extend_from_slice(&r.tag.to_le_bytes());
        out.extend_from_slice(&(r.size as u64).to_le_bytes());
        out.extend_from_slice(&r.cap.to_le_bytes());
        out.push(u8::from(r.lossy));
        match r.remote {
            None => out.push(0),
            Some(x) => {
                out.push(1 + u8::from(x.outgoing));
                out.extend_from_slice(&x.host.to_le_bytes());
                out.extend_from_slice(&x.world.to_le_bytes());
                out.extend_from_slice(&x.lane.to_le_bytes());
            }
        }
    }
    out
}
