// spec: CONTRACT.md, M2 (no process meaning): a trait object carries a vtable address. `Box<dyn Fn() + Send + Sync>` satisfies the State bounds, so rustc
// accepts it; Plain refuses it.

#[trame::state(steps(step))]
struct Hold {
    f: Box<dyn Fn(u32) -> u32 + Send + Sync>, //~ ERROR: cannot be held by a trame state or pool
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}
