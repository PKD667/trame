// The host lowering, against a workload this crate invented: `#[parallel]` is the list in order on
// the calling thread, and `concurrent!` is one scoped thread per arm with no round barrier.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Barrier, Mutex, mpsc};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::SeqCst};
use std::thread;
use std::time::{Duration, Instant};

use crate::sync::{Exclusive, Locked, with};
use crate::{Invoked, Keyed, Step};

#[derive(Clone, Copy)]
struct Hit {
    cell: usize,
    amount: i64,
}

struct Rig;

impl Rig {
    #[crate::parallel]
    fn note(&self, hit: Hit, seen: &Mutex<Vec<i64>>) -> Result<(), i64> {
        seen.lock().unwrap().push(hit.amount);
        if hit.amount < 0 { Err(hit.amount) } else { Ok(()) }
    }

    #[crate::parallel]
    #[crate::ordered(key = hit.cell: usize)]
    fn charge(&self, hit: Hit, cell: &mut i64, charges: &AtomicUsize) -> Result<(), ()> {
        *cell += hit.amount;
        charges.fetch_add(1, SeqCst);
        Ok(())
    }
}

#[test]
fn parallel_visits_every_item_once_and_answers_with_its_first_err_in_list_order() {
    let hits = [5, -1, 7, -2].map(|amount| Hit { cell: 0, amount });
    let seen = Mutex::new(Vec::new());
    assert_eq!(crate::invoke!(Rig.note, &seen, &hits), Err(-1));
    let mut seen = seen.into_inner().unwrap();
    seen.sort();
    assert_eq!(seen, [-2, -1, 5, 7], "an Err does not stop the items after it");
}

#[test]
fn an_ordered_call_reaches_the_slot_its_key_names() {
    let hits = [(0, 3), (1, 5), (0, 7)].map(|(cell, amount)| Hit { cell, amount });
    let mut cells = [0i64; 2];
    let charges = AtomicUsize::new(0);
    let rig = Rig;
    crate::invoke!(rig.charge, &charges, &hits, Keyed::new(&mut cells[..])).expect("in range");
    assert_eq!((cells, charges.load(SeqCst)), ([10, 5], 3));

    let astray = [Hit { cell: 2, amount: 1 }, Hit { cell: 0, amount: 1 }];
    assert_eq!(
        crate::invoke!(rig.charge, &charges, &astray, Keyed::new(&mut cells[..])),
        Err(Invoked::OutOfRange { key: 2, len: 2 })
    );
    assert_eq!((cells, charges.load(SeqCst)), ([11, 5], 4), "the item after the refused one ran");
}

/// A bound on a wait another thread is about to end, so a failure is a failure and not a hang.
fn within(what: &str, ready: impl Fn() -> bool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        if Instant::now() > deadline {
            return Err(format!("timed out waiting for {what}"));
        }
        thread::yield_now();
    }
    Ok(())
}

/// Waits inside one step for the fast arm's hundredth.
#[crate::process]
struct Slow<'a> {
    fast: &'a AtomicU32,
    steps: &'a mut u32,
}

impl Slow<'_> {
    fn step(&mut self) -> Result<Step, String> {
        *self.steps += 1;
        within("100 fast steps", || self.fast.load(SeqCst) >= 100)?;
        Ok(Step::Done)
    }
}

#[crate::process]
struct Fast<'a>(&'a AtomicU32);

impl Fast<'_> {
    fn step(&mut self) -> Result<Step, String> {
        let n = self.0.fetch_add(1, SeqCst) + 1;
        Ok(if n >= 100 { Step::Done } else { Step::Progress })
    }
}

#[test]
fn a_fast_arm_runs_many_steps_while_a_slow_arm_is_inside_one() {
    let fast = AtomicU32::new(0);
    let mut slow_steps = 0;
    crate::concurrent!(Slow { fast: &fast, steps: &mut slow_steps }, Fast(&fast))
        .expect("a round barrier would have timed out here");
    assert_eq!(slow_steps, 1);
}

/// Fails with `order` once both arms are inside a step; the first waits for the second's failure.
#[crate::process]
struct Fails<'a> {
    both: &'a Barrier,
    other_failed: &'a AtomicBool,
    order: u32,
}

impl Fails<'_> {
    fn step(&mut self) -> Result<Step, u32> {
        self.both.wait();
        if self.order == 0 {
            within("the second arm's error", || self.other_failed.load(SeqCst)).expect("it fails");
        } else {
            self.other_failed.store(true, SeqCst);
        }
        Err(self.order)
    }
}

#[test]
fn competing_errors_answer_in_source_order_not_time_order() {
    let both = Barrier::new(2);
    let second_failed = AtomicBool::new(false);
    let answer = crate::concurrent!(
        Fails { both: &both, other_failed: &second_failed, order: 0 },
        Fails { both: &both, other_failed: &second_failed, order: 1 },
    );
    assert_eq!(answer, Err(0));
}

/// Fails once its sibling has stepped.
#[crate::process]
struct Blocked<'a>(&'a AtomicBool);

impl Blocked<'_> {
    fn step(&mut self) -> Result<Step, String> {
        within("a sibling step", || self.0.load(SeqCst))?;
        Err("failed".to_string())
    }
}

/// Steps `Idle` until its tenth, slowly after the first.
#[crate::process]
struct Sibling<'a> {
    stepped: &'a AtomicBool,
    steps: &'a mut u32,
}

impl Sibling<'_> {
    fn step(&mut self) -> Result<Step, String> {
        *self.steps += 1;
        if *self.steps > 1 {
            // Long past the moment the runner publishes the sibling's error.
            thread::sleep(Duration::from_millis(50));
        }
        self.stepped.store(true, SeqCst);
        Ok(if *self.steps == 10 { Step::Done } else { Step::Idle })
    }
}

#[test]
fn no_step_starts_after_an_error_is_published() {
    let stepped = AtomicBool::new(false);
    let mut steps = 0;
    let answer = crate::concurrent!(Blocked(&stepped), Sibling { stepped: &stepped, steps: &mut steps });
    assert_eq!(answer, Err("failed".to_string()));
    assert!(steps <= 2, "{steps} steps: only one may have been in flight when the error was published");
}

/// Panics inside `value` once its sibling has started.
#[crate::process]
struct Tears<'a> {
    value: &'a Exclusive<u32>,
    started: &'a AtomicBool,
}

impl Tears<'_> {
    fn step(&mut self) -> Result<Step, ()> {
        within("the sibling", || self.started.load(SeqCst)).expect("it starts");
        with::<_, ()>(self.value, |v| {
            *v = 1;
            panic!("torn");
        })
        .expect("free");
        Ok(Step::Done)
    }
}

#[crate::process]
struct Counts<'a> {
    started: &'a AtomicBool,
    steps: &'a AtomicU32,
}

impl Counts<'_> {
    fn step(&mut self) -> Result<Step, ()> {
        self.started.store(true, SeqCst);
        self.steps.fetch_add(1, SeqCst);
        Ok(Step::Idle)
    }
}

/// Reads `value` once and is done.
#[crate::process]
struct Reads<'a> {
    value: &'a Exclusive<u32>,
    seen: Option<Locked>,
}

impl Reads<'_> {
    fn step(&mut self) -> Result<Step, ()> {
        self.seen = with(self.value, |v| *v).err();
        Ok(Step::Done)
    }
}

#[test]
fn a_panic_joins_its_sibling_then_resumes_and_abandons_its_value() {
    let mut value = Exclusive::new(0u32);
    let started = AtomicBool::new(false);
    let sibling_steps = AtomicU32::new(0);
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        crate::concurrent!(Tears { value: &value, started: &started }, Counts { started: &started, steps: &sibling_steps })
    }));
    let payload = unwound.expect_err("the panic is resumed");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"torn"));
    let joined = sibling_steps.load(SeqCst);
    thread::sleep(Duration::from_millis(20));
    assert_eq!(sibling_steps.load(SeqCst), joined, "the sibling was joined, not left running");
    let mut reads = Reads { value: &value, seen: None };
    crate::concurrent!(&mut reads).expect("no arm fails");
    assert_eq!(reads.seen, Some(Locked::Abandoned));
    assert_eq!(*value.get_mut(), 1);
}

/// Holds the unwinding closure's frame open until the sibling has tried the value `TRIES` times.
struct Unwinding<'a> {
    started: &'a AtomicBool,
    tries: &'a AtomicU32,
    waited: &'a AtomicBool,
}

const TRIES: u32 = 3;

impl Drop for Unwinding<'_> {
    fn drop(&mut self) {
        self.started.store(true, SeqCst);
        let waited = within("the sibling's tries", || self.tries.load(SeqCst) >= TRIES);
        self.waited.store(waited.is_ok(), SeqCst);
    }
}

/// Panics inside `value` with the unwinding frame held open.
#[crate::process]
struct Unwinds<'a> {
    value: &'a Exclusive<(u32, u32)>,
    started: &'a AtomicBool,
    tries: &'a AtomicU32,
    waited: &'a AtomicBool,
}

impl Unwinds<'_> {
    fn step(&mut self) -> Result<Step, ()> {
        with::<_, ()>(self.value, |v| {
            v.0 = 1;
            let _open = Unwinding { started: self.started, tries: self.tries, waited: self.waited };
            panic!("torn");
        })
        .expect("free");
        Ok(Step::Done)
    }
}

/// Tries `value` once it has started to unwind, and notes what it met.
#[crate::process]
struct Tries<'a> {
    value: &'a Exclusive<(u32, u32)>,
    started: &'a AtomicBool,
    tries: &'a AtomicU32,
    seen: Vec<Result<bool, Locked>>,
}

impl Tries<'_> {
    fn step(&mut self) -> Result<Step, ()> {
        if self.started.load(SeqCst) {
            self.seen.push(with(self.value, |v| v.0 == 1 && v.1 == 0));
            self.tries.fetch_add(1, SeqCst);
        }
        Ok(Step::Idle)
    }
}

#[test]
fn a_sibling_sees_busy_or_abandoned_while_a_panic_unwinds_and_never_a_torn_value() {
    let mut value = Exclusive::new((0u32, 0u32));
    let (started, waited) = (AtomicBool::new(false), AtomicBool::new(false));
    let tries = AtomicU32::new(0);
    let mut tried = Tries { value: &value, started: &started, tries: &tries, seen: Vec::new() };
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        crate::concurrent!(Unwinds { value: &value, started: &started, tries: &tries, waited: &waited }, &mut tried)
    }));
    let seen = tried.seen;
    assert_eq!(unwound.expect_err("the panic is resumed").downcast_ref::<&str>(), Some(&"torn"));
    assert!(waited.load(SeqCst), "the sibling tried while the panic unwound");
    let busy = seen.iter().filter(|&&s| s == Err(Locked::Busy)).count();
    assert!(busy >= TRIES as usize, "{busy} Busy while held");
    assert!(
        seen.iter().all(|s| matches!(s, Err(Locked::Busy | Locked::Abandoned))),
        "{seen:?}"
    );
    assert_eq!(*value.get_mut(), (1, 0), "torn, and reached only by its owner");
}

#[test]
fn a_panic_in_setup_after_an_arm_spawned_stops_that_arm_before_the_join() {
    let (tell, told) = mpsc::channel();
    let setup = thread::spawn(move || {
        let mut idle = crate::run::arm(|| Ok::<_, ()>(Step::Idle));
        let unwound = catch_unwind(AssertUnwindSafe(|| {
            crate::run::concurrent::<_, (), 1>(|run| {
                run.arm(&mut idle);
                panic!("setup");
            })
        }));
        tell.send(unwound.is_err()).expect("the test is waiting");
    });
    let resumed = told
        .recv_timeout(Duration::from_secs(10))
        .expect("an arm Idle forever hung the join");
    assert!(resumed, "the setup panic is resumed");
    setup.join().expect("the setup thread ends");
}

/// One item per hit, keyed by cell: each slot collects its amounts in the order it was given them.
fn histories(threads: usize, hits: &[Hit], cells: usize) -> (Vec<Vec<i64>>, Result<(), Invoked<i64>>) {
    let mut slots = vec![Vec::new(); cells];
    let answer = crate::run::ordered_on(
        threads,
        &(),
        hits,
        Keyed::new(&mut slots[..]),
        |hit| hit.cell,
        |hit, slot: &mut Vec<i64>, _: &()| {
            slot.push(hit.amount);
            if hit.amount < 0 { Err(hit.amount) } else { Ok(()) }
        },
    );
    (slots, answer)
}

#[test]
fn a_key_keeps_its_history_and_the_answer_is_the_same_at_any_thread_count() {
    let hits: Vec<Hit> = (0..200).map(|i| Hit { cell: (i * 7) % 13, amount: if i == 32 || i == 150 { -(i as i64) } else { i as i64 } }).collect();
    let (one, answer) = histories(1, &hits, 13);
    assert_eq!(answer, Err(Invoked::Failed(-32)), "the first failure in list order");
    for threads in [2, 5, 64] {
        assert_eq!(histories(threads, &hits, 13), (one.clone(), answer), "{threads} threads");
    }
    let astray = [Hit { cell: 3, amount: 1 }, Hit { cell: 40, amount: 1 }, Hit { cell: 3, amount: -9 }];
    assert_eq!(histories(4, &astray, 13).1, Err(Invoked::OutOfRange { key: 40, len: 13 }), "the astray key is item 1, before the failure at item 2");
}

#[test]
fn distinct_keys_run_on_distinct_threads() {
    let hits: Vec<Hit> = (0..8).map(|cell| Hit { cell, amount: 0 }).collect();
    let mut slots = vec![None; 8];
    crate::run::ordered_on(
        4,
        &(),
        &hits,
        Keyed::new(&mut slots[..]),
        |hit| hit.cell,
        |_, slot: &mut Option<thread::ThreadId>, _: &()| {
            *slot = Some(thread::current().id());
            Ok::<_, ()>(())
        },
    )
    .expect("every key names a slot");
    let threads: std::collections::HashSet<_> = slots.into_iter().collect();
    assert_eq!(threads.len(), 4, "one thread per group");
}

#[test]
fn a_panic_joins_the_other_keys_work_before_it_unwinds() {
    let done = AtomicUsize::new(0);
    let hits: Vec<Hit> = (0..8).map(|cell| Hit { cell, amount: 0 }).collect();
    let mut slots = vec![(); 8];
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        crate::run::ordered_on(
            8,
            &(),
            &hits,
            Keyed::new(&mut slots[..]),
            |hit| hit.cell,
            |hit, _: &mut (), _: &()| {
                assert!(hit.cell != 3, "torn");
                done.fetch_add(1, SeqCst);
                Ok::<_, ()>(())
            },
        )
    }));
    assert!(unwound.is_err());
    assert_eq!(done.load(SeqCst), 7, "every key but the one that panicked finished");
}
