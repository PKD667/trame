// spec: CONTRACT.md, M2, M3. A user type is plain only by #[derive(trame::Plain)], which checks its fields: plain fields
// are not enough without it. The error's note names the derive.
struct Point {
    x: u32,
    y: u32,
}

#[trame::state(steps(step))]
struct Hold {
    p: Point, //~ ERROR: cannot be held by a trame state or pool
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}
