//@check-pass
// spec: Section 7 (drain): a bounded receive loop. `drain::<M, N>` takes at most N messages whatever the senders do,
// so the loop is counted by construction and the step needs no `while let Some(m) = recv()`. Its status says whether the
// bound or the route ended it. The pair of drain_vec_push_step.rs, which shows the closure is still judged.
#[trame::message]
struct Job {
    id: u32,
}

#[derive(trame::Plain)]
struct Tally {
    sum: u32,
    n: u32,
}

#[trame::state(steps(step))]
struct S {
    t: Tally,
}

#[trame::step(S, receives = Job)]
fn step(t: &mut Tally) -> trame::Status {
    let d = trame::drain::<Job, 64>(|job| {
        t.sum += job.id;
        t.n += 1;
    });
    d.status()
}

fn main() {}
