//! Plain-data types are messages, arrays included (spec.md 3, "Messages"): a field `[Spike; 4]` and
//! a route of type `[Spike; 4]` both launch and round-trip.

extern crate std;

use super::tests::{await_frame, within_10s};
use crate::io;
use crate::status::Status;
use crate::topology::{Leader, Topology};

#[crate::message]
#[derive(Debug, PartialEq, Eq)]
struct Spike {
    neuron: u32,
    t: u32,
}

#[crate::message]
#[derive(Debug, PartialEq, Eq)]
struct Burst {
    id: u32,
    spikes: [Spike; 4],
}

/// Sends one `Burst` to the leader.
#[crate::state(steps(send_burst))]
struct Sender {}

#[crate::step(Sender)]
fn send_burst() -> Status {
    let spikes = [
        Spike { neuron: 1, t: 10 },
        Spike { neuron: 2, t: 11 },
        Spike { neuron: 3, t: 12 },
        Spike { neuron: 4, t: 13 },
    ];
    match io::send(Leader, Burst { id: 9, spikes }) {
        Ok(()) => Status::Done,
        Err(_) => Status::wait().room::<Burst>(Leader),
    }
}

#[test]
fn a_message_with_an_array_field_round_trips() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Sender>();
        g.set(&w, || Sender {});
        g.route::<Burst>(w.id(), Leader, 1);
        let run = g.launch().expect("accepted");
        let got: Burst = await_frame(&run);
        assert_eq!(got.id, 9);
        assert_eq!(got.spikes[3], Spike { neuron: 4, t: 13 });
        assert_eq!(run.join().len(), 1);
    });
}

#[test]
fn an_array_of_message_structs_is_a_message_type() {
    fn message<M: crate::io::Message>() {}
    message::<[Spike; 4]>();
}
