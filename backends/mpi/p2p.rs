// Point-to-point messaging for every MPI build: the `Message` route, and the lane route too on
// the backend whose lanes are ordinary frames.
//
// Frames are received into the *caller's* buffer, so nothing allocates per frame and a short buffer
// is reported instead of consuming the frame. Every call is one attempt: a buffered send copies or
// is refused, a matched probe finds a frame or nothing.
//
// A rank is `u32` and `Tag` is `u16` in the contract, and both are `i32` in MPI. The conversion is
// checked at every boundary and a value that does not fit is `Invalid`: a truncated rank delivers a
// frame to the wrong participant.

use mpi::datatype::Equivalence;
use mpi::point_to_point::{Message, Source, Status};
use mpi::raw::traits::AsRawMut;
use mpi::topology::Communicator;
use std::collections::VecDeque;
use std::sync::Mutex;

use super::context::{Context, MAX_FRAME};
use crate::invoke::{Owner, Receive};
use crate::contract::{
    Addr, BackendFault, Error, Failure, FailureKind, Frame, Invalid, Participant, Tag,
};

/// Frames removed from MPI while another arm owned the next queued tag.
pub(crate) struct Deferred(Mutex<VecDeque<(i32, Tag, Vec<u8>)>>);

impl Deferred {
    pub(crate) fn new() -> Self {
        Self(Mutex::new(VecDeque::new()))
    }

    pub(super) fn push(&self, frame: (i32, Tag, Vec<u8>), me: Participant) -> Result<(), Error> {
        self.0.lock().map_err(|_| transport(me))?.push_back(frame);
        Ok(())
    }
}

fn unrepresentable<T>(_: T) -> Error {
    Error::Invalid(Invalid::Unrepresentable)
}

fn contract_rank(rank: i32) -> Result<u32, Error> {
    u32::try_from(rank).map_err(unrepresentable)
}

fn contract_tag(tag: i32) -> Result<Tag, Error> {
    u16::try_from(tag).map(Tag::new).map_err(unrepresentable)
}

/// How many bytes MPI reported behind a probed message.
fn counted(status: &Status) -> Result<usize, Error> {
    let count = status.count(u8::equivalent_datatype());
    usize::try_from(count).map_err(unrepresentable)
}

/// Send one frame on `comm`, in buffered mode: the copy happens inside the call, so the sender
/// never waits on a receiver, and an attached buffer with no room is `Full`.
///
/// Generic over the communicator, because the leader's route is a communicator of a different
/// *kind* rather than a different protocol. `take` below is generic for the same reason. `me` is
/// who a transport failure is reported as observed by.
///
/// `to` is a rank on `comm` its caller already bounded: `link::route` for a peer, `Leader::send`
/// for a worker, and the bridge's one leader. A rank outside the communicator is refused there, as
/// the caller's mistake the contract has a value for, and not left to MPI.
pub(crate) fn send_on<C: Communicator>(
    comm: &C,
    me: Participant,
    to: i32,
    tag: Tag,
    data: &[u8],
) -> Result<(), Error> {
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

/// Take one frame from any worker into the caller's buffer: this deployment's communicator and the
/// link, each call starting with the one the last did not.
pub fn recv(cx: &mut Context, out: &mut [u8]) -> Result<Option<Frame>, Error> {
    let me = Participant::Worker(cx.rank());
    let link_first = cx.flip();
    for link in [link_first, !link_first] {
        let frame = if link {
            super::link::take(cx.link(), cx.rank(), Owner::ALL, &mut 0, out, &cx.link_deferred)?
        } else {
            peer(take_with_deferred(cx.world(), me, Owner::ALL, &mut 0, out, &cx.world_deferred)?)?
        };
        if frame.is_some() {
            return Ok(frame);
        }
    }
    Ok(None)
}

/// A frame from a worker of this deployment, named by its local rank.
pub(crate) fn peer(taken: Option<(i32, Tag, usize)>) -> Result<Option<Frame>, Error> {
    taken
        .map(|(source, tag, len)| Ok(Frame::new(Some(Addr::Local(contract_rank(source)?)), tag, len)))
        .transpose()
}

/// One attempt at the next frame `owner` receives on `comm`: its remote rank, tag and length.
///
/// Unscoped receive, used where every tag is eligible and therefore no frame needs deferring.
pub(crate) fn take<C: Communicator>(
    comm: &C,
    me: Participant,
    owner: Owner<'_>,
    turn: &mut usize,
    out: &mut [u8],
) -> Result<Option<(i32, Tag, usize)>, Error> {
    take_pending(comm, me, owner, turn, out, &Deferred::new())
}

/// Only a tag's owner matches frames under it. Scoped `All` receives defer intervening unowned
/// messages in communicator order; a short buffer therefore takes nothing. `turn` is where an arm
/// that lists its tags starts looking, so no tag starves the others.
pub(crate) fn take_with_deferred<C: Communicator>(
    comm: &C,
    me: Participant,
    owner: Owner<'_>,
    turn: &mut usize,
    out: &mut [u8],
    deferred: &Deferred,
) -> Result<Option<(i32, Tag, usize)>, Error> {
    match owner.mine() {
        Receive::Nothing => Ok(None),
        _ => {
            if let Some(frame) = take_deferred(deferred, me, owner, out)? {
                return Ok(Some(frame));
            }
            take_pending(comm, me, owner, turn, out, deferred)
        }
    }
}

fn take_pending<C: Communicator>(
    comm: &C,
    me: Participant,
    owner: Owner<'_>,
    turn: &mut usize,
    out: &mut [u8],
    deferred: &Deferred,
) -> Result<Option<(i32, Tag, usize)>, Error> {
    match owner.mine() {
        Receive::Nothing => Ok(None),
        Receive::All if owner.every() => tagged(comm, me, None, out),
        // Every tag no earlier arm names: look at the next frame, and take it only if it is ours.
        Receive::All => {
            loop {
                let Some(status) = comm.any_process().immediate_probe() else {
                    return Ok(None);
                };
                let tag = contract_tag(status.tag())?;
                if owner.owns(tag) {
                    return matched(comm, me, status.source_rank(), tag, counted(&status)?, out);
                }
                let source = status.source_rank();
                let size = counted(&status)?;
                let mut data = vec![0; size];
                let Some((message, found)) = comm.process_at_rank(source)
                    .immediate_matched_probe_with_tag(status.tag()) else {
                    return Err(transport(me));
                };
                let (source, tag, len) = receive(me, message, &found, &mut data)?;
                data.truncate(len);
                deferred.push((source, tag, data), me)?;
            }
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

pub(super) fn take_deferred(
    deferred: &Deferred,
    me: Participant,
    owner: Owner<'_>,
    out: &mut [u8],
) -> Result<Option<(i32, Tag, usize)>, Error> {
    let mut queue = deferred.0.lock().map_err(|_| transport(me))?;
    let Some(at) = queue.iter().position(|(_, tag, _)| owner.owns(*tag)) else {
        return Ok(None);
    };
    let (source, tag, data) = &queue[at];
    if data.len() > out.len() {
        return Err(Error::TooSmall { needed: data.len() });
    }
    let (source, tag, len) = (*source, *tag, data.len());
    out[..len].copy_from_slice(data);
    queue.remove(at);
    Ok(Some((source, tag, len)))
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
pub(crate) fn recv_from_either(cx: &mut Context, out: &mut [u8]) -> Result<Option<Frame>, Error> {
    if let Some(frame) = recv(cx, out)? {
        return Ok(Some(frame));
    }
    cx.next_lane_frame(out)
}
