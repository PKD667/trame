// The host lowering, against a workload this crate invented: `#[parallel]` is the list in order on
// the calling thread, and `concurrent!` is one scoped thread per arm with no round barrier.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Barrier, mpsc};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::SeqCst};
use std::thread;
use std::time::{Duration, Instant};

use crate::sync::{Exclusive, Locked};
use crate::{Invoked, Keyed, Step};

#[derive(Clone, Copy)]
struct Hit {
    cell: usize,
    amount: i64,
}

struct Rig;

impl Rig {
    #[crate::parallel]
    fn note(&self, hit: Hit, seen: &mut Vec<i64>) -> Result<(), i64> {
        seen.push(hit.amount);
        if hit.amount < 0 { Err(hit.amount) } else { Ok(()) }
    }

    #[crate::parallel]
    #[crate::ordered(key = hit.cell: usize)]
    fn charge(&self, hit: Hit, cell: &mut i64, charges: &mut usize) -> Result<(), ()> {
        *cell += hit.amount;
        *charges += 1;
        Ok(())
    }
}

#[test]
fn parallel_runs_the_list_in_order_and_answers_with_its_first_err() {
    let hits = [5, -1, 7, -2].map(|amount| Hit { cell: 0, amount });
    let mut seen = Vec::new();
    assert_eq!(crate::invoke!(Rig.note, &mut seen, &hits), Err(-1));
    assert_eq!(seen, [5, -1, 7, -2], "an Err does not stop the items after it");
}

#[test]
fn an_ordered_call_reaches_the_slot_its_key_names() {
    let hits = [(0, 3), (1, 5), (0, 7)].map(|(cell, amount)| Hit { cell, amount });
    let mut cells = [0i64; 2];
    let mut charges = 0;
    let rig = Rig;
    crate::invoke!(rig.charge, &mut charges, &hits, Keyed::new(&mut cells[..])).expect("in range");
    assert_eq!((cells, charges), ([10, 5], 3));

    let astray = [Hit { cell: 2, amount: 1 }, Hit { cell: 0, amount: 1 }];
    assert_eq!(
        crate::invoke!(rig.charge, &mut charges, &astray, Keyed::new(&mut cells[..])),
        Err(Invoked::OutOfRange { key: 2, len: 2 })
    );
    assert_eq!((cells, charges), ([11, 5], 4), "the item after the refused one ran");
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

#[test]
fn a_fast_arm_runs_many_steps_while_a_slow_arm_is_inside_one() {
    let fast = AtomicU32::new(0);
    let mut slow_steps = 0;
    crate::concurrent! {
        || {
            slow_steps += 1;
            within("100 fast steps", || fast.load(SeqCst) >= 100)?;
            Ok::<_, String>(Step::Done)
        },
        || {
            let n = fast.fetch_add(1, SeqCst) + 1;
            Ok(if n >= 100 { Step::Done } else { Step::Progress })
        },
    }
    .expect("a round barrier would have timed out here");
    assert_eq!(slow_steps, 1);
}

#[test]
fn competing_errors_answer_in_source_order_not_time_order() {
    let both = Barrier::new(2);
    let second_failed = AtomicBool::new(false);
    let answer = crate::concurrent! {
        || {
            both.wait();
            within("the second arm's error", || second_failed.load(SeqCst)).expect("it fails");
            Err::<Step, u32>(0)
        },
        || {
            both.wait();
            second_failed.store(true, SeqCst);
            Err(1)
        },
    };
    assert_eq!(answer, Err(0));
}

#[test]
fn no_step_starts_after_an_error_is_published() {
    let stepped = AtomicBool::new(false);
    let mut steps = 0;
    let answer = crate::concurrent! {
        || {
            within("a sibling step", || stepped.load(SeqCst))?;
            Err("failed".to_string())
        },
        || {
            steps += 1;
            if steps > 1 {
                // Long past the moment the runner publishes the sibling's error.
                thread::sleep(Duration::from_millis(50));
            }
            stepped.store(true, SeqCst);
            Ok(if steps == 10 { Step::Done } else { Step::Idle })
        },
    };
    assert_eq!(answer, Err("failed".to_string()));
    assert!(steps <= 2, "{steps} steps: only one may have been in flight when the error was published");
}

#[test]
fn a_panic_joins_its_sibling_then_resumes_and_abandons_its_value() {
    let mut value = Exclusive::new(0u32);
    let started = AtomicBool::new(false);
    let sibling_steps = AtomicU32::new(0);
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        crate::concurrent! {
            || {
                within("the sibling", || started.load(SeqCst)).expect("it starts");
                value
                    .with::<()>(|v| {
                        *v = 1;
                        panic!("torn");
                    })
                    .expect("free");
                Ok::<_, ()>(Step::Done)
            },
            || {
                started.store(true, SeqCst);
                sibling_steps.fetch_add(1, SeqCst);
                Ok::<_, ()>(Step::Idle)
            },
        }
    }));
    let payload = unwound.expect_err("the panic is resumed");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"torn"));
    let joined = sibling_steps.load(SeqCst);
    thread::sleep(Duration::from_millis(20));
    assert_eq!(sibling_steps.load(SeqCst), joined, "the sibling was joined, not left running");
    let mut seen = None;
    crate::concurrent! { || { seen = value.with(|v| *v).err(); Ok::<_, ()>(Step::Done) } }
        .expect("no arm fails");
    assert_eq!(seen, Some(Locked::Abandoned));
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

#[test]
fn a_sibling_sees_busy_or_abandoned_while_a_panic_unwinds_and_never_a_torn_value() {
    let mut value = Exclusive::new((0u32, 0u32));
    let (started, waited) = (AtomicBool::new(false), AtomicBool::new(false));
    let tries = AtomicU32::new(0);
    let mut seen = Vec::new();
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        crate::concurrent! {
            || {
                value
                    .with::<()>(|v| {
                        v.0 = 1;
                        let _open = Unwinding { started: &started, tries: &tries, waited: &waited };
                        panic!("torn");
                    })
                    .expect("free");
                Ok::<_, ()>(Step::Done)
            },
            || {
                if started.load(SeqCst) {
                    seen.push(value.with(|v| v.0 == 1 && v.1 == 0));
                    tries.fetch_add(1, SeqCst);
                }
                Ok::<_, ()>(Step::Idle)
            },
        }
    }));
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
