// Point-to-point messaging for every MPI build: the `Message` route, and the lane route too on
// the backend whose lanes are ordinary frames.
//
// Three things changed from the old shape, and each is the contract rather than a preference.
// Frames are received into the *caller's* buffer, so nothing allocates per frame and a short buffer
// is reported instead of consuming the frame. A receive is `Result<Option<Frame>, Error>` rather
// than an `Option`, because "nothing arrived" and "your buffer is too small" are different answers
// with different fixes. And nothing here exits: the old `fatal` printed and called `process::exit`,
// which the entry rules make non-portable, so every failure is a value.
//
// `Rank` and `Tag` are `u32` in the contract and `i32` in MPI. The conversion is checked at every
// boundary and a value that does not fit is `Invalid`: a truncated rank delivers a frame to the
// wrong participant, which is the one fault the whole contract is arranged to make unmistakable.

use mpi::datatype::Equivalence;
use mpi::point_to_point::{Destination, Message, Source, Status};
use mpi::topology::Communicator;

use super::context::Context;
use crate::contract::{Error, Frame, Rank, Tag, Wait};

/// Offset MPI error codes live past this so that a diagnosis can name which refusal it was.
const RANK: u32 = 1;
const TAG: u32 = 2;

fn wire_rank(rank: Rank) -> Result<i32, Error> {
    i32::try_from(rank).map_err(|_| Error::Invalid { code: RANK })
}

fn wire_tag(tag: Tag) -> Result<i32, Error> {
    i32::try_from(tag).map_err(|_| Error::Invalid { code: TAG })
}

fn contract_rank(rank: i32) -> Result<Rank, Error> {
    u32::try_from(rank).map_err(|_| Error::Invalid { code: RANK })
}

fn contract_tag(tag: i32) -> Result<Tag, Error> {
    u32::try_from(tag).map_err(|_| Error::Invalid { code: TAG })
}

/// How many bytes MPI reported behind a probed message.
pub(crate) fn counted(status: &Status) -> Result<u32, Error> {
    let count = status.count(u8::equivalent_datatype());
    u32::try_from(count).map_err(|_| Error::Invalid { code: 3 })
}

/// Send one frame, copying it out of the caller's borrow before this returns.
///
/// `Poll` uses buffered mode, whose whole property is that the copy happens inside the call and
/// the sender never waits on a receiver that is busy elsewhere. `Wait` uses the blocking send,
/// which is what a frame larger than the attached buffer needs — and which is why the two are
/// different requests rather than a threshold: a symmetric exchange that blocks on both sides
/// deadlocks, and only the caller knows whether its exchange is symmetric.
pub fn send(cx: &mut Context, to: Rank, tag: Tag, data: &[u8], wait: Wait) -> Result<(), Error> {
    send_on(cx.world(), cx.bsend_bytes(), to, tag, data, wait)
}

/// Send one frame on `comm`.
///
/// Generic over the communicator, because the leader's route is a communicator of a different
/// *kind* rather than a different protocol. An inter-communicator addresses remote ranks exactly
/// the way this code already addresses ranks, so writing the same rule twice for two kinds of
/// communicator would be two copies of one rule kept in step by hand — and the copy that drifts is
/// the one nobody is running. `take` below is generic for the same reason.
///
/// The bound is checked here rather than left to `process_at_rank`, which asserts: a rank outside
/// the communicator is a caller's mistake that the contract has a value for, and a panic is not
/// that value.
pub(crate) fn send_on<C: Communicator>(
    comm: &C,
    bsend_bytes: usize,
    to: Rank,
    tag: Tag,
    data: &[u8],
    wait: Wait,
) -> Result<(), Error> {
    let _ = wire_rank(to)?;
    if to >= comm.target_size() as u32 {
        return Err(Error::Invalid { code: RANK });
    }
    let tag = wire_tag(tag)?;
    let destination = comm.process_at_rank(to as i32);
    match wait {
        Wait::Poll => {
            let count = i32::try_from(data.len()).map_err(|_| Error::TooLarge {
                limit: u32::try_from(bsend_bytes).unwrap_or(u32::MAX),
            })?;
            // Called through the raw interface, because rsmpi's wrapper panics on a refusal and a
            // refusal is the answer the contract asks for: a refusal for lack of capacity reports
            // `Full` whenever an attempt is refused for capacity, *including* when the attempt's
            // own failure is the only observation of it. `MPI_ERR_BUFFER` is the class MPI uses
            // when the attached buffer has no room, and the communicator returns errors rather than
            // aborting because `init` asked it to. SAFETY: the buffer is live for the call, the
            // count is checked against an `int`, and the communicator is live while the context
            // that owns it is.
            let code = unsafe {
                mpi::ffi::MPI_Bsend(
                    data.as_ptr().cast(),
                    count,
                    mpi::ffi::RSMPI_UINT8_T,
                    to as i32,
                    tag,
                    comm.as_raw(),
                )
            };
            match u32::try_from(code) {
                Ok(0) => Ok(()),
                Ok(mpi::ffi::MPI_ERR_BUFFER) => Err(Error::Full),
                _ => Err(Error::Invalid { code: 4 }),
            }
        }
        Wait::Wait => {
            destination.send_with_tag(data, tag);
            Ok(())
        }
    }
}

/// Take one frame from any source into the caller's buffer.
///
/// The match and the receive are one operation, and that is not an optimisation. A plain probe
/// followed by a receive is *not* atomic: the probe names a message and the receive matches
/// whatever is first when it arrives, and a rank whose peers are all sending can have a different
/// message arrive in between — which delivers one frame's length with another frame's bytes. That
/// is the failure this shape exists to prevent, and it bit the `particles` experiment as
/// `MPI_ERR_TRUNCATE` before the matched probe replaced the plain one.
///
/// A matched message belongs to this call. If it does not fit, it is held rather than abandoned —
/// MPI has no un-probe and cancelling is not guaranteed — and the next call hands it over once
/// the caller has made room. That is the whole of what a `TooSmall` refusal promises.
pub fn recv(cx: &mut Context, out: &mut [u8], wait: Wait) -> Result<Option<Frame>, Error> {
    let (comm, held) = cx.split();
    take(comm, held, out, wait)
}

/// Take one frame from any source on `comm` into the caller's buffer.
///
/// The hold slot is the caller's, because a frame that was matched and did not fit has to survive
/// the refusal and there is one such slot per route, not one per process: a participant holding a
/// message route's frame must not be made to surrender it because the leader route refused one.
/// Everything else here is the rule stated once — match, measure, hold or deliver — and the two
/// routes differ only in which communicator and which slot they pass in.
pub(crate) fn take<C: Communicator>(
    comm: &C,
    held: &mut Option<Message>,
    out: &mut [u8],
    wait: Wait,
) -> Result<Option<Frame>, Error> {
    if let Some(held) = held.take() {
        return deliver(held, out);
    }
    let matched = match wait {
        Wait::Poll => comm.any_process().immediate_matched_probe(),
        Wait::Wait => Some(comm.any_process().matched_probe()),
    };
    let Some((message, status)) = matched else {
        return Ok(None);
    };
    let needed = counted(&status)?;
    if needed as usize > out.len() {
        *held = Some(message);
        return Err(Error::TooSmall { needed });
    }
    deliver(message, out)
}

/// Receive a matched message into the caller's buffer.
pub(crate) fn deliver(message: Message, out: &mut [u8]) -> Result<Option<Frame>, Error> {
    let status = message.matched_receive_into(out);
    Ok(Some(Frame {
        source: contract_rank(status.source_rank())?,
        tag: contract_tag(status.tag())?,
        len: counted(&status)?,
    }))
}

/// The next frame from either route, for a backend whose lanes are not the wire.
///
/// The wire is checked first. Control, sink and log traffic ride it, and a rank behind on those
/// cannot even be told to stop — so a lane may not starve them, and a window is drained only once
/// the wire has nothing.
///
/// Two sources have no single blocking wait, so this polls both and backs off between attempts
/// rather than blocking on one: a peer sending only lane traffic would never wake a wait that
/// watched the wire alone.
///
/// A waiting call must return a frame or an error, so the loop has no exit other than its returns.
/// The case that broke that: a participant with no lane geometry — a sink, which never reshapes —
/// asked for a *waiting* receive, found the wire empty at that instant, and was answered `None`,
/// because there was no window to look in. "Nothing arrived" is a poll's answer and reporting it to
/// a waiting caller is the silent weakening the contract forbids. With no window the wire is the
/// only source there will ever be, so the call belongs to the wire, with the caller's own wait and
/// a blocking probe underneath it.
#[cfg(feature = "ring")]
pub(crate) fn recv_from_either(
    cx: &mut Context,
    out: &mut [u8],
    wait: Wait,
) -> Result<Option<Frame>, Error> {
    let Ok(tag) = cx.lane_tag() else {
        return recv(cx, out, wait);
    };
    if let Some(frame) = recv(cx, out, Wait::Poll)? {
        return Ok(Some(frame));
    }
    let mut attempts = 0u32;
    loop {
        for (source, data) in cx.drain()? {
            let len = u32::try_from(data.len()).map_err(|_| Error::TooLarge { limit: u32::MAX })?;
            if data.len() > out.len() {
                return Err(Error::TooSmall { needed: len });
            }
            out[..data.len()].copy_from_slice(&data);
            return Ok(Some(Frame { source, tag, len }));
        }
        if wait == Wait::Poll {
            return Ok(None);
        }
        attempts += 1;
        backoff(attempts);
    }
}

/// Report that every accepted send has released its send resource.
///
/// Immediate, and not as a convenience: a buffered send copies into the attached buffer inside the
/// call, so by the time it returns there is no caller-owned storage left outstanding for a flush
/// to release. What the attached buffer's *drain* then does is MPI's business and no caller can
/// act on it — which is also why this backend cannot report capacity pressure as `Full`: MPI offers
/// no query for the free space in an attached buffer, so the only bound this backend can state is
/// the one `send` checks, a frame no larger than the buffer itself.
pub fn flush(_cx: &mut Context, _wait: Wait) -> Result<(), Error> {
    Ok(())
}

/// A blocking wait that polls both a wire and a window, backing off between attempts.
///
/// Two sources have no single blocking wait, so a backend whose lanes are *not* on the wire cannot
/// block on the wire alone: a peer sending only lane traffic would never wake it. This is that
/// backend's device for a blocking wait, and it is a poll loop with a bounded backoff rather than
/// a scheduler.
#[cfg(feature = "ring")]
pub(crate) fn backoff(attempts: u32) {
    if attempts < 64 {
        std::thread::yield_now();
        return;
    }
    let micros = 1u64 << (attempts / 64).min(12);
    std::thread::sleep(std::time::Duration::from_micros(micros));
}
