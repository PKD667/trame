// spec: Section 7 (drain): the bound belongs to `drain`, not to what the closure does. The closure is a body of the step and
// is judged at its level: it may not grow a Vec. The pair of drain_step_ok.rs.
#[trame::message]
struct Job {
    id: u32,
}

#[trame::state(steps(step))]
struct S {
    v: Vec<u32>,
}

#[trame::step(S, receives = Job)]
fn step(v: &mut Vec<u32>) -> trame::Status {
    let d = trame::drain::<Job, 8>(|job| v.push(job.id)); //~ ERROR: [allocation] step
    d.status()
}

fn main() {}
