//! bench_flow PATTERN PER SIZE CAP STEPS [trace]: a stream of SIZE-byte frames from every worker to one other,
//! over a lossless route of capacity CAP. Each host runs PER workers; global worker g = h * PER + k.
//! PATTERN `ring` sends from g to g + 1 (mod H * PER): one route in PER leaves each host. PATTERN `shift`
//! sends from (h, k) to (h + 1 mod H, k): with two or more hosts every route crosses hosts, and on one host
//! it is `ring`. A worker sends WARM + STEPS frames and receives as many, checking their order; its clock
//! runs from the end of the warm-up (WARM sent and WARM received) to the end. Bytes per nanosecond of one
//! worker are STEPS * SIZE / ns, on its own clock; the sum over workers is the aggregate bandwidth.
//! With `trace`, every worker records its trace and the leader replays it through the model (CONTRACT S9):
//! the line `trace` says whether every trace is a run of the model. Each leader prints one JSON line per
//! worker of its host, then its outcomes. Built with `--cfg trame_checked`.

#[macro_use]
#[path = "common.rs"]
mod common;

use trame::{Leader, Reading, Status, Topology, Worker, WorkerId};

const WARM: u64 = 64;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    g: u64,
    ns: u64,
}

/// One message type, state, step and builder per size (common::sizes!).
macro_rules! flow {
    ($S:ident, $pad:literal, $i:literal) => {
        mod $S {
            use super::*;

            #[trame::message]
            #[derive(Debug, PartialEq, Eq)]
            pub struct Frame {
                seq: u64,
                from: u64,
                pad: [u8; $pad],
            }

            #[trame::state(steps(stream))]
            pub struct Node {
                g: u64,
                src: u64,
                to: WorkerId,
                total: u64,
                sent: u64,
                got: u64,
                t0: Option<Reading>,
                ns: Option<u64>,
            }

            /// Sends what the route takes, receives what has arrived, then waits for room or a frame.
            #[trame::step(Node, receives = Frame)]
            #[allow(clippy::too_many_arguments)]
            fn stream(
                g: &u64,
                src: &u64,
                to: &WorkerId,
                total: &u64,
                sent: &mut u64,
                got: &mut u64,
                t0: &mut Option<Reading>,
                ns: &mut Option<u64>,
            ) -> Status {
                if let Some(ns) = *ns {
                    return match trame::send(Leader, Report { g: *g, ns }) {
                        Ok(()) => Status::Done,
                        Err(_) => Status::wait().room::<Report>(Leader),
                    };
                }
                let mut moved = false;
                while *sent < *total && trame::send(*to, Frame { seq: *sent, from: *g, pad: [0; $pad] }).is_ok() {
                    *sent += 1;
                    moved = true;
                }
                while let Some(f) = trame::recv::<Frame>() {
                    assert!(f.seq == *got && f.from == *src, "a frame out of order, lost, duplicated or misrouted");
                    *got += 1;
                    moved = true;
                }
                if t0.is_none() && *sent >= WARM && *got >= WARM {
                    *t0 = Some(trame::now());
                }
                if *sent == *total && *got == *total {
                    *ns = Some(common::ns(trame::now().since(t0.expect("the warm-up ended")).expect("one worker has one clock")));
                    return Status::Progress;
                }
                if moved {
                    return Status::Progress;
                }
                let w = Status::wait().frame();
                if *sent < *total { w.room::<Frame>(*to) } else { w }
            }

            /// Declares this host's PER workers, their route out and, from another host, the route in.
            pub fn build(
                t: &mut Topology,
                me: u32,
                per: u32,
                next: &dyn Fn(u32) -> u32,
                prev: &dyn Fn(u32) -> u32,
                cap: usize,
                total: u64,
            ) -> usize {
                let ws: Vec<Worker<Node>> = (0..per).map(|_| t.worker()).collect();
                for (k, w) in ws.iter().enumerate() {
                    let g = me * per + k as u32;
                    let (to, from) = (common::place(next(g), per), common::place(prev(g), per));
                    let (gg, src) = (u64::from(g), u64::from(prev(g)));
                    t.set(w, move || Node { g: gg, src, to, total, sent: 0, got: 0, t0: None, ns: None });
                    t.route::<Frame>(w.id(), to, cap);
                    if prev(g) / per != me {
                        t.route::<Frame>(from, w.id(), cap);
                    }
                    t.route::<Report>(w.id(), Leader, 1);
                }
                $i
            }
        }
    };
}
sizes!(flow);

fn main() {
    let pattern: String = common::arg(1, "PATTERN");
    let per: u32 = common::arg(2, "PER");
    let size: usize = common::arg(3, "SIZE");
    let cap: usize = common::arg(4, "CAP");
    let steps: u64 = common::arg(5, "STEPS");
    let trace = std::env::args().nth(6).as_deref() == Some("trace");
    let (me, hosts) = (trame::host().0, trame::hosts());
    let n = hosts * per;
    let shift = pattern == "shift" && hosts > 1;
    if !(pattern == "ring" || pattern == "shift") || per == 0 || steps == 0 || n < 2 {
        eprintln!("usage: bench_flow ring|shift PER>=1 SIZE CAP STEPS>=1 [trace], with H * PER >= 2");
        std::process::exit(2);
    }
    let next = move |g: u32| if shift { (g + per) % n } else { (g + 1) % n };
    let prev = move |g: u32| if shift { (g + n - per) % n } else { (g + n - 1) % n };
    let mut t = Topology::new();
    if trace {
        t.__trace();
    }
    let total = WARM + steps;
    let i = common::size_index(size);
    let built = [s16::build, s64::build, s256::build, s1k::build, s4k::build, s16k::build, s64k::build][i](
        &mut t, me, per, &next, &prev, cap, total,
    );
    assert_eq!(built, i);
    let run = t.launch().unwrap_or_else(|e| common::refused(e));
    common::collect::<Report>(&run, per as usize, |r| {
        println!(
            "{{\"bench\":\"flow\",\"pattern\":\"{pattern}\",\"hosts\":{hosts},\"host\":{me},\"per\":{per},\"g\":{},\"size\":{size},\"cap\":{cap},\"steps\":{steps},\"ns\":{}}}",
            r.g, r.ns
        );
    });
    if trace {
        let (outcomes, checked) = run.__join_checked(false);
        let ok = checked.is_ok();
        println!("{{\"trace\":{ok},\"detail\":\"{checked:?}\"}}");
        println!("{{\"outcomes\":\"{outcomes:?}\"}}");
    } else {
        common::finish(run);
    }
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
