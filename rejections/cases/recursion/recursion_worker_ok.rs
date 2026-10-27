//! expect: accepted
// spec: Requirement (Worker): a worker may use any loop and may recurse. This is the pair of recursion_step.rs:
// the same recursion, and a while loop, in a worker, are accepted.
fn countdown(n: u32) -> u32 {
    if n == 0 { 0 } else { countdown(n - 1) + 1 }
}

fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let mut i = 0u32;
        while i < 3 {
            i += 1;
        }
        let _ = countdown(i);
    });
}
