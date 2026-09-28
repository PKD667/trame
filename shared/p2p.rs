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
use mpi::raw::traits::AsRawMut;
use mpi::topology::Communicator;

use super::context::{Context, MAX_FRAME};
use crate::invoke::{Owner, Receive};
use crate::contract::{
    BackendFault, Error, Failure, FailureKind, Frame, Invalid, Participant, Rank, Tag,
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
fn counted(status: &Status) -> Result<usize, Error> {
    let count = status.count(u8::equivalent_datatype());
    usize::try_from(count).map_err(unrepresentable)
}

/// Send one frame, copying it out of the caller's borrow before this returns.
pub fn send(cx: &mut Context, to: Rank, tag: Tag, data: &[u8]) -> Result<(), Error> {
    let me = Participant::Worker(cx.rank());
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
    me: Participant,
    to: Rank,
    tag: Tag,
    data: &[u8],
) -> Result<(), Error> {
    if to.get() >= comm.target_size() as u32 {
        return Err(Error::Invalid(Invalid::RankOutsideJob));
    }
    let to = i32::try_from(to.get()).map_err(unrepresentable)?;
    if data.len() > MAX_FRAME {
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
    let me = Participant::Worker(cx.rank());
    peer(take(cx.world(), me, Owner::ALL, &mut 0, out)?)
}

/// A frame from a worker, named by its contract rank.
pub(crate) fn peer(taken: Option<(i32, Tag, usize)>) -> Result<Option<Frame>, Error> {
    taken
        .map(|(source, tag, len)| Ok(Frame::new(Some(contract_rank(source)?), tag, len)))
        .transpose()
}

/// One attempt at the next frame `owner` receives on `comm`: its remote rank, tag and length.
///
/// Only a tag's owner ever matches frames under it, so a size probe and the match after it find
/// the same message. A short buffer therefore takes nothing, and nothing is ever held. `turn` is
/// where an arm that lists its tags starts looking, so no tag starves the others.
pub(crate) fn take<C: Communicator>(
    comm: &C,
    me: Participant,
    owner: Owner<'_>,
    turn: &mut usize,
    out: &mut [u8],
) -> Result<Option<(i32, Tag, usize)>, Error> {
    match owner.mine() {
        Receive::Nothing => Ok(None),
        Receive::All if owner.every() => tagged(comm, me, None, out),
        // Every tag no earlier arm names: look at the next frame, and take it only if it is ours.
        Receive::All => {
            let Some(status) = comm.any_process().immediate_probe() else {
                return Ok(None);
            };
            let tag = contract_tag(status.tag())?;
            if !owner.owns(tag) {
                return Ok(None);
            }
            matched(comm, me, status.source_rank(), tag, counted(&status)?, out)
        }
        Receive::Only(tags) => {
            for k in 0..tags.len() {
                let at = (*turn + k) % tags.len();
                if !owner.owns(tags[at]) {
                    continue;
                }
                if let Some(frame) = tagged(comm, me, Some(tags[at]), out)? {
                    *turn = at + 1;
                    return Ok(Some(frame));
                }
            }
            Ok(None)
        }
    }
}

/// The next frame under `tag`, or under any tag.
fn tagged<C: Communicator>(
    comm: &C,
    me: Participant,
    tag: Option<Tag>,
    out: &mut [u8],
) -> Result<Option<(i32, Tag, usize)>, Error> {
    let any = comm.any_process();
    if out.len() >= MAX_FRAME {
        // No frame is longer than the buffer, so match at once.
        let found = match tag {
            None => any.immediate_matched_probe(),
            Some(tag) => any.immediate_matched_probe_with_tag(i32::from(tag.get())),
        };
        return match found {
            Some((message, status)) => receive(me, message, &status, out).map(Some),
            None => Ok(None),
        };
    }
    let probed = match tag {
        None => any.immediate_probe(),
        Some(tag) => any.immediate_probe_with_tag(i32::from(tag.get())),
    };
    let Some(status) = probed else {
        return Ok(None);
    };
    let tag = contract_tag(status.tag())?;
    matched(comm, me, status.source_rank(), tag, counted(&status)?, out)
}

/// Match the frame a size probe found at `source` under `tag`, once the buffer holds it.
fn matched<C: Communicator>(
    comm: &C,
    me: Participant,
    source: i32,
    tag: Tag,
    needed: usize,
    out: &mut [u8],
) -> Result<Option<(i32, Tag, usize)>, Error> {
    if needed > out.len() {
        return Err(Error::TooSmall { needed });
    }
    // Only this owner matches `tag`, so the frame the probe found is still first in line.
    let found = comm.process_at_rank(source).immediate_matched_probe_with_tag(i32::from(tag.get()));
    let Some((message, status)) = found else {
        return Err(transport(me));
    };
    receive(me, message, &status, out).map(Some)
}

/// Receive a matched message into `out`.
fn receive(
    me: Participant,
    mut message: Message,
    status: &Status,
    out: &mut [u8],
) -> Result<(i32, Tag, usize), Error> {
    let needed = counted(status)?;
    if needed > out.len() {
        // Only a sender past `MAX_FRAME` gets here. MPI owns the handle and rsmpi's Drop would assert.
        core::mem::forget(message);
        return Err(transport(me));
    }
    // SAFETY: the matched handle and output buffer are live, `needed <= out.len()` was checked, and a valid MPI count fits `i32`; MPI initializes status and nulls the handle on success.
    let status = unsafe {
        let mut status = core::mem::MaybeUninit::<mpi::ffi::MPI_Status>::uninit();
        let code = mpi::ffi::MPI_Mrecv(
            out.as_mut_ptr().cast(),
            needed as i32,
            mpi::ffi::RSMPI_UINT8_T,
            message.as_raw_mut(),
            status.as_mut_ptr(),
        );
        if code == 0 {
            Some(Status::from_raw(status.assume_init()))
        } else {
            None
        }
    };
    let Some(status) = status else {
        // MPI owns a failed matched handle and rsmpi's Drop would assert.
        core::mem::forget(message);
        return Err(transport(me));
    };
    let tag = contract_tag(status.tag()).map_err(|_| transport(me))?;
    let len = counted(&status).map_err(|_| transport(me))?;
    Ok((status.source_rank(), tag, len))
}

fn transport(me: Participant) -> Error {
    Error::Failed(Failure {
        participant: me,
        operation: "recv",
        kind: FailureKind::Backend(BackendFault::Transport),
    })
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
