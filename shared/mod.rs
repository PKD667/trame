// The published segment, and the two modules every MPI build needs.
//
// Lane windows belong to their selected backend, because acknowledgement differs between them.
// The MPI environment, the point-to-point route and the segment are the same in all three, and
// they are here rather than in each backend because they are three lane transports over one MPI
// world rather than three worlds.
//
// `fatal` used to live here: it printed and called `process::exit`. It is gone. The entry rules
// make process exit non-portable and requires a failure to reach the caller as a record, so every
// path that used to end the process now returns an `Error` and lets the caller decide what to do
// about it.

pub mod context;
pub mod leader;
pub mod p2p;

use crate::contract::Error;

/// The published segment: one read-only copy per node, mapped zero-copy on every rank.
pub struct Shared(mpi_rma::SharedWindow);

/// Publish this rank's slice and map the node-local segment.
///
/// Collective: every member of the cohort calls it at the same point of a load with the same
/// `total`.
pub fn share(cx: &mut context::Context, mine: &[u8], total: usize) -> Result<Shared, Error> {
    let window = mpi_rma::SharedWindow::publish(cx.together(), mine, total)
        .map_err(|_| cx.failure("share"))?;
    Ok(Shared(window))
}

/// The segment, read-only. Every rank reads the same bytes.
pub fn bytes(segment: &Shared) -> &[u8] {
    segment.0.get()
}

/// Retire this participant's mapping. Dropping the window is the collective free.
pub fn unshare(_cx: &mut context::Context, segment: Shared) -> Result<(), (Shared, Error)> {
    drop(segment);
    Ok(())
}
