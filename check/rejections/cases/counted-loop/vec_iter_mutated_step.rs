// spec: Definition (Counted loop): a loop is counted when it is a `for` over a range, slice or array that the
// body does not mutate. Safe Rust does not let the body mutate the iterated vector: the borrow checker refuses
// it before the driver runs. This case shows that, and the driver's own mutation test is a backstop only
// (check/rejections/README.md, finding G1).
#[trame::message]
struct Job {
    id: u32,
}

#[derive(trame::Plain)]
struct Queue {
    n: u32,
}

#[trame::state(steps(step))]
struct S {
    q: Queue,
}

#[trame::step(S, receives = Job)]
fn step(q: &mut Queue) -> trame::Status {
    let mut v: Vec<u32> = vec![1, 2];
    for x in v.iter() {
        v.push(*x); //~ ERROR: cannot borrow `v` as mutable because it is also borrowed as immutable
    }
    q.n = v.len() as u32;
    trame::Status::Done
}

fn main() {}
