//! A batch: K programs as K examples of one crate, one `cargo-trame check --examples --keep-going`
//! for all of them, then the comparison of each example's diagnostics with R (oracle.rs).

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

use crate::emit;
use crate::json::quote;
use crate::oracle::{self, Expect};
use crate::prog::Prog;
use crate::verdict::{self, Diff, Parsed};

pub struct Case {
    /// The example's name, e.g. `p00042`.
    pub name: String,
    pub prog: Prog,
    pub expect: Expect,
    pub source: String,
}

impl Case {
    pub fn new(name: String, prog: Prog) -> Case {
        let expect = oracle::expect(&prog);
        let source = emit::source(&prog);
        Case { name, prog, expect, source }
    }
}

/// The directory of the stand-in trame, relative to a batch directory `work/<name>/`.
const SUPPORT: &str = "../../support/trame";

/// A batch directory must be a direct child of `work/`: the stand-in's relative path is fixed at
/// `../../support/trame` (two levels above `work/<batch>/`).
fn check_depth(dir: &Path) -> io::Result<()> {
    if dir.parent().and_then(|p| p.file_name()).is_none_or(|n| n != "work") {
        return Err(io::Error::other(format!("batch dir {} must be work/<name>", dir.display())));
    }
    Ok(())
}

pub fn write(dir: &Path, cases: &[Case]) -> io::Result<()> {
    check_depth(dir)?;
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    fs::create_dir_all(dir.join("examples"))?;
    let mut manifest = String::new();
    manifest.push_str(
        "[package]\nname = \"batch\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\n\n[dependencies]\n",
    );
    manifest.push_str(&format!("trame = {{ path = \"{SUPPORT}\" }}\n"));
    fs::write(dir.join("Cargo.toml"), manifest)?;
    let mut exp = String::from("[\n");
    for (i, c) in cases.iter().enumerate() {
        fs::write(dir.join("examples").join(format!("{}.rs", c.name)), &c.source)?;
        exp.push_str(&format!(
            "{{\"example\":{},\"seed\":{},\"index\":{},\"roots\":[{}],\"refusals\":[{}]}}",
            quote(&c.name),
            c.prog.seed,
            c.prog.index,
            c.expect
                .roots
                .iter()
                .map(|(n, l)| format!("{{\"name\":{},\"level\":{}}}", quote(n), quote(l.noun())))
                .collect::<Vec<_>>()
                .join(","),
            c.expect
                .refusals
                .iter()
                .map(|r| format!(
                    "{{\"rule\":{},\"level\":{},\"roots\":[{}]}}",
                    quote(&r.rule),
                    quote(r.level.noun()),
                    r.roots.iter().map(|x| quote(x)).collect::<Vec<_>>().join(",")
                ))
                .collect::<Vec<_>>()
                .join(","),
        ));
        exp.push_str(if i + 1 < cases.len() { ",\n" } else { "\n" });
    }
    exp.push_str("]\n");
    fs::write(dir.join("expect.json"), exp)?;
    Ok(())
}

/// Run the driver once over the batch. `driver_dir` (if any) is put first on PATH so the
/// `cargo-trame` of that build is the one used. Returns the combined output.
pub fn run_driver(dir: &Path, driver_dir: Option<&Path>) -> io::Result<String> {
    let mut path = std::env::var("PATH").unwrap_or_default();
    if let Some(d) = driver_dir {
        path = format!("{}:{}", d.display(), path);
    }
    // A fresh target dir: cargo caches checks, and a cached example would be silently skipped,
    // which the comparator would report as a miss on every refusal.
    let target = dir.join("target");
    let out = Command::new("cargo-trame")
        .args(["check", "--examples", "--keep-going", "--target-dir"])
        .arg(&target)
        .current_dir(dir)
        .env("PATH", path)
        .output()?;
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    // Cargo prints `Checking batch vX` once the driver has been asked to check the batch. Without
    // it the driver never ran (a manifest or metadata failure), and the output must not be read as
    // "everything accepted": say so as a harness line.
    if !s.lines().any(|l| l.trim_start().starts_with("Checking batch v")) {
        s.insert_str(0, "error: harness: cargo-trame did not check the batch (no `Checking batch` line)\n");
    }
    Ok(s)
}

/// The result for one case.
#[derive(Clone)]
pub struct Outcome {
    pub prog: Prog,
    pub expect: Expect,
    pub unit: verdict::Unit,
    pub diff: Diff,
}

/// Compare a run's output with each case's expectation. A harness line (anything that is not a
/// driver refusal, see verdict.rs) is returned as an error: such a run says nothing.
pub fn judge(cases: Vec<Case>, parsed: &Parsed) -> Result<Vec<Outcome>, Vec<String>> {
    if !parsed.harness.is_empty() {
        return Err(parsed.harness.clone());
    }
    let mut out = Vec::new();
    for c in cases {
        // An accepted case prints nothing, so it has no unit. A crash prints a harness line, which
        // failed the batch above: a missing unit here is an acceptance, not missing data.
        let unit = parsed.units.get(&c.name).cloned().unwrap_or_default();
        let diff = verdict::compare(&c.expect, &unit.recs);
        out.push(Outcome { prog: c.prog, expect: c.expect, unit, diff });
    }
    Ok(out)
}

/// The whole batch: write, run, parse, judge. Returns the outcomes, or the harness problems.
pub fn run(dir: &Path, cases: Vec<Case>, driver_dir: Option<&Path>) -> Result<Vec<Outcome>, Vec<String>> {
    if let Err(e) = write(dir, &cases) {
        return Err(vec![format!("writing {}: {e}", dir.display())]);
    }
    let out = match run_driver(dir, driver_dir) {
        Ok(s) => s,
        Err(e) => return Err(vec![format!("running cargo-trame: {e}")]),
    };
    // Keep the driver's full output with the batch, for the report.
    let _ = fs::write(dir.join("driver-output.txt"), &out);
    let parsed = verdict::parse(&out);
    judge(cases, &parsed)
}

/// Remove a batch's build directory (its sources and expectation stay).
pub fn clean_target(dir: &Path) {
    let _ = fs::remove_dir_all(dir.join("target"));
}

/// Print a line to stdout and flush (progress lines for long runs).
pub fn say(msg: &str) {
    let mut so = io::stdout().lock();
    let _ = writeln!(so, "{msg}");
    let _ = so.flush();
}
