//! The leader blocked in `Run::wait` when nothing can arrive: the call panics (INTERFACE.md 5, Deadlock),
//! and does not hang. Every test runs under the 10 s guard.

extern crate std;

use super::tests::{Ping, Quick, Stuck, panic_text, within_10s};
use crate::io;
use crate::status::Status;
use crate::topology::{Leader, Topology};

const STUCK: &str = "trame: deadlock: the leader waits and nothing can arrive";

/// Sends `Ping(7)` to the leader, once.
#[crate::state(steps(sends))]
struct Sender {}

#[crate::step(Sender)]
fn sends() -> Status {
    match io::send(Leader, Ping(7)) {
        Ok(()) => Status::Done,
        Err(_) => Status::wait().room::<Ping>(Leader),
    }
}

/// Two workers return and nothing is sent to the leader: its wait can never return.
#[test]
fn wait_after_every_worker_returned_panics() {
    within_10s(|| {
        let mut g = Topology::new();
        let a = g.worker::<Quick>();
        g.set(&a, || Quick {});
        let b = g.worker::<Quick>();
        g.set(&b, || Quick {});
        g.route::<Ping>(a.id(), Leader, 1);
        let run = g.launch().expect("accepted");
        let text = panic_text(move || {
            loop {
                run.wait();
            }
        });
        assert_eq!(text, STUCK);
    });
}

/// A worker waits for a write to a field that no step of its worker writes, and the other worker
/// has returned: nothing can move.
#[test]
#[cfg_attr(
    miri,
    ignore = "the run is dropped with a worker stuck, so its thread outlives the test; Miri refuses a main thread that ends first"
)]
fn wait_with_a_worker_that_waits_for_a_write_nothing_makes_panics() {
    within_10s(|| {
        let mut g = Topology::new();
        let a = g.worker::<Quick>();
        g.set(&a, || Quick {});
        let b = g.worker::<Stuck>();
        g.set(&b, || Stuck { never: false });
        let run = g.launch().expect("accepted");
        let text = panic_text(move || {
            loop {
                run.wait();
            }
        });
        assert_eq!(text, STUCK);
    });
}

/// A frame the leader can take is not stuck: the wait for it returns once the worker has sent.
#[test]
fn a_frame_from_a_worker_wakes_the_leader() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Sender>();
        g.set(&w, || Sender {});
        g.route::<Ping>(w.id(), Leader, 1);
        let run = g.launch().expect("accepted");
        let got = loop {
            if let Some(p) = run.recv::<Ping>().map(|s| s.value) {
                break p;
            }
            run.wait();
        };
        assert_eq!(got, Ping(7));
    });
}
