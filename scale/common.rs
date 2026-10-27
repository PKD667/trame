#![allow(dead_code)] // each example uses a subset of these helpers
//! Shared by the scale programs (included with `#[path]`): argument parsing on every rank, the
//! leader's collection loop, and the outcomes line. Workers never print; only the leader does.

use std::str::FromStr;
use trame::{Message, Outcome, Run};

/// Argument `i` of argv, parsed. Every rank runs this before `launch`, so a bad argument stops all of them.
pub fn arg<T: FromStr>(i: usize, name: &str) -> T {
    let argv: Vec<String> = std::env::args().collect();
    match argv.get(i).and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => {
            eprintln!("usage: {} needs {name} as argument {i}", argv.first().map_or("scale", |s| s.as_str()));
            std::process::exit(2);
        }
    }
}

/// Receive `n` messages of type `M` at the leader, calling `each` on every one, waiting between checks.
pub fn collect<M: Message>(run: &Run, n: usize, mut each: impl FnMut(M)) {
    let mut got = 0;
    while got < n {
        match run.recv::<M>() {
            Some(m) => {
                each(m);
                got += 1;
            }
            None => run.wait(),
        }
    }
}

/// Wait for every worker's result and print them as the last line.
pub fn finish(run: Run) {
    let outcomes: Vec<Outcome> = run.join();
    println!("{{\"outcomes\":\"{outcomes:?}\"}}");
}

/// A launch refusal ends the program with a message on stderr, so stdout stays JSON lines only.
pub fn refused<E: core::fmt::Debug>(e: E) -> ! {
    eprintln!("refused: {e:?}");
    std::process::exit(1);
}

/// Nanoseconds of a span, refusing to truncate.
pub fn ns(d: core::time::Duration) -> u64 {
    u64::try_from(d.as_nanos()).expect("a span over 584 years")
}
