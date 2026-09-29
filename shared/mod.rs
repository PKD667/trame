// The published segment's worker half, and the two modules every MPI build needs.
//
// Lane windows belong to their selected backend, because acknowledgement differs between them.
// The MPI environment and the point-to-point route are the same in all three, and
// they are here rather than in each backend because they are three lane transports over one MPI
// world rather than three worlds.
//
// `fatal` used to live here: it printed and called `process::exit`. It is gone. The entry rules
// make process exit non-portable and requires a failure to reach the caller as a record, so every
// path that used to end the process now returns an `Error` and lets the caller decide what to do
// about it.

pub mod context;
pub mod leader;
pub(crate) mod link;
pub mod p2p;

use std::ffi::CString;
use std::io;
use std::marker::PhantomData;
use std::num::NonZeroU64;

use crate::contract::{BackendFault, Error, Failure, FailureKind, Handle, Invalid, Participant};

/// The node-wide name of a leader's publication: its process and the revision. The token is the
/// leader's process id, so a worker can name the object without discovery.
fn segment_name(token: u32, revision: NonZeroU64) -> CString {
    CString::new(format!("/trame-{token}-{}", revision.get()))
        .expect("formatted name contains no NUL")
}

/// Translate a segment syscall's error into the contract's failure record. The `io::Error` comes
/// from a real syscall, so it carries an errno. `leader.rs` reaches this as `super::os_failure`.
fn os_failure(participant: Participant, operation: &'static str, e: io::Error) -> Error {
    Error::Failed(Failure {
        participant,
        operation,
        kind: FailureKind::Backend(BackendFault::Os(e.raw_os_error().expect("OS failure"))),
    })
}

/// The leader's segment, mapped read-only in this worker.
pub struct Shared(
    mpi_rma::Segment,
    // Auto traits match the nv backend's, so the public surface is the same on every backend.
    PhantomData<*const ()>,
);

/// Map the leader's segment read-only.
///
/// # Safety
/// `handle` came from `leader::handle` of a segment not retired before the returned `Shared` is
/// detached or dropped.
pub unsafe fn attach(cx: &mut context::Context, handle: Handle) -> Result<Shared, Error> {
    let token =
        u32::try_from(handle.token()).map_err(|_| Error::Invalid(Invalid::Unrepresentable))?;
    let length =
        usize::try_from(handle.length()).map_err(|_| Error::Invalid(Invalid::Unrepresentable))?;
    let size = length
        .checked_add(64)
        .ok_or(Error::Invalid(Invalid::Unrepresentable))?;
    libc::off_t::try_from(size).map_err(|_| Error::Invalid(Invalid::Unrepresentable))?;
    let name = segment_name(token, handle.revision());
    let segment = unsafe { mpi_rma::Segment::open(&name, handle.revision(), length) }.map_err(
        |e| {
            if e.kind() == io::ErrorKind::InvalidData {
                Error::Invalid(Invalid::NoSegment)
            } else {
                os_failure(Participant::Worker(context::rank(cx)), "attach", e)
            }
        },
    )?;
    Ok(Shared(segment, PhantomData))
}

/// The segment, read-only.
pub fn bytes(segment: &Shared) -> &[u8] {
    segment.0.payload()
}

/// Retire this worker's mapping: it consumes the segment on success and returns the live segment
/// with the error on refusal.
pub fn detach(cx: &mut context::Context, mut segment: Shared) -> Result<(), (Shared, Error)> {
    if let Err(e) = segment.0.detach() {
        return Err((
            segment,
            os_failure(Participant::Worker(context::rank(cx)), "detach", e),
        ));
    }
    Ok(())
}
