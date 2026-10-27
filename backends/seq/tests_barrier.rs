//! The barrier: every worker waits for the others, every worker sees the host's publication, and a
//! leader publication takes effect at the barrier that fires after it.

extern crate std;

use super::tests::{await_frame, within_10s, Ping, Pong};
use crate::io;
use crate::run::Outcome;
use crate::topology::{Context, Topology, LEADER};

/// A worker's report: its index, and whether each barrier showed the expected publication.
#[crate::message]
#[derive(Debug, PartialEq, Eq)]
struct Rep {
    who: u32,
    first: u32,
    second: u32,
}

fn gate(cx: &Context) {
    let first = *io::barrier() == *b"first";
    while io::send(LEADER, Ping(cx.me().0)).is_err() {}
    // Wait for the leader's go, which it sends only after its publication.
    while io::recv::<Pong>().is_none() {}
    let second = *io::barrier() == *b"second";
    let report = Rep { who: cx.me().0, first: first as u32, second: second as u32 };
    while io::send(LEADER, report).is_err() {}
}

#[test]
fn barrier_shows_the_publication_and_a_later_leader_publication_takes_effect() {
    within_10s(|| {
        let mut g = Topology::new();
        let w0 = g.worker(gate);
        let w1 = g.worker(gate);
        for w in [w0, w1] {
            g.route::<Ping>(w, LEADER, 1);
            g.route::<Pong>(LEADER, w, 1);
            g.route::<Rep>(w, LEADER, 1);
        }
        g.publish(b"first");
        let run = g.launch().expect("accepted");
        await_frame::<Ping>(&run);
        await_frame::<Ping>(&run);
        run.publish(b"second");
        assert_eq!(run.send(w0, Pong(0)), Ok(()));
        assert_eq!(run.send(w1, Pong(0)), Ok(()));
        let mut reports = [await_frame::<Rep>(&run), await_frame::<Rep>(&run)];
        reports.sort_by_key(|r| r.who);
        let all_seen = Rep { who: 0, first: 1, second: 1 };
        assert_eq!(reports, [all_seen, Rep { who: 1, ..all_seen }]);
        assert_eq!(run.join(), [Outcome::Returned, Outcome::Returned]);
    });
}
