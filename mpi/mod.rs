// Lanes as ordinary tagged frames.
//
// Two-sided MPI supplies matching and ordering, so this backend opens no window and keeps no
// out-of-band state: a lane is a frame under the tag `reshape` named, and the `Lane` route differs
// from `Message` only in who chose the tag.

use crate::contract::{Backend, Channel, Edge, Error, Frame, Participant, Rank, Tag};
use crate::shared::context::lane_tag;
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

pub const ID: Backend = Backend::Mpi;

/// Send one frame. A lane is an ordinary frame under the load's tag.
pub fn send(
    cx: &mut Context,
    to: Rank,
    channel: Channel,
    data: &[u8],
) -> Result<(), Error> {
    let tag = match channel {
        Channel::Message(tag) => tag,
        Channel::Lane => cx.lane_tag()?,
    };
    crate::shared::p2p::send(cx, to, tag, data)
}

/// Every frame here is on the wire, so there is one route to receive from.
impl Io<'_> {
    /// Lanes are frames under the lane tag, like any other message.
    pub fn send(&mut self, to: Rank, channel: Channel, data: &[u8]) -> Result<(), Error> {
        let tag = match channel {
            Channel::Message(tag) => tag,
            Channel::Lane => lane_tag(self.workers, self.lane)?,
        };
        send_on(self.world, Participant::Worker(self.rank), to, tag, data)
    }
}

pub fn recv(cx: &mut Context, out: &mut [u8]) -> Result<Option<Frame>, Error> {
    crate::shared::p2p::recv(cx, out)
}

pub fn flush(cx: &mut Context) -> Result<(), Error> {
    crate::shared::p2p::flush(cx)
}

/// Take the lane table. No window is allocated: nothing has to be built for a lane to exist.
pub fn reshape(
    cx: &mut Context,
    workers: &[Rank],
    edges: &[Edge],
    bytes: usize,
    tag: Tag,
) -> Result<(), Error> {
    crate::cpu::lanes::validate(workers, edges, bytes, size(cx), MAX_FRAME)?;
    cx.set_lane(tag, workers.to_vec());
    Ok(())
}

pub fn release(cx: &mut Context) -> Result<(), Error> {
    cx.clear_lane();
    Ok(())
}
