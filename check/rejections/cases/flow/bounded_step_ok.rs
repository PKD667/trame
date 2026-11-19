//@check-pass
// spec: Section 7 (Bounded): a bounded collection is trame's own code, trusted at every level, and its methods never
// allocate after construction (the builder allocates). A step may push, pop, clear, index and iterate. `Full` is the same
// error `send` answers. The pair of allocation/vec_push_step.rs, where the same push on a Vec is refused.
#[trame::state(steps(step))]
struct S {
    q: trame::Bounded<u32>,
}

#[trame::step(S)]
fn step(q: &mut trame::Bounded<u32>) -> trame::Status {
    if q.push(7).is_err() {
        q.clear();
    }
    let mut total = 0;
    for x in q.iter() {
        total += *x;
    }
    if total > 100 {
        let _ = q.pop();
    }
    trame::Status::Progress
}

fn main() {}
