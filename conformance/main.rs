// The conformance claims under a process launcher: one process per participant.
//
//     TRAME_WORKERS=4 TRAME_HOSTS=1 mpirun -x TRAME_WORKERS -x TRAME_HOSTS \
//         -n 4 conformance worker 0 : -x TRAME_WORKERS -x TRAME_HOSTS -n 1 conformance leader 0
//
// Each host has W / H workers and one colocated leader. Worker contexts come first in host
// order, then leader contexts: host h's leader is launch rank W + h. The launcher states the
// host in argv and the common table dimensions in the environment; MPI ranks do not imply roles.

#[path = "claims.rs"]
mod claims;

use trame::{Environment, Launch};

fn count(name: &str) -> u32 {
    let stated = std::env::var(name).unwrap_or_else(|e| panic!("{name}: {e}"));
    stated.parse().unwrap_or_else(|_| panic!("{name}: `{stated}` is not a count"))
}

fn main() {
    let workers = count("TRAME_WORKERS");
    let hosts = count("TRAME_HOSTS");
    assert!(hosts > 0 && workers > 0 && workers % hosts == 0, "TRAME_HOSTS={hosts} does not divide TRAME_WORKERS={workers}");
    let args: Vec<String> = std::env::args().skip(1).collect();
    assert!(args.len() == 2, "usage: conformance <worker|leader|pressure|pressure-leader|link|link-leader> <host>");
    let here: u16 = args[1].parse().expect("host is a u16");
    let per = workers / hosts;
    let rows: Vec<Vec<Launch>> = (0..hosts).map(|h| (0..per).map(|r| Launch::new(h * per + r)).collect()).collect();
    let table: Vec<&[Launch]> = rows.iter().map(Vec::as_slice).collect();
    let leader = Launch::new(workers + u32::from(here));
    let env = Environment::default();
    let passed = match args[0].as_str() {
        "worker" => claims::worker(env, &table, here, leader),
        "leader" => claims::leader(env, leader, &table, here),
        "pressure" => claims::pressure(env, &table, here, leader),
        "pressure-leader" => claims::pressure_leader(env, leader, &table, here, "M5"),
        "link" => claims::link(env, &table, here, leader),
        "link-leader" => claims::pressure_leader(env, leader, &table, here, "F1"),
        role => panic!("unknown role `{role}`"),
    };
    std::process::exit(if passed { 0 } else { 1 });
}
