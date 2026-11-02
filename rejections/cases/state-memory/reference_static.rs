// spec: CONTRACT.md, M2 (no process meaning): a reference is an address in one process, even `&'static`: another process of the domain has its own.

#[trame::state(steps(step))]
struct Hold {
    name: &'static str, //~ ERROR: [state-memory] Hold.name: &str holds a reference (an address in one process) (CONTRACT.md, M2)
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}
