//@check-pass
// spec: Section 7 (Message::zeroed): a worker may start a message from its all-zero value and send it.
// Message::zeroed is trame's default method; its body calls bytemuck's Zeroable::zeroed, which is
// trame's implementation, not the program's code. crates/driver/README.md: trust follows the call path.
#[trame::message]
struct Msg {
    n: u32,
}

fn main() {
    trame::__check::worker(&|| {
        let m = <Msg as trame::Message>::zeroed();
        let _ = trame::send(trame::Leader, m);
    });
}
