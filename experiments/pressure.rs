// Does MPI report an exhausted attached buffer, or does it block?
//
// The contract requires a backend to report `Full` whenever an attempt is refused for capacity,
// including when the attempt's own failure is the only observation of it. This asks whether that
// observation exists for `MPI_Bsend`: rank 0 attaches a buffer exactly as wide as the payload,
// which cannot also hold MPI's mandatory buffered-send overhead, and makes one polling attempt
// while rank 1 does not receive. If MPI reports it, the backend can map it to `Full`. If it blocks,
// the route is `unreported` and the choice is between blocking and not using buffered mode at all —
// which is a different backend, not a flag.

#[path = "wire.rs"]
mod wire;
use wire::*;

const FRAME: usize = 64 * 1024;
const ATTACHED: usize = FRAME;

fn main() {
    let mut wire = Wire::attach(ATTACHED);
    if wire.rank() == 1 {
        // Never receive. A polling send must report its own refusal without this peer's progress.
        std::thread::sleep(std::time::Duration::from_secs(1));
        wire.done();
        return;
    }

    let payload = vec![7u8; FRAME];
    let outcome = wire.try_post(1, 1, &payload);
    assert_eq!(
        outcome,
        Err(Error::Full),
        "capacity refusal was not reported"
    );
    eprintln!("capacity refusal reported as Full");
    wire.done();
}
