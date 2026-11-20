//! Benchmark of the AVX2 lane slice functions against the scalar loop (src/math/lanes/avx2.rs).
//! For each function: the scalar loop calls `trame::math::f64::<f>` once per element; the lane version
//! is the slice function. Both run over N = 1 << 20 inputs from a xorshift generator. Each time is the
//! minimum of 9 repetitions after 2 warmups, in ns per element.
//!
//! Before any timing, the lane output must equal the scalar output bit for bit, on the random inputs and
//! on a list of special values (NaN, infinities, signed zeros, subnormals, the extremes). A speedup of a
//! wrong result is therefore not printed. Bit equality is stricter than "NaN equals NaN".
//!
//! Run (from the repository root; see the header printed by the program):
//!   nix develop "path:$PWD#default" -c cargo run --offline --release --no-default-features --features seq --example math_bench
//!   nix develop "path:$PWD#default" -c cargo run --offline --release --no-default-features --features seq,lanes-avx2 --example math_bench

use std::hint::black_box;
use std::process::Command;
use std::time::Instant;

use trame::math::f64 as s;

#[cfg(feature = "lanes-avx2")]
use trame::math::lanes::avx2 as lane;

const N: usize = 1 << 20;
const WARMUPS: usize = 2;
const REPS: usize = 9;

/// Inputs appended for the bit check only (not timed).
#[cfg(feature = "lanes-avx2")]
const SPECIALS: [f64; 15] = [
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    0.0,
    -0.0,
    f64::MIN_POSITIVE,
    5e-324,
    -5e-324,
    f64::MAX,
    f64::MIN,
    1.0,
    -1.0,
    0.5,
    2.5,
    -2.5,
];

/// xorshift64: small, fixed seed, the same inputs on every run.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }
}

fn uniform(r: &mut Rng, lo: f64, hi: f64) -> Vec<f64> {
    (0..N).map(|_| lo + (hi - lo) * r.unit()).collect()
}

/// Log-uniform magnitudes: 10^e with e uniform in [lo, hi].
fn log_uniform(r: &mut Rng, lo: f64, hi: f64) -> Vec<f64> {
    (0..N).map(|_| 10f64.powf(lo + (hi - lo) * r.unit())).collect()
}

/// Minimum over REPS repetitions after WARMUPS runs, in ns per element.
fn timed<F: FnMut()>(mut f: F) -> f64 {
    for _ in 0..WARMUPS {
        f();
    }
    let mut best = f64::INFINITY;
    for _ in 0..REPS {
        let t = Instant::now();
        f();
        let ns = t.elapsed().as_secs_f64() * 1e9 / N as f64;
        best = best.min(ns);
    }
    best
}

#[cfg(feature = "lanes-avx2")]
fn same_bits(name: &str, inputs: &[&[f64]], want: &[f64], got: &[f64]) {
    assert_eq!(want.len(), got.len(), "{name}: length");
    for (i, (w, g)) in want.iter().zip(got).enumerate() {
        if w.to_bits() != g.to_bits() {
            let ins: Vec<f64> = inputs.iter().map(|v| v[i]).collect();
            panic!(
                "{name}: lane differs from scalar at {i}: inputs {ins:?} scalar {w:?} ({:#018x}) lanes {g:?} ({:#018x})",
                w.to_bits(),
                g.to_bits()
            );
        }
    }
}

/// One row of the table: scalar ns/elem, and lanes ns/elem when compiled in.
struct Row {
    name: &'static str,
    scalar: f64,
    lanes: Option<f64>,
}

macro_rules! unary {
    ($name:literal, $xs:expr, $sc:path, $lane:ident) => {{
        let xs: Vec<f64> = $xs;
        #[cfg(feature = "lanes-avx2")]
        {
            let chk: Vec<f64> = xs.iter().copied().chain(SPECIALS).collect();
            let want: Vec<f64> = chk.iter().map(|&x| $sc(x)).collect();
            let mut got = vec![0.0; chk.len()];
            lane::$lane(&chk, &mut got);
            same_bits($name, &[&chk], &want, &got);
        }
        let mut buf = vec![0.0; xs.len()];
        let scalar = timed(|| {
            for (o, &x) in buf.iter_mut().zip(black_box(&xs)) {
                *o = $sc(x);
            }
            black_box(&buf);
        });
        #[cfg(feature = "lanes-avx2")]
        let lanes = Some(timed(|| {
            lane::$lane(black_box(&xs), &mut buf);
            black_box(&buf);
        }));
        #[cfg(not(feature = "lanes-avx2"))]
        let lanes = None;
        Row { name: $name, scalar, lanes }
    }};
}

macro_rules! binary {
    ($name:literal, $a:expr, $b:expr, $sc:path, $lane:ident) => {{
        let a: Vec<f64> = $a;
        let b: Vec<f64> = $b;
        #[cfg(feature = "lanes-avx2")]
        {
            let ca: Vec<f64> = a.iter().copied().chain(SPECIALS).collect();
            let cb: Vec<f64> = b.iter().copied().chain(SPECIALS.iter().rev().copied()).collect();
            let want: Vec<f64> = ca.iter().zip(&cb).map(|(&x, &y)| $sc(x, y)).collect();
            let mut got = vec![0.0; ca.len()];
            lane::$lane(&ca, &cb, &mut got);
            same_bits($name, &[&ca, &cb], &want, &got);
        }
        let mut buf = vec![0.0; a.len()];
        let scalar = timed(|| {
            for ((o, &x), &y) in buf.iter_mut().zip(black_box(&a)).zip(black_box(&b)) {
                *o = $sc(x, y);
            }
            black_box(&buf);
        });
        #[cfg(feature = "lanes-avx2")]
        let lanes = Some(timed(|| {
            lane::$lane(black_box(&a), black_box(&b), &mut buf);
            black_box(&buf);
        }));
        #[cfg(not(feature = "lanes-avx2"))]
        let lanes = None;
        Row { name: $name, scalar, lanes }
    }};
}

macro_rules! ternary {
    ($name:literal, $a:expr, $b:expr, $c:expr, $sc:path, $lane:ident) => {{
        let a: Vec<f64> = $a;
        let b: Vec<f64> = $b;
        let c: Vec<f64> = $c;
        #[cfg(feature = "lanes-avx2")]
        {
            let ca: Vec<f64> = a.iter().copied().chain(SPECIALS).collect();
            let cb: Vec<f64> = b.iter().copied().chain(SPECIALS.iter().rev().copied()).collect();
            let cc: Vec<f64> = c.iter().copied().chain(SPECIALS).collect();
            let want: Vec<f64> = ca.iter().zip(&cb).zip(&cc).map(|((&x, &y), &z)| $sc(x, y, z)).collect();
            let mut got = vec![0.0; ca.len()];
            lane::$lane(&ca, &cb, &cc, &mut got);
            same_bits($name, &[&ca, &cb, &cc], &want, &got);
        }
        let mut buf = vec![0.0; a.len()];
        let scalar = timed(|| {
            for (((o, &x), &y), &z) in buf.iter_mut().zip(black_box(&a)).zip(black_box(&b)).zip(black_box(&c)) {
                *o = $sc(x, y, z);
            }
            black_box(&buf);
        });
        #[cfg(feature = "lanes-avx2")]
        let lanes = Some(timed(|| {
            lane::$lane(black_box(&a), black_box(&b), black_box(&c), &mut buf);
            black_box(&buf);
        }));
        #[cfg(not(feature = "lanes-avx2"))]
        let lanes = None;
        Row { name: $name, scalar, lanes }
    }};
}

fn command_output(prog: &str, args: &[&str]) -> String {
    match Command::new(prog).args(args).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => format!("{prog}: unavailable"),
    }
}

fn cpu_model() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|t| {
            t.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_string())
        })
        .unwrap_or_else(|| "unknown (no /proc/cpuinfo)".to_string())
}

fn main() {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);

    let rows = vec![
        unary!("exp", uniform(&mut r, -700.0, 700.0), s::exp, exp_slice),
        unary!("ln", log_uniform(&mut r, -300.0, 300.0), s::ln, ln_slice),
        unary!("sqrt", uniform(&mut r, 0.0, 1e300), s::sqrt, sqrt_slice),
        unary!("floor", uniform(&mut r, -1e6, 1e6), s::floor, floor_slice),
        unary!("ceil", uniform(&mut r, -1e6, 1e6), s::ceil, ceil_slice),
        unary!("trunc", uniform(&mut r, -1e6, 1e6), s::trunc, trunc_slice),
        unary!("round_ties_even", uniform(&mut r, -1e6, 1e6), s::round_ties_even, round_ties_even_slice),
        binary!("max", uniform(&mut r, -1e300, 1e300), uniform(&mut r, -1e300, 1e300), s::max, max_slice),
        binary!("min", uniform(&mut r, -1e300, 1e300), uniform(&mut r, -1e300, 1e300), s::min, min_slice),
        ternary!(
            "mul_add",
            uniform(&mut r, -1e100, 1e100),
            uniform(&mut r, -1e100, 1e100),
            uniform(&mut r, -1e100, 1e100),
            s::mul_add,
            mul_add_slice
        ),
    ];

    let rustc = command_output("rustc", &["-V"]);
    println!("# math_bench: trame::math::f64 scalar loop vs trame::math::lanes::avx2 slice functions");
    println!("# rustc: {rustc}");
    println!("# cpu: {}", cpu_model());
    println!(
        "# features: seq={} lanes-avx2={} lanes-avx512={} mpi={}",
        cfg!(feature = "seq"),
        cfg!(feature = "lanes-avx2"),
        cfg!(feature = "lanes-avx512"),
        cfg!(feature = "mpi"),
    );
    println!("# target: arch={} os={}", std::env::consts::ARCH, std::env::consts::OS);
    #[cfg(feature = "lanes-avx2")]
    println!("# avx2+fma detected at run time (lane path active): {}", lane::has_avx2_fma());
    println!(
        "# n: {N} inputs, min of {REPS} reps after {WARMUPS} warmups, ns per element, lane output checked bit-equal to scalar first"
    );
    println!("# commands:");
    println!(
        "#   nix develop \"path:$PWD#default\" -c cargo run --offline --release --no-default-features --features seq --example math_bench"
    );
    println!(
        "#   nix develop \"path:$PWD#default\" -c cargo run --offline --release --no-default-features --features seq,lanes-avx2 --example math_bench"
    );
    println!("# RUSTFLAGS: {}", std::env::var("RUSTFLAGS").unwrap_or_else(|_| "(unset)".into()));

    #[cfg(feature = "lanes-avx2")]
    {
        println!("function\tscalar_ns_per_elem\tlanes_ns_per_elem\tspeedup");
        for row in &rows {
            let lanes = row.lanes.unwrap_or(f64::NAN);
            println!("{}\t{:.3}\t{:.3}\t{:.2}", row.name, row.scalar, lanes, row.scalar / lanes);
        }
    }
    #[cfg(not(feature = "lanes-avx2"))]
    {
        println!("# lanes: compiled out (build with --features lanes-avx2 to measure)");
        println!("function\tscalar_ns_per_elem");
        for row in &rows {
            debug_assert!(row.lanes.is_none());
            println!("{}\t{:.3}", row.name, row.scalar);
        }
    }
}
