//! bench_ping SIZE SAMPLES GAP_NS [CAP]: round trips of a SIZE-byte frame between two workers, A and B. On one
//! host both are on it; on two, A is worker 0 of host 0 and B worker 0 of host 1, so every trip crosses hosts.
//! A takes a reading, sends, and takes another when B's echo arrives: `ns` is the round trip on A's clock,
//! through the whole interface (send, the route, B's wake and step, the route back, A's wake and step).
//! Before each send A waits a gap drawn uniformly from [0, GAP_NS] (xorshift64*, seed 1) with `.until`, so the
//! spacing does not phase-lock with the backend. After each trip A sends one Sample to its leader, outside the
//! timed span; host 0's leader prints one JSON line per sample, then the outcomes. The first WARM samples
//! carry "warm":true. Built with `--cfg trame_checked`.

#[macro_use]
#[path = "common.rs"]
mod common;

use core::time::Duration;
use trame::{HostId, Leader, Reading, Status, Topology, WorkerId};

const WARM: u64 = 100;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Sample {
    seq: u64,
    ns: u64,
    gap: u64,
}

macro_rules! ping {
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

            /// A: `out` is the reading at the send of the frame in flight, `next` the end of the gap, `rng` the
            /// gap's generator, `owed` a sample not yet given to the leader.
            #[trame::state(steps(trip))]
            pub struct A {
                b: WorkerId,
                samples: u64,
                gap_max: u64,
                seq: u64,
                out: Option<Reading>,
                next: Option<Reading>,
                gap: u64,
                rng: u64,
                owed: Option<Sample>,
            }

            #[trame::step(A, receives = Frame)]
            #[allow(clippy::too_many_arguments)]
            fn trip(
                b: &WorkerId, samples: &u64, gap_max: &u64, seq: &mut u64, out: &mut Option<Reading>,
                next: &mut Option<Reading>, gap: &mut u64, rng: &mut u64, owed: &mut Option<Sample>,
            ) -> Status {
                if let Some(s) = *owed {
                    if trame::send(Leader, s).is_err() {
                        return Status::wait().room::<Sample>(Leader);
                    }
                    *owed = None;
                }
                if let Some(t) = *out {
                    let Some(f) = trame::recv::<Frame>() else { return Status::wait().frame() };
                    let ns = common::ns(trame::now().since(t));
                    assert!(f.seq == *seq, "an echo out of order");
                    *owed = Some(Sample { seq: *seq, ns, gap: *gap });
                    *seq += 1;
                    *out = None;
                    // xorshift64*: the gap before the next send, uniform in [0, gap_max] up to a bias below 2^-40.
                    *rng ^= *rng >> 12;
                    *rng ^= *rng << 25;
                    *rng ^= *rng >> 27;
                    *gap = if *gap_max == 0 { 0 } else { rng.wrapping_mul(0x2545_F491_4F6C_DD1D) % (*gap_max + 1) };
                    *next = Some(trame::now().after(Duration::from_nanos(*gap)));
                    return Status::Progress;
                }
                if *seq == *samples {
                    return Status::Done;
                }
                if let Some(at) = *next {
                    if at.since(trame::now()) > Duration::ZERO {
                        return Status::wait().until(at);
                    }
                }
                let t = trame::now();
                match trame::send(*b, Frame { seq: *seq, from: 0, pad: [0; $pad] }) {
                    Ok(()) => {
                        *out = Some(t);
                        Status::Progress
                    }
                    Err(_) => Status::wait().room::<Frame>(*b),
                }
            }

            /// B: echoes each frame; `held` is an echo the route back had no room for.
            #[trame::state(steps(echo))]
            pub struct B {
                a: WorkerId,
                samples: u64,
                done: u64,
                held: Option<Frame>,
            }

            #[trame::step(B, receives = Frame)]
            fn echo(a: &WorkerId, samples: &u64, done: &mut u64, held: &mut Option<Frame>) -> Status {
                loop {
                    if let Some(f) = held.take() {
                        if let Err(_) = trame::send(*a, f) {
                            *held = Some(f);
                            return Status::wait().room::<Frame>(*a);
                        }
                        *done += 1;
                    }
                    if *done == *samples {
                        return Status::Done;
                    }
                    match trame::recv::<Frame>() {
                        Some(f) => *held = Some(f),
                        None => return Status::wait().frame(),
                    }
                }
            }

            /// Declares the workers of this host and the routes they share. Answers the size's index.
            pub fn build(t: &mut Topology, me: u32, hosts: u32, samples: u64, gap_max: u64, cap: usize) -> usize {
                let (a, b) = if hosts == 1 {
                    (WorkerId::from((HostId(0), 0)), WorkerId::from((HostId(0), 1)))
                } else {
                    (WorkerId::from((HostId(0), 0)), WorkerId::from((HostId(1), 0)))
                };
                if me == 0 {
                    let w = t.worker::<A>();
                    t.set(&w, move || A { b, samples, gap_max, seq: 0, out: None, next: None, gap: 0, rng: 1, owed: None });
                    t.route::<Sample>(w.id(), Leader, 1024);
                }
                if (hosts == 1 && me == 0) || (hosts > 1 && me == 1) {
                    let w = t.worker::<B>();
                    t.set(&w, move || B { a, samples, done: 0, held: None });
                }
                if me <= 1 {
                    t.route::<Frame>(a, b, cap);
                    t.route::<Frame>(b, a, cap);
                }
                $i
            }
        }
    };
}
sizes!(ping);

fn main() {
    let size: usize = common::arg(1, "SIZE");
    let samples: u64 = common::arg(2, "SAMPLES");
    let gap: u64 = common::arg(3, "GAP_NS");
    let cap: usize = std::env::args().nth(4).map_or(1, |s| s.parse().expect("CAP is a number"));
    let (me, hosts) = (trame::host().0, trame::hosts());
    if hosts > 2 || samples <= WARM || cap == 0 {
        eprintln!("usage: bench_ping SIZE SAMPLES>{WARM} GAP_NS [CAP>=1], on one or two hosts");
        std::process::exit(2);
    }
    let total = samples;
    let mut t = Topology::new();
    let i = common::size_index(size);
    let built = [s16::build, s64::build, s256::build, s1k::build, s4k::build, s16k::build, s64k::build][i](
        &mut t, me, hosts, total, gap, cap,
    );
    assert_eq!(built, i);
    let run = t.launch().unwrap_or_else(|e| common::refused(e));
    if me == 0 {
        common::collect::<Sample>(&run, total as usize, |s| {
            println!(
                "{{\"bench\":\"ping\",\"hosts\":{hosts},\"size\":{size},\"cap\":{cap},\"seq\":{},\"warm\":{},\"gap_ns\":{},\"ns\":{}}}",
                s.seq,
                s.seq < WARM,
                s.gap,
                s.ns
            );
        });
    }
    common::finish(run);
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
