// The MPI calls of a lane, wrapped: a request is started, tested, waited for or cancelled here and nowhere else in
// cross/. A failed call is a panic: a lane has no way to go on without its message.

extern crate std;

use mpi::ffi;

pub(super) fn ok(rc: i32, what: &str) {
    assert_eq!(rc, ffi::MPI_SUCCESS as i32, "trame: MPI_{what} failed on a route between hosts");
}

pub(super) fn null() -> ffi::MPI_Request {
    // SAFETY: a constant of the MPI library (PROOFS.md, Assumptions: Foreign calls).
    unsafe { ffi::RSMPI_REQUEST_NULL }
}

/// Whether request `r` is complete; a null request is. Drives MPI's progress.
pub(super) fn test(r: &mut ffi::MPI_Request) -> bool {
    let mut flag = 0;
    // SAFETY: `r` is a request of this process, or null (Foreign calls).
    ok(unsafe { ffi::MPI_Test(r, &mut flag, ffi::RSMPI_STATUS_IGNORE) }, "Test");
    flag != 0
}

pub(super) fn wait(r: &mut ffi::MPI_Request) {
    // SAFETY: as `test` (Foreign calls).
    ok(unsafe { ffi::MPI_Wait(r, ffi::RSMPI_STATUS_IGNORE) }, "Wait");
}

/// Cancel a posted receive and complete it. One that already matched completes normally.
pub(super) fn cancel(r: &mut ffi::MPI_Request) {
    if *r != null() {
        // SAFETY: `r` is a receive this process posted (Foreign calls).
        ok(unsafe { ffi::MPI_Cancel(r) }, "Cancel");
        wait(r);
    }
}

/// Post a receive of `n` bytes at `buf`. The buffer must stay put until the request completes.
pub(super) fn irecv(buf: *mut u8, n: usize, peer: i32, tag: i32, comm: ffi::MPI_Comm, r: &mut ffi::MPI_Request) {
    // SAFETY: `buf` holds `n` bytes of a boxed slice of the lane, which lives until `quiesce` has
    // completed or cancelled every request on it (F5; Foreign calls).
    ok(unsafe { ffi::MPI_Irecv(buf.cast(), n as i32, ffi::RSMPI_UINT8_T, peer, tag, comm, r) }, "Irecv");
}

/// Send `n` bytes at `buf`. The buffer must stay unchanged until the request completes.
pub(super) fn isend(buf: *const u8, n: usize, peer: i32, tag: i32, comm: ffi::MPI_Comm, r: &mut ffi::MPI_Request) {
    // SAFETY: as `irecv`, and the buffer is not written until the send completes: `put` waits on it
    // first (F5; Foreign calls).
    ok(unsafe { ffi::MPI_Isend(buf.cast(), n as i32, ffi::RSMPI_UINT8_T, peer, tag, comm, r) }, "Isend");
}
