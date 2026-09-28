// The conformance claims under a process launcher: one process per participant.
//
//     TRAME_WORKERS=4 TRAME_LEADERS=2 mpirun -n 4 conformance worker : -n 1 conformance leader 4 : -n 1 conformance leader 5
//     TRAME_WORKERS=4 mpirun -n 4 conformance pressure
//
// Launch ranks `0..TRAME_WORKERS` are the workers, in contract-rank order, as in the experiments.
// `TRAME_LEADERS` more follow them, and worker `i` is led by `TRAME_WORKERS + i * leaders /
// workers`: consecutive workers share a leader, the way a launcher groups them by host. A process
// cannot discover whether it is a leader, so the launch says so in argv, with its launch rank.

#[path = "claims.rs"]
mod claims;

use trame::{Environment, Launch};

fn count(name: &str) -> u32 {
    let stated = std::env::var(name).unwrap_or_else(|e| panic!("{name}: {e}"));
    stated.parse().unwrap_or_else(|_| panic!("{name}: `{stated}` is not a count"))
}

fn main() {
    let workers = count("TRAME_WORKERS");
    let leaders = count("TRAME_LEADERS");
    let w: Vec<Launch> = (0..workers).map(Launch::new).collect();
    let l: Vec<Launch> = (0..workers).map(|i| Launch::new(workers + i * leaders / workers)).collect();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let passed = match (args.as_slice(), leaders) {
        (["worker"], 0) => claims::worker(Environment::default(), &w, None),
        (["worker"], _) => claims::worker(Environment::default(), &w, Some(&l)),
        (["leader", me], 1..) => {
            let me = me.parse().unwrap_or_else(|_| panic!("leader `{me}` is not a launch rank"));
            claims::leader(Environment::default(), Launch::new(me), &w, &l)
        }
        (["pressure"], 0) => claims::pressure(Environment::default(), &w),
        _ => panic!("usage: conformance worker | leader <launch> | pressure (TRAME_LEADERS=0)"),
    };
    std::process::exit(if passed { 0 } else { 1 });
}
