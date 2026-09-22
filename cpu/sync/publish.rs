// Family B, sampled member: a cheap writer and a waiting reader.
//
// The shape asked for is "one side writes freely, the other reads coherent versions". The honest
// realization on CPU, and the one built here, is an ownership transfer rather than a shared
// buffer that both sides touch:
//
//   * The writer drafts into storage it *owns*, through an ordinary `&mut T`. No atomic, no
//     fence, no lock per field. That part is genuinely a plain store, and is the only part of
//     this module that is free.
//   * Publication moves the drafted buffer into the shared slot under a metadata lock and takes
//     a different buffer back to draft into. One lock acquisition per version, not per field.
//   * The reader *takes* the published buffer out of the slot and keeps it until it takes the
//     next one. While the reader holds a buffer, the writer cannot name it, let alone recycle it:
//     it is not a pin count that a race could get wrong, it is Rust ownership. There is no
//     unsafe code in this file and no way to express illegal reuse.
//
// Because the buffers are moved rather than copied, a fresh draft holds some older version's
// value, not the one just published. The producer keeps its cumulative state privately and fills
// a complete snapshot before each publication; nothing here promises cumulative continuity.
//
// Storage is fixed at construction, and the writer never waits on the reader — not for a buffer
// and not for the metadata lock either. A publication that cannot go through reports which kind
// of pressure stopped it and keeps its draft:
//
//   * `Pressure::Full` is capacity: every buffer is spoken for. With `slots >= 3` and one reader
//     this cannot happen — the writer holds one, the reader at most one, and the third is always
//     either in the slot or free. That is what three buffers buy, and it is all they buy.
//   * `Pressure::Busy` is contention: the reader held the short metadata lock at that instant.
//     More buffers do not prevent it and it is not a statement about capacity. The writer is
//     told to come back rather than parked behind a reader that may be descheduled, which is
//     what "the writer never waits" has to mean if it is to mean anything.
//
// The reader may wait: `after` parks until a version arrives, the writer goes away, or the wait
// is cancelled. The asymmetry is the point of this member.

use std::cell::Cell as NotSync;
use std::marker::PhantomData;
use std::ops::Deref;

use super::rt::{Arc, Mutex, Thread, thread};
use super::{Cancel, if_free, metadata};

/// Buffers that make `Pressure::Full` unreachable for one reader, and the number to pass unless
/// the caller has measured a reason for more. A generic default: this module does not know how
/// many versions an application wants in flight.
pub const SLOTS: usize = 3;

/// A committed version number. It advances only on a successful publication, so a reader that
/// compares the version it holds with the one before it sees exactly how many committed versions
/// it skipped. Failed attempts are not versions and are counted separately by the writer.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Version(u64);

impl Version {
    /// Before the first publication.
    pub const NONE: Version = Version(0);

    /// How many versions have been committed up to and including this one.
    pub const fn count(self) -> u64 {
        self.0
    }
}

/// Why a publication was not accepted. The draft, its contents and the committed version are
/// untouched in either case, and the two are counted apart by the writer.
///
/// Both are transient and neither is an error: a publication is a sample, and the next one
/// carries the same cumulative state. A caller that needs every version uses `handoff`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Pressure {
    /// No buffer was free: the reader holds one and the writer holds its draft. Capacity, and
    /// unreachable with `SLOTS` buffers and one reader.
    Full,
    /// The reader held the metadata lock at that instant. Contention, not capacity: nothing is
    /// implied about the buffers, and the next attempt may well be accepted.
    Busy,
}

/// Why a wait ended with no version.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Ended {
    /// The cancel token was raised.
    Cancelled,
    /// The writer is gone and no further version can arrive.
    Closed,
}

/// Split handles, so "writer-private" is enforced by the type system rather than by a comment.
/// `slots` buffers are built here and never allocated again; `slots >= 2` is required and
/// `SLOTS` is the number that never reports `Full`.
pub fn published<T>(slots: usize, mut init: impl FnMut() -> T) -> (Writer<T>, Reader<T>) {
    assert!(slots >= 2, "a publication needs at least two buffers");
    let mut free = Vec::with_capacity(slots);
    // One of the `slots` buffers leaves as the writer's first draft.
    for _ in 1..slots {
        free.push(init());
    }
    let shared = Arc::new(Shared {
        cell: Mutex::new(Cell {
            slot: None,
            version: 0,
            free,
            parked: None,
            closed: false,
        }),
    });
    let writer = Writer {
        shared: Arc::clone(&shared),
        draft: init(),
        committed: 0,
        refused: 0,
        busy: 0,
        overwritten: 0,
        alone: PhantomData,
    };
    let reader = Reader {
        shared,
        held: None,
        version: Version::NONE,
        alone: PhantomData,
    };
    (writer, reader)
}

struct Shared<T> {
    cell: Mutex<Cell<T>>,
}

/// Everything both sides touch. Each critical section is a handful of moves with no caller code
/// in it, which bounds how long a peer can be found holding it — but does not make it free, so
/// the writer refuses rather than waits when it finds it held.
struct Cell<T> {
    /// The committed version, until the reader takes it.
    slot: Option<T>,
    /// Number of the last committed version, whether or not it is still in the slot.
    version: u64,
    /// Buffers owned by neither side.
    free: Vec<T>,
    /// The reader's thread while it waits for a version.
    parked: Option<Thread>,
    /// Set when the writer is dropped.
    closed: bool,
}

/// The one writer. Neither `Clone` nor `Sync`: there is exactly one.
pub struct Writer<T> {
    shared: Arc<Shared<T>>,
    draft: T,
    /// The last version this writer committed. Its own copy, so reporting it needs no lock.
    committed: u64,
    refused: u64,
    busy: u64,
    overwritten: u64,
    alone: PhantomData<NotSync<()>>,
}

impl<T> Writer<T> {
    /// Ordinary exclusive access to private storage. No synchronization per field update.
    pub fn draft(&mut self) -> &mut T {
        &mut self.draft
    }

    /// The one synchronized point: commit the draft as the next version and take a fresh buffer
    /// to draft into. This call never waits, for a buffer or for the reader's bookkeeping.
    ///
    /// `Err(Pressure::Full)` when no buffer is free and `Err(Pressure::Busy)` when the reader
    /// held the metadata at that instant. In both cases the draft, its contents and the
    /// committed version are exactly as they were, and the attempt is counted — by `refused`
    /// and `busy` respectively, which are different questions and are not added together here.
    /// A caller that must eventually publish retries; one that samples may skip this round.
    ///
    /// # Panics
    ///
    /// When the version counter is exhausted. That is checked before any buffer moves, so a
    /// publication that cannot number itself has no effect at all rather than half of one.
    pub fn publish(&mut self) -> Result<Version, Pressure> {
        let Some(mut cell) = if_free(&self.shared.cell) else {
            self.busy = self.busy.saturating_add(1);
            return Err(Pressure::Busy);
        };
        // A version number is an identity a reader subtracts to learn what it skipped, so it
        // must not wrap. Checked first: taking the committed slot and then failing would lose a
        // version the reader could still have had.
        let Some(next) = cell.version.checked_add(1) else {
            drop(cell);
            panic!("publication: the version counter is exhausted");
        };
        // A version still sitting in the slot was never taken: recycle it and count the skip.
        let missed = cell.slot.is_some();
        let Some(spare) = cell.slot.take().or_else(|| cell.free.pop()) else {
            drop(cell);
            self.refused = self.refused.saturating_add(1);
            return Err(Pressure::Full);
        };
        cell.version = next;
        cell.slot = Some(std::mem::replace(&mut self.draft, spare));
        let waiting = cell.parked.take();
        drop(cell);
        self.committed = next;
        if missed {
            self.overwritten = self.overwritten.saturating_add(1);
        }
        if let Some(reader) = waiting {
            reader.unpark();
        }
        Ok(Version(next))
    }

    /// Publications refused for want of a buffer. Not versions: nothing was committed.
    ///
    /// This and the two below are telemetry rather than identities, so they saturate at
    /// `u64::MAX` instead of wrapping or panicking: an observation that stops counting is better
    /// than a working writer that stops publishing.
    pub fn refused(&self) -> u64 {
        self.refused
    }

    /// Publications that found the reader holding the metadata. Not capacity, and not versions.
    pub fn busy(&self) -> u64 {
        self.busy
    }

    /// Committed versions the reader never took, because a later one replaced them.
    pub fn overwritten(&self) -> u64 {
        self.overwritten
    }

    /// The last version this writer committed, or `Version::NONE`. Reads the writer's own copy,
    /// so it takes no lock and cannot be delayed by the reader.
    pub fn version(&self) -> Version {
        Version(self.committed)
    }
}

impl<T> Drop for Writer<T> {
    /// Teardown, not a working path: this one waits for the metadata lock, because a reader
    /// parked for a version that can no longer arrive has to be told, and a report that a
    /// contended instant could lose would not be a report.
    fn drop(&mut self) {
        let mut cell = metadata(&self.shared.cell);
        cell.closed = true;
        let waiting = cell.parked.take();
        drop(cell);
        // A reader waiting for a version that can no longer arrive is woken to be told so.
        if let Some(reader) = waiting {
            reader.unpark();
        }
    }
}

/// The one reader. Neither `Clone` nor `Sync`.
pub struct Reader<T> {
    shared: Arc<Shared<T>>,
    /// The version this reader owns outright. The writer cannot reach it.
    held: Option<T>,
    version: Version,
    alone: PhantomData<NotSync<()>>,
}

/// What one look at the cell produced.
enum Step {
    Took,
    Wait,
    Closed,
}

impl<T> Reader<T> {
    /// The latest committed version, or `None` before the first publication.
    pub fn latest(&mut self) -> Option<Pinned<'_, T>> {
        self.step(self.version);
        self.pinned()
    }

    /// Block until a version after `seen` is committed, the writer is gone, or `cancel` is
    /// raised. A version this reader already holds counts: `after(Version::NONE, ..)` returns
    /// immediately once anything has been published.
    ///
    /// # Panics
    ///
    /// When more callers wait on `cancel` than its table holds.
    pub fn after(&mut self, seen: Version, cancel: &Cancel) -> Result<Pinned<'_, T>, Ended> {
        // Registered before the first check, so a raise between the two cannot be missed.
        let watching = cancel.watch();
        loop {
            match self.step(seen) {
                Step::Took => break,
                Step::Closed => {
                    drop(watching);
                    return Err(Ended::Closed);
                }
                Step::Wait => {}
            }
            if cancel.raised() {
                drop(watching);
                return Err(Ended::Cancelled);
            }
            thread::park();
        }
        drop(watching);
        Ok(self
            .pinned()
            .expect("a version newer than `seen` was just taken"))
    }

    /// The version this reader holds, or `Version::NONE`.
    pub fn version(&self) -> Version {
        self.version
    }

    /// Hold the metadata lock, as a reader descheduled inside `step` would. Tests only: it is
    /// how the writer's contention answer is made deterministic instead of raced for.
    #[cfg(all(test, not(loom)))]
    pub(crate) fn hold(&self) -> impl Drop + '_ {
        metadata(&self.shared.cell)
    }

    /// Take the committed version if it is newer than `seen`, otherwise register for a wake-up.
    /// Registration happens under the same lock a publication takes, so a version committed
    /// after this returns `Wait` always unparks this reader.
    ///
    /// This waits for the metadata lock. The reader is the side that may wait, and a writer
    /// holds this lock only across a handful of moves.
    fn step(&mut self, seen: Version) -> Step {
        let mut cell = metadata(&self.shared.cell);
        if cell.version > seen.0 {
            if let Some(value) = cell.slot.take() {
                // The buffer this reader was holding goes back for the writer to draft into.
                if let Some(old) = self.held.replace(value) {
                    cell.free.push(old);
                }
                self.version = Version(cell.version);
                return Step::Took;
            }
            // The newer version is the one already in hand: it was taken by an earlier call.
            if self.version > seen {
                return Step::Took;
            }
        }
        if cell.closed {
            return Step::Closed;
        }
        cell.parked = Some(thread::current());
        Step::Wait
    }

    fn pinned(&self) -> Option<Pinned<'_, T>> {
        let version = self.version;
        self.held.as_ref().map(|value| Pinned { value, version })
    }
}

impl<T> Drop for Reader<T> {
    fn drop(&mut self) {
        let Some(held) = self.held.take() else { return };
        // Hand the buffer back, so a writer that outlives its reader keeps publishing. Teardown
        // waits for the lock: a buffer that failed to come back would be lost for good.
        metadata(&self.shared.cell).free.push(held);
    }
}

/// A committed version, held by the reader for as long as this exists. The writer cannot recycle
/// the storage behind it, because the reader owns it rather than borrowing it.
pub struct Pinned<'a, T> {
    value: &'a T,
    version: Version,
}

impl<T> Pinned<'_, T> {
    /// Which committed version this is.
    pub fn version(&self) -> Version {
        self.version
    }
}

impl<T> Deref for Pinned<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value
    }
}
