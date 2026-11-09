//! take_rotate: worker 0 (`Rx`) takes `Item` frames from two routes with the same tag. Route 0 is fed by the
//! leader and never drains: after each take `Rx` reports `Took` and waits for `Ack`, and the leader sends
//! `Ack` only after a refill of route 0 was accepted. So `Rx` never takes twice without a refill between,
//! and route 0 holds at least three of its four frames at every take, even if `Ack` arrives before the
//! refill. Route 1 is fed once by worker 1 (`Tx`) with `Item 2`. Spec.md 6, Progress: a frame that stays an
//! offer while its receiver keeps receiving is eventually received, so `Rx` must reach `Item 2`. A take
//! that always starts at route 0 takes `Item 1` forever: the run never ends, and a timeout reports it.
//! Spec.md 5.2 allows any order across routes, so the program prints only that `Item 2` arrived.

use trame::{Leader, Outcome, Status, Topology, WorkerId};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Item {
    id: u32,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Took;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Ack;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Got;

#[derive(Clone, Copy, PartialEq, Eq, trame::Plain)]
enum Phase {
    Take,
    Report,
    AwaitAck,
}

/// Worker 0: takes one `Item` per round. `Item 1` is reported with `Took`, then `Rx` waits for the
/// leader's `Ack`; `Item 2` ends the program with `Got`.
#[trame::state(steps(rx))]
struct Rx {
    phase: Phase,
}

#[trame::step(Rx, receives = Item, Ack)]
fn rx(phase: &mut Phase) -> Status {
    match *phase {
        Phase::Report => match trame::send(Leader, Took) {
            Ok(()) => {
                *phase = Phase::AwaitAck;
                Status::Progress
            }
            Err(_) => Status::wait().room::<Took>(Leader),
        },
        Phase::AwaitAck => match trame::recv::<Ack>() {
            Some(Ack) => {
                *phase = Phase::Take;
                Status::Progress
            }
            None => Status::wait().frame(),
        },
        Phase::Take => match trame::recv::<Item>() {
            Some(Item { id: 2 }) => {
                assert!(trame::send(Leader, Got).is_ok(), "got did not fit");
                Status::Done
            }
            Some(Item { id: 1 }) => {
                *phase = Phase::Report;
                Status::Progress
            }
            Some(_) => panic!("an unknown item"),
            None => Status::wait().frame(),
        },
    }
}

/// Worker 1: sends `Item 2` to `to`, the receiver.
#[trame::state(steps(tx))]
struct Tx {
    to: WorkerId,
}

#[trame::step(Tx)]
fn tx(to: &WorkerId) -> Status {
    assert!(trame::send(*to, Item { id: 2 }).is_ok(), "item 2 did not fit");
    Status::Done
}

/// Send `m` to `to`, waiting while the route is full. Across nodes a credit can arrive after the `Took`
/// that freed it (INTERFACE.md 5.2, Full); the leader's wait returns when it does.
fn send_when_room<M: trame::Message>(run: &trame::Run, to: trame::WorkerId, m: M) {
    while run.send(to, m).is_err() {
        run.wait();
    }
}

fn main() {
    let mut t = Topology::new();
    let rx = t.worker();
    let tx = t.worker();
    let (rx_id, tx_id) = (rx.id(), tx.id());
    t.set(&rx, || Rx { phase: Phase::Take });
    t.set(&tx, move || Tx { to: rx_id });
    t.route::<Item>(Leader, rx_id, 4);
    t.route::<Item>(tx_id, rx_id, 1);
    t.route::<Ack>(Leader, rx_id, 1);
    t.route::<Took>(rx_id, Leader, 1);
    t.route::<Got>(rx_id, Leader, 1);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    for _ in 0..4 {
        assert!(run.send(rx_id, Item { id: 1 }).is_ok(), "stock did not fit");
    }
    loop {
        if run.recv::<Got>().is_some() {
            break;
        }
        if run.recv::<Took>().is_some() {
            send_when_room(&run, rx_id, Item { id: 1 });
            send_when_room(&run, rx_id, Ack);
            continue;
        }
        run.wait();
    }
    println!("received item 2");
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
