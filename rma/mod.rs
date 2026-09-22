// Acknowledged remote write lanes.
//
// A lane lands in the receiver's own window, so nothing is lost and the sender is held to `depth`
// slots ahead of its receiver. The acknowledgement that makes the route reliable is also what
// makes a send wait, which is a fact about the route rather than a tuning choice.
//
// One consequence is refused by name rather than absorbed: a `Poll` lane send has no bounded
// attempt here, because the ring spins on the acknowledgement counter. The wait policy requires
// `Poll` not to wait for a peer, so the attempt is an input this backend cannot interpret, and a
// caller that needs a one-shot lane needs a transport whose send can be attempted once.

use std::sync::Arc;

use mpi_rma::Ring;

use crate::contract::{
    Channel, Declarations, Edge, Error, Frame, LANE_RELIABLE, PUBLICATION_COLLECTIVE,
    RELEASE_COHORT, Rank, Scopes, Tag, Wait,
};

pub use crate::shared::context::{
    Context, Environment, cohort, done, hosts, init, rank, reading, size, slice,
};
pub use crate::shared::leader;
pub use crate::shared::{Shared, bytes, share, unshare};

/// The host answers these two by being a host: OS threads for execution, and the primitives built
/// on `std::sync`. A backend whose participants are not host threads answers neither name, which
/// is a refusal rather than a weaker guarantee.
pub use crate::cpu::clock;
pub use crate::cpu::sync;
pub use crate::cpu::run;

/// Backend identifier stored in `LOAD` records.
pub const ID: u8 = 1;

pub const DECLARATIONS: Declarations = Declarations {
    atomic_scopes: Scopes {
        participant: true,
        domain: false,
        system: false,
    },
    lane_reliability: LANE_RELIABLE,
    // Blocking: the wire has a blocking probe, and the window is polled with a backoff because a
    // peer sending only lane traffic would never wake a probe on the wire.
    // Blocking: the wire's receive waits in MPI, and a lane drain polls both a wire and a window
    // with a bounded backoff until something arrives.
    waiting_message: 0,
    waiting_lane: 0,
    // A polling send observes the attached buffer's exhaustion as `Full`. A lane does not: the
    // safe ring waits for room against its acknowledgement counter, so there is no refusal to
    // observe and no `Full` to report — a caller that needs lane flow control requires
    // `backpressure`, which this backend refuses.
    pressure_message: true,
    pressure_lane: false,
    // Freeing a window is collective over the cohort, and it is also the barrier that gets every
    // worker past its old window before any of them starts the next load's collectives.
    release: RELEASE_COHORT,
    publication: PUBLICATION_COLLECTIVE,
    priority: true,
    resident: false,
    // An MPI count is an `int`, and the slot a window holds is bounded by its declaration, which
    // `reshape` checks. The contract's ceiling for this backend is the transport's own width.
    max_frame: i32::MAX as u32,
    tag_limit: i32::MAX as u32,
    lowering: "rma-ring",
};

/// Send one frame: `Message` on the wire, `Lane` into the receiver's window.
pub fn send(
    cx: &mut Context,
    to: Rank,
    channel: Channel,
    data: &[u8],
    wait: Wait,
) -> Result<(), Error> {
    match channel {
        Channel::Message(tag) => crate::shared::p2p::send(cx, to, tag, data, wait),
        Channel::Lane => {
            let index = cx.lane_index(to)?;
            let ring = cx.ring().ok_or(Error::Invalid { code: 1 })?;
            // See the header: the safe ring spins on the acknowledgement counter, so there is no
            // one-shot attempt to make. Refused rather than silently blocking.
            if wait == Wait::Poll {
                return Err(Error::Invalid { code: 2 });
            }
            ring.send(index, data)
                .map(|_| ())
                .map_err(|_| cx.failure("lane"))
        }
    }
}

/// The next frame from either route. One implementation, in `shared`, because the two ring
/// transports differ in what they overwrite and not in how they receive — and the copy that was
/// here had drifted out of step with the contract while the other one had too.
pub fn recv(cx: &mut Context, out: &mut [u8], wait: Wait) -> Result<Option<Frame>, Error> {
    crate::shared::p2p::recv_from_either(cx, out, wait)
}

pub fn flush(cx: &mut Context, wait: Wait) -> Result<(), Error> {
    crate::shared::p2p::flush(cx, wait)
}

/// Open the window the declaration asks for. Collective, and only at a load.
pub fn reshape(
    cx: &mut Context,
    workers: &[Rank],
    edges: &[Edge],
    bytes: u32,
    tag: Tag,
) -> Result<(), Error> {
    crate::shared::context::validate(workers, edges, bytes, DECLARATIONS.max_frame)?;
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
