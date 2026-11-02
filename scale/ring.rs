//! scale_ring W STEPS SIZE CAP: W >= 3 workers in a ring. Worker w has a lossless route of capacity CAP
//! to each neighbour, w+1 and w-1, carrying `Small` (64 bytes) or `Big` (4096 bytes). Each step a worker
//! sends one message to each neighbour and receives one from each. Payload: bytes 0..8 the step, 8..16 the
//! sender. The first WARM steps are a warm-up; the clock starts when it ends. The leader prints one JSON
//! line per worker, then the outcomes. Built with `--cfg trame_checked`.

#[path = "common.rs"]
mod common;

use trame::{LEADER, Reading, Status, Topology, Worker, WorkerId};

const WARM: u64 = 10;
#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    worker: u64,
    ns: u64,
}

/// One message type, state and step per size: a step names its message type in `receives`, so all are stamped out.
/// `rx[k]`: frames received from side k (0 right, 1 left). A step ends when both sends and both frames of the
/// step are done; an early frame of the next step is counted, not stored. `$build` declares and fills the W
/// workers of this size and routes each to its two neighbours; it answers their ids.
macro_rules! ring {
    ($state:ident, $step:ident, $msg:ident, $pad:literal, $build:ident) => {
        #[trame::message]
        #[derive(Debug, PartialEq, Eq)]
        struct $msg { seq: u64, from: u64, pad: [u8; $pad] }

        #[trame::state(steps($step))]
        struct $state {
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

        #[trame::step($state, receives = $msg)]
        fn $step(
            me: &u64, right: &u64, left: &u64, peers: &[WorkerId; 2], cur: &mut u64, total: &u64,
            sent: &mut [bool; 2], rx: &mut [u64; 2], t0: &mut Option<Reading>, ns: &mut Option<u64>,
            reported: &mut bool,
        ) -> Status {
            if *cur == WARM && t0.is_none() {
                *t0 = Some(trame::now());
            }
            if *cur == *total {
                let start = t0.expect("the warm-up ended");
                let ns = *ns.get_or_insert_with(|| common::ns(trame::now().since(start)));
                if *reported {
                    return Status::Done;
                }
                return match trame::send(LEADER, Report { worker: *me, ns }) {
                    Ok(()) => {
                        *reported = true;
                        Status::Progress
                    }
                    Err(_) => Status::wait().room::<Report>(LEADER),
                };
            }
            for k in 0..2 {
                if !sent[k] {
                    let m = $msg { seq: *cur, from: *me, pad: [0; $pad] };
                    sent[k] = trame::send(peers[k], m).is_ok();
                }
            }
            while let Some(m) = trame::recv::<$msg>() {
                let k = if m.from == *right {
                    0
                } else if m.from == *left {
                    1
                } else {
                    panic!("a frame from worker {}, which is not a neighbour of {}", m.from, *me)
                };
                assert_eq!(m.seq, rx[k], "a frame out of order from worker {}", m.from);
                rx[k] += 1;
            }
            if *sent == [true, true] && rx[0] > *cur && rx[1] > *cur {
                *cur += 1;
                *sent = [false; 2];
                return Status::Progress;
            }
            let mut wait = Status::wait().frame();
            for k in (0..2).filter(|&k| !sent[k]) {
                wait = wait.room::<$msg>(peers[k]);
            }
            wait
        }

        fn $build(t: &mut Topology, w: usize, steps: u64, cap: usize) -> Vec<WorkerId> {
            let ws: Vec<Worker<$state>> = (0..w).map(|_| t.worker()).collect();
            for (i, h) in ws.iter().enumerate() {
                let (right, left) = ((i + 1) % w, (i + w - 1) % w);
                let peers = [ws[right].id(), ws[left].id()];
                let (me, right, left) = (i as u64, right as u64, left as u64);
                t.set(h, move || $state {
                    me, right, left, peers, cur: 0, total: WARM + steps, sent: [false; 2], rx: [0; 2],
                    t0: None, ns: None, reported: false,
                });
                for to in peers {
                    t.route::<$msg>(h.id(), to, cap);
                }
            }
            ws.iter().map(Worker::id).collect()
        }
    };
}

ring!(RingSmall, ring_small, Small, 48, build_small);
ring!(RingBig, ring_big, Big, 4080, build_big);

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
    let ids = if size == 64 { build_small(&mut t, w, steps, cap) } else { build_big(&mut t, w, steps, cap) };
    for id in ids {
        t.route::<Report>(id, LEADER, 1);
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

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
