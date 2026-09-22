// Family B, reliable member: bounded ownership transfer.
//
// The log-block pattern, as a primitive:
//
//   producer owns free block -> producer fills -> publishes full block
//                                                 |
//   producer regains free block <- consumer gives back <- consumer owns full block
//
// One producer, one consumer, both handles neither `Clone` nor `Sync`. Payloads *move*: while
// the consumer owns a block the producer has no name for it, so "no simultaneous producer
// mutation and consumer read" is a type error rather than a convention, and there is no unsafe
// code here.
//
// Nothing is ever dropped silently. A payload the queue will not take comes back to its caller
// with the reason it came back, and the caller decides — trace drops it and counts it, a control
// message fails explicitly. Capacity is fixed at construction: `stocked` also builds the blocks,
// so steady-state production allocates nothing.
//
// Neither side waits for the other. `send`, `recv`, `spare` and `give` take the metadata lock
// only if it is free this instant and otherwise report `Busy`, payload in hand. That is what
// "nonblocking" has to mean here: the peer may be descheduled while holding that lock, and a
// producer parked behind it has waited on a consumer it was supposed to be decoupled from. The
// two diagnostics, `pending` and `spares`, do wait for the lock, and say so; nothing on a
// working path calls them.
//
// Three distinctions the caller needs, and which this module therefore keeps apart:
//
//   * `Full`/`Empty` is capacity. Retry once the peer has done its half.
//   * `Busy` is one contended instant. Retry immediately; nothing is implied about capacity.
//   * `Closed` is permanent. The peer handle is gone, so a retry loop must stop on it rather
//     than spin against an outcome that will never change.
//
// What acceptance means, exactly. A `send` that returns `Ok` has put the payload in a queue a
// consumer still had a handle to. It is not delivery, and it is not a promise that the consumer
// will ever look: a consumer that is dropped with payloads queued takes them with it, and this
// module makes no claim otherwise. The guarantee is narrower and is the useful one — the queue
// never drops a payload *instead of* telling its sender, and it never overwrites one it has
// accepted. Information that must survive a peer's disappearance needs an acknowledgement above
// this primitive, which is the caller's protocol and not a queue's business.
//
// What it costs: one metadata lock and unlock per accepted send, receive, spare or give, and one
// relaxed-free atomic load to notice a departed peer.
//
// A note from the first caller (NERVE's log blocks): with
// `stocked(depth, ..)` the pool *is* the queue's bound, because the payloads in flight, in the
// pool, and in the producer's hands are the same `depth` objects. Such a caller never sees
// `Full` on `send` or on `give` — it sees `spare` come back `Empty` instead, and that is where
// its backpressure decision belongs. `Full` remains meaningful for a caller that sends payloads
// it did not get from `spare`.

use std::cell::Cell as NotSync;
use std::collections::VecDeque;
use std::marker::PhantomData;

use super::rt::{Arc, AtomicBool, Mutex, SeqCst};
use super::{if_free, metadata};

/// Why the queue did not take a payload. It is handed back with this.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The far side is already `depth` deep. Retry when the peer has taken one.
    Full,
    /// The peer held the metadata at that instant, and this side does not wait for it. Retry at
    /// once: nothing is implied about capacity.
    Busy,
    /// The peer handle is gone. Every later attempt is refused the same way, so a retry loop
    /// stops here.
    Closed,
}

/// A payload the queue did not accept, and why. It is returned intact, exactly as it was handed
/// in: nothing here drops a caller's value on its behalf.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Unsent<T> {
    /// Which of the three refusals this is, and therefore what a retry is worth.
    pub why: Refused,
    /// The payload, back in the caller's hands.
    pub value: T,
}

impl<T> Unsent<T> {
    /// The payload alone, for a caller that has already decided what to do about `why`.
    pub fn into_inner(self) -> T {
        self.value
    }
}

/// Why nothing was taken. `Empty` and `Closed` are deliberately different answers: a consumer
/// that must finish what was sent drains until `Closed`, and one that is merely idle comes back.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Idle {
    /// Nothing there at that instant. The peer still exists and may put something there.
    Empty,
    /// The peer held the metadata at that instant. Retry at once.
    Busy,
    /// The peer handle is gone *and* what it left has been taken. Nothing more can arrive.
    Closed,
}

/// A queue of at most `depth` payloads, with room to keep `depth` emptied blocks for reuse.
pub fn handoff<T>(depth: usize) -> (Sender<T>, Receiver<T>) {
    assert!(depth >= 1, "a handoff needs room for at least one payload");
    let shared = Arc::new(Shared {
        yard: Mutex::new(Yard {
            full: VecDeque::with_capacity(depth),
            free: Vec::with_capacity(depth),
        }),
        sending: AtomicBool::new(true),
        receiving: AtomicBool::new(true),
    });
    let sender = Sender {
        shared: Arc::clone(&shared),
        depth,
        alone: PhantomData,
    };
    let receiver = Receiver {
        shared,
        depth,
        alone: PhantomData,
    };
    (sender, receiver)
}

/// As `handoff`, with `depth` blocks built up front for the producer to fill. Reserving capacity
/// before steady-state production is the point.
pub fn stocked<T>(depth: usize, mut init: impl FnMut() -> T) -> (Sender<T>, Receiver<T>) {
    let (sender, receiver) = handoff(depth);
    // Construction: both handles are still here and no peer can be holding this.
    let mut yard = metadata(&sender.shared.yard);
    for _ in 0..depth {
        yard.free.push(init());
    }
    drop(yard);
    (sender, receiver)
}

struct Shared<T> {
    yard: Mutex<Yard<T>>,
    /// Cleared when the producer handle is dropped, so the consumer can tell "nothing yet" from
    /// "nothing ever again" without holding a lock to find out.
    sending: AtomicBool,
    /// Cleared when the consumer handle is dropped.
    receiving: AtomicBool,
}

struct Yard<T> {
    /// Published payloads, oldest first.
    full: VecDeque<T>,
    /// Emptied blocks the producer may fill again.
    free: Vec<T>,
}

/// The one producer.
pub struct Sender<T> {
    shared: Arc<Shared<T>>,
    depth: usize,
    alone: PhantomData<NotSync<()>>,
}

impl<T> Sender<T> {
    /// Hand `value` to the consumer, waiting for nothing.
    ///
    /// `Ok` means the payload is queued for a consumer that existed when it was queued. That is
    /// acceptance, not delivery: see the module note. `Err` gives the payload straight back with
    /// the reason — `Full` when the queue already holds `depth`, `Busy` when the consumer held
    /// the metadata at that instant, `Closed` when the consumer handle is gone. Nothing already
    /// accepted is disturbed by a refusal, and nothing is dropped here.
    pub fn send(&mut self, value: T) -> Result<(), Unsent<T>> {
        // Checked first, so a consumer that is already gone is reported rather than queued
        // behind. A consumer that goes away after this point takes the payload with it, which
        // is the documented limit of what acceptance promises.
        if !self.shared.receiving.load(SeqCst) {
            return Err(Unsent {
                why: Refused::Closed,
                value,
            });
        }
        let Some(mut yard) = if_free(&self.shared.yard) else {
            return Err(Unsent {
                why: Refused::Busy,
                value,
            });
        };
        if yard.full.len() == self.depth {
            return Err(Unsent {
                why: Refused::Full,
                value,
            });
        }
        yard.full.push_back(value);
        Ok(())
    }

    /// A block the consumer gave back, if one is there. Waits for nothing.
    ///
    /// `Err(Idle::Empty)` means fill new storage or refuse, `Err(Idle::Busy)` means the consumer
    /// held the metadata and this is worth retrying, and `Err(Idle::Closed)` means the consumer
    /// handle is gone and the pool it would have refilled is drained.
    pub fn spare(&mut self) -> Result<T, Idle> {
        // Loaded before the pool is looked at: a consumer that gives a block back and then drops
        // does so in that order, so a pool found empty after this load is empty for good.
        let live = self.shared.receiving.load(SeqCst);
        let Some(mut yard) = if_free(&self.shared.yard) else {
            return Err(Idle::Busy);
        };
        match yard.free.pop() {
            Some(block) => Ok(block),
            None if live => Err(Idle::Empty),
            None => Err(Idle::Closed),
        }
    }

    /// Whether the consumer handle still exists. One atomic load, no lock.
    pub fn taking(&self) -> bool {
        self.shared.receiving.load(SeqCst)
    }

    /// Payloads the consumer has not taken yet. Diagnostics only, and this one waits for the
    /// metadata lock: never a settling or safety signal.
    pub fn pending(&self) -> usize {
        metadata(&self.shared.yard).full.len()
    }

    /// Hold the metadata lock, as a producer descheduled mid-call would. Tests only: it is how
    /// the consumer's contention answer is made deterministic instead of raced for.
    #[cfg(all(test, not(loom)))]
    pub(crate) fn hold(&self) -> impl Drop + '_ {
        metadata(&self.shared.yard)
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        // One store, no lock: neither side waits on the other's departure either. Nothing parks
        // on this queue, so there is nobody to wake — a polling consumer learns it on its next
        // `recv`, once it has taken what was already sent.
        self.shared.sending.store(false, SeqCst);
    }
}

/// The one consumer.
pub struct Receiver<T> {
    shared: Arc<Shared<T>>,
    depth: usize,
    alone: PhantomData<NotSync<()>>,
}

impl<T> Receiver<T> {
    /// Take the oldest payload, if there is one. Ownership moves to the caller, and this waits
    /// for nothing.
    ///
    /// `Err(Idle::Empty)` means nothing is queued right now, `Err(Idle::Busy)` means the
    /// producer held the metadata at that instant, and `Err(Idle::Closed)` means the producer
    /// handle is gone and everything it sent has already been taken. A consumer that must finish
    /// the stream drains until `Closed`; `Empty` is not that answer.
    pub fn recv(&mut self) -> Result<T, Idle> {
        // Loaded before the queue is looked at. A producer sends and then drops in that order,
        // so a queue found empty after this load can gain nothing more: the reverse order could
        // report `Closed` with a payload still queued behind it.
        let live = self.shared.sending.load(SeqCst);
        let Some(mut yard) = if_free(&self.shared.yard) else {
            return Err(Idle::Busy);
        };
        match yard.full.pop_front() {
            Some(value) => Ok(value),
            None if live => Err(Idle::Empty),
            None => Err(Idle::Closed),
        }
    }

    /// Give emptied storage back for the producer to fill again. Waits for nothing.
    ///
    /// `Err` hands the block back rather than dropping it: `Full` when the pool is already
    /// `depth` deep, `Busy` when the producer held the metadata at that instant, `Closed` when
    /// the producer handle is gone and nothing will ever fill this block again.
    pub fn give(&mut self, value: T) -> Result<(), Unsent<T>> {
        if !self.shared.sending.load(SeqCst) {
            return Err(Unsent {
                why: Refused::Closed,
                value,
            });
        }
        let Some(mut yard) = if_free(&self.shared.yard) else {
            return Err(Unsent {
                why: Refused::Busy,
                value,
            });
        };
        if yard.free.len() == self.depth {
            return Err(Unsent {
                why: Refused::Full,
                value,
            });
        }
        yard.free.push(value);
        Ok(())
    }

    /// Whether the producer handle still exists. One atomic load, no lock.
    pub fn sending(&self) -> bool {
        self.shared.sending.load(SeqCst)
    }

    /// Blocks waiting to be filled again. Diagnostics only, and waits for the metadata lock, as
    /// `pending` does.
    pub fn spares(&self) -> usize {
        metadata(&self.shared.yard).free.len()
    }

    /// Hold the metadata lock, as a consumer descheduled mid-call would. Tests only: it is how
    /// the producer's contention answer is made deterministic instead of raced for.
    #[cfg(all(test, not(loom)))]
    pub(crate) fn hold(&self) -> impl Drop + '_ {
        metadata(&self.shared.yard)
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        // As the producer's drop. Payloads still queued are dropped with the shared state once
        // both handles are gone; the producer is told `Closed` for everything it sends after
        // this store, and was never promised more than that.
        self.shared.receiving.store(false, SeqCst);
    }
}
