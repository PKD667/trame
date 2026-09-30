// Bounded ownership transfer between one producer and one consumer, over storage the owner holds
// inline. A payload is moved, never copied; a refused one comes back with the reason.

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};
use super::atomic::{AtomicBool, Ordering::Acquire, Ordering::Relaxed, Ordering::Release};

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
pub struct Unsent<T> {
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
pub struct Handoff<T, const D: usize> {
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
// between execution owners, which needs `T: Send`.
unsafe impl<T: Send, const D: usize> Sync for Handoff<T, D> {}

/// `D` spares built up front, so steady-state production allocates nothing. A panicking `init`
/// drops the spares already built.
pub fn new<T, const D: usize>(mut init: impl FnMut() -> T) -> Handoff<T, D> {
    const { assert!(D >= 1, "a handoff needs room for at least one payload") };
    let mut yard = Yard {
        full: [const { MaybeUninit::uninit() }; D],
        head: 0,
        queued: 0,
        spare: [const { MaybeUninit::uninit() }; D],
        spares: 0,
    };
    for at in 0..D {
        yard.spare[at].write(init());
        yard.spares += 1;
    }
    Handoff {
        claim: AtomicBool::new(false),
        sending: AtomicBool::new(false),
        receiving: AtomicBool::new(false),
        yard: UnsafeCell::new(yard),
    }
}

/// Callable again once both endpoints are gone, so one handoff serves successive drives.
pub fn split<T, const D: usize>(queue: &mut Handoff<T, D>) -> (Sender<'_, T, D>, Receiver<'_, T, D>) {
    queue.sending.store(true, Relaxed);
    queue.receiving.store(true, Relaxed);
    let on = &*queue;
    (Sender { on }, Receiver { on })
}

impl<T, const D: usize> Handoff<T, D> {
    fn claim(&self) -> Option<Claimed<'_, T, D>> {
        match self.claim.compare_exchange(false, true, Acquire, Relaxed) {
            Ok(_) => Some(Claimed { on: self }),
            Err(_) => None,
        }
    }
}

impl<T, const D: usize> Drop for Yard<T, D> {
    fn drop(&mut self) {
        for at in 0..self.queued {
            // SAFETY: the `queued` slots from `head` on are initialized and owned by the yard.
            unsafe { self.full[(self.head + at) % D].assume_init_drop() };
        }
        for slot in &mut self.spare[..self.spares] {
            // SAFETY: the first `spares` spare slots are initialized and owned by the yard.
            unsafe { slot.assume_init_drop() };
        }
    }
}

fn live(flag: &AtomicBool) -> bool {
    flag.load(Acquire)
}

struct Claimed<'a, T, const D: usize> {
    on: &'a Handoff<T, D>,
}

impl<T, const D: usize> Deref for Claimed<'_, T, D> {
    type Target = Yard<T, D>;

    fn deref(&self) -> &Yard<T, D> {
        // SAFETY: this claim is the one `claim` admitted.
        unsafe { &*self.on.yard.get() }
    }
}

impl<T, const D: usize> DerefMut for Claimed<'_, T, D> {
    fn deref_mut(&mut self) -> &mut Yard<T, D> {
        // SAFETY: as `deref`.
        unsafe { &mut *self.on.yard.get() }
    }
}

impl<T, const D: usize> Drop for Claimed<'_, T, D> {
    fn drop(&mut self) {
        self.on.claim.store(false, Release);
    }
}

/// The one producer.
pub struct Sender<'a, T, const D: usize> {
    on: &'a Handoff<T, D>,
}

/// `Ok` is acceptance into a queue whose consumer existed, not delivery.
pub fn send<T, const D: usize>(sender: &mut Sender<'_, T, D>, value: T) -> Result<(), Unsent<T>> {
    if !live(&sender.on.receiving) {
        return Err(Unsent { why: Refused::Closed, value });
    }
    let Some(mut yard) = sender.on.claim() else {
        return Err(Unsent { why: Refused::Busy, value });
    };
    if yard.queued == D {
        return Err(Unsent { why: Refused::Full, value });
    }
    let tail = (yard.head + yard.queued) % D;
    yard.full[tail].write(value);
    yard.queued += 1;
    Ok(())
}

pub fn spare<T, const D: usize>(sender: &mut Sender<'_, T, D>) -> Result<T, Idle> {
    // Loaded before the pool: a consumer gives back and then drops, so an empty pool after
    // a cleared flag is empty for good.
    let live = live(&sender.on.receiving);
    let Some(mut yard) = sender.on.claim() else {
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

impl<T, const D: usize> Drop for Sender<'_, T, D> {
    fn drop(&mut self) {
        self.on.sending.store(false, Release);
    }
}

/// The one consumer.
pub struct Receiver<'a, T, const D: usize> {
    on: &'a Handoff<T, D>,
}

/// The oldest payload.
pub fn recv<T, const D: usize>(receiver: &mut Receiver<'_, T, D>) -> Result<T, Idle> {
    // Loaded before the queue, for the reason `spare` gives.
    let live = live(&receiver.on.sending);
    let Some(mut yard) = receiver.on.claim() else {
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
pub fn give<T, const D: usize>(receiver: &mut Receiver<'_, T, D>, value: T) -> Result<(), Unsent<T>> {
    if !live(&receiver.on.sending) {
        return Err(Unsent { why: Refused::Closed, value });
    }
    let Some(mut yard) = receiver.on.claim() else {
        return Err(Unsent { why: Refused::Busy, value });
    };
    if yard.spares == D {
        return Err(Unsent { why: Refused::Full, value });
    }
    let at = yard.spares;
    yard.spare[at].write(value);
    yard.spares += 1;
    Ok(())
}

pub fn sending<T, const D: usize>(receiver: &Receiver<'_, T, D>) -> bool {
    live(&receiver.on.sending)
}

impl<T, const D: usize> Drop for Receiver<'_, T, D> {
    fn drop(&mut self) {
        self.on.receiving.store(false, Release);
    }
}

#[cfg(test)]
mod tests {
    use super::{Idle, Refused, Unsent, give, new, recv, send, spare, split};

    #[test]
    fn a_held_claim_refuses_each_endpoint_operation_until_released() {
        let mut handoff = new::<u32, 2>(|| 0);
        let (mut sender, mut receiver) = split(&mut handoff);
        let held = sender.on.claim().expect("claim was free");

        assert_eq!(send(&mut sender, 7), Err(Unsent { why: Refused::Busy, value: 7 }));
        assert_eq!(recv(&mut receiver), Err(Idle::Busy));
        assert_eq!(spare(&mut sender), Err(Idle::Busy));
        assert_eq!(give(&mut receiver, 9), Err(Unsent { why: Refused::Busy, value: 9 }));

        drop(held);
        let value = spare(&mut sender).expect("claim released");
        send(&mut sender, value).expect("send after release");
        let value = recv(&mut receiver).expect("receive after release");
        give(&mut receiver, value).expect("give after release");
    }
}
