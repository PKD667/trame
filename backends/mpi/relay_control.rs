// The control part of the out thread's scan (relay_out.rs): barrier arrival, results, exits, abort, and on
// the root the release and END. Each is a word the local agents set, read here with an acquire load, and
// sent once (the sent flags are the out thread's).

extern crate std;

use std::sync::atomic::Ordering::Acquire;

use super::layout::{COUNT, END, K_ARRIVE, K_EXITED, K_OUTCOME};
use super::nodes::arrival_done;
use super::relay_out::Out;
use super::wire::Record;

impl Out<'_> {
    /// The control words: barrier arrival, results, exits, abort, and on the root the release and END.
    pub(super) fn control(&mut self) {
        let ctx = self.ctx;
        let t = &ctx.port;
        let nodes = t.nodes();
        let me = nodes.me as usize;
        let workers = t.lay().workers;
        let root = nodes.node_of[workers as usize] as usize;
        let c = t.win().word(COUNT).load(Acquire);
        if let Some(g) = arrival_done(c, u64::from(workers), u64::from(nodes.local_workers)) {
            if g > self.reported {
                self.reported = g;
                self.push(root, &Record::Arrived { generation: g });
            }
        }
        // The local agents in rank order; the leader's rank W, if it is here, comes last and is not a worker.
        for &w in nodes.local.iter().take_while(|&&w| w < workers) {
            let wi = w as usize;
            if !self.result_sent[wi] {
                let outcome = t.win().word(t.lay().per_worker(K_OUTCOME, w)).load(Acquire);
                if outcome != 0 {
                    self.result_sent[wi] = true;
                    self.to_others(me, &Record::Result { worker: w, outcome });
                }
            }
            if !self.exit_sent[wi] && t.win().word(t.lay().per_worker(K_EXITED, w)).load(Acquire) == 1 {
                // Exited first, then arrived (gate.rs): the arrive word is final by now.
                let arrive = t.win().word(t.lay().per_worker(K_ARRIVE, w)).load(Acquire);
                self.exit_sent[wi] = true;
                self.to_others(me, &Record::Exited { worker: w, arrive });
            }
        }
        // An abort this node has, and did not get from the network, goes to every other node once.
        if t.aborted() && !self.abort_sent && !ctx.aborted.load(Acquire) {
            self.abort_sent = true;
            self.to_others(me, &Record::Abort);
        }
        if me == root {
            let release = ctx.release.lock().expect("trame: the release lock").take();
            if let Some(r) = release {
                for n in 0..nodes.count as usize {
                    self.push(n, &Record::Release { generation: r.generation, no: r.no, bytes: &r.bytes });
                }
            }
            if !self.end_sent && t.win().word(END).load(Acquire) == 1 {
                self.end_sent = true;
                for n in 0..nodes.count as usize {
                    self.push(n, &Record::End);
                }
            }
        }
    }

    /// Queue `r` for every node but `me`.
    fn to_others(&mut self, me: usize, r: &Record) {
        for n in 0..self.bufs.len() {
            if n != me {
                self.push(n, r);
            }
        }
    }
}
