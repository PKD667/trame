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
        match run.recv::<M>().map(|s| s.value) {
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

/// The message sizes a benchmark can be asked for, in bytes. A message type has one size, so a program
/// stamps out one type per size with `sizes!` and picks one at run time with `size_index`.
pub const SIZES: [usize; 7] = [16, 64, 256, 1024, 4096, 16384, 65536];

/// The index of `size` in SIZES; any other size is a usage error on every rank.
pub fn size_index(size: usize) -> usize {
    SIZES.iter().position(|&s| s == size).unwrap_or_else(|| {
        eprintln!("SIZE must be one of {SIZES:?}, got {size}");
        std::process::exit(2);
    })
}

/// The host of global worker `g` when each host runs `per` workers, and its index there.
pub fn place(g: u32, per: u32) -> trame::WorkerId {
    trame::WorkerId::from((trame::HostId(g / per), g % per))
}

/// Calls `$m!(Name, pad)` once per size of SIZES, with the padding that makes a 16-byte header
/// (`seq`, `from`) plus `pad` bytes equal to the size.
#[macro_export]
macro_rules! sizes {
    ($m:ident) => {
        $m!(s16, 0, 0);
        $m!(s64, 48, 1);
        $m!(s256, 240, 2);
        $m!(s1k, 1008, 3);
        $m!(s4k, 4080, 4);
        $m!(s16k, 16368, 5);
        $m!(s64k, 65520, 6);
    };
}
