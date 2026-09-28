// Unacknowledged remote write lanes.
//
// A lane lands in the receiver's own window and an unread slot is overwritten once the depth is
// exhausted, so the sender never waits on a slow peer and a frame can go missing. That is the
// whole of what `LOSSY` permits, and it is why message accounting cannot balance here: a
// dropped frame counts as sent and not as received.

use std::sync::Arc;

use mpi_rma::Ring;

use crate::contract::{Backend, Channel, Invalid, Edge, Error, Frame, Participant, Rank, Tag};
use crate::shared::context::{failure, lane_index};
use crate::shared::p2p::send_on;

pub use crate::shared::context::{
    Context, Environment, Io, MAX_FRAME, barrier, concurrent_io, done, hosts, init, rank, size,
};
pub use crate::shared::leader;
pub use crate::shared::{Shared, attach, bytes, detach};

/// The host answers these by being a host: OS threads for execution, and the primitives built on
/// `std::sync`. A backend whose participants are not host threads answers none of these names.
pub use crate::cpu::clock;
pub use crate::cpu::run;
pub use crate::cpu::sync;

pub const ID: Backend = Backend::RmaLossy;

/// Send one frame: `Message` on the wire, `Lane` into the receiver's window.
pub fn send(
    cx: &mut Context,
    to: Rank,
    channel: Channel,
    data: &[u8],
) -> Result<(), Error> {
    match channel {
        Channel::Message(tag) => crate::shared::p2p::send(cx, to, tag, data),
        Channel::Lane => {
            let index = cx.lane_index(to)?;
            let ring = cx.ring().ok_or(Error::Invalid(Invalid::LaneNotConfigured))?;
            // The raw ring never waits: an unread slot is overwritten, which is what `LOSSY`
            // permits, so the route has no capacity refusal to report.
            ring.send(index, data)
                .map(|_| ())
                .map_err(|_| cx.failure("lane"))
        }
    }
}

/// The next frame from either route. One implementation, in `shared`, because the two ring
/// transports differ in what they overwrite and not in how they receive — and the copy that was
/// here had drifted out of step with the contract while the other one had too.
impl Io<'_> {
    pub fn send(&mut self, to: Rank, channel: Channel, data: &[u8]) -> Result<(), Error> {
        match channel {
            Channel::Message(tag) => send_on(self.world, Participant::Worker(self.rank), to, tag, data),
            Channel::Lane => {
                let index = lane_index(self.workers, to)?;
                let ring = self.ring.ok_or(Error::Invalid(Invalid::LaneNotConfigured))?;
                // The raw ring overwrites instead of refusing, so any refusal fails the lane.
                ring.send(index, data).map(|_| ()).map_err(|_| failure(self.rank, "lane"))
            }
        }
    }
}

pub fn recv(cx: &mut Context, out: &mut [u8]) -> Result<Option<Frame>, Error> {
    crate::shared::p2p::recv_from_either(cx, out)
}

pub fn flush(cx: &mut Context) -> Result<(), Error> {
    crate::shared::p2p::flush(cx)
}

/// Open the window the declaration asks for. Collective, and only at a load.
pub fn reshape(
    cx: &mut Context,
    workers: &[Rank],
    edges: &[Edge],
    bytes: usize,
    tag: Tag,
) -> Result<(), Error> {
    crate::cpu::lanes::validate(workers, edges, bytes, size(cx), MAX_FRAME)?;
    let lanes = crate::shared::context::window(workers, edges, bytes)?;
    let ring = Ring::raw(cx.together(), &lanes).map_err(|_| cx.failure("reshape"))?;
    cx.set_window(tag, workers.to_vec(), Arc::new(ring));
    Ok(())
}

/// Drop the window. Its drop is the collective free, and the barrier that follows it.
pub fn release(cx: &mut Context) -> Result<(), Error> {
    cx.clear_lane();
    Ok(())
}
