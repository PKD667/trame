//! expect: refused [allow-list] worker, [unsafe] worker
// spec: Requirement (All levels): no unsafe. A worker that calls a dependency's unsafe-using function
// directly is judged in full: trust does not reach the program's own calls (driver/README.md, trust.rs).
// bytemuck's bytes_of is safe to call and is an unsafe block inside. Its core calls (size_of, align_of) are
// judged in full too, so the refusal set names allow-list as well.
fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let x = 1_u32;
        let _ = trame::__bytemuck::bytes_of(&x);
    });
}
