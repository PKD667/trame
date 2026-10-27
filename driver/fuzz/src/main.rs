//! trame-fuzz: a fuzzer for the compile-time analysis of trame (driver/fuzz/README.md).
//!
//! Subcommands:
//!   run   --seed S --batches B --k K [--extra] [--driver-bin DIR] [--out DIR]
//!   sweep [--k K] [--driver-bin DIR] [--out DIR]
//!   show  --seed S --index I [--extra]        print one program and R's expectation
//!
//! Exit status: 0 when driver and oracle agree everywhere; 1 on any mismatch; 2 on a harness
//! problem (a run whose output cannot be read as verdicts, see verdict.rs).

mod batch;
mod emit;
mod json;
mod oracle;
mod prog;
mod rng;
mod shrink;
mod sweep;
mod verdict;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use batch::{Case, Outcome};
use prog::{GenOpts, Prog};

struct Args(Vec<String>);

impl Args {
    fn value(&self, flag: &str) -> Option<String> {
        self.0.iter().position(|a| a == flag).and_then(|i| self.0.get(i + 1).cloned())
    }
    fn has(&self, flag: &str) -> bool {
        self.0.iter().any(|a| a == flag)
    }
    fn num(&self, flag: &str, default: u64) -> u64 {
        match self.value(flag) {
            None => default,
            Some(v) => v.parse().unwrap_or_else(|_| die(2, &format!("{flag}: not a number: {v}"))),
        }
    }
}

fn die(code: i32, msg: &str) -> ! {
    eprintln!("trame-fuzz: {msg}");
    exit(code)
}

/// The fuzzer's own directory: the parent of `src/` at build time, or the current directory.
fn fuzz_root(args: &Args) -> PathBuf {
    args.value("--out").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn driver_dir(args: &Args) -> Option<PathBuf> {
    args.value("--driver-bin").map(PathBuf::from)
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = argv.first().cloned() else { die(2, "usage: trame-fuzz <run|sweep|show> [options]") };
    let args = Args(argv[1..].to_vec());
    match cmd.as_str() {
        "run" => exit(cmd_run(&args)),
        "sweep" => exit(cmd_sweep(&args)),
        "show" => cmd_show(&args),
        other => die(2, &format!("unknown subcommand {other}")),
    }
}

fn opts(args: &Args) -> GenOpts {
    GenOpts { extra: args.has("--extra") }
}

fn cmd_show(args: &Args) {
    let seed = args.num("--seed", 1);
    let index = args.num("--index", 0);
    let p = prog::generate(seed, index, opts(args));
    let e = oracle::expect(&p);
    print!("{}", emit::source(&p));
    println!("// R: roots {:?}", e.roots.iter().map(|(n, l)| format!("{n}@{}", l.noun())).collect::<Vec<_>>());
    if e.accepts() {
        println!("// R: accept");
    }
    for r in &e.refusals {
        println!("// R: [{}] {} roots={:?}", r.rule, r.level.noun(), r.roots);
    }
}

/// The programs of batch `b` of a run: indices b*K .. (b+1)*K.
fn batch_programs(seed: u64, b: u64, k: u64, o: GenOpts) -> Vec<Prog> {
    (b * k..(b + 1) * k).map(|i| prog::generate(seed, i, o)).collect()
}

#[derive(Default)]
struct Totals {
    programs: usize,
    roots: usize,
    refusals_expected: usize,
    agreements: usize,
    verdict_misses: usize,
    attr_misses: usize,
    over: usize,
    extra: usize,
    undecided: usize,
    accepted_by_r: usize,
    accepted_by_v: usize,
    by_rule: BTreeMap<String, (usize, usize)>, // (expected, attribution misses) per rule/level
}

fn cmd_run(args: &Args) -> i32 {
    let seed = args.num("--seed", 1);
    let batches = args.num("--batches", 1);
    let k = args.num("--k", 50).max(1);
    let o = opts(args);
    let root = fuzz_root(args);
    let drv = driver_dir(args);
    let work = root.join("work");
    let found = root.join("found");
    let mut t = Totals::default();
    let mut mismatches: Vec<(String, Outcome)> = Vec::new();
    let mut harness: Vec<String> = Vec::new();
    let start = std::time::Instant::now();
    for b in 0..batches {
        let progs = batch_programs(seed, b, k, o);
        let cases: Vec<Case> = progs
            .into_iter()
            .enumerate()
            .map(|(i, p)| Case::new(format!("p{:05}", b * k + i as u64), p))
            .collect();
        let dir = work.join(format!("run-{seed}-{b}"));
        match batch::run(&dir, cases, drv.as_deref()) {
            Ok(outs) => {
                for out in outs {
                    account(&mut t, &out);
                    if !out.diff.agrees() {
                        mismatches.push((format!("seed {seed} index {}", out.prog.index), out));
                    }
                }
            }
            Err(hs) => {
                harness.push(format!("batch {b}: {}", hs.join(" | ")));
                break;
            }
        }
        batch::clean_target(&dir);
        batch::say(&format!("batch {}/{} done: {} programs so far, {} mismatches", b + 1, batches, t.programs, mismatches.len()));
    }
    // One shrink per (mismatch, key): a program can carry several keys, and shrinking against one
    // of them can drop another (the shrunk case must keep the key it is filed under).
    let mut jobs: Vec<(String, Outcome, String)> = Vec::new();
    for (label, out) in &mismatches {
        for key in out.diff.keys() {
            jobs.push((label.clone(), out.clone(), key));
        }
    }
    let mut shrunk_count = 0;
    for (n, (label, out, key)) in jobs.iter().enumerate() {
        let sw = shrink::shrink(out.prog.clone(), key, &work, &format!("shrink-{seed}-{n}"), drv.as_deref(), 60);
        let (small, rounds, tried) = match sw {
            Ok(s) => (s.prog, s.rounds, s.candidates),
            Err(e) => {
                harness.push(e);
                (out.prog.clone(), 0, 0)
            }
        };
        shrunk_count += 1;
        let final_out = final_verdict(&small, &work.join(format!("final-{seed}-{n}")), drv.as_deref());
        let path = found.join(format!("{seed}-{n}.rs"));
        let text = found_text(label, key, out, &small, final_out.as_ref(), rounds, tried);
        let _ = fs::create_dir_all(&found);
        if let Err(e) = fs::write(&path, text) {
            harness.push(format!("writing {}: {e}", path.display()));
        }
        batch::say(&format!("found {} ({label}): shrunk in {rounds} rounds, {tried} candidates", path.display()));
    }
    let elapsed = start.elapsed().as_secs_f64();
    print_report(seed, batches, k, o, &t, mismatches.len(), shrunk_count, &harness, elapsed);
    if !harness.is_empty() {
        return 2;
    }
    if !mismatches.is_empty() { 1 } else { 0 }
}

/// Re-run the shrunk program alone to report the driver's verdict on it.
fn final_verdict(p: &Prog, dir: &Path, drv: Option<&Path>) -> Option<Outcome> {
    let res = batch::run(dir, vec![Case::new("final".into(), p.clone())], drv);
    batch::clean_target(dir);
    res.ok().and_then(|mut v| v.pop())
}

fn account(t: &mut Totals, out: &Outcome) {
    t.programs += 1;
    t.roots += out.expect.roots.len();
    t.refusals_expected += out.expect.refusals.len();
    if out.expect.accepts() {
        t.accepted_by_r += 1;
    }
    if out.unit.recs.is_empty() {
        t.accepted_by_v += 1;
    }
    if out.diff.agrees() {
        t.agreements += 1;
    }
    t.verdict_misses += out.diff.verdict_misses.len();
    t.attr_misses += out.diff.attr_misses.len();
    t.over += out.diff.over.len();
    t.extra += out.diff.extra.len();
    t.undecided += out.diff.undecided.len();
    for r in &out.expect.refusals {
        t.by_rule.entry(format!("{}/{}", r.rule, r.level.noun())).or_default().0 += 1;
    }
    for m in &out.diff.attr_misses {
        t.by_rule.entry(format!("{}/{}", m.rule, m.level.noun())).or_default().1 += 1;
    }
}

fn found_text(label: &str, key: &str, first: &Outcome, small: &Prog, final_out: Option<&Outcome>, rounds: usize, tried: usize) -> String {
    let diff_text = final_out.map_or_else(|| "(shrunk program not re-run)".to_string(), |f| f.diff.describe());
    let e = oracle::expect(small);
    let mut s = String::new();
    s.push_str(&format!("// trame-fuzz found: {label}\n"));
    s.push_str(&format!("// key: {key} (the shrunk program keeps this difference)\n"));
    s.push_str(&format!("// first seen: seed={} index={}; shrunk in {rounds} rounds, {tried} candidates\n", first.prog.seed, first.prog.index));
    s.push_str("// Expected (R, from the abstract program):\n");
    if e.refusals.is_empty() {
        s.push_str("//   accept\n");
    }
    for r in &e.refusals {
        s.push_str(&format!("//   [{}] {} roots={:?}\n", r.rule, r.level.noun(), r.roots));
    }
    s.push_str("// Actual (driver, from `cargo trame check --examples --keep-going`):\n");
    match final_out {
        Some(f) if f.unit.recs.is_empty() => s.push_str("//   accept\n"),
        Some(f) => {
            for raw in &f.unit.raw {
                s.push_str(&format!("//   {raw}\n"));
            }
        }
        None => s.push_str("//   (not re-run: the shrunk program's batch failed)\n"),
    }
    s.push_str(&format!("// Differences (shrunk program): {diff_text}\n"));
    s.push_str(&format!("// Differences (first seen): {}\n", first.diff.describe()));
    s.push_str("// Decide whether R or V is wrong by re-reading spec.md section 3 and driver/lists/.\n");
    s.push_str("// Requires driver/fuzz/support/trame (the stand-in crate) as a path dependency.\n\n");
    s.push_str(&emit::source(small));
    s
}

#[allow(clippy::too_many_arguments)]
fn print_report(seed: u64, batches: u64, k: u64, o: GenOpts, t: &Totals, mismatches: usize, shrunk: usize, harness: &[String], elapsed: f64) {
    println!("trame-fuzz run: seed={seed} batches={batches} K={k} extra={}", o.extra);
    println!("  programs              {}", t.programs);
    println!("  roots                 {}", t.roots);
    println!("  R refusals (expected) {}", t.refusals_expected);
    println!("  R accepts             {}", t.accepted_by_r);
    println!("  V accepts             {}", t.accepted_by_v);
    println!("  agreements            {}", t.agreements);
    println!("  mismatched programs   {mismatches}  (shrunk: {shrunk})");
    println!("  VERDICT MISSES (sound.) {}", t.verdict_misses);
    println!("  over-refusals (prec.) {}", t.over);
    println!("  attribution misses    {}", t.attr_misses);
    println!("  extra rules           {}", t.extra);
    println!("  undecided (drop glue) {}", t.undecided);
    for (rule, (exp, miss)) in &t.by_rule {
        if *miss > 0 {
            println!("    {rule}: R expects {exp}, attribution-missed {miss}");
        }
    }
    for h in harness {
        println!("  HARNESS: {h}");
    }
    println!("  wall time             {elapsed:.1}s");
}

fn cmd_sweep(args: &Args) -> i32 {
    let k = args.num("--k", 400).max(1) as usize;
    let root = fuzz_root(args);
    let work = root.join("work");
    let drv = driver_dir(args);
    let start = std::time::Instant::now();
    let (rep, _n) = match sweep::run(&work, k, drv.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            println!("HARNESS: {e}");
            return 2;
        }
    };
    println!("trame-fuzz sweep: K={k}");
    println!("  cases                 {}", rep.cases);
    println!("  agreements with R     {}", rep.agreements);
    println!("  VERDICT MISSES (sound.) {}", rep.verdict_misses);
    println!("  over-refusals (prec.) {}", rep.over);
    println!("  attribution misses    {}", rep.attr_misses);
    println!("  extra rules           {}", rep.extra);
    println!("  metamorphic violations {}", rep.meta_violations.len());
    for m in rep.meta_violations.iter().take(50) {
        println!("    {m}");
    }
    for (label, o) in rep.mismatches.iter().take(50) {
        println!("    R/V differ: {label}: {}", o.diff.describe());
    }
    println!("  wall time             {:.1}s", start.elapsed().as_secs_f64());
    if rep.mismatches.is_empty() && rep.meta_violations.is_empty() { 0 } else { 1 }
}
