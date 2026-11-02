//! The sequential backend through the surface: routes, the leader, lossy routes and `Full`. Each
//! test declares workers as states and steps (worker.rs), launches them and watches from the leader.
//! Every test runs under a 10 s guard, so a hang fails, not stalls. The states need a checked build,
//! so the modules that use them are compiled only with `--cfg trame_checked` (mod.rs).

extern crate std;

use crate::io::{self, Full, Message};
use crate::run::{Outcome, Run};
use crate::status::Status;
use crate::topology::{LEADER, Topology, WorkerId};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::string::String;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

#[crate::message]
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Ping(pub u32);

#[crate::message]
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Pong(pub u32);

/// Run `f` on its own thread. Fail if it has not answered in 10 s, so a hang cannot stall the suite.
/// A panic of `f` is raised again here, with its own payload.
pub(super) fn within_10s<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (answer, answered) = mpsc::channel();
    let body = thread::spawn(move || answer.send(f()));
    match answered.recv_timeout(Duration::from_secs(10)) {
        Ok(out) => out,
        Err(RecvTimeoutError::Timeout) => panic!("hung: no answer within 10 s"),
        Err(RecvTimeoutError::Disconnected) => match body.join() {
            Err(payload) => std::panic::resume_unwind(payload),
            Ok(_) => unreachable!("the body dropped its sender without answering or panicking"),
        },
    }
}

/// The message of the panic that `f` ends with, read as the leader reports one (`run::panic_message`).
/// `f` must panic.
pub(super) fn panic_text(f: impl FnOnce()) -> String {
    let payload = catch_unwind(AssertUnwindSafe(f)).expect_err("the leader must panic");
    crate::run::panic_message(&*payload)
}

/// The leader's next frame of type `M`: `recv`, and `wait` while there is none (run.rs, `wait`).
pub(super) fn await_frame<M: Message>(run: &Run) -> M {
    loop {
        if let Some(m) = run.recv::<M>() {
            return m;
        }
        run.wait();
    }
}

/// A state with one step that is done at once: a worker that returns.
#[crate::state(steps(quick))]
pub(super) struct Quick {}

#[crate::step(Quick)]
pub(super) fn quick() -> Status {
    Status::Done
}

/// A state with one step that waits for a write to a field nothing writes: a worker that can never
/// return, so the run's `join` gives it `Deadlock`.
#[crate::state(steps(stuck))]
pub(super) struct Stuck {
    pub(super) never: bool,
}

#[crate::step(Stuck)]
pub(super) fn stuck(never: &bool) -> Status {
    if *never { Status::Done } else { Status::wait() }
}

/// Worker A: sends `Ping(7)` to `to`, waits for the `Pong`, and passes it to the leader.
#[crate::state(steps(pinger))]
struct Pinger {
    to: WorkerId,
    sent: bool,
}

#[crate::step(Pinger, receives = Pong)]
fn pinger(to: &WorkerId, sent: &mut bool) -> Status {
    if !*sent {
        if io::send(*to, Ping(7)).is_err() {
            return Status::wait().room::<Ping>(*to);
        }
        *sent = true;
    }
    match io::recv::<Pong>() {
        Some(reply) => {
            assert!(io::send(LEADER, reply).is_ok(), "the leader's route is empty");
            Status::Done
        }
        None => Status::wait().frame(),
    }
}

/// Worker B: answers a `Ping(v)` from `to` with `Pong(v + 1)`.
#[crate::state(steps(ponger))]
struct Ponger {
    to: WorkerId,
}

#[crate::step(Ponger, receives = Ping)]
fn ponger(to: &WorkerId) -> Status {
    match io::recv::<Ping>() {
        Some(Ping(v)) => match io::send(*to, Pong(v + 1)) {
            Ok(()) => Status::Done,
            Err(Full) => panic!("the route back is empty, so it has room"),
        },
        None => Status::wait().frame(),
    }
}

#[test]
fn ping_pong_between_two_workers() {
    within_10s(|| {
        let mut g = Topology::new();
        let a = g.worker::<Pinger>();
        let b = g.worker::<Ponger>();
        let (ia, ib) = (a.id(), b.id());
        g.set(&a, move || Pinger { to: ib, sent: false });
        g.set(&b, move || Ponger { to: ia });
        g.route::<Ping>(ia, ib, 1);
        g.route::<Pong>(ib, ia, 1);
        g.route::<Pong>(ia, LEADER, 1);
        let run = g.launch().expect("accepted");
        assert_eq!(await_frame::<Pong>(&run), Pong(8), "B answers A's 7 with 8");
        assert_eq!(run.join(), [Outcome::Returned, Outcome::Returned]);
    });
}

/// Doubles the `Ping` it receives and sends it to the leader as a `Pong`.
#[crate::state(steps(echo))]
struct Echo {}

#[crate::step(Echo, receives = Ping)]
fn echo() -> Status {
    match io::recv::<Ping>() {
        Some(Ping(v)) => {
            assert!(io::send(LEADER, Pong(v * 2)).is_ok(), "the leader's route is empty");
            Status::Done
        }
        None => Status::wait().frame(),
    }
}

#[test]
fn leader_sends_and_receives() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Echo>();
        g.set(&w, || Echo {});
        g.route::<Ping>(LEADER, w.id(), 1);
        g.route::<Pong>(w.id(), LEADER, 1);
        let run = g.launch().expect("accepted");
        assert_eq!(run.send(w.id(), Ping(21)), Ok(()));
        assert_eq!(await_frame::<Pong>(&run), Pong(42));
        assert_eq!(run.join(), [Outcome::Returned]);
    });
}

/// Sends five `Ping`s on a lossy route of capacity 2, then one `Pong` on another route. The leader
/// takes nothing before the `Pong`, so the `Ping`s are all sent before it reads any.
#[crate::state(steps(burst))]
struct Bursty {}

#[crate::step(Bursty)]
fn burst() -> Status {
    for i in 0..5 {
        io::send(LEADER, Ping(i)).expect("a lossy route never answers Full");
    }
    assert!(io::send(LEADER, Pong(0)).is_ok(), "the leader's route is empty");
    Status::Done
}

#[test]
fn lossy_route_overwrites_the_oldest_frame() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Bursty>();
        g.set(&w, || Bursty {});
        g.lossy::<Ping>(w.id(), LEADER, 2);
        g.route::<Pong>(w.id(), LEADER, 1);
        let run = g.launch().expect("accepted");
        let _done: Pong = await_frame(&run);
        assert_eq!(run.recv::<Ping>(), Some(Ping(3)));
        assert_eq!(run.recv::<Ping>(), Some(Ping(4)));
        assert_eq!(run.recv::<Ping>(), None);
        assert_eq!(run.join(), [Outcome::Returned]);
    });
}

/// Fills a route of capacity 2 and sends a third frame, which must answer `Full`.
#[crate::state(steps(fill))]
struct Filler {}

#[crate::step(Filler)]
fn fill() -> Status {
    assert_eq!(io::send(LEADER, Ping(1)), Ok(()));
    assert_eq!(io::send(LEADER, Ping(2)), Ok(()));
    assert_eq!(io::send(LEADER, Ping(3)), Err(Full), "third frame on a capacity-2 route");
    assert!(io::send(LEADER, Pong(9)).is_ok(), "the leader's route is empty");
    Status::Done
}

#[test]
fn full_only_at_capacity_on_a_non_lossy_route() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Filler>();
        g.set(&w, || Filler {});
        g.route::<Ping>(w.id(), LEADER, 2);
        g.route::<Pong>(w.id(), LEADER, 1);
        let run = g.launch().expect("accepted");
        assert_eq!(await_frame::<Pong>(&run), Pong(9));
        assert_eq!(run.recv::<Ping>(), Some(Ping(1)));
        assert_eq!(run.recv::<Ping>(), Some(Ping(2)));
        assert_eq!(run.recv::<Ping>(), None);
        assert_eq!(run.join(), [Outcome::Returned]);
    });
}
