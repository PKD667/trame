// MPI messages carry batches; local lanes use acknowledged RMA rings unless `lossy` selects
// overwrite-on-full rings. The compile-time feature makes that delivery contract rank-uniform.

use std::sync::Arc;

pub mod optim;
pub mod context;
pub mod leader;
pub(crate) mod link;
pub mod p2p;
mod segment;

use mpi_rma::Ring;

use crate::contract::{Addr, Backend, Channel, Invalid, Edge, Error, Frame, Tag};
use crate::mpi::context::{failure, lane_index, lane_tag};
use crate::mpi::link::Peer;

pub use crate::mpi::context::{
    Context, Environment, Io, MAX_FRAME, barrier, concurrent_io, done, init, rank, size,
};

pub use segment::{Shared, attach, bytes, detach};

/// The host answers these by being a host: OS threads for execution, and the primitives built on
/// `std::sync`. A backend whose participants are not host threads answers none of these names.
pub use crate::host::clock;
pub use crate::host::run;
pub use crate::host::sync;

pub const ID: Backend = Backend::Mpi;

// The window's lane table, which needs no communicator.
#[cfg(test)]
mod tests;

/// Send one frame: `Message` on the wire, `Lane` into the receiver's window when it is here, and
/// on the link when it is on another host, where no window reaches.
pub fn send(cx: &mut Context, to: Addr, channel: Channel, data: &[u8]) -> Result<(), Error> {
    cx.io().send(to, channel, data)
}

impl Io<'_> {
    pub fn send(&mut self, to: Addr, channel: Channel, data: &[u8]) -> Result<(), Error> {
        match channel {
            Channel::Message(tag) => crate::mpi::link::send(self.world, self.link, self.rank, to, tag, data),
            Channel::Lane => match crate::mpi::link::route(self.link, to)? {
                Peer::Here(at) => {
                    let index = lane_index(self.workers, at)?;
                    let ring = self.ring.ok_or(Error::Invalid(Invalid::LaneNotConfigured))?;
                    #[cfg(not(feature = "lossy"))]
                    return refused(ring.send(index, data), self.rank);
                    #[cfg(feature = "lossy")]
                    return ring.send(index, data).map(|_| ()).map_err(|_| failure(self.rank, "lane"));
                }
                Peer::There(at) => {
                    let tag = lane_tag(self.workers, self.lane)?;
                    crate::mpi::link::lane(self.link, self.far, self.rank, to, at, tag, data)
                }
            },
        }
    }
}

/// A full reliable lane is `Full`; a transport refusal is a backend failure.
#[cfg(not(feature = "lossy"))]
fn refused(sent: Result<u64, mpi_rma::Error>, rank: u32) -> Result<(), Error> {
    sent.map(|_| ()).map_err(|e| match e {
        mpi_rma::Error::Full => Error::Full,
        _ => failure(rank, "lane"),
    })
}

/// The next frame from either route. One implementation, in `shared`, because the two ring
/// transports differ in what they overwrite and not in how they receive — and the copy that was
/// here had drifted out of step with the contract while the other one had too.
pub fn recv(cx: &mut Context, out: &mut [u8]) -> Result<Option<Frame>, Error> {
    crate::mpi::p2p::recv_from_either(cx, out)
}

pub fn flush(cx: &mut Context) -> Result<(), Error> {
    crate::mpi::p2p::flush(cx)
}

/// Open the window the declaration asks for. Collective, and only at a load.
pub fn reshape(
    cx: &mut Context,
    workers: &[u32],
    edges: &[Edge],
    bytes: usize,
    tag: Tag,
) -> Result<(), Error> {
    crate::host::lanes::validate(workers, edges, bytes, size(cx), MAX_FRAME)?;
    let far = crate::mpi::link::far(cx.link(), rank(cx), edges, bytes)?;
    let lanes = crate::mpi::context::window(workers, edges, bytes)?;
    #[cfg(not(feature = "lossy"))]
    let ring = Ring::safe(cx.together(), &lanes).map_err(|_| failure(rank(cx), "reshape"))?;
    #[cfg(feature = "lossy")]
    let ring = Ring::raw(cx.together(), &lanes).map_err(|_| failure(rank(cx), "reshape"))?;
    cx.set_window(tag, workers.to_vec(), far, Arc::new(ring));
    Ok(())
}

/// Drop the window. Its drop is the collective free, and the barrier that follows it.
pub fn release(cx: &mut Context) -> Result<(), Error> {
    cx.clear_lane();
    Ok(())
}
