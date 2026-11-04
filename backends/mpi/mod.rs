//! The MPI backend of trame, on one node (spec.md 6). `Selected` (imp.rs) is the backend the surface
//! calls; the transport below it is what the test binaries use.
//!
//! `open` (open.rs) gives this rank a `Role` (spec.md 4): ranks `0..workers` get a worker endpoint,
//! and the last rank, `workers`, gets the leader endpoint. Both are `Transport`s: the leader is an
//! endpoint like any other, with its own routes. `agree` checks that every rank built the same
//! topology, and it runs before `open`, so a refusal is decided on every rank together.
//!
//! Mechanism, in short (the reasoning is in README.md and the files named in it):
//! - Every route is a ring of `cap` slots in one `MPI_Win_allocate_shared` window. The sender
//!   owns the counter P and the receiver owns C, and each is published with a release store.
//! - A lossless send answers `Full` only when an acquire load of C shows `P - C == cap`. That load
//!   is the linearization point of the answer.
//! - A lossy send never reads C. A slot is a seqlock, so a receiver that reads a slot the sender
//!   is overwriting detects it and skips the frame.
//! - Results and aborts are futex words (results.rs). Agents sleep on their worker's epoch (epoch.rs), and
//!   every act that can change what an agent waits for bumps it. Every epoch and futex word is 32 bits. No
//!   MPI call after launch.

mod epoch;
mod futex;
mod harness;
mod imp;
mod layout;
mod leader;
mod nodes;
mod object;
mod open;
mod plan;
mod poll;
mod recv;
mod refusal;
mod hosts;
mod relay;
mod relay_control;
mod relay_in;
mod relay_out;
mod relay_routes;
mod results;
mod ring;
mod segment;
mod shape;
mod shm;
mod state;
mod types;
mod wire;
mod worker;

/// The backend the surface calls: `impl Backend for Selected` lives in imp.rs.
pub struct Selected;

extern crate std;

use std::vec;
use std::vec::Vec;

pub use layout::{Route, Topology};
pub use open::{agree, launch, open};
pub use refusal::Refusal;
pub use shape::encode;
pub use state::finalize;

pub use harness::{leader_join, returned};
pub use types::{Answer, Error, Event, Received, Role, RouteId, clock};

use layout::{Layout, RouteLayout};
use nodes::Nodes;
use shm::Window;
use std::sync::Arc;

/// One endpoint's transport: a worker's, or the leader's. Not `Send`: it holds a pointer into the
/// window, and it belongs to the rank's thread.
pub struct Transport {
    me: u32,
    win: Window,
    lay: Layout,
    /// The node table: which node each rank is on (nodes.rs).
    nodes: Arc<Nodes>,
    /// Lossy routes: the next index this endpoint will read. Local, because the receiver of a
    /// lossy route never publishes progress.
    cursor: Vec<u64>,
    /// Per owned route set, where `recv` starts its scan (recv.rs). Indexed by the set's first route.
    rr: Vec<usize>,
    trace: Option<Vec<Event>>,
}

impl Drop for Transport {
    /// A leader dropped by a panic ends the run (leader.rs). A worker needs nothing: its result is its exit.
    fn drop(&mut self) {
        if self.me >= self.lay.workers && std::thread::panicking() {
            self.leader_panicked();
        }
    }
}

impl Transport {
    pub(crate) fn new(me: u32, win: Window, lay: Layout, nodes: Arc<Nodes>) -> Transport {
        let routes = lay.routes.len();
        Transport {
            me,
            win,
            lay,
            nodes,
            cursor: vec![0; routes],
            rr: vec![0; routes],
            trace: None,
        }
    }

    /// The node table of this launch.
    pub(crate) fn nodes(&self) -> &Nodes {
        &self.nodes
    }

    /// This endpoint's rank: a worker's index, or `workers` for the leader.
    pub fn me(&self) -> u32 {
        self.me
    }

    /// Start recording every send and consume. Costs two clock reads per send.
    pub fn trace_on(&mut self) {
        self.trace = Some(Vec::new());
    }

    /// Take the recorded events. Empty if tracing is off.
    pub fn take_trace(&mut self) -> Vec<Event> {
        self.trace.as_mut().map(std::mem::take).unwrap_or_default()
    }

    pub(crate) fn route(&self, id: RouteId) -> Result<&RouteLayout, Error> {
        self.lay.routes.get(id.0 as usize).ok_or(Error::NoSuchRoute(id))
    }

    pub(crate) fn push(&mut self, e: Event) {
        if let Some(t) = self.trace.as_mut() {
            t.push(e);
        }
    }

    pub(crate) fn lay(&self) -> &Layout {
        &self.lay
    }

    pub(crate) fn win(&self) -> &Window {
        &self.win
    }

    pub(crate) fn cursor_mut(&mut self) -> &mut Vec<u64> {
        &mut self.cursor
    }
}
