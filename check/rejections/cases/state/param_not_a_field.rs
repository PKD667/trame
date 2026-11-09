// spec: a step parameter is the name of a field of its state. A parameter that names no field is a compile error, at
// the associated constant the macro reads, and at the field it projects (crates/macros/state.rs).
#[trame::state(steps(step))]
struct S {
    n: u32,
}

#[trame::step(S)]
fn step(m: &mut u32) -> trame::Status { //~ ERROR: no associated function or constant named `__trame_f_m`
    //~^ ERROR: no field `m` on type `S`
    *m += 1;
    trame::Status::Done
}

fn main() {}
