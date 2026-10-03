// Round-trip cost of the backend send disciplines.
// Rank 0 sends a frame and waits for the echo over a size sweep.
//
//     mpirun -n 2 pingpong [reps] [max_bytes]
//
// The measurement uses ranks 0 and 1. Other ranks remain idle.

#[path = "wire.rs"]
mod wire;
use wire::*;

const PING: Tag = 0;
const PONG: Tag = 1;
/// End of the sweep. The echo loop uses this tag to stop.
const STOP: Tag = 2;

fn main() {
    let mut wire = Wire::start();
    if wire.size() < 2 {
        eprintln!("pingpong needs at least 2 ranks");
        wire.done();
        return;
    }
    if wire.size() > 2 && wire.rank() == 0 {
        eprintln!(
            "pingpong measures ranks 0 and 1; the other {} idle",
            wire.size() - 2
        );
    }
    let mut args = std::env::args().skip(1);
    let reps: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(2000);
    let max: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(65536);

    if wire.rank() == 0 {
        println!("mode\tbytes\treps\trtt_us");
        let mut bytes = 8;
        while bytes <= max {
            for (mode, buffered) in [("post", true), ("put", false)] {
                let payload = vec![0u8; bytes];
                let start = std::time::Instant::now();
                for _ in 0..reps {
                    match buffered {
                        true => wire.post(1, PING, &payload),
                        false => wire.put(1, PING, &payload),
                    }
                    let (_, echo) = wire.take(PONG);
                    assert_eq!(echo, payload, "the echo changed in transit");
                }
                let us = start.elapsed().as_secs_f64() * 1e6 / reps as f64;
                println!("{mode}\t{bytes}\t{reps}\t{us:.3}");
            }
            bytes *= 4;
        }
        wire.post(1, STOP, &[]);
    } else if wire.rank() == 1 {
        // MPI preserves order from rank 0, so STOP follows the last PING.
        loop {
            match wire.poll() {
                Some((_, PING, data)) => wire.post(0, PONG, &data),
                Some((_, STOP, _)) => break,
                _ => {}
            }
        }
    }
    wire.done();
}
