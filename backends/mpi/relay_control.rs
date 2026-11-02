// The control part of the out thread's scan (relay_out.rs): results, abort, and on the root END. Each is a word the local agents set, read here with an acquire load, and
// sent once (the sent flags are the out thread's).

extern crate std;

use std::sync::atomic::Ordering::Acquire;

use super::layout::{END, K_OUTCOME};
use super::relay_out::Out;
use super::wire::Record;

impl Out<'_> {
    /// The control words: results, abort, and on the root END.
    pub(super) fn control(&mut self) {
        let ctx = self.ctx;
        let t = &ctx.port;
        let nodes = t.nodes();
        let me = nodes.me as usize;
        let workers = t.lay().workers;
        let root = nodes.node_of[workers as usize] as usize;
        // The local agents in rank order; the leader's rank W, if it is here, comes last and is not a worker.
        for &w in nodes.local.iter().take_while(|&&w| w < workers) {
            let wi = w as usize;
            if !self.result_sent[wi] {
                let outcome = t.win().word(t.lay().per_worker(K_OUTCOME, w)).load(Acquire);
                if outcome != 0 {
                    self.result_sent[wi] = true;
                    self.queue_others(me, &Record::Result { worker: w, outcome });
                }
            }
        }
        // An abort this node has, and did not get from the network, goes to every other node once.
        if t.aborted() && !self.abort_sent && !ctx.aborted.load(Acquire) {
            self.abort_sent = true;
            let (worker, bytes) = t.abort_record();
            self.queue_others(me, &Record::Abort { worker, bytes: &bytes });
        }
        if me == root {
            if !self.end_sent && t.win().word(END).load(Acquire) == 1 {
                self.end_sent = true;
                for n in 0..nodes.count as usize {
                    self.push(n, &Record::End);
                }
            }
        }
    }

    /// Queue `r` for every node but `me`.
    fn queue_others(&mut self, me: usize, r: &Record) {
        for n in 0..self.bufs.len() {
            if n != me {
                self.push(n, r);
            }
        }
    }
}
