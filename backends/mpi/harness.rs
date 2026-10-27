// What the test harnesses call at the end of a run. They stand in for the runner (imp.rs) and the
// leader (leader.rs), which do the same things on the real path.

use super::{layout, Transport};

/// For the test harnesses, which stand in for the runner (imp.rs) and the leader (leader.rs): a
/// worker that returns records its result, as the runner does.
pub fn returned(t: &Transport) {
    t.finish(layout::OUT_RETURNED);
}

/// For the test harnesses: a worker whose barrier answered Deadlock records that, as the runner does.
pub fn deadlocked(t: &Transport) {
    t.finish(layout::OUT_DEADLOCK);
}

/// For the test harnesses: the leader's end of a run, as `leader::join` does it. Waits for every
/// worker's result, then ends the relays (NODES.md, End).
pub fn leader_join(t: &Transport) {
    t.join();
    t.end_run();
}
