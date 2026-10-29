// spec: Requirement (All levels): no FFI, in a worker too. A worker calls a C function declared in an extern block.
unsafe extern "C" {
    fn abs(x: i32) -> i32;
}

fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let _ = unsafe { abs(-1) };
        //~^ ERROR: [unsafe] worker
        //~| ERROR: [ffi] worker
    });
}
