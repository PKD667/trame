//@check-pass
// CONTRACT.md, M2: an error type that is plain by derive is accepted.

#[derive(trame::Plain)]
struct Why(u32);

#[trame::state(steps(step), error = Why)]
struct Hold {
    n: u32,
}

#[trame::step(Hold)]
fn step(n: &mut u32) -> Result<trame::Status, Why> {
    *n += 1;
    Ok(trame::Status::Done)
}

fn main() {}
