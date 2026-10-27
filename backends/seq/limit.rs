//! The step limit of the sequential backend, TRAME_SEQ_STEP_LIMIT. A scheduler step is one hand-off
//! of the baton (`State::advance`). With the variable set to n, the run ends its process at step
//! n + 1, with a message and `EXIT`. Unset, there is no limit. Read once at launch, as
//! TRAME_MPI_NODE_RANKS is on mpi. The mutants check of validate (experiments/validate.nix) sets it,
//! so a mutant that loops forever is killed by a count, not by the clock.

extern crate std;

/// The exit status of a run that reached its step limit. A program's own exits are 0 and 101 (panic).
pub(super) const EXIT: i32 = 3;

/// The limit in the environment: `Ok(None)` when unset, `Ok(Some(n))` for n >= 1.
pub(super) fn setting() -> Result<Option<u64>, &'static str> {
    parse(std::env::var("TRAME_SEQ_STEP_LIMIT").ok().as_deref())
}

pub(super) fn parse(text: Option<&str>) -> Result<Option<u64>, &'static str> {
    match text {
        None => Ok(None),
        Some(t) => match t.parse::<u64>() {
            Ok(n) if n >= 1 => Ok(Some(n)),
            _ => Err("TRAME_SEQ_STEP_LIMIT must be a number of scheduler steps, 1 or more"),
        },
    }
}

/// The run took more steps than its limit: say so, and end the process.
pub(super) fn exceeded(limit: u64) -> ! {
    std::eprintln!("trame: TRAME_SEQ_STEP_LIMIT={limit} reached: the run took more scheduler steps");
    std::process::exit(EXIT)
}
