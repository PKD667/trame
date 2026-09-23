// Bounded ownership transfer between one producer and one consumer, over storage the owner holds
// inline. Payloads are padding-free copy values; a refused one comes back with the reason.

use super::NoUninit;
use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering::Acquire, Ordering::Relaxed, Ordering::Release};

/// Why a payload was not taken. It is handed back with this.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Already `D` deep: capacity.
    Full,
    /// The peer was mid-call: contention, nothing implied about capacity.
    Busy,
    /// The peer endpoint is gone: every later attempt is refused the same way.
    Closed,
}

/// A payload that was not accepted, returned intact.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Unsent<T: NoUninit> {
    pub why: Refused,
    pub value: T,
}

/// Why nothing was taken. `Empty` and `Closed` differ so a consumer that must drain knows when
/// nothing more can arrive.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Idle {
    Empty,
    Busy,
    Closed,
}

/// Every buffer is in exactly one place: held by an endpoint, on the spare stack, or in one
/// queued slot; a refused `send` or `give` hands back that same value.
pub struct Handoff<T: NoUninit, const D: usize> {
    claim: AtomicBool,
    /// Cleared by the endpoint's drop, so its peer can tell "not yet" from "never".
    sending: AtomicBool,
    receiving: AtomicBool,
    yard: UnsafeCell<Yard<T, D>>,
}

struct Yard<T, const D: usize> {
    full: [MaybeUninit<T>; D],
    head: usize,
    queued: usize,
    spare: [MaybeUninit<T>; D],
    spares: usize,
}

// SAFETY: `yard` is reached only under `claim`, one endpoint at a time, and payloads move whole
// between the endpoints' threads, which needs `T: Send`.
unsafe impl<T: NoUninit + Send, const D: usize> Sync for Handoff<T, D> {}

impl<T: NoUninit, const D: usize> Handoff<T, D> {
    /// `D` spares built up front, so steady-state production allocates nothing.
    pub fn new(mut init: impl FnMut() -> T) -> Self {
        const { assert!(D >= 1, "a handoff needs room for at least one payload") };
        Handoff {
            claim: AtomicBool::new(false),
            sending: AtomicBool::new(false),
            receiving: AtomicBool::new(false),
            yard: UnsafeCell::new(Yard {
                full: [const { MaybeUninit::uninit() }; D],
                head: 0,
                queued: 0,
                spare: std::array::from_fn(|_| MaybeUninit::new(init())),
                spares: D,
            }),
        }
    }

    /// Callable again once both endpoints are gone, so one handoff serves successive drives.
    pub fn split(&mut self) -> (Sender<'_, T, D>, Receiver<'_, T, D>) {
        self.sending.store(true, Relaxed);
        self.receiving.store(true, Relaxed);
        let on = &*self;
        (Sender { on }, Receiver { on })
    }

    fn claim(&self) -> Option<Claimed<'_, T, D>> {
        self.claim
            .compare_exchange(false, true, Acquire, Relaxed)
            .ok()
            .map(|_| Claimed { on: self })
    }
}

fn live(flag: &AtomicBool) -> bool {
    flag.load(Acquire)
}

struct Claimed<'a, T: NoUninit, const D: usize> {
    on: &'a Handoff<T, D>,
}

impl<T: NoUninit, const D: usize> Deref for Claimed<'_, T, D> {
    type Target = Yard<T, D>;

    fn deref(&self) -> &Yard<T, D> {
        // SAFETY: this claim is the one `claim` admitted.
        unsafe { &*self.on.yard.get() }
    }
}

impl<T: NoUninit, const D: usize> DerefMut for Claimed<'_, T, D> {
    fn deref_mut(&mut self) -> &mut Yard<T, D> {
        // SAFETY: as `deref`.
        unsafe { &mut *self.on.yard.get() }
    }
}

impl<T: NoUninit, const D: usize> Drop for Claimed<'_, T, D> {
    fn drop(&mut self) {
        self.on.claim.store(false, Release);
    }
}

/// The one producer.
pub struct Sender<'a, T: NoUninit, const D: usize> {
    on: &'a Handoff<T, D>,
}

impl<T: NoUninit, const D: usize> Sender<'_, T, D> {
    /// `Ok` is acceptance into a queue whose consumer existed, not delivery.
    pub fn send(&mut self, value: T) -> Result<(), Unsent<T>> {
        if !live(&self.on.receiving) {
            return Err(Unsent {
                why: Refused::Closed,
                value,
            });
        }
        let Some(mut yard) = self.on.claim() else {
            return Err(Unsent {
                why: Refused::Busy,
                value,
            });
        };
        if yard.queued == D {
            return Err(Unsent {
                why: Refused::Full,
                value,
            });
        }
        let tail = (yard.head + yard.queued) % D;
        yard.full[tail].write(value);
        yard.queued += 1;
        Ok(())
    }

    pub fn spare(&mut self) -> Result<T, Idle> {
        // Loaded before the pool: a consumer gives back and then drops, so an empty pool after
        // a cleared flag is empty for good.
        let live = live(&self.on.receiving);
        let Some(mut yard) = self.on.claim() else {
            return Err(Idle::Busy);
        };
        if yard.spares == 0 {
            return Err(if live { Idle::Empty } else { Idle::Closed });
        }
        let at = yard.spares - 1;
        // SAFETY: slot `at` is below `spares`, so it holds a spare, and the count moves below it.
        let value = unsafe { yard.spare[at].assume_init_read() };
        yard.spares = at;
        Ok(value)
    }
}

impl<T: NoUninit, const D: usize> Drop for Sender<'_, T, D> {
    fn drop(&mut self) {
        self.on.sending.store(false, Release);
    }
}

/// The one consumer.
pub struct Receiver<'a, T: NoUninit, const D: usize> {
    on: &'a Handoff<T, D>,
}

impl<T: NoUninit, const D: usize> Receiver<'_, T, D> {
    /// The oldest payload.
    pub fn recv(&mut self) -> Result<T, Idle> {
        // Loaded before the queue, for the reason `spare` gives.
        let live = live(&self.on.sending);
        let Some(mut yard) = self.on.claim() else {
            return Err(Idle::Busy);
        };
        if yard.queued == 0 {
            return Err(if live { Idle::Empty } else { Idle::Closed });
        }
        let at = yard.head;
        // SAFETY: slot `at` is the head of the queued payloads, and the head moves past it.
        let value = unsafe { yard.full[at].assume_init_read() };
        yard.head = (at + 1) % D;
        yard.queued -= 1;
        Ok(value)
    }

    /// Return emptied storage for the producer to fill again.
    pub fn give(&mut self, value: T) -> Result<(), Unsent<T>> {
        if !live(&self.on.sending) {
            return Err(Unsent {
                why: Refused::Closed,
                value,
            });
        }
        let Some(mut yard) = self.on.claim() else {
            return Err(Unsent {
                why: Refused::Busy,
                value,
            });
        };
        if yard.spares == D {
            return Err(Unsent {
                why: Refused::Full,
                value,
            });
        }
        let at = yard.spares;
        yard.spare[at].write(value);
        yard.spares += 1;
        Ok(())
    }

    pub fn sending(&self) -> bool {
        live(&self.on.sending)
    }
}

impl<T: NoUninit, const D: usize> Drop for Receiver<'_, T, D> {
    fn drop(&mut self) {
        self.on.receiving.store(false, Release);
    }
}

#[cfg(test)]
mod tests {
    use super::{Handoff, Idle, Refused, Unsent};

    #[test]
    fn a_held_claim_refuses_each_endpoint_operation_until_released() {
        let mut handoff = Handoff::<u32, 2>::new(|| 0);
        let (mut sender, mut receiver) = handoff.split();
        let held = sender.on.claim().expect("claim was free");

        assert_eq!(
            sender.send(7),
            Err(Unsent {
                why: Refused::Busy,
                value: 7,
            })
        );
        assert_eq!(receiver.recv(), Err(Idle::Busy));
        assert_eq!(sender.spare(), Err(Idle::Busy));
        assert_eq!(
            receiver.give(9),
            Err(Unsent {
                why: Refused::Busy,
                value: 9,
            })
        );

        drop(held);
        let value = sender.spare().expect("claim released");
        sender.send(value).expect("send after release");
        let value = receiver.recv().expect("receive after release");
        receiver.give(value).expect("give after release");
    }
}
