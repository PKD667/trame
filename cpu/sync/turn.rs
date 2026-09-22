// Family A: an exclusive turn, handed on by registered priority.
//
// An ordinary mutual-exclusion primitive. The one addition to `std::sync::Mutex` is that the
// order in which *waiting* callers acquire is decided here rather than by the operating system:
// highest registered priority first, earliest registered request among equals. With every caller
// at `NORMAL` that is FIFO among registered waiters. `std::sync::Mutex` promises no such thing,
// and nothing in this file should be read as a claim about it.
//
// What it costs: one metadata lock and unlock on acquisition and one on release, plus one
// `unpark` when a waiter is handed the turn. A contended acquisition pays a second pair, because
// it registers with its cancellation token before it takes a ticket. An uncontended acquisition
// allocates nothing. The waiter table is allocated once, at construction, and never grows.

use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use super::rt::{Mutex, Thread, thread};
use super::{Cancel, Cancelled, if_free, metadata};

/// The priority every production call site passes today.
pub const NORMAL: Priority = Priority(1);

/// Waiting callers `Exclusive::new` makes room for, when the caller states no number of its own.
///
/// A generic default, not a participant count. This primitive does not know how many callers an
/// application points at one resource and does not decide; the application knows, and either
/// passes that number to `Exclusive::with_waiters` or checks this default against it. Overflow
/// is refused explicitly rather than silently grown, so an unchecked assumption fails loudly.
pub const WAITERS: usize = 4;

/// Higher takes the turn first. Deliberately not `Default`: a call site states its priority,
/// even when it states `NORMAL`.
///
/// This is not the reservoir's model-time order and not an operation-domain order. It
/// decides one thing only: which already-waiting caller enters the critical section next.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Priority(pub u8);

/// A value exactly one caller may mutate at a time.
///
/// Acquisition is nonpreemptive: a holder is never interrupted and never loses its guard.
/// There is no priority inheritance, so a `NORMAL` holder can block a higher-priority waiter for
/// as long as it holds the turn, and a continuous stream of higher-priority requests can starve a
/// lower-priority waiter. Both are properties of this primitive, not defects to be found later.
pub struct Exclusive<T> {
    queue: Mutex<Queue>,
    value: UnsafeCell<T>,
}

/// Who holds the turn and who is waiting for it. Every decision below is taken under this lock,
/// which is what makes cancellation and hand-off exclusive rather than racy.
struct Queue {
    /// Whether the turn is owned, including while it is reserved for a named waiter.
    held: bool,
    /// Set when a holder released while unwinding. A poisoned resource is fatal here, by the
    /// same reasoning as the `.lock().unwrap()` the current tree is full of: it is written down
    /// rather than inherited.
    poisoned: bool,
    /// Next registration number. Ties in priority are broken by it, so it is the FIFO order.
    next: u64,
    /// The waiter the turn has been handed to, which no arriving caller may overtake.
    granted: Option<u64>,
    /// Registered waiting callers. Bounded: `limit` is the documented capacity.
    waiting: Vec<Waiting>,
    limit: usize,
}

impl Queue {
    /// Claim the turn if it is free, under an already-held metadata lock.
    ///
    /// `None` reports a poisoned resource, which the caller turns into a panic *after* dropping
    /// the lock: panicking while holding it would poison the bookkeeping of a resource that is
    /// merely reporting someone else's failure.
    ///
    /// A free turn means no registered waiter, since a caller that finds the turn held registers
    /// and a release hands the turn on rather than freeing it. So claiming it is not barging.
    fn claim(&mut self) -> Option<bool> {
        if self.poisoned {
            return None;
        }
        let free = !self.held;
        self.held |= free;
        Some(free)
    }
}

struct Waiting {
    ticket: u64,
    at: Priority,
    thread: Thread,
}

// SAFETY: `Exclusive` hands out `&mut T` only through a `Turn`, and the queue invariant is that
// at most one `Turn` exists at a time: `held` is set under the metadata lock before a guard is
// built, and cleared or handed to exactly one named waiter when a guard is dropped. Sharing it
// between threads therefore moves `T` between them and needs `T: Send`, exactly as `Mutex<T>`
// does. It is not `Sync` for `T: !Send` and never exposes `&T` to two threads at once.
unsafe impl<T: Send> Sync for Exclusive<T> {}

impl<T> Exclusive<T> {
    /// Room for `WAITERS` waiting callers.
    pub fn new(value: T) -> Self {
        Self::with_waiters(value, WAITERS)
    }

    /// Room for `waiters` waiting callers. The table is allocated here and never again.
    pub fn with_waiters(value: T, waiters: usize) -> Self {
        Exclusive {
            queue: Mutex::new(Queue {
                held: false,
                poisoned: false,
                next: 0,
                granted: None,
                waiting: Vec::with_capacity(waiters),
                limit: waiters,
            }),
            value: UnsafeCell::new(value),
        }
    }

    /// Wait for the turn at `NORMAL`. The idiomatic call.
    ///
    /// # Panics
    ///
    /// When the resource is poisoned, or when more callers wait than the table holds.
    pub fn lock(&self) -> Turn<'_, T> {
        self.take(NORMAL)
    }

    /// Wait for the turn at a stated priority.
    ///
    /// # Panics
    ///
    /// As `lock`.
    pub fn take(&self, at: Priority) -> Turn<'_, T> {
        self.acquire(at, None)
            .expect("no cancel token was given, so this wait cannot be cancelled")
    }

    /// Take the turn only if it is free now, and wait for nothing at all.
    ///
    /// Never registers, so it never changes the order in which waiting callers acquire — and,
    /// because a caller registers whenever the turn is held, a free turn means nobody is
    /// waiting. There is nothing here to barge past.
    ///
    /// `None` means either that the turn was taken or handed on, or that another caller held the
    /// bookkeeping at that instant. The two are not distinguished, because the answer to both is
    /// the same and this call will not block on either: a `try` that waits for a metadata lock
    /// is a `try` in name only, and the caller holding it may be descheduled.
    ///
    /// # Panics
    ///
    /// When the resource is poisoned.
    pub fn try_lock(&self) -> Option<Turn<'_, T>> {
        let mut queue = if_free(&self.queue)?;
        let claimed = queue.claim();
        drop(queue);
        claimed.expect(FATAL).then(|| self.turn())
    }

    /// Wait unless `cancel` is raised first. Used by shutdown and reset paths.
    ///
    /// Linearization, stated once and relied on below. The call takes the turn at the moment it
    /// sets the owner flag under the metadata lock, or at the moment a release names its ticket;
    /// it observes cancellation at the moment it loads the raised flag. Therefore:
    ///
    /// * A raise that is ordered before this call is seen by the first load, and the call
    ///   returns `Cancelled` having touched nothing — even if the turn was free. A caller told
    ///   to stop does not acquire a resource it would only have to release.
    /// * A raise concurrent with an acquisition that already happened does not undo it: the call
    ///   returns `Ok` and the caller releases the turn as usual. Cancellation loses that race
    ///   deliberately, because the alternative is an owner nobody can account for.
    /// * A waiter checks its grant before it checks the token, so a turn already handed to it is
    ///   taken rather than abandoned.
    ///
    /// # Panics
    ///
    /// As `lock`, and when more callers wait on `cancel` than its table holds. Such a refusal
    /// happens before this caller takes a ticket, so it leaves no registration behind for a
    /// release to hand the turn to.
    pub fn take_until(&self, at: Priority, cancel: &Cancel) -> Result<Turn<'_, T>, Cancelled> {
        self.acquire(at, Some(cancel))
    }

    /// Diagnostics only: never a settling or safety signal. This one waits for the
    /// metadata lock, which is disclosed here because nothing on a working path calls it.
    pub fn waiters(&self) -> usize {
        metadata(&self.queue).waiting.len()
    }

    /// Whether a holder released this resource while unwinding. Waits for the metadata lock, as
    /// `waiters` does.
    pub fn poisoned(&self) -> bool {
        metadata(&self.queue).poisoned
    }

    /// The value, once no handle to the resource remains.
    pub fn into_inner(self) -> T {
        self.value.into_inner()
    }

    /// Hold the metadata lock, as a caller descheduled inside its bookkeeping would. Tests only:
    /// it is how `try_lock`'s contention answer is made deterministic instead of raced for.
    #[cfg(all(test, not(loom)))]
    pub(crate) fn hold(&self) -> impl Drop + '_ {
        metadata(&self.queue)
    }

    /// Set the next ticket number, so the no-wrap check below can be reached without registering
    /// `u64::MAX` waiters first. Tests only.
    #[cfg(all(test, not(loom)))]
    pub(crate) fn tickets_from(&self, next: u64) {
        metadata(&self.queue).next = next;
    }

    fn acquire(&self, at: Priority, cancel: Option<&Cancel>) -> Result<Turn<'_, T>, Cancelled> {
        // A raise ordered before this call ends it here, before the queue is touched at all.
        if cancel.is_some_and(Cancel::raised) {
            return Err(Cancelled);
        }
        {
            let mut queue = metadata(&self.queue);
            let claimed = queue.claim();
            drop(queue);
            if claimed.expect(FATAL) {
                return Ok(self.turn());
            }
        }
        // The turn is held by someone else, so this caller must wait for it, and registering
        // with the token is the part of waiting that can be refused. It happens first: a refusal
        // then unwinds a caller that owns no ticket, where the other order leaves a registration
        // behind whose caller is gone and hands it the turn it will never release.
        let watching = cancel.map(Cancel::watch);
        let ticket = {
            let mut queue = metadata(&self.queue);
            match queue.claim() {
                // Poisoned between the two locks. Reported after the lock is dropped.
                None => {
                    drop(queue);
                    panic!("{FATAL}");
                }
                // Released while this caller was registering. A release hands the turn only to
                // waiters already in the table, so a turn found free now must be taken now
                // rather than waited for: the wake-up for it has already happened.
                Some(true) => {
                    drop(queue);
                    let turn = self.turn();
                    drop(watching);
                    return Ok(turn);
                }
                Some(false) => {}
            }
            if queue.waiting.len() == queue.limit {
                let limit = queue.limit;
                drop(queue);
                panic!("exclusive resource: more than {limit} waiting callers");
            }
            // A ticket is an identity and the tie-break that makes equal priorities FIFO, so it
            // must not wrap: two live waiters with one number is a lost hand-off. The check
            // comes before anything is pushed, so a refusal leaves the table, the counter and
            // the poison flag exactly as they were.
            let Some(after) = queue.next.checked_add(1) else {
                drop(queue);
                panic!("exclusive resource: the ticket counter is exhausted");
            };
            let ticket = queue.next;
            queue.next = after;
            queue.waiting.push(Waiting {
                ticket,
                at,
                thread: thread::current(),
            });
            ticket
        };
        loop {
            let mut queue = metadata(&self.queue);
            if queue.granted == Some(ticket) {
                queue.granted = None;
                take_out(&mut queue.waiting, ticket);
                let poisoned = queue.poisoned;
                drop(queue);
                // Built before anything else that can unwind — deregistration and the poison
                // report included — so an unwind hands the turn to the next waiter instead of
                // stranding a resource that is held by nobody.
                let turn = self.turn();
                drop(watching);
                assert!(!poisoned, "{FATAL}");
                return Ok(turn);
            }
            if cancel.is_some_and(Cancel::raised) {
                take_out(&mut queue.waiting, ticket);
                drop(queue);
                drop(watching);
                return Err(Cancelled);
            }
            drop(queue);
            thread::park();
        }
    }

    fn turn(&self) -> Turn<'_, T> {
        Turn {
            on: self,
            value: PhantomData,
        }
    }
}

/// Deregister `ticket`. Order in the table carries no meaning; the ticket does.
fn take_out(waiting: &mut Vec<Waiting>, ticket: u64) {
    if let Some(at) = waiting.iter().position(|one| one.ticket == ticket) {
        waiting.swap_remove(at);
    }
}

/// The waiter that takes the turn next: highest priority, earliest registration among equals.
/// Tickets are unique, so the winner is unique and the choice is total.
fn next_up(waiting: &[Waiting]) -> Option<usize> {
    waiting
        .iter()
        .enumerate()
        .max_by_key(|(_, one)| (one.at, std::cmp::Reverse(one.ticket)))
        .map(|(at, _)| at)
}

/// The turn itself. `Deref`/`DerefMut` reach the value; dropping hands the turn on, including
/// while unwinding, which also poisons the resource.
///
/// `PhantomData<&mut T>` is what gives the guard the right auto traits: it is `Send` when `T` is
/// and `Sync` when `T` is. Without it the guard would inherit them from `&Exclusive<T>` and a
/// `T: !Sync` value could be shared through `&Turn`.
pub struct Turn<'a, T> {
    on: &'a Exclusive<T>,
    value: PhantomData<&'a mut T>,
}

impl<T> Deref for Turn<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: this guard exists, so the queue named this caller the one owner of the value
        // and will not name another until the guard is dropped.
        unsafe { &*self.on.value.get() }
    }
}

impl<T> DerefMut for Turn<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as `deref`, and `&mut self` rules out a second reference through this guard.
        unsafe { &mut *self.on.value.get() }
    }
}

impl<T> Drop for Turn<'_, T> {
    fn drop(&mut self) {
        let mut queue = metadata(&self.on.queue);
        queue.poisoned |= thread::panicking();
        match next_up(&queue.waiting) {
            // The turn is handed to that waiter, not released: `held` stays set so a caller
            // arriving now registers behind it.
            Some(at) => {
                queue.granted = Some(queue.waiting[at].ticket);
                let next = queue.waiting[at].thread.clone();
                drop(queue);
                next.unpark();
            }
            None => queue.held = false,
        }
    }
}

const FATAL: &str = "exclusive resource poisoned: a holder panicked";
