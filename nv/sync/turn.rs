//! Family A on a device: one mutable owner at a time, handed on in registration order.
//!
//! A rank here is a warp and its lanes move together, so the primitive a worker needs is not a
//! lock between lanes — that is `warp::sync`, and it is free. It is a turn between *warps*: two
//! logical threads of one participant, each running on its own warp, both wanting the same state.
//!
//! # Why this is not `cpu::sync::turn`
//!
//! The host family decides who is next under a `std::sync::Mutex` and hands the turn on by
//! unparking a named waiter. Neither exists here: there is no OS, no thread handle, and nothing
//! may park. So the ordering has to be carried by the memory itself, and the shape changes from
//! "one metadata lock plus a waiter table" to "a bounded slot array plus one file of state".
//! What does *not* change is the semantics, and that is the whole point of declaring a family
//! rather than a function: a caller that moves from `cpu::sync::turn` to this one should be
//! changing an import, not a proof.
//!
//! # The algorithm
//!
//! A caller that wants the turn **claims a slot** (one compare-exchange, no lock), and **registers
//! a sequence number** (one `fetch_add`). The turn itself is a single word, `holder`, naming the
//! slot that owns it. Registration order is the sequence number, so among equals the lowest
//! outstanding sequence gets the turn, which is FIFO.
//!
//! Every *decision* about who holds the turn — starting it when it is free, and handing it on at
//! release — is taken under one spinlock. The first design tried had no such lock, and it loses a
//! wake-up in a way that is worth writing down because it is invisible in testing: a releaser that
//! scanned before the next claimant published its sequence would find nobody and record `NONE`,
//! and a claimant that had already found `NONE` and taken the turn itself would have its grant
//! overwritten a moment later. The claimant then waits for a turn that has already been given
//! away. Serialising just the decision removes the window, and the lock is held for a few
//! instructions and never across a wait, so it cannot invert with the turn.
//!
//! # Cancellation, and why a ticket is not enough
//!
//! Acquisition supports polling, a declared wait, and explicit cancellation. A plain FIFO ticket
//! cannot: a caller that draws ticket 7 and then gives up leaves the counter unable to reach 7,
//! so every later caller spins forever. Withdrawal has to be *representable*, which is what the
//! slot array buys — a slot can be released, and the next decision simply does not see it.
//!
//! Cancelling is checked only while the caller does not hold the turn, so a cancellation that
//! loses the race to a grant is not an error: the caller owns the turn, and owns the obligation
//! to pass it on.
//!
//! # Bounds
//!
//! Storage is fixed and allocated by whoever owns the memory: two words of state plus
//! [`WAITERS`] slots of two words each, and the value itself. Nothing allocates, nothing grows.
//! Registration beyond [`WAITERS`] is refused as [`Refused::Full`] rather than waited for, which
//! is the difference between capacity and contention that the families keep separate.

use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::Ordering;

use super::atomic::{AtomicBool, AtomicU32};

/// Callers that may be registered at once. A bounded table, and the bound is the point: a
/// participant knows how many logical threads it runs, and a primitive that silently grew would
/// turn a design mistake into a slowdown instead of a refusal.
pub const WAITERS: usize = 4;

/// Words of storage a header needs: three of state, then a slot each.
pub const WORDS: usize = 3 + WAITERS * 2;

/// The only priority.
///
/// A type with one inhabitant rather than a number, because this backend declares that it
/// supports equal priorities only: a caller cannot write a non-default priority because the
/// surface has no way to say one. That is a compile-time refusal, which is what the family
/// requires, rather than a run-time rejection of a value that should never have been writable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Priority;

/// The default priority, and for this backend the only one.
pub const NORMAL: Priority = Priority;

/// A wait that ended without the turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Every slot was registered when the attempt arrived. Capacity, not contention.
    Full,
    /// The declared number of attempts ran out. Contention, and not the same as `Full`.
    Exhausted { attempts: u32 },
    /// Someone raised the token this attempt waited against.
    Cancelled,
}

const FREE: u32 = 0;
const TAKEN: u32 = 1;
const NONE: u32 = u32::MAX;

/// One registration: whether it is claimed, and its place in the queue.
#[repr(C)]
struct Slot {
    state: AtomicU32,
    seq: AtomicU32,
}

/// The state one participant's turn is built on. Lives in memory the participant owns, which on a
/// device means memory the launch owns and every warp of it can see.
#[repr(C)]
struct Header {
    /// Next registration number. Monotone, so a sequence is never reused.
    next_seq: AtomicU32,
    /// The slot holding the turn, or `NONE`.
    holder: AtomicU32,
    /// Serialises the decision of who holds the turn. Never held across a wait.
    grant: AtomicU32,
    slots: [Slot; WAITERS],
}

/// Write the initial state of a turn into a host-side image of its words.
///
/// This runs where the memory is *prepared*, not where it is used: a caller builds the words,
/// hands them to the transport that will carry them, and the warps find a header that already
/// says nobody holds the turn and no slot is taken. It is a free function rather than a method
/// because the header type is not something a caller should be able to name — the words are.
pub fn init_header(words: &mut [u32]) {
    assert!(words.len() >= WORDS, "a header needs {WORDS} words");
    words[0] = 0; // next_seq
    words[1] = NONE; // holder: free, which is not the same as slot 0
    words[2] = FREE; // grant
    for slot in 0..WAITERS {
        words[3 + slot * 2] = FREE;
        words[4 + slot * 2] = NONE;
    }
}

/// Ask present and future waits against this token to stop.
///
/// Level-triggered and permanent, for the reason the host family gives: a token that could be
/// lowered turns a missed edge into a lost wake-up.
pub struct Cancel {
    raised: AtomicBool,
}

impl Default for Cancel {
    fn default() -> Self {
        Cancel {
            raised: AtomicBool::new(false),
        }
    }
}

impl Cancel {
    pub fn raise(&self) {
        self.raised.store(true, Ordering::Release);
    }

    pub fn raised(&self) -> bool {
        self.raised.load(Ordering::Acquire)
    }
}

/// A value exactly one warp may mutate at a time.
///
/// Built over two addresses the caller owns: the header words, and the value. Nothing is
/// allocated and nothing is owned, which is what lets this same type sit over host memory in the
/// model and over device memory in a launch.
pub struct Exclusive<T> {
    header: *mut Header,
    value: *mut T,
}

// SAFETY: the header's atomics serialise every access to `value`, and a `Turn` is the only way to
// reach it. Sharing across the warps of one participant therefore moves `T` between them, which
// needs `T: Send` exactly as a host lock does. `T` is never handed out as `&T` to two owners.
unsafe impl<T: Send> Sync for Exclusive<T> {}
unsafe impl<T: Send> Send for Exclusive<T> {}

impl<T> Exclusive<T> {
    /// Name the memory this turn is built on.
    ///
    /// # Safety
    ///
    /// `words` must point to at least [`WORDS`] words initialised by [`Header::init`] and must
    /// outlive every `Turn`. `value` must point to a valid `T`. Both must be visible to every
    /// warp that will call these methods, which on a device means one allocation of the launch.
    pub unsafe fn new(words: *mut u32, value: *mut T) -> Self {
        Exclusive {
            header: words.cast::<Header>(),
            value,
        }
    }

    fn header(&self) -> &Header {
        // SAFETY: `new` requires the pointer to be valid for the lifetime of every `Turn`, and a
        // `Turn` cannot outlive `&self`.
        unsafe { &*self.header }
    }

    fn slots(&self) -> &[Slot; WAITERS] {
        &self.header().slots
    }

    fn spin_lock(&self, cell: &AtomicU32) {
        while cell
            .compare_exchange(FREE, TAKEN, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            super::spin();
        }
    }

    fn unlock(&self, cell: &AtomicU32) {
        cell.store(FREE, Ordering::Release);
    }

    /// Claim a registration slot. `None` is capacity, and it is reported rather than waited for.
    fn claim(&self) -> Option<usize> {
        let slots = self.slots();
        (0..WAITERS).find(|&i| {
            slots[i]
                .state
                .compare_exchange(FREE, TAKEN, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        })
    }

    /// The slot that should hold the turn after `leaving`: lowest outstanding sequence, which
    /// among equals is the earliest registration.
    ///
    /// A slot that is taken but has not published its sequence yet is passed over, and that is
    /// deliberate: registration publishes the sequence *before* the decision lock is taken, so
    /// such a slot either is seen by a later scan or takes a free turn itself. Skipping it cannot
    /// strand it, and reading a stale sequence could hand the turn to the wrong caller.
    fn next_holder(&self, leaving: usize) -> u32 {
        let slots = self.slots();
        let mut best = NONE;
        let mut best_seq = u32::MAX;
        for (slot, entry) in slots.iter().enumerate() {
            if slot == leaving || entry.state.load(Ordering::Acquire) != TAKEN {
                continue;
            }
            let seq = entry.seq.load(Ordering::Acquire);
            if seq != NONE && seq < best_seq {
                best_seq = seq;
                best = slot as u32;
            }
        }
        best
    }

    /// Register and take the turn, or say why not.
    fn acquire(&self, attempts: u32, cancel: Option<&Cancel>) -> Result<Turn<'_, T>, Refused> {
        let header = self.header();
        let Some(slot) = self.claim() else {
            return Err(Refused::Full);
        };
        let seq = header.next_seq.fetch_add(1, Ordering::Relaxed);
        // Published before the decision, so a releaser that scans after this point sees us.
        self.slots()[slot].seq.store(seq, Ordering::Release);

        self.spin_lock(&header.grant);
        if header.holder.load(Ordering::Relaxed) == NONE {
            header.holder.store(slot as u32, Ordering::Relaxed);
        }
        self.unlock(&header.grant);

        let mut spent = 0u32;
        loop {
            if header.holder.load(Ordering::Acquire) == slot as u32 {
                return Ok(Turn {
                    exclusive: self,
                    slot,
                    marker: PhantomData,
                });
            }
            if let Some(token) = cancel {
                if token.raised() {
                    self.withdraw(slot);
                    return Err(Refused::Cancelled);
                }
            }
            if attempts > 0 && spent >= attempts {
                self.withdraw(slot);
                return Err(Refused::Exhausted { attempts });
            }
            spent += 1;
            super::spin();
        }
    }

    /// Give up a registration, handing the turn on if it had already been granted.
    ///
    /// The second case is the one that matters: a caller that stopped waiting a moment too late
    /// owns the turn, and a withdrawal that merely cleared its slot would lose that turn for
    /// everyone behind it.
    fn withdraw(&self, slot: usize) {
        let header = self.header();
        self.spin_lock(&header.grant);
        self.slots()[slot].state.store(FREE, Ordering::Release);
        if header.holder.load(Ordering::Relaxed) == slot as u32 {
            let next = self.next_holder(slot);
            header.holder.store(next, Ordering::Release);
        }
        self.unlock(&header.grant);
    }

    /// Take the turn, waiting until it comes.
    ///
    /// The unrestricted form, and the one a portable call site writes: the host family spells it
    /// the same way, and a caller that needs the turn but not a *bound* has no reason to name one.
    ///
    /// It panics when every seat is taken, which is the same answer the host family gives under the
    /// same name — a waiter table that overflows there is a panic rather than growth. A participant
    /// knows how many logical threads it runs, so exhausting the table is a design error, and the
    /// two families refuse it the same way rather than one reporting and the other aborting.
    pub fn lock(&self) -> Turn<'_, T> {
        match self.acquire(0, None) {
            Ok(turn) => turn,
            Err(refused) => panic!("the turn's waiter table is full: {refused:?}"),
        }
    }

    /// Take the turn, waiting at most `attempts` checks. `attempts` of zero waits until it comes.
    pub fn take(&self, _at: Priority, attempts: u32) -> Result<Turn<'_, T>, Refused> {
        self.acquire(attempts, None)
    }

    /// Take the turn, stopping as soon as `cancel` is raised.
    ///
    /// Cancellation is checked only while this caller does not hold the turn, so a grant that
    /// wins the race is not unmade.
    pub fn take_until(
        &self,
        _at: Priority,
        attempts: u32,
        cancel: &Cancel,
    ) -> Result<Turn<'_, T>, Refused> {
        self.acquire(attempts, Some(cancel))
    }

    /// Registered waiters. The holder is not one of them, and it is not counted.
    ///
    /// A diagnostic, and deliberately the *same* diagnostic the host family answers rather than a
    /// similar one: the name is shared, so the meaning has to be. The first version of this
    /// counted every claimed slot, which put the holder in the number, and the differential
    /// experiment against the host family caught the disagreement immediately — which is the point
    /// of running one workload against both rather than reasoning that two counts must agree.
    pub fn waiters(&self) -> usize {
        let holder = self.header().holder.load(Ordering::Acquire);
        self.slots()
            .iter()
            .enumerate()
            .filter(|(slot, entry)| {
                entry.state.load(Ordering::Acquire) == TAKEN && *slot as u32 != holder
            })
            .count()
    }

    /// One attempt. `None` means the turn was not available and nothing was left registered.
    pub fn try_lock(&self) -> Option<Turn<'_, T>> {
        self.acquire(1, None).ok()
    }
}

/// The turn, held for as long as this value lives.
///
/// Release is on drop and is nonpreemptive: a holder is never interrupted and never loses the
/// turn, which is why nothing here takes a callback.
pub struct Turn<'a, T> {
    exclusive: &'a Exclusive<T>,
    slot: usize,
    marker: PhantomData<&'a mut T>,
}

impl<T> Deref for Turn<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: a `Turn` exists only while its slot is the holder, and the holder is the only
        // thing that reaches `value`.
        unsafe { &*self.exclusive.value }
    }
}

impl<T> DerefMut for Turn<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as `deref`, and `&mut self` means this is the one live reference.
        unsafe { &mut *self.exclusive.value }
    }
}

impl<T> Drop for Turn<'_, T> {
    fn drop(&mut self) {
        let header = self.exclusive.header();
        self.exclusive.spin_lock(&header.grant);
        self.exclusive.slots()[self.slot]
            .state
            .store(FREE, Ordering::Release);
        let next = self.exclusive.next_holder(self.slot);
        header.holder.store(next, Ordering::Release);
        self.exclusive.unlock(&header.grant);
    }
}
