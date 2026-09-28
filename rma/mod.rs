// Acknowledged remote write lanes.
//
// A lane lands in the receiver's own window, so nothing is lost and the sender is held to `depth`
// slots ahead of its receiver. The acknowledgement that makes the route reliable is also what
// makes a send refuse with `Full`, which is a fact about the route rather than a tuning choice.

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

pub const ID: Backend = Backend::Rma;

// The window's lane table, which needs no communicator.
#[cfg(test)]
mod tests;

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
            refused(ring.send(index, data), cx.rank())
        }
    }
}

impl Io<'_> {
    pub fn send(&mut self, to: Rank, channel: Channel, data: &[u8]) -> Result<(), Error> {
        match channel {
            Channel::Message(tag) => send_on(self.world, Participant::Worker(self.rank), to, tag, data),
            Channel::Lane => {
                let index = lane_index(self.workers, to)?;
                let ring = self.ring.ok_or(Error::Invalid(Invalid::LaneNotConfigured))?;
                refused(ring.send(index, data), self.rank)
            }
        }
    }
}

/// A full safe lane is `Full`, the caller's to retry. Any other refusal fails the lane.
fn refused(sent: Result<u64, mpi_rma::Error>, rank: Rank) -> Result<(), Error> {
    sent.map(|_| ()).map_err(|e| match e {
        mpi_rma::Error::Full => Error::Full,
        _ => failure(rank, "lane"),
    })
}

/// The next frame from either route. One implementation, in `shared`, because the two ring
/// transports differ in what they overwrite and not in how they receive — and the copy that was
/// here had drifted out of step with the contract while the other one had too.
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
    let ring = Ring::safe(cx.together(), &lanes).map_err(|_| cx.failure("reshape"))?;
    cx.set_window(tag, workers.to_vec(), Arc::new(ring));
    Ok(())
}

/// Drop the window. Its drop is the collective free, and the barrier that follows it.
pub fn release(cx: &mut Context) -> Result<(), Error> {
    cx.clear_lane();
    Ok(())
}
