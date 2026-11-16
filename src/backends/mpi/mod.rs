//! The MPI backend of trame (CONTRACT.md). `Selected` (backend.rs) is the backend the surface calls; the
//! transport below it, `Transport`, is what the test binaries use. README.md lists the directories; each has
//! its own README.md.
//!
//! `open` (setup/open.rs) gives this rank a `Role` (INTERFACE.md 4): ranks `0..workers` get a worker endpoint,
//! and the last rank, `workers`, gets the leader endpoint. Both are `Transport`s. `agree` checks that every rank
//! built the same topology, and it runs before `open`, so a refusal is decided on every rank together.
//!
//! After launch, MPI is called for routes between hosts (transport/cross/), the leaders' peer messages
//! (transport/hosts.rs) and the detector's rounds (roles/detect.rs). A route inside a host uses the window only.

mod backend;
mod harness;
mod memory;
mod rings;
mod roles;
mod setup;
mod transport;
mod types;
mod waiting;

/// The backend the surface calls: `impl Backend for Selected` lives in backend.rs.
pub struct Selected;

extern crate std;

use std::vec;
use std::vec::Vec;

pub use memory::layout::{Route, Topology};
pub use setup::open::{agree, launch, open};
pub use setup::refusal::Refusal;
pub use setup::shape::encode;
pub use setup::state::finalize;

pub use harness::{leader_join, returned};
pub use types::{Answer, Error, Event, Received, Role, RouteId, clock};

use memory::layout::{Layout, RouteLayout};
use memory::shm::Window;

/// One endpoint's transport: a worker's, or the leader's. Not `Send`: it holds a pointer into the
/// window, and it belongs to the rank's thread.
pub struct Transport {
    me: u32,
    win: Window,
    lay: Layout,
    /// Lossy routes: the next index this endpoint will read. Local, because the receiver of a
    /// lossy route never publishes progress.
    cursor: Vec<u64>,
    /// Per owned route set, where `recv` starts its scan (recv.rs). Indexed by the set's first route.
    rr: Vec<usize>,
    trace: Option<Vec<Event>>,
    /// Per route, this endpoint's lane when the route joins it to another host (transport/cross/).
    lanes: Vec<Option<transport::cross::Lane>>,
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
    pub(crate) fn new(me: u32, win: Window, lay: Layout) -> Transport {
        let routes = lay.routes.len();
        Transport { me, win, lay, cursor: vec![0; routes], rr: vec![0; routes], trace: None, lanes: Vec::new() }
    }

    /// Post this endpoint's lanes (transport/cross/). At launch, once, when a route joins hosts.
    pub(crate) fn open_lanes(&mut self) {
        if self.lay.routes.iter().any(|r| r.remote.is_some()) {
            self.lanes = transport::cross::lanes(&self.lay, self.me, &self.win);
        }
    }

    /// Whether this endpoint has a lane: then its agents poll instead of sleeping (worker.rs `idle`).
    pub(crate) fn has_lanes(&self) -> bool {
        self.lanes.iter().any(Option::is_some)
    }

    /// Progress every lane; an arrival moves this endpoint's epoch (S3).
    pub(crate) fn poll_lanes(&mut self) {
        let mut any = false;
        for l in self.lanes.iter_mut().flatten() {
            any |= l.poll();
        }
        if any {
            self.bump(self.me);
        }
    }

    /// Before MPI_Finalize (state.rs): complete this endpoint's lanes (transport/cross/ `quiesce`).
    pub(crate) fn quiesce(&mut self) {
        transport::cross::quiesce(&mut self.lanes);
    }

    fn lane(&mut self, id: RouteId) -> &mut transport::cross::Lane {
        self.lanes.get_mut(id.0 as usize).and_then(Option::as_mut).expect("trame: a route between hosts has its lane")
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
