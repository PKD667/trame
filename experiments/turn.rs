// Family A, certified by running one workload against whichever implementation the selected
// backend offers.
//
// The host family decides who is next under a metadata lock and hands the turn on by unparking a
// named waiter. The device family has no lock and no parking: it is a bounded slot table and a
// sequence number, with the decision of who holds the turn taken under a spinlock. Two different
// machines and the same guarantee, which is the thing worth measuring and the reason a family is
// declared rather than a function.
//
// Run it twice, once per backend, and compare the two reports:
//
//     cargo run --example turn
//     cargo run --features nv --example turn
//
// The semantic fields must be equal — exclusion, FIFO among equals, cancellation, the turn
// surviving every refusal, a poll leaving nothing registered. They are the family's promise, so a
// difference is a defect in one implementation rather than a property of a machine.
//
// What is not yet equal is the *shape* of the calls, and `shim` is where that lives rather than
// being spread through the workload. Three things differ: where the turn's storage comes from, how
// a wait is bounded, and which early endings a family can name. The host family blocks or is
// cancelled; it has no notion of a wait that ran out of attempts and none of a full table, because
// its table is allocated at construction and an overflow is a panic. The device family states a
// budget and reports capacity, contention and cancellation distinctly.
//
// So a portable call site cannot be written against the two at once *yet*, and saying so is part
// of the measurement rather than a caveat on it. Section 2 puts the difference on the cooperative
// lowering — "cooperative lowering shall transform mutable parameters and every access to them
// together" — so `shim` is that lowering standing in, in one place, and every line below it is one
// workload rather than two.

use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// The seat the cooperative lowering will take: four calls whose shape the families do not share,
/// and nothing else.
mod shim {
    pub type Ex = trame::sync::Exclusive<u64>;
    pub type Cancel = trame::sync::Cancel;

    /// Where the turn's storage comes from.
    ///
    /// The host family is given a value and owns it. The device family is given the words of its
    /// header and the address of the value, because on a device both belong to the launch. The
    /// boxes are kept alive here rather than leaked, so a mistake in this shim is a use-after-free
    /// that a sanitizer catches instead of a comment that promises it cannot happen.
    pub struct Storage {
        // Held for their addresses, not their contents: the device family points into both, so
        // dropping either would leave the turn over freed memory. Nothing reads them, which is
        // exactly why they are named here rather than being temporaries.
        #[cfg(feature = "nv")]
        #[allow(dead_code)]
        words: Vec<u32>,
        #[cfg(feature = "nv")]
        #[allow(dead_code)]
        value: Box<u64>,
        exclusive: Ex,
    }

    impl Storage {
        pub fn open() -> Storage {
            #[cfg(not(feature = "nv"))]
            {
                Storage { exclusive: Ex::new(0) }
            }
            #[cfg(feature = "nv")]
            {
                // The word count and the initial contents are the family's business: a caller
                // prepares the words through the backend and never writes them by hand.
                let mut words = vec![0u32; trame::sync::WORDS];
                trame::sync::init_header(&mut words);
                let mut value = Box::new(0u64);
                let exclusive = unsafe { Ex::new(words.as_mut_ptr(), &mut *value as *mut u64) };
                Storage { words, value, exclusive }
            }
        }

        pub fn exclusive(&self) -> &Ex {
            &self.exclusive
        }
    }

    /// The turn, held for as long as this lives. Released on drop.
    pub struct Hold<'a> {
        inner: trame::sync::Turn<'a, u64>,
    }

    impl core::ops::Deref for Hold<'_> {
        type Target = u64;
        fn deref(&self) -> &u64 {
            &self.inner
        }
    }

    impl core::ops::DerefMut for Hold<'_> {
        fn deref_mut(&mut self) -> &mut u64 {
            &mut self.inner
        }
    }

    /// Shape 2 of 3: waiting. The host family waits until it is given the turn; the device family
    /// states a budget, because a warp that spins on a warp which is not resident makes no progress
    /// and the only honest answer there is a bound that runs out.
    pub fn take(ex: &Ex) -> Hold<'_> {
        #[cfg(not(feature = "nv"))]
        {
            // The host family blocks rather than reporting, so its wait has no outcome to unwrap.
            Hold { inner: ex.take(trame::sync::NORMAL) }
        }
        #[cfg(feature = "nv")]
        {
            Hold { inner: ex.take(trame::sync::NORMAL, 0).expect("an uncontended turn") }
        }
    }

    /// The turn lent for one call, which is the obligation the family states.
    pub fn with_turn<R>(ex: &Ex, body: impl FnOnce(&mut u64) -> R) -> R {
        let mut held = take(ex);
        body(&mut held)
    }

    /// Shape 3 of 3: a wait someone asks to stop, named the way each family names it.
    pub fn with_turn_until<R>(
        ex: &Ex,
        cancel: &Cancel,
        body: impl FnOnce(&mut u64) -> R,
    ) -> Result<R, &'static str> {
        #[cfg(not(feature = "nv"))]
        let outcome = ex
            .take_until(trame::sync::NORMAL, cancel)
            .map_err(|_| "Cancelled");
        #[cfg(feature = "nv")]
        let outcome = ex
            .take_until(trame::sync::NORMAL, 0, cancel)
            .map_err(|refused| match refused {
                trame::sync::turn::Refused::Cancelled => "Cancelled",
                trame::sync::turn::Refused::Full => "Full",
                trame::sync::turn::Refused::Exhausted { .. } => "Exhausted",
            });
        outcome.map(|mut held| body(&mut held))
    }

    /// Whether a poll against the turn acquires it. Both families answer this the same way, so it
    /// needs no shape of its own.
    pub fn poll_acquires(ex: &Ex) -> bool {
        ex.try_lock().is_some()
    }

    /// Registered waiters, the holder excluded. Both families mean the same thing by it, which
    /// the differential run checked rather than assumed.
    pub fn registered(ex: &Ex) -> usize {
        ex.waiters()
    }
}

use shim::{Cancel, Storage};

/// Poll a condition another thread is about to make true, with a bound, so that a failure is a
/// failure rather than a hang.
fn until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::yield_now();
    }
}

/// No two holders overlap. The read-modify-write is not atomic, so the total is short by exactly
/// the number of overlaps.
fn exclusion() -> bool {
    const THREADS: u64 = 4;
    const ROUNDS: u64 = 20_000;
    let storage = Storage::open();
    let ex = storage.exclusive();

    thread::scope(|scope| {
        for _ in 0..THREADS {
            scope.spawn(move || {
                for _ in 0..ROUNDS {
                    shim::with_turn(ex, |value| *value += 1);
                }
            });
        }
    });

    match storage.exclusive().try_lock() {
        Some(held) => *held == THREADS * ROUNDS,
        None => false,
    }
}

/// Among equals, the earliest registration is served first.
///
/// Registration order is made exact rather than hoped for: each contender is released to register
/// only once the previous one is in the table, which `registered` reports. A sleep would have made
/// this a test of the operating system's scheduler rather than of the family.
fn fifo() -> bool {
    const CONTENDERS: usize = 3;
    let storage = Storage::open();
    let ex = storage.exclusive();
    let (order_tx, order_rx) = mpsc::channel::<usize>();
    let mut release = Vec::new();

    thread::scope(|scope| {
        // The holder takes the turn first, so each contender below queues behind it. The hold is
        // dropped inside the scope rather than outside it, because the scope joins the contenders
        // when it ends and a contender cannot finish while this one still owns the turn.
        let held = shim::take(ex);
        for index in 0..CONTENDERS {
            let (go_tx, go_rx) = mpsc::channel::<()>();
            release.push(go_tx);
            let order_tx = order_tx.clone();
            scope.spawn(move || {
                go_rx.recv().expect("told to go");
                shim::with_turn(ex, |_| {
                    order_tx.send(index).expect("its turn");
                });
            });
        }
        for (index, go_tx) in release.iter().enumerate() {
            go_tx.send(()).expect("release the next contender");
            until("a contender to register", || shim::registered(ex) == index + 1);
        }
        drop(held);
    });

    let observed: Vec<usize> = order_rx.iter().take(CONTENDERS).collect();
    observed == (0..CONTENDERS).collect::<Vec<_>>()
}

/// A cancelled wait ends, and the turn outlives it.
fn cancellation() -> (&'static str, bool) {
    let storage = Storage::open();
    let ex = storage.exclusive();
    let cancel = Arc::new(Cancel::default());

    // The registration, the raise and the join all happen while the turn is held, and the hold is
    // released inside the scope because the scope joins the waiter when it ends.
    let outcome = thread::scope(|scope| {
        let held = shim::take(ex);
        let waiter = {
            let cancel = Arc::clone(&cancel);
            scope.spawn(move || shim::with_turn_until(ex, &cancel, |value| *value += 1))
        };
        until("the waiter to register", || shim::registered(ex) == 1);
        cancel.raise();
        let name = match waiter.join().expect("the waiter did not panic") {
            Ok(()) => "granted",
            Err(name) => name,
        };
        drop(held);
        name
    });

    // A cancelled wait consumed nothing and left nothing behind, so the turn is there for the next
    // caller rather than being something it has to wait for.
    let survives = shim::poll_acquires(ex) && shim::registered(ex) == 0;
    (outcome, survives)
}

/// A poll against a held turn acquires nothing and registers nothing.
fn polling() -> bool {
    let storage = Storage::open();
    let ex = storage.exclusive();
    let held = shim::take(ex);
    let clean = !shim::poll_acquires(ex) && shim::registered(ex) == 0;
    drop(held);
    clean && shim::poll_acquires(ex)
}

fn main() {
    let (cancelled, survives) = cancellation();
    println!(
        "{{\"schema\":\"backend.turn.v1\",\"backend\":\"{}\",\
         \"excluded\":{},\"fifo\":{},\"cancelled\":\"{cancelled}\",\
         \"turn_survives_refusals\":{survives},\"poll_registers_nothing\":{},\
         \"waiter_table\":\"bounded\"}}",
        if cfg!(feature = "nv") { "nv" } else { "host" },
        exclusion(),
        fifo(),
        polling(),
    );
}
