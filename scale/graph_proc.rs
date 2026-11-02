//! The per-size state and step of scale_graph, stamped out by `graph!` (included by graph.rs with `#[macro_use]`). Its
//! names (WARM, CAP, Report, LEADER, common) resolve where the macro is invoked, in graph.rs.

/// One message type, state and step per size, stamped out as in ring.rs. `slot[s]` is the index in `rx` of the
/// in-neighbour with worker index s, or usize::MAX when s is not an in-neighbour. `$build` declares and fills the
/// W workers of this size, routes each to its out-neighbours, and answers their ids.
macro_rules! graph {
    ($state:ident, $step:ident, $msg:ident, $pad:literal, $build:ident) => {
        #[trame::message]
        #[derive(Debug, PartialEq, Eq)]
        struct $msg { seq: u64, from: u64, pad: [u8; $pad] }

        #[trame::state(steps($step))]
        struct $state {
            me: u64,
            outs: Vec<WorkerId>,
            slot: Vec<usize>,
            iters: u64,
            total: u64,
            cur: u64,
            started: bool,
            sent: Vec<bool>,
            rx: Vec<u64>,
            x: u64,
            acc: u64,
            compute: u64,
            t0: Option<Reading>,
            ns: Option<u64>,
            reported: bool,
        }

        #[trame::step($state, receives = $msg)]
        fn $step(
            me: &u64, outs: &Vec<WorkerId>, slot: &Vec<usize>, iters: &u64, total: &u64, cur: &mut u64,
            started: &mut bool, sent: &mut Vec<bool>, rx: &mut Vec<u64>, x: &mut u64, acc: &mut u64,
            compute: &mut u64, t0: &mut Option<Reading>, ns: &mut Option<u64>, reported: &mut bool,
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
                let r = Report { worker: *me, ns, compute_ns: *compute, checksum: *x ^ *acc };
                return match trame::send(LEADER, r) {
                    Ok(()) => {
                        *reported = true;
                        Status::Progress
                    }
                    Err(_) => Status::wait().room::<Report>(LEADER),
                };
            }
            if !*started {
                let c0 = trame::now();
                *x = graph_net::rounds(*x, *iters);
                if *cur >= WARM {
                    *compute += common::ns(trame::now().since(c0));
                }
                *started = true;
            }
            for k in 0..outs.len() {
                if !sent[k] {
                    let m = $msg { seq: *cur, from: *me, pad: [0; $pad] };
                    sent[k] = trame::send(outs[k], m).is_ok();
                }
            }
            while let Some(m) = trame::recv::<$msg>() {
                let k = slot.get(m.from as usize).copied().unwrap_or(usize::MAX);
                if k == usize::MAX {
                    panic!("a frame from worker {}, which is not an in-neighbour of {}", m.from, *me);
                }
                assert_eq!(m.seq, rx[k], "a frame out of order from worker {}", m.from);
                rx[k] += 1;
                *acc = acc.wrapping_add(((m.from + 1) << 32) | (m.seq + 1));
            }
            if sent.iter().all(|&s| s) && rx.iter().all(|&r| r > *cur) {
                *cur += 1;
                sent.iter_mut().for_each(|s| *s = false);
                *started = false;
                return Status::Progress;
            }
            let mut wait = Status::wait().frame();
            for k in (0..outs.len()).filter(|&k| !sent[k]) {
                wait = wait.room::<$msg>(outs[k]);
            }
            wait
        }

        fn $build(t: &mut Topology, w: usize, offs: &[u64], steps: u64, iters: u64) -> Vec<WorkerId> {
            let ws: Vec<Worker<$state>> = (0..w).map(|_| t.worker()).collect();
            let ids: Vec<WorkerId> = ws.iter().map(Worker::id).collect();
            for (i, h) in ws.iter().enumerate() {
                let (me, wn) = (i as u64, w as u64);
                let outs: Vec<WorkerId> = offs.iter().map(|&o| ids[((me + o) % wn) as usize]).collect();
                for &to in &outs {
                    t.route::<$msg>(ids[i], to, CAP);
                }
                let offs = offs.to_vec();
                // Built at launch, where the worker runs (CONTRACT.md, M1): its vectors are allocated there.
                t.set(h, move || {
                    let mut slot = vec![usize::MAX; wn as usize];
                    for (k, &o) in offs.iter().enumerate() {
                        slot[((me + wn - o) % wn) as usize] = k;
                    }
                    let d = offs.len();
                    $state {
                        me, outs, slot, iters, total: WARM + steps, cur: 0, started: false,
                        sent: vec![false; d], rx: vec![0; d], x: me + 1, acc: 0, compute: 0,
                        t0: None, ns: None, reported: false,
                    }
                });
            }
            ids
        }
    };
}
