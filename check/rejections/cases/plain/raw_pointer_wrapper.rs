// spec: CONTRACT.md, M2 (no process meaning): a raw pointer hidden under a user type with an unsafe Send and Sync. rustc accepts the promise; the layout holds a
// pointer to one process's memory.
#[derive(trame::Plain)]
struct Handle(*const u8); //~ ERROR: cannot be held by a trame state, pool or error

// SAFETY: for the case only.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

#[trame::state(steps(step))]
struct Hold {
    h: Option<Box<[Handle; 2]>>,
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}
