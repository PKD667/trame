//! The sequential backend through the surface: routes, the leader, lossy routes, `Full`, and the
//! barrier with a publication. Every test runs under a 10 s guard, so a hang fails, not stalls.

extern crate std;

use crate::io::{self, Full, Message};
use crate::run::{Outcome, Run};
use crate::topology::{Context, LEADER, Topology};
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

/// Spin until this worker has a frame of type `M`.
fn await_mine<M: Message>() -> M {
    loop {
        if let Some(m) = io::recv::<M>() {
            return m;
        }
    }
}

fn ping(cx: &Context) {
    let b = cx.workers()[1];
    while io::send(b, Ping(7)).is_err() {}
    let reply: Pong = await_mine();
    while io::send(LEADER, reply).is_err() {}
}

fn pong(cx: &Context) {
    let a = cx.workers()[0];
    let Ping(v) = await_mine();
    while io::send(a, Pong(v + 1)).is_err() {}
}

#[test]
fn ping_pong_between_two_workers() {
    within_10s(|| {
        let mut g = Topology::new();
        let a = g.worker(ping);
        let b = g.worker(pong);
        g.route::<Ping>(a, b, 1);
        g.route::<Pong>(b, a, 1);
        g.route::<Pong>(a, LEADER, 1);
        let run = g.launch().expect("accepted");
        assert_eq!(await_frame::<Pong>(&run), Pong(8), "B answers A's 7 with 8");
        assert_eq!(run.join(), [Outcome::Returned, Outcome::Returned]);
    });
}

fn echo(_cx: &Context) {
    let Ping(v) = await_mine();
    while io::send(LEADER, Pong(v * 2)).is_err() {}
}

#[test]
fn leader_sends_and_receives() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(echo);
        g.route::<Ping>(LEADER, w, 1);
        g.route::<Pong>(w, LEADER, 1);
        let run = g.launch().expect("accepted");
        assert_eq!(run.send(w, Ping(21)), Ok(()));
        assert_eq!(await_frame::<Pong>(&run), Pong(42));
        assert_eq!(run.join(), [Outcome::Returned]);
    });
}

fn burst(_cx: &Context) {
    for i in 0..5 {
        io::send(LEADER, Ping(i)).expect("a lossy route never answers Full");
    }
    while io::send(LEADER, Pong(0)).is_err() {}
}

#[test]
fn lossy_route_overwrites_the_oldest_frame() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(burst);
        g.lossy::<Ping>(w, LEADER, 2);
        g.route::<Pong>(w, LEADER, 1);
        let run = g.launch().expect("accepted");
        let _done: Pong = await_frame(&run);
        assert_eq!(run.recv::<Ping>(), Some(Ping(3)));
        assert_eq!(run.recv::<Ping>(), Some(Ping(4)));
        assert_eq!(run.recv::<Ping>(), None);
        assert_eq!(run.join(), [Outcome::Returned]);
    });
}

fn fill(_cx: &Context) {
    assert_eq!(io::send(LEADER, Ping(1)), Ok(()));
    assert_eq!(io::send(LEADER, Ping(2)), Ok(()));
    assert_eq!(io::send(LEADER, Ping(3)), Err(Full), "third frame on a capacity-2 route");
    while io::send(LEADER, Pong(9)).is_err() {}
}

#[test]
fn full_only_at_capacity_on_a_non_lossy_route() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(fill);
        g.route::<Ping>(w, LEADER, 2);
        g.route::<Pong>(w, LEADER, 1);
        let run = g.launch().expect("accepted");
        assert_eq!(await_frame::<Pong>(&run), Pong(9));
        assert_eq!(run.recv::<Ping>(), Some(Ping(1)));
        assert_eq!(run.recv::<Ping>(), Some(Ping(2)));
        assert_eq!(run.recv::<Ping>(), None);
        assert_eq!(run.join(), [Outcome::Returned]);
    });
}
