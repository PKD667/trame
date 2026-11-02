// spec: CONTRACT.md, M2 (no process meaning): a file handle is a descriptor of one process.
use std::fs::File;

#[trame::state(steps(step))]
struct Hold {
    f: Option<File>, //~ ERROR: [state-memory] Hold.f: std::option::Option<std::fs::File> holds a file handle
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}
