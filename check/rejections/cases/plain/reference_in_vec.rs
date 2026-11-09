// spec: CONTRACT.md, M2 (no process meaning): a reference inside a container is found through the container's argument.

#[trame::state(steps(step))]
struct Hold {
    names: Vec<&'static [u8]>, //~ ERROR: cannot be held by a trame state, pool or error
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}
