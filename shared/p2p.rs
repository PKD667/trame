// Point-to-point messaging for every MPI build: the `Message` route, and the lane route too on
// the backend whose lanes are ordinary frames.
//
// Frames are received into the *caller's* buffer, so nothing allocates per frame and a short buffer
// is reported instead of consuming the frame. Every call is one attempt: a buffered send copies or
// is refused, a matched probe finds a frame or nothing.
//
// `Rank` is `u32` and `Tag` is `u16` in the contract, and both are `i32` in MPI. The conversion is
// checked at every boundary and a value that does not fit is `Invalid`: a truncated rank delivers a
// frame to the wrong participant.

use mpi::datatype::Equivalence;
use mpi::point_to_point::{Message, Source, Status};
use mpi::topology::Communicator;

use super::context::{Context, MAX_FRAME};
use crate::contract::{
    BackendFault, Error, Failure, FailureKind, Frame, FrameBytes, Invalid, Rank, Tag,
};

fn unrepresentable<T>(_: T) -> Error {
    Error::Invalid(Invalid::Unrepresentable)
}

fn contract_rank(rank: i32) -> Result<Rank, Error> {
    u32::try_from(rank)
        .map(Rank::from_index)
        .map_err(unrepresentable)
}

fn contract_tag(tag: i32) -> Result<Tag, Error> {
    u16::try_from(tag).map(Tag::new).map_err(unrepresentable)
}

/// How many bytes MPI reported behind a probed message.
fn counted(status: &Status) -> Result<FrameBytes, Error> {
    let count = status.count(u8::equivalent_datatype());
    usize::try_from(count)
        .map_err(unrepresentable)
        .and_then(|count| FrameBytes::try_from(count).map_err(Error::Invalid))
}

/// Send one frame, copying it out of the caller's borrow before this returns.
pub fn send(cx: &mut Context, to: Rank, tag: Tag, data: &[u8]) -> Result<(), Error> {
    let me = cx.rank();
    send_on(cx.world(), me, to, tag, data)
}

/// Send one frame on `comm`, in buffered mode: the copy happens inside the call, so the sender
/// never waits on a receiver, and an attached buffer with no room is `Full`.
///
/// Generic over the communicator, because the leader's route is a communicator of a different
/// *kind* rather than a different protocol. `take` below is generic for the same reason. `me` is
/// who a transport failure is reported as observed by.
///
/// The bound is checked here rather than left to MPI: a rank outside the communicator is a
/// caller's mistake that the contract has a value for.
pub(crate) fn send_on<C: Communicator>(
    comm: &C,
    me: Rank,
    to: Rank,
    tag: Tag,
    data: &[u8],
) -> Result<(), Error> {
    if to.get() >= comm.target_size() as u32 {
        return Err(Error::Invalid(Invalid::RankOutsideJob));
    }
    let to = i32::try_from(to.get()).map_err(unrepresentable)?;
    if data.len() > MAX_FRAME.get() as usize {
        return Err(Error::TooLarge { limit: MAX_FRAME });
    }
    // Called through the raw interface, because rsmpi's wrapper panics on a refusal and a refusal
    // is the answer the contract asks for. `MPI_ERR_BUFFER` is the class MPI uses when the attached
    // buffer has no room, and the communicator returns errors rather than aborting because `init`
    // asked it to. SAFETY: the buffer is live for the call, the count is at most `MAX_FRAME`, which
    // fits an `int`, and the communicator is live while the context that owns it is.
    let code = unsafe {
        mpi::ffi::MPI_Bsend(
            data.as_ptr().cast(),
            data.len() as i32,
            mpi::ffi::RSMPI_UINT8_T,
            to,
            i32::from(tag.get()),
            comm.as_raw(),
        )
    };
    match u32::try_from(code) {
        Ok(0) => Ok(()),
        Ok(mpi::ffi::MPI_ERR_BUFFER) => Err(Error::Full),
        _ => Err(Error::Failed(Failure {
            participant: me,
            operation: "send",
            kind: FailureKind::Backend(BackendFault::Transport),
        })),
    }
}

/// Take one frame from any source into the caller's buffer.
pub fn recv(cx: &mut Context, out: &mut [u8]) -> Result<Option<Frame>, Error> {
    let (comm, held) = cx.split();
    take(comm, held, out)
}

/// Take one frame from any source on `comm` into the caller's buffer.
///
/// The match and the receive are one operation: a plain probe followed by a receive can deliver
/// one frame's length with another frame's bytes when a second message arrives in between.
///
/// A matched message belongs to this call. If it does not fit, it is held rather than abandoned —
/// MPI has no un-probe and cancelling is not guaranteed — and the next call hands it over once
/// the caller has made room. The hold slot is the caller's, one per route, so a participant holding
/// a message route's frame is not made to surrender it because the leader route refused one.
pub(crate) fn take<C: Communicator>(
    comm: &C,
    held: &mut Option<(Message, FrameBytes)>,
    out: &mut [u8],
) -> Result<Option<Frame>, Error> {
    let (message, needed) = match held.take() {
        Some(held) => held,
        None => {
            let Some((message, status)) = comm.any_process().immediate_matched_probe() else {
                return Ok(None);
            };
            (message, counted(&status)?)
        }
    };
    if needed.get() as usize > out.len() {
        *held = Some((message, needed));
        return Err(Error::TooSmall { needed });
    }
    let status = message.matched_receive_into(out);
    Ok(Some(Frame::new(
        contract_rank(status.source_rank())?,
        contract_tag(status.tag())?,
        counted(&status)?,
    )))
}

/// Every accepted send has released its send resource: a buffered send copies into the attached
/// buffer inside the call, so there is no caller-owned storage left outstanding.
pub fn flush(_cx: &mut Context) -> Result<(), Error> {
    Ok(())
}

/// The next frame from either route, for a backend whose lanes are not the wire.
///
/// The wire is checked first. Control, sink and log traffic ride it, and a rank behind on those
/// cannot even be told to stop — so a lane may not starve them.
#[cfg(feature = "ring")]
pub(crate) fn recv_from_either(cx: &mut Context, out: &mut [u8]) -> Result<Option<Frame>, Error> {
    if let Some(frame) = recv(cx, out)? {
        return Ok(Some(frame));
    }
    cx.next_lane_frame(out)
}
