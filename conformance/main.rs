// The conformance claims under a process launcher: one process per participant.
//
//     TRAME_WORKERS=4 mpirun -n 4 conformance worker : -n 1 conformance leader
//     TRAME_WORKERS=4 mpirun -n 4 conformance pressure : -n 1 conformance pressure-leader
//     TRAME_WORKERS=4 TRAME_HOSTS=2 mpirun -n 2 conformance link 0 : -n 2 conformance link 1 \\
//         : -n 1 conformance link-leader 0 : -n 1 conformance link-leader 1
//
// The main and pressure launches are one host. Launch ranks `0..TRAME_WORKERS` are its workers, in
// local-rank order, and its one leader is launch rank `TRAME_WORKERS`. The link launch is
// `TRAME_HOSTS` hosts of `W / H` workers: launch rank `i` is host `i / (W / H)`'s worker
// `i % (W / H)`, and host `h`'s leader is launch rank `W + h`. A process cannot discover whether it
// is a leader, nor its host, so the launch says so in argv, in launch-rank order.

#[path = "claims.rs"]
mod claims;

use trame::{Environment, Launch};

fn count(name: &str) -> u32 {
    let stated = std::env::var(name).unwrap_or_else(|e| panic!("{name}: {e}"));
    stated.parse().unwrap_or_else(|_| panic!("{name}: `{stated}` is not a count"))
}

/// The link launch's table, and `host` from argv as this process's host.
fn linked(workers: u32, host: &str) -> (Vec<Vec<Launch>>, u16) {
    let hosts = count("TRAME_HOSTS");
    assert!(hosts > 0 && workers % hosts == 0, "TRAME_HOSTS={hosts} does not divide TRAME_WORKERS={workers}");
    let per = workers / hosts;
    let rows = (0..hosts).map(|h| (0..per).map(|r| Launch::new(h * per + r)).collect()).collect();
    let here = host.parse().unwrap_or_else(|_| panic!("`{host}` is not a host"));
    (rows, here)
}

fn main() {
    let workers = count("TRAME_WORKERS");
    let w: Vec<Launch> = (0..workers).map(Launch::new).collect();
    let hosts: [&[Launch]; 1] = [&w];
    let leader = Launch::new(workers);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let env = Environment::default();
    let passed = match args.as_slice() {
        ["worker"] => claims::worker(env, &hosts, 0, leader),
        ["leader"] => claims::leader(env, leader, &hosts, 0),
        ["pressure"] => claims::pressure(env, &hosts, 0, leader),
        ["pressure-leader"] => claims::pressure_leader(env, leader, &hosts, 0, "M5"),
        ["link", host] => {
            let (rows, here) = linked(workers, host);
            let table: Vec<&[Launch]> = rows.iter().map(Vec::as_slice).collect();
            claims::link(env, &table, here, Launch::new(workers + u32::from(here)))
        }
        ["link-leader", host] => {
            let (rows, here) = linked(workers, host);
            let table: Vec<&[Launch]> = rows.iter().map(Vec::as_slice).collect();
            claims::pressure_leader(env, Launch::new(workers + u32::from(here)), &table, here, "F1")
        }
        _ => panic!("usage: conformance worker | leader | pressure | pressure-leader | link <host> | link-leader <host>"),
    };
    std::process::exit(if passed { 0 } else { 1 });
}
