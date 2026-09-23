// Lanes as ordinary tagged frames.
//
// Two-sided MPI supplies matching and ordering, so this backend opens no window and keeps no
// out-of-band state: a lane is a frame under the tag `reshape` named, and the `Lane` route differs
// from `Message` only in who chose the tag.

use crate::contract::{Backend, Channel, Edge, Error, Frame, Rank, Tag};

pub use crate::shared::context::{
    Context, Environment, MAX_FRAME, align, done, hosts, init, rank, size,
};
pub use crate::shared::leader;
pub use crate::shared::{Shared, bytes, share, unshare};

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
    crate::shared::context::validate(workers, edges, bytes)?;
    cx.set_lane(tag, workers.to_vec());
    Ok(())
}

pub fn release(cx: &mut Context) -> Result<(), Error> {
    cx.clear_lane();
    Ok(())
}
