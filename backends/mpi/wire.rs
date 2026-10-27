// The records the relays send each other (NODES.md). A record is a kind byte and its fields, all
// little-endian; a message is a run of records, and a record never spans two messages. A malformed
// message is a bug in the transport (both ends are this code), so parsing panics rather than guesses.

extern crate std;

use std::vec::Vec;

/// Largest message a relay sends or receives, in bytes. One record never exceeds it: a frame is at
/// most `MAX` bytes plus 17 bytes of header.
pub(super) const BATCH: usize = 1 << 20;

const FRAMES: u8 = 1;
const CREDIT: u8 = 2;
const ARRIVED: u8 = 3;
const RELEASE: u8 = 4;
const RESULT: u8 = 5;
const EXITED: u8 = 6;
const ABORT: u8 = 7;
const END: u8 = 8;

/// One record. Byte slices borrow from the message they were parsed from, or from the caller's buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Record<'a> {
    /// Frame `index` of route `route`, with its payload.
    Frames { route: u32, index: u64, bytes: &'a [u8] },
    /// The receiver of route `route` has consumed `c` frames.
    Credit { route: u32, c: u64 },
    /// This node's workers have all arrived at barrier generation `generation`.
    Arrived { generation: u64 },
    /// Barrier generation `generation` is released, with publication number `no` and its bytes.
    Release { generation: u64, no: u64, bytes: &'a [u8] },
    /// Worker `worker` has its result: `outcome` is its outcome word.
    Result { worker: u32, outcome: u64 },
    /// Worker `worker` has returned; `arrive` is its last barrier generation.
    Exited { worker: u32, arrive: u64 },
    /// The run aborted.
    Abort,
    /// The leader has joined: no more records follow from the root.
    End,
}

/// Append `r` to `buf`.
pub(super) fn put(buf: &mut Vec<u8>, r: &Record) {
    match *r {
        Record::Frames { route, index, bytes } => {
            buf.push(FRAMES);
            buf.extend_from_slice(&route.to_le_bytes());
            buf.extend_from_slice(&index.to_le_bytes());
            put_bytes(buf, bytes);
        }
        Record::Credit { route, c } => {
            buf.push(CREDIT);
            buf.extend_from_slice(&route.to_le_bytes());
            buf.extend_from_slice(&c.to_le_bytes());
        }
        Record::Arrived { generation } => {
            buf.push(ARRIVED);
            buf.extend_from_slice(&generation.to_le_bytes());
        }
        Record::Release { generation, no, bytes } => {
            buf.push(RELEASE);
            buf.extend_from_slice(&generation.to_le_bytes());
            buf.extend_from_slice(&no.to_le_bytes());
            put_bytes(buf, bytes);
        }
        Record::Result { worker, outcome } => {
            buf.push(RESULT);
            buf.extend_from_slice(&worker.to_le_bytes());
            buf.extend_from_slice(&outcome.to_le_bytes());
        }
        Record::Exited { worker, arrive } => {
            buf.push(EXITED);
            buf.extend_from_slice(&worker.to_le_bytes());
            buf.extend_from_slice(&arrive.to_le_bytes());
        }
        Record::Abort => buf.push(ABORT),
        Record::End => buf.push(END),
    }
}

fn put_bytes(buf: &mut Vec<u8>, bytes: &[u8]) {
    let len = u32::try_from(bytes.len()).expect("a record's bytes fit in u32");
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(bytes);
}

/// The record at the start of `buf`, and the number of bytes it takes. None when `buf` is empty.
/// Panics on a truncated record or an unknown kind.
pub(super) fn next(buf: &[u8]) -> Option<(Record<'_>, usize)> {
    let kind = *buf.first()?;
    let mut r = Reader { buf, at: 1 };
    let rec = match kind {
        FRAMES => Record::Frames { route: r.u32(), index: r.u64(), bytes: r.bytes() },
        CREDIT => Record::Credit { route: r.u32(), c: r.u64() },
        ARRIVED => Record::Arrived { generation: r.u64() },
        RELEASE => Record::Release { generation: r.u64(), no: r.u64(), bytes: r.bytes() },
        RESULT => Record::Result { worker: r.u32(), outcome: r.u64() },
        EXITED => Record::Exited { worker: r.u32(), arrive: r.u64() },
        ABORT => Record::Abort,
        END => Record::End,
        other => panic!("trame: MPI relay record of unknown kind {other}"),
    };
    Some((rec, r.at))
}

struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let end = self.at.checked_add(n).filter(|&e| e <= self.buf.len());
        let end = end.unwrap_or_else(|| panic!("trame: MPI relay record truncated at byte {}", self.at));
        let s = &self.buf[self.at..end];
        self.at = end;
        s
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().expect("four bytes"))
    }

    fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take(8).try_into().expect("eight bytes"))
    }

    fn bytes(&mut self) -> &'a [u8] {
        let n = self.u32() as usize;
        self.take(n)
    }
}

#[cfg(test)]
#[path = "wire_test.rs"]
mod tests;
