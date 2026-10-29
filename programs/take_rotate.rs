//! take_rotate: worker `Rx` takes `Item` frames from two routes with the same tag. Route 0 is fed by the
//! leader and never drains: after each take `Rx` reports `Took` and waits for `Ack`, and the leader sends
//! `Ack` only after a refill of route 0 was accepted. So `Rx` never takes twice without a refill between,
//! and route 0 holds at least three of its four frames at every take, even if `Ack` arrives before the
//! refill. Route 1 is fed once by worker `Tx` with `Item 2`. Spec.md 6, Progress: a frame that stays an
//! offer while its receiver keeps receiving is eventually received, so `Rx` must reach `Item 2`. A take
//! that always starts at route 0 takes `Item 1` forever: the run never ends, and a timeout reports it.
//! Spec.md 5.2 allows any order across routes, so the program prints only that `Item 2` arrived.

use trame::{Context, LEADER, Outcome, Status, Topology};

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Take,
    Report,
    AwaitAck,
}

/// Takes one `Item` per round. `Item 1` is reported with `Took`, then `Rx` waits for the leader's `Ack`;
/// `Item 2` ends the program with `Got`.
struct Rx {
    phase: Phase,
}

#[trame::process(receives = Item, Ack)]
impl Rx {
    fn step(&mut self) -> Status {
        match self.phase {
            Phase::Report => match trame::send(LEADER, Took) {
                Ok(()) => {
                    self.phase = Phase::AwaitAck;
                    Status::Progress
                }
                Err(_) => Status::wait().room::<Took>(LEADER),
            },
            Phase::AwaitAck => match trame::recv::<Ack>() {
                Some(Ack) => {
                    self.phase = Phase::Take;
                    Status::Progress
                }
                None => Status::wait().frame(),
            },
            Phase::Take => match trame::recv::<Item>() {
                Some(Item { id: 2 }) => {
                    assert!(trame::send(LEADER, Got).is_ok(), "got did not fit");
                    Status::Done
                }
                Some(Item { id: 1 }) => {
                    self.phase = Phase::Report;
                    Status::Progress
                }
                Some(_) => panic!("an unknown item"),
                None => Status::wait().frame(),
            },
        }
    }
}

fn rx(_cx: &Context) {
    let r = trame::concurrent!(Rx { phase: Phase::Take };);
    assert_eq!(r, Ok(()));
}

fn tx(cx: &Context) {
    // The receiver is the first worker declared (workers() lists them in declaration order).
    assert!(trame::send(cx.workers()[0], Item { id: 2 }).is_ok(), "item 2 did not fit");
}

/// Send `m` to `to`, waiting while the route is full. Across nodes a credit can arrive after the `Took`
/// that freed it (spec.md 5.2, Full); the leader's wait returns when it does.
fn send_when_room<M: trame::Message>(run: &trame::Run, to: trame::WorkerId, m: M) {
    while run.send(to, m).is_err() {
        run.wait();
    }
}

fn main() {
    let mut t = Topology::new();
    let rx_id = t.worker(rx);
    let tx_id = t.worker(tx);
    t.route::<Item>(LEADER, rx_id, 4);
    t.route::<Item>(tx_id, rx_id, 1);
    t.route::<Ack>(LEADER, rx_id, 1);
    t.route::<Took>(rx_id, LEADER, 1);
    t.route::<Got>(rx_id, LEADER, 1);
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
