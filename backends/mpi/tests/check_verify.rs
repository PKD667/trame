// The verify command: checks the fifo logs in a directory. No MPI.

#![allow(unused_imports)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mpi::traits::*;
use trame::__mpi::{clock, deadlocked, launch, leader_join, returned, Answer, Error, Event, Received, Role, Route, RouteId, Topology, Transport};

pub(crate) fn verify(dir: &Path) {
    let topo = fs::read_to_string(dir.join("topo.txt")).expect("topo.txt");
    let mut routes: Vec<(u32, u32, u32, u64, bool)> = Vec::new(); // (from, to, cap, size, lossy)
    for line in topo.lines() {
        let f: Vec<u64> = line.split_whitespace().skip(1).map(|x| x.parse().unwrap()).collect();
        // f = [id, from, to, size, cap, lossy]
        routes.push((f[1] as u32, f[2] as u32, f[4] as u32, f[3], f[5] == 1));
    }
    let nroutes = routes.len();
    let mut sends: Vec<Vec<(u64, u64, u64, u64, bool)>> = vec![Vec::new(); nroutes]; // index, read, before, after, ok
    let mut consumes: Vec<Vec<(u64, u64, u64)>> = vec![Vec::new(); nroutes]; // index, before, after
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .expect("log dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("rank-")))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no logs");
    for path in &files {
        for line in fs::read_to_string(path).expect("log").lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            let num = |i: usize| -> u64 { f[i].parse().expect("number") };
            match f[0] {
                "S" => {
                    let r = num(1) as usize;
                    sends[r].push((num(2), num(3), num(4), num(5), num(6) == 0));
                }
                "C" => {
                    let r = num(1) as usize;
                    consumes[r].push((num(2), num(3), num(4)));
                }
                other => panic!("unknown log tag {other}"),
            }
        }
    }
    let mut total_sent = 0u64;
    let mut total_full = 0u64;
    for r in 0..nroutes {
        let (_, _, cap, _, lossy) = routes[r];
        if lossy {
            continue;
        }
        let cons = &mut consumes[r];
        cons.sort_by_key(|c| c.0);
        for (i, c) in cons.iter().enumerate() {
            assert_eq!(c.0, i as u64, "route {r}: consume {i} is frame {}", c.0);
            assert!(c.1 <= c.2, "route {r}: consume {i} ends before it starts");
        }
        // Sorted start times, for "how many consumes had started before t" by binary search.
        let mut before_sorted: Vec<u64> = cons.iter().map(|c| c.1).collect();
        before_sorted.sort_unstable();
        let started_before = |t: u64| before_sorted.partition_point(|&b| b < t) as u64;

        let mut oks: Vec<u64> = Vec::new();
        for &(index, read, _before, after, ok) in &sends[r] {
            // A credit may lag: spec 5.2 lets a sender see the receiver's progress late, so C is at most
            // the consumes that had started, and no lower bound is checked. A credit from the future
            // (more consumes than had started) is an error.
            let hi = started_before(after);
            assert!(
                read <= hi,
                "route {r}: send {index} read C={read}, but at most {hi} consumes had started"
            );
            if ok {
                assert!(read <= index && index - read < u64::from(cap),
                    "route {r}: Ok send {index} with C={read}, cap {cap}");
                oks.push(index);
            } else {
                assert_eq!(index - read, u64::from(cap),
                    "route {r}: spurious Full: P={index} C={read} cap={cap}");
                total_full += 1;
            }
        }
        oks.sort_unstable();
        for (i, p) in oks.iter().enumerate() {
            assert_eq!(*p, i as u64, "route {r}: Ok sends are not P = 0,1,2,...");
        }
        assert_eq!(cons.len(), oks.len(), "route {r}: {} Ok sends but {} consumes", oks.len(), cons.len());
        total_sent += oks.len() as u64;
    }
    println!(
        "verify {}: {} logs, {} routes, {total_sent} frames sent and consumed in FIFO order, {total_full} Full answers, each with cap outstanding",
        dir.display(),
        files.len(),
        nroutes
    );
}
