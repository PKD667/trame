// Lanes as ordinary tagged frames.
//
// Two-sided MPI supplies matching and ordering, so this backend opens no window and keeps no
// out-of-band state: a lane is a frame under the tag `reshape` named, and the `Lane` route differs
// from `Message` only in who chose the tag.

use crate::contract::{Addr, Backend, Channel, Edge, Error, Frame, Participant, Tag};
use crate::shared::context::lane_tag;
use crate::shared::link::{self, Peer};
use crate::shared::p2p::send_on;

pub use crate::shared::context::{
    Context, Environment, Io, MAX_FRAME, barrier, concurrent_io, done, init, rank, size,
};
pub use crate::shared::leader;
pub use crate::shared::{Shared, attach, bytes, detach};

/// The host answers these by being a host: OS threads for execution, and the primitives built on
/// `std::sync`. A backend whose participants are not host threads answers none of these names.
pub use crate::cpu::clock;
pub use crate::cpu::run;
pub use crate::cpu::sync;

pub const ID: Backend = Backend::Mpi;

/// Send one frame. A lane is an ordinary frame under the load's tag: on this deployment's
/// communicator to a worker here, and on the link, once declared, to a worker of another host.
pub fn send(cx: &mut Context, to: Addr, channel: Channel, data: &[u8]) -> Result<(), Error> {
    cx.io().send(to, channel, data)
}

/// Every frame here is on the wire: this deployment's communicator or the link.
impl Io<'_> {
    /// Lanes are frames under the lane tag, like any other message.
    pub fn send(&mut self, to: Addr, channel: Channel, data: &[u8]) -> Result<(), Error> {
        match channel {
            Channel::Message(tag) => link::send(self.world, self.link, self.rank, to, tag, data),
            Channel::Lane => {
                let tag = lane_tag(self.workers, self.lane)?;
                match link::route(self.link, to)? {
                    Peer::Here(at) => send_on(self.world, Participant::Worker(self.rank), at, tag, data),
                    Peer::There(at) => link::lane(self.link, self.far, self.rank, to, at, tag, data),
                }
            }
        }
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
    workers: &[u32],
    edges: &[Edge],
    bytes: usize,
    tag: Tag,
) -> Result<(), Error> {
    crate::cpu::lanes::validate(workers, edges, bytes, size(cx), MAX_FRAME)?;
    let far = link::far(cx.link(), rank(cx), edges, bytes)?;
    cx.set_lane(tag, workers.to_vec(), far);
    Ok(())
}

pub fn release(cx: &mut Context) -> Result<(), Error> {
    cx.clear_lane();
    Ok(())
}
