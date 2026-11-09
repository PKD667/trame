// spec: CONTRACT.md, M2 (no process meaning): a step's error is read by the leader from the worker's domain, on mpi from another process, so it is plain too. A `&'static str` names memory of the process that made it.

struct Why(&'static str);

#[trame::state(steps(step), error = Why)] //~ ERROR: cannot be held by a trame state, pool or error
struct Hold {
    n: u32,
}

#[trame::step(Hold)]
fn step(n: &mut u32) -> Result<trame::Status, Why> {
    *n += 1;
    Ok(trame::Status::Done)
}

fn main() {}
