// spec: CONTRACT.md, M2 (no process meaning): a trait object carries a vtable address. `Box<dyn Fn() + Send + Sync>` satisfies the State bounds, so rustc
// accepts it; the driver refuses it.

#[trame::state(steps(step))]
struct Hold {
    f: Box<dyn Fn(u32) -> u32 + Send + Sync>, //~ ERROR: [state-memory] Hold.f: std::boxed::Box<dyn std::ops::Fn(u32) -> u32 + std::marker::Send + std::marker::Sync> holds a trait object (a vtable address), found `dyn std::ops::Fn(u32) -> u32 + std::marker::Send + std::marker::Sync` (CONTRACT.md, M2)
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}
