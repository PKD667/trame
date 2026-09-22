// Acknowledged remote write lanes.
//
// A lane lands in the receiver's own window, so nothing is lost and the sender is held to `depth`
// slots ahead of its receiver. The acknowledgement that makes the route reliable is also what
// makes a send wait, which is a fact about the route rather than a tuning choice.
//
// So a lane send here is not yet one attempt: `mpi-rma`'s safe ring has no send that reports a
// full slot instead of waiting for it.

use std::sync::Arc;

use mpi_rma::Ring;

use crate::contract::{Backend, Channel, Invalid, Edge, Error, Frame, FrameBytes, Rank, Tag};

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

pub const ID: Backend = Backend::Rma;

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
            // C3: the safe ring spins on the receiver's acknowledgement until a slot is free.
            ring.send(index, data)
                .map(|_| ())
                .map_err(|_| cx.failure("lane"))
        }
    }
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
    bytes: FrameBytes,
    tag: Tag,
) -> Result<(), Error> {
    crate::shared::context::validate(workers, edges, bytes)?;
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
