//! expect: refused [allow-list] worker, [unsafe] worker
// spec: Requirement (All levels): no unsafe. The program's own Zeroable::zeroed is called through trame's
// Message::zeroed. Trame is trusted, but the program's impl is the program's code: judged in full
// (trust.rs, an impl item of a user type is an Own edge).
#[derive(Clone, Copy)]
#[repr(C)]
struct Msg {
    n: u32,
}

unsafe impl trame::__bytemuck::Zeroable for Msg {
    fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

unsafe impl trame::__bytemuck::Pod for Msg {}

fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let m = <Msg as trame::Message>::zeroed();
        let _ = trame::send(trame::LEADER, m);
    });
}
