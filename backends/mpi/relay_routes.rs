// The route part of the out thread's scan (relay_out.rs): frames out on outbound routes, and credits back on
// inbound lossless routes. Both read the route's words with the same acquire loads a local agent uses.

extern crate std;

use std::sync::atomic::Ordering::Acquire;
use std::vec;

use super::layout::{RouteLayout, LINE};
use super::relay_out::Out;
use super::wire::Record;

impl Out<'_> {
    /// The routes with one end here: frames out on outbound routes, credits back on inbound lossless ones.
    pub(super) fn routes(&mut self) {
        let ctx = self.ctx;
        let t = &ctx.port;
        let nodes = t.nodes();
        for (id, r) in t.lay().routes.iter().enumerate() {
            let from_here = nodes.node_of[r.from as usize] == nodes.me;
            let to_here = nodes.node_of[r.to as usize] == nodes.me;
            if from_here && !to_here {
                self.frames(id, *r);
            } else if to_here && !from_here && !r.lossy {
                self.credit(id, *r);
            }
        }
    }

    /// Frames F..P of an outbound route (lossy: from the newest `cap`, skipping the overwritten ones).
    fn frames(&mut self, id: usize, r: RouteLayout) {
        let ctx = self.ctx;
        let t = &ctx.port;
        let p = t.win().word(r.base).load(Acquire);
        let start = if r.lossy { self.next[id].max(p.saturating_sub(r.cap)) } else { self.next[id] };
        let mut buf = vec![0u8; r.size];
        let dest = t.nodes().node_of[r.to as usize] as usize;
        for index in start..p {
            let read = t.read_slot(&r, index, Some(&mut buf[..]));
            if read.unwrap_or_else(|e| panic!("trame: relay read of route {id}: {e}")) {
                self.push(dest, &Record::Frames { route: id as u32, index, bytes: &buf });
            } else if !r.lossy {
                panic!("trame: slot overwritten on a lossless route (route {id}, frame {index})");
            }
        }
        self.next[id] = p;
    }

    /// The consumer's count C of an inbound lossless route, when it moved since the last credit.
    fn credit(&mut self, id: usize, r: RouteLayout) {
        let ctx = self.ctx;
        let t = &ctx.port;
        let c = t.win().word(r.base + LINE).load(Acquire);
        if c > self.credited[id] {
            self.credited[id] = c;
            let dest = t.nodes().node_of[r.from as usize] as usize;
            self.push(dest, &Record::Credit { route: id as u32, c });
        }
    }
}
