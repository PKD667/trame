// Lanes as ordinary tagged frames.
//
// Two-sided MPI supplies matching and ordering, so this backend opens no window and keeps no
// out-of-band state: a lane is a frame under the tag `reshape` named, and the `Lane` route differs
// from `Message` only in who chose the tag.

use crate::contract::{
    Channel, Declarations, Edge, Error, Frame, LANE_RELIABLE, PUBLICATION_COLLECTIVE,
    RELEASE_LOCAL, Rank, Scopes, Tag, Wait,
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
pub const ID: u8 = 0;

pub const DECLARATIONS: Declarations = Declarations {
    // A rank is a process, so an atomic orders that process's threads and nothing else. The
    // scopes wider than a participant are the node's shared memory, which these primitives are
    // not, and a scope is declared rather than assumed.
    atomic_scopes: Scopes {
        participant: true,
        domain: false,
        system: false,
    },
    lane_reliability: LANE_RELIABLE,
    // Blocking: MPI's receive waits on the wire, which is the whole of this backend's traffic.
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
    // There is no window to free, so a release is this participant's own obligation and says
    // nothing about anyone else.
    release: RELEASE_LOCAL,
    publication: PUBLICATION_COLLECTIVE,
    priority: true,
    resident: false,
    // An MPI count is an `int`, and the tag ceiling is the same width.
    max_frame: i32::MAX as u32,
    tag_limit: i32::MAX as u32,
    lowering: "scalar",
};

/// Send one frame. A lane is an ordinary frame under the load's tag.
pub fn send(
    cx: &mut Context,
    to: Rank,
    channel: Channel,
    data: &[u8],
    wait: Wait,
) -> Result<(), Error> {
    let tag = match channel {
        Channel::Message(tag) => tag,
        Channel::Lane => cx.lane_tag()?,
    };
    crate::shared::p2p::send(cx, to, tag, data, wait)
}

/// Every frame here is on the wire, so there is one route to receive from.
pub fn recv(cx: &mut Context, out: &mut [u8], wait: Wait) -> Result<Option<Frame>, Error> {
    crate::shared::p2p::recv(cx, out, wait)
}

pub fn flush(cx: &mut Context, wait: Wait) -> Result<(), Error> {
    crate::shared::p2p::flush(cx, wait)
}

/// Take the lane table. No window is allocated: nothing has to be built for a lane to exist.
pub fn reshape(
    cx: &mut Context,
    workers: &[Rank],
    edges: &[Edge],
    bytes: u32,
    tag: Tag,
) -> Result<(), Error> {
    crate::shared::context::validate(workers, edges, bytes, DECLARATIONS.max_frame)?;
    cx.set_lane(tag, workers.to_vec());
    Ok(())
}

pub fn release(cx: &mut Context) -> Result<(), Error> {
    cx.clear_lane();
    Ok(())
}
