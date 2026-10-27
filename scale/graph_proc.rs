//! The per-size process of scale_graph, stamped out by `graph!` (included by graph.rs with `#[macro_use]`). Its
//! names (WARM, CAP, Report, LEADER, common) resolve where the macro is invoked, in graph.rs.

/// One message type and process per size, stamped out as in ring.rs. `slot[s]` is the index in `rx` of the
/// in-neighbour with worker index s, or usize::MAX when s is not an in-neighbour.
macro_rules! graph {
    ($proc:ident, $msg:ident, $pad:literal, $worker:ident) => {
        #[trame::message]
        #[derive(Debug, PartialEq, Eq)]
        struct $msg { seq: u64, from: u64, pad: [u8; $pad] }

        struct $proc {
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
                    let r = Report { worker: self.me, ns, compute_ns: self.compute, checksum: self.x ^ self.acc };
                    return match trame::send(LEADER, r) {
                        Ok(()) => {
                            self.reported = true;
                            Status::Progress
                        }
                        Err(_) => Status::wait().room::<Report>(LEADER),
                    };
                }
                if !self.started {
                    let c0 = trame::now();
                    self.x = graph_net::rounds(self.x, self.iters);
                    if self.cur >= WARM {
                        self.compute += common::ns(trame::now().since(c0));
                    }
                    self.started = true;
                }
                for k in 0..self.outs.len() {
                    if !self.sent[k] {
                        let m = $msg { seq: self.cur, from: self.me, pad: [0; $pad] };
                        self.sent[k] = trame::send(self.outs[k], m).is_ok();
                    }
                }
                while let Some(m) = trame::recv::<$msg>() {
                    let k = self.slot.get(m.from as usize).copied().unwrap_or(usize::MAX);
                    if k == usize::MAX {
                        panic!("a frame from worker {}, which is not an in-neighbour of {}", m.from, self.me);
                    }
                    assert_eq!(m.seq, self.rx[k], "a frame out of order from worker {}", m.from);
                    self.rx[k] += 1;
                    self.acc = self.acc.wrapping_add(((m.from + 1) << 32) | (m.seq + 1));
                }
                if self.sent.iter().all(|&s| s) && self.rx.iter().all(|&r| r > self.cur) {
                    self.cur += 1;
                    self.sent.iter_mut().for_each(|s| *s = false);
                    self.started = false;
                    return Status::Progress;
                }
                let mut wait = Status::wait().frame();
                for k in (0..self.outs.len()).filter(|&k| !self.sent[k]) {
                    wait = wait.room::<$msg>(self.outs[k]);
                }
                wait
            }
        }

        fn $worker(cx: &Context, me: u64, offs: &[u64], steps: u64, iters: u64) {
            let ids = cx.workers();
            let w = ids.len() as u64;
            let outs = offs.iter().map(|&o| ids[((me + o) % w) as usize]).collect::<Vec<_>>();
            let mut slot = vec![usize::MAX; w as usize];
            for (k, &o) in offs.iter().enumerate() {
                slot[((me + w - o) % w) as usize] = k;
            }
            let d = offs.len();
            // A variable, not the literal: concurrent! pastes its process expression twice.
            let p = $proc {
                me, outs, slot, iters, total: WARM + steps, cur: 0, started: false,
                sent: vec![false; d], rx: vec![0; d], x: me + 1, acc: 0, compute: 0,
                t0: None, ns: None, reported: false,
            };
            let r = trame::concurrent!(p;);
            assert_eq!(r, Ok(()));
        }
    };
}
