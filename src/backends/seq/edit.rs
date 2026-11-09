//! Edits by a leader in the workers' process (CONTRACT.md, S4, P2): hold the fields in the slot's
//! model, run the edit, give them back. The agent loop (worker.rs) is the same one a leader in
//! another process drives by `worker_edit_request`.

extern crate std;

use crate::sched::Event;
use crate::worker::Slot;
use alloc::sync::Arc;
use std::sync::PoisonError;

impl Slot {
    /// Hold `fields` for an edit (S4): from now no step touching one starts; returns once none
    /// runs. Answers whether the worker is live (no result, not dead). Give them back with `release`.
    pub(crate) fn hold(&self, fields: u64) -> bool {
        let mut g = self.lock();
        if g.dead {
            return false;
        }
        g.s.editing |= fields;
        Slot::record(&mut g, Event::Hold { fields });
        while g.s.touching(fields) && !g.dead {
            g = self.changed.wait(g).unwrap_or_else(PoisonError::into_inner);
        }
        !g.dead && g.s.answer().is_none()
    }

    /// Give back `fields` after an edit: every step touching one is woken (S3).
    pub(crate) fn release(&self, fields: u64) {
        let mut g = self.lock();
        if !g.dead {
            g.s.edited(fields);
            Slot::record(&mut g, Event::Edit { fields });
        }
        self.changed.notify_all();
    }

    /// Edit `fields` of this worker: hold, run `f` if the worker is live, release.
    pub(crate) fn edit(&self, fields: u64, f: &mut dyn FnMut()) -> bool {
        let live = self.hold(fields);
        if live {
            f();
        }
        self.release(fields);
        live
    }
}

/// Edit the pool (S4 over the POOL bit): hold it in every live worker, run `f`, release everywhere.
/// Holding one slot after another cannot deadlock: only the leader holds more than one, and a step
/// never waits for a hold.
pub(crate) fn edit_pool(slots: &[Arc<Slot>], f: &mut dyn FnMut()) {
    use crate::sched::POOL;
    for s in slots {
        s.hold(POOL);
    }
    f();
    for s in slots {
        s.release(POOL);
    }
}
