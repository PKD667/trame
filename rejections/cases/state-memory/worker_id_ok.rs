//@check-pass
// backend.rs M2: a WorkerId is an index, not an address: a state may name its peers.
use trame::WorkerId;

#[trame::state(steps(step))]
struct Hold {
    to: WorkerId,
    peers: Vec<WorkerId>,
    maybe: Option<WorkerId>,
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}
