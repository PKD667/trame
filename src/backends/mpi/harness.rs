// What the test harnesses call at the end of a run. They stand in for the runner (roles/runner.rs) and the
// leader (leader.rs), which do the same things on the real path.

use crate::backends::mpi::Transport;
use crate::backends::mpi::memory::layout;

/// For the test harnesses, which stand in for the runner (roles/runner.rs) and the leader (leader.rs): a
/// worker that returns records its result, as the runner does.
pub fn returned(t: &Transport) {
    t.finish(layout::OUT_RETURNED);
}

/// For the test harnesses: the leader's end of a run, as `leader::join` does it. Waits for every
/// worker's result, then ends the run.
pub fn leader_join(t: &Transport) {
    t.join();
    t.fail_if_aborted();
    t.end_run();
}
