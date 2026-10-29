// The backend's view of a plan: its exact encoding (what ranks agree on), the transport's topology,
// and the route table each endpoint looks its routes up in. Ranks are workers `0..W` and the leader
// `W`; `LEADER` in the surface is the leader's rank.

extern crate std;

use core::any::TypeId;
use core::hash::{Hash, Hasher};
use std::vec;
use std::vec::Vec;

use crate::launch::{Plan, Refused};
use crate::topology::{LEADER, WorkerId};

use super::{Route, RouteId, Topology};

/// A hasher that keeps every byte it is given. A `TypeId` hashes its own bytes, so this encodes the
/// id exactly, with no collision to let two different types look equal.
struct Capture(Vec<u8>);

impl Hasher for Capture {
    fn write(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }
    fn finish(&self) -> u64 {
        0
    }
}

fn type_bytes(ty: TypeId) -> Vec<u8> {
    let mut c = Capture(Vec::new());
    ty.hash(&mut c);
    c.0
}

/// The rank of an agent: a worker's index, or `workers` for the leader.
pub(super) fn rank_of(id: WorkerId, workers: u32) -> u32 {
    if id == LEADER { workers } else { id.0 }
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// The plan as bytes, exactly: equal bytes mean equal plans. Ranks compare these (`agree`).
///
/// Worker bodies are not in it. A function pointer differs between processes (address space layout
/// randomization), so the plan gives only the worker count. Every rank runs the same binary, so
/// worker `i` is the same code on each rank.
pub(super) fn encode_plan(plan: &Plan) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(plan.workers.len() as u64).to_le_bytes());
    put_bytes(&mut out, &plan.publication);
    out.extend_from_slice(&(plan.routes.len() as u64).to_le_bytes());
    for r in &plan.routes {
        out.extend_from_slice(&r.from.0.to_le_bytes());
        out.extend_from_slice(&r.to.0.to_le_bytes());
        put_bytes(&mut out, &type_bytes(r.ty));
        put_bytes(&mut out, r.name.as_bytes());
        out.extend_from_slice(&(r.size as u64).to_le_bytes());
        out.extend_from_slice(&(r.cap as u64).to_le_bytes());
        out.push(u8::from(r.lossy));
    }
    out
}

/// The transport's topology for a checked plan. Each distinct message type is a tag, numbered in the
/// order the plan first uses it, so every rank numbers them the same.
pub(super) fn topology(plan: &Plan) -> Result<Topology, Refused> {
    let workers = u32::try_from(plan.workers.len()).map_err(|_| Refused::Backend { reason: "too many workers" })?;
    let mut tags: Vec<TypeId> = Vec::new();
    let mut routes = Vec::with_capacity(plan.routes.len());
    for r in &plan.routes {
        let tag = match tags.iter().position(|t| *t == r.ty) {
            Some(i) => i,
            None => {
                tags.push(r.ty);
                tags.len() - 1
            }
        };
        let cap = u32::try_from(r.cap)
            .map_err(|_| Refused::Backend { reason: "a route's capacity does not fit in 32 bits" })?;
        routes.push(Route {
            from: rank_of(r.from, workers),
            to: rank_of(r.to, workers),
            tag: tag as u64,
            size: r.size,
            cap,
            lossy: r.lossy,
        });
    }
    Ok(Topology { workers, routes, publication: plan.publication.clone() })
}

/// One route as an endpoint looks it up: sender and receiver ranks, its message type, and its id.
pub(super) struct Entry {
    pub from: u32,
    pub to: u32,
    pub ty: TypeId,
    pub id: RouteId,
}

pub(super) fn entries(plan: &Plan) -> Vec<Entry> {
    let workers = plan.workers.len() as u32;
    plan.routes
        .iter()
        .enumerate()
        .map(|(i, r)| Entry {
            from: rank_of(r.from, workers),
            to: rank_of(r.to, workers),
            ty: r.ty,
            id: RouteId(i as u32),
        })
        .collect()
}

/// The routes that rank `me` receives, grouped by message type. The scan of each group keeps its
/// own rotation (see recv.rs), so the group of one type never shares a rotation with another.
pub(super) fn inbox(entries: &[Entry], me: u32) -> Vec<(TypeId, Vec<RouteId>)> {
    let mut groups: Vec<(TypeId, Vec<RouteId>)> = Vec::new();
    for e in entries.iter().filter(|e| e.to == me) {
        match groups.iter_mut().find(|(t, _)| *t == e.ty) {
            Some((_, ids)) => ids.push(e.id),
            None => groups.push((e.ty, vec![e.id])),
        }
    }
    groups
}

/// The receive set of type `ty` in an inbox, or none.
pub(super) fn owned(inbox: &[(TypeId, Vec<RouteId>)], ty: TypeId) -> &[RouteId] {
    inbox.iter().find(|(t, _)| *t == ty).map_or(&[], |(_, ids)| ids.as_slice())
}

/// The routes rank `from` sends, in plan order. The position of a route in this sequence is the index
/// `Backend::worker_route` answers, so the sequential backend and this one number them alike.
pub(super) fn outgoing(entries: &[Entry], from: u32) -> impl Iterator<Item = &Entry> {
    entries.iter().filter(move |e| e.from == from)
}

/// The route from `from` to `to` with message type `ty`, if the plan has one.
pub(super) fn find(entries: &[Entry], from: u32, to: u32, ty: TypeId) -> Option<RouteId> {
    entries.iter().find(|e| e.from == from && e.to == to && e.ty == ty).map(|e| e.id)
}
