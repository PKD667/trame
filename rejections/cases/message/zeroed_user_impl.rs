// spec: Requirement (All levels): no unsafe. The program's own Zeroable::zeroed is called through trame's Message::zeroed.
// Trame is trusted, but the program's impl is the program's code: judged in full (trust.rs, an impl item of a user type is
// an Own edge).
// Refused [unsafe] worker (two): the unsafe block and the call to core::mem::zeroed inside the program's zeroed.
// Refused [allow-list] worker: core::mem::zeroed is not in A_W (driver/lists/A_W.txt), so the core call is refused.
#[derive(Clone, Copy)]
#[repr(C)]
struct Msg {
    n: u32,
}

unsafe impl trame::__bytemuck::Zeroable for Msg {
    fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
        //~^ ERROR: [unsafe] worker
        //~| ERROR: [unsafe] worker
        //~| ERROR: [allow-list] worker
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
