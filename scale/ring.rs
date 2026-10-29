//! scale_ring W STEPS SIZE CAP: W >= 3 workers in a ring. Worker w has a lossless route of capacity CAP
//! to each neighbour, w+1 and w-1, carrying `Small` (64 bytes) or `Big` (4096 bytes). Each step a worker
//! sends one message to each neighbour and receives one from each. Payload: bytes 0..8 the step, 8..16 the
//! sender. The first WARM steps are a warm-up; the clock starts when it ends. The leader prints one JSON
//! line per worker, then the outcomes. Built with `--cfg trame_checked`.

#[path = "common.rs"]
mod common;

use trame::{Context, LEADER, Message, Reading, Status, Topology, WorkerId};

const WARM: u64 = 10;
#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    worker: u64,
    ns: u64,
}

/// One message type and process per size: a process names its type in `receives`, so both are stamped out.
macro_rules! ring {
    ($proc:ident, $msg:ident, $pad:literal, $worker:ident) => {
        #[trame::message]
        #[derive(Debug, PartialEq, Eq)]
        struct $msg { seq: u64, from: u64, pad: [u8; $pad] }

        /// `rx[k]`: frames received from side k (0 right, 1 left). A step ends when both sends and both
        /// frames of the step are done; an early frame of the next step is counted, not stored.
        struct $proc {
            me: u64,
            right: u64,
            left: u64,
            peers: [WorkerId; 2],
            cur: u64,
            total: u64,
            sent: [bool; 2],
            rx: [u64; 2],
            t0: Option<Reading>,
            ns: Option<u64>,
            reported: bool,
        }

        #[trame::process(receives = $msg)]
        impl $proc {
            fn step(&mut self) -> Status {
                if self.cur == WARM && self.t0.is_none() {
                    self.t0 = Some(trame::now());
                }
                if self.cur == self.total {
                    let t0 = self.t0.expect("the warm-up ended");
                    let ns = *self.ns.get_or_insert_with(|| common::ns(trame::now().since(t0)));
                    if self.reported {
                        return Status::Done;
                    }
                    return match trame::send(LEADER, Report { worker: self.me, ns }) {
                        Ok(()) => {
                            self.reported = true;
                            Status::Progress
                        }
                        Err(_) => Status::wait().room::<Report>(LEADER),
                    };
                }
                let (right, left, peers) = (self.right, self.left, self.peers);
                for k in 0..2 {
                    if !self.sent[k] {
                        let m = $msg { seq: self.cur, from: self.me, pad: [0; $pad] };
                        self.sent[k] = trame::send(peers[k], m).is_ok();
                    }
                }
                while let Some(m) = trame::recv::<$msg>() {
                    let k = if m.from == right {
                        0
                    } else if m.from == left {
                        1
                    } else {
                        panic!("a frame from worker {}, which is not a neighbour of {}", m.from, self.me)
                    };
                    assert_eq!(m.seq, self.rx[k], "a frame out of order from worker {}", m.from);
                    self.rx[k] += 1;
                }
                if self.sent == [true, true] && self.rx[0] > self.cur && self.rx[1] > self.cur {
                    self.cur += 1;
                    self.sent = [false; 2];
                    return Status::Progress;
                }
                let mut wait = Status::wait().frame();
                for k in (0..2).filter(|&k| !self.sent[k]) {
                    wait = wait.room::<$msg>(peers[k]);
                }
                wait
            }
        }

        fn $worker(cx: &Context, me: u64, steps: u64) {
            let ids = cx.workers();
            let n = ids.len() as u64;
            let right = (me + 1) % n;
            let left = (me + n - 1) % n;
            let peers = [ids[right as usize], ids[left as usize]];
            let r = trame::concurrent!($proc { me, right, left, peers, cur: 0, total: WARM + steps, sent: [false; 2], rx: [0; 2], t0: None, ns: None, reported: false };);
            assert_eq!(r, Ok(()));
        }
    };
}

ring!(RingSmall, Small, 48, worker_small);
ring!(RingBig, Big, 4080, worker_big);

fn wire<M: Message>(t: &mut Topology, from: WorkerId, peers: [WorkerId; 2], cap: usize) {
    for to in peers {
        t.route::<M>(from, to, cap);
    }
}

fn main() {
    let w: usize = common::arg(1, "W");
    let steps: u64 = common::arg(2, "STEPS");
    let size: usize = common::arg(3, "SIZE");
    let cap: usize = common::arg(4, "CAP");
    // W >= 3: with two workers both neighbours are one worker, and the topology refuses its duplicate routes.
    if w < 3 || (size != 64 && size != 4096) || steps == 0 {
        eprintln!("usage: scale_ring W>=3 STEPS>=1 SIZE in {{64,4096}} CAP");
        std::process::exit(2);
    }
    let mut t = Topology::new();
    let mut ids = Vec::new();
    for i in 0..w {
        let id = if size == 64 {
            t.worker(move |cx: &Context| worker_small(cx, i as u64, steps))
        } else {
            t.worker(move |cx: &Context| worker_big(cx, i as u64, steps))
        };
        ids.push(id);
    }
    for i in 0..w {
        let peers = [ids[(i + 1) % w], ids[(i + w - 1) % w]];
        if size == 64 {
            wire::<Small>(&mut t, ids[i], peers, cap);
        } else {
            wire::<Big>(&mut t, ids[i], peers, cap);
        }
        t.route::<Report>(ids[i], LEADER, 1);
    }
    let run = t.launch().unwrap_or_else(|e| common::refused(e));
    common::collect::<Report>(&run, w, |r| {
        println!(
            "{{\"bench\":\"ring\",\"workers\":{w},\"worker\":{},\"steps\":{steps},\"size\":{size},\"cap\":{cap},\"ns\":{}}}",
            r.worker, r.ns
        );
    });
    common::finish(run);
}
