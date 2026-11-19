//! Each f64 function against correctly rounded reference values in `check/math/vectors.tsv`, made by
//! `check/math/vectors.py` with mpmath at 400 bits (see `check/math/README.md`). The reference does
//! not use libm or Rust std, so this is an oracle independent of both. One test: it measures every
//! function, prints a table (`--nocapture`), and fails listing each function whose maximum error
//! exceeds `BOUND`, with the input of its worst case.

extern crate std;
use std::eprintln;
use std::format;
use std::string::String;
use std::vec::Vec;

use super::super::{
    acos, acosh, asin, asinh, atan, atan2, atanh, cbrt, cos, cosh, exp, exp_m1, exp2, hypot, ln, ln_1p, log2, log10,
    powf, sin, sinh, sqrt, tan, tanh,
};
use super::ulps;

const DATA: &str = include_str!("../../../../check/math/vectors.tsv");

/// Largest allowed distance in ulp from the correctly rounded result. 1 is faithful rounding: the
/// result is one of the two binary64 values around the exact one. A function measured above 1 fails.
const BOUND: u64 = 1;

/// Functions allowed more than `BOUND`, each with its measured bound and the reason. A function listed here
/// that measures within `BOUND` fails too, so the table cannot go stale. `tanh` is `expm1`-based (fdlibm
/// s_tanh.c) and `exp_m1` is faithful, not correctly rounded: its 1 ulp at -2x carries into the quotient
/// (x = 0.5098647897710773: exp_m1 is 0xbfe475351309a316, correct ...315; tanh lands 2 ulp away). Getting to 1 ulp
/// needs a double-double `expm1`, which is not written.
const WIDER: [(&str, u64); 1] = [("tanh", 2)];

fn bound_of(name: &str) -> u64 {
    WIDER.iter().find(|(n, _)| *n == name).map_or(BOUND, |(_, b)| *b)
}

/// The functions, in the order of the table.
const NAMES: [&str; 24] = [
    "exp", "exp_m1", "ln", "ln_1p", "log2", "log10", "exp2", "sqrt", "cbrt", "powf", "hypot", "sin", "cos", "tan",
    "atan", "asin", "acos", "atan2", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh",
];

/// Lines per function below this count mean the file was truncated or the generator broke.
const MIN_LINES: u64 = 1000;

fn eval(name: &str, a: &[f64]) -> f64 {
    match (name, a) {
        ("exp", [x]) => exp(*x),
        ("exp_m1", [x]) => exp_m1(*x),
        ("ln", [x]) => ln(*x),
        ("ln_1p", [x]) => ln_1p(*x),
        ("log2", [x]) => log2(*x),
        ("log10", [x]) => log10(*x),
        ("exp2", [x]) => exp2(*x),
        ("sqrt", [x]) => sqrt(*x),
        ("cbrt", [x]) => cbrt(*x),
        ("powf", [x, y]) => powf(*x, *y),
        ("hypot", [x, y]) => hypot(*x, *y),
        ("sin", [x]) => sin(*x),
        ("cos", [x]) => cos(*x),
        ("tan", [x]) => tan(*x),
        ("atan", [x]) => atan(*x),
        ("asin", [x]) => asin(*x),
        ("acos", [x]) => acos(*x),
        ("atan2", [y, x]) => atan2(*y, *x),
        ("sinh", [x]) => sinh(*x),
        ("cosh", [x]) => cosh(*x),
        ("tanh", [x]) => tanh(*x),
        ("asinh", [x]) => asinh(*x),
        ("acosh", [x]) => acosh(*x),
        ("atanh", [x]) => atanh(*x),
        _ => panic!("vectors: no function {name} taking {} arguments", a.len()),
    }
}

#[derive(Clone, Default)]
struct Stat {
    lines: u64,
    max: u64,
    /// Ulp distances above 0, as a count: how often the result is not correctly rounded.
    inexact: u64,
    worst: String,
}

fn bits(h: &str) -> u64 {
    u64::from_str_radix(h, 16).unwrap_or_else(|_| panic!("vectors: bad hex {h:?}"))
}

#[test]
fn correctly_rounded_vectors() {
    let mut stats: [Stat; 24] = core::array::from_fn(|_| Stat::default());
    for line in DATA.lines() {
        let mut fields: Vec<&str> = line.split('\t').collect();
        let want_hex = fields.pop().expect("vectors: empty line");
        let name = fields.remove(0);
        let args: Vec<f64> = fields.iter().map(|h| f64::from_bits(bits(h))).collect();
        let want = f64::from_bits(bits(want_hex));
        let idx = NAMES.iter().position(|n| *n == name).unwrap_or_else(|| panic!("vectors: unknown function {name:?}"));
        let got = eval(name, &args);
        let d = if got.is_nan() { u64::MAX } else { ulps(got, want) };
        let s = &mut stats[idx];
        s.lines += 1;
        if d > 0 {
            s.inexact += 1;
        }
        if s.worst.is_empty() || d > s.max {
            s.max = d;
            s.worst = format!("{name}({}) = {got:?}, correctly rounded {want:?}", join(&args));
        }
    }

    eprintln!("{:<8} {:>6} {:>8} {:>8}  worst input", "function", "lines", "max ulp", "inexact");
    for (name, s) in NAMES.iter().zip(stats.iter()) {
        eprintln!("{:<8} {:>6} {:>8} {:>8}  {}", name, s.lines, s.max, s.inexact, s.worst);
    }

    let mut failures: Vec<String> = Vec::new();
    for (name, s) in NAMES.iter().zip(stats.iter()) {
        if s.lines < MIN_LINES {
            failures.push(format!("{name}: only {} lines (want at least {MIN_LINES})", s.lines));
        }
        let bound = bound_of(name);
        if s.max > bound {
            failures.push(format!("{name}: max {} ulp > {bound} at {}", s.max, s.worst));
        }
        if bound > BOUND && s.max <= BOUND {
            failures.push(format!("{name}: measures {} ulp, within {BOUND}: remove it from WIDER", s.max));
        }
    }
    assert!(failures.is_empty(), "correctly rounded check failed:\n{}", failures.join("\n"));
}

/// Arguments as `a, b` in Debug form, for the report.
fn join(a: &[f64]) -> String {
    let parts: Vec<String> = a.iter().map(|x| format!("{x:?}")).collect();
    parts.join(", ")
}
