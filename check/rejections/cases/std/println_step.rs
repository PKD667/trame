// spec: Requirement (All levels): no std. A step prints, which calls std. The unsafe rule also fires on core's
// `fmt::Arguments::new` (check/rejections/README.md, finding D1).
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
    //~^ ERROR: [unsafe] step
    //~| ERROR: [std] step
    trame::Status::Done
}

fn main() {}
