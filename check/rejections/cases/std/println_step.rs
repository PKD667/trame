// spec: Requirement (All levels): no std. A step prints, which calls std. The format_args! expansion's unsafe
// `fmt::Arguments::new` is not judged (README.md, finding D1, as corrected again).
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
    std::println!("n = {}", q.n);
    //~^ ERROR: [std] step
    trame::Status::Done
}

fn main() {}
