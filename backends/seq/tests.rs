//! The sequential backend through the surface: routes, the leader, lossy routes, `Full`, and the
//! barrier with a publication. Every test runs under a 10 s guard, so a hang fails, not stalls.

extern crate std;

use crate::io::{self, Full, Message};
use crate::run::{Outcome, Run};
use crate::topology::{Context, Topology, LEADER};
use std::thread;
use std::time::{Duration, Instant};

#[crate::message]
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Ping(pub u32);

#[crate::message]
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Pong(pub u32);

/// Run `f` on its own thread. Fail if it has not finished in 10 s, so a hang cannot stall the suite.
pub(super) fn within_10s<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let handle = thread::spawn(f);
    let start = Instant::now();
    while !handle.is_finished() {
        assert!(start.elapsed() < Duration::from_secs(10), "hung: no answer within 10 s");
        thread::sleep(Duration::from_millis(5));
    }
    handle.join().expect("the test body panicked (its message is above)")
}

/// Spin until the leader has a frame of type `M`. Only ever waits on a worker's effects.
pub(super) fn await_frame<M: Message>(run: &Run) -> M {
    loop {
        if let Some(m) = run.recv::<M>() {
            return m;
        }
        thread::yield_now();
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
