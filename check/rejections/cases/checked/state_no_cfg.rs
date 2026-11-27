//! runner: plain cargo
// spec: Requirement (Checked builds): without --cfg trame_checked, #[trame::state] expands to compile_error!. The error says how
// to build. This case runs under plain cargo (see the runner line), so the flag is not set.
#[trame::state(steps(step))] //~ ERROR: this build needs a cfg
struct S {
    n: u32,
}

#[trame::step(S)]
fn step(n: &mut u32) -> trame::Status {
    *n += 1;
    trame::Status::Done
}

fn main() {}
