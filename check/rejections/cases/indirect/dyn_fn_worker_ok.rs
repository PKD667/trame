//@check-pass
// spec: Requirement (All levels) forbids indirect calls. This is the pair of dyn_fn_worker.rs: a generic
// parameter is monomorphized, so the call is to the closure's own body, a direct call. Accepted.
fn apply(f: impl Fn(u32) -> u32, x: u32) -> u32 {
    f(x)
}

fn main() {
    trame::__check::worker(&|| {
        let _ = apply(|x| x + 1, 3);
    });
}
