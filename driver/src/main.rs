//! `trame-driver`: rustc with trame's compile-time checks (spec.md, Compile time).
//!
//! Cargo runs it in place of rustc through `RUSTC_WORKSPACE_WRAPPER` (see `cargo-trame`):
//! `trame-driver <rustc> <args...>`. A crate that depends on trame is compiled with
//! `--cfg trame_checked` and checked after analysis; every other invocation behaves like rustc.

#![feature(rustc_private)]

extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

mod check;

use rustc_driver::Compilation;
use rustc_interface::interface;
use rustc_middle::ty::TyCtxt;
use std::env;
use std::path::Path;
use std::process::{Command, ExitCode};

/// Output of `rustc -vV` the driver was built for (from build.rs).
const BUILT_FOR: &str = env!("TRAME_RUSTC_VERSION");
/// The sysroot of the toolchain the driver was built with (from build.rs).
const SYSROOT: &str = env!("TRAME_SYSROOT");

struct Checker {
    enabled: bool,
    errors: usize,
}

impl rustc_driver::Callbacks for Checker {
    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> Compilation {
        if !self.enabled {
            return Compilation::Continue;
        }
        self.errors = check::run(tcx);
        if self.errors > 0 { Compilation::Stop } else { Compilation::Continue }
    }
}

/// `rustc` given by cargo as the first argument of the wrapper, if any.
fn rustc_arg(arg: &str) -> bool {
    Path::new(arg).file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("rustc"))
}

/// Whether this invocation compiles a crate (as opposed to querying rustc).
fn is_compile(args: &[String]) -> bool {
    let queries = args.iter().any(|a| a == "--print" || a.starts_with("--print=") || a == "-vV" || a == "-V" || a == "--version");
    args.iter().any(|a| a == "--crate-name") && !queries
}

/// The crate depends on trame and is not trame itself: only these are checked.
fn is_checked(args: &[String]) -> bool {
    let depends = args.windows(2).any(|w| w[0] == "--extern" && w[1].starts_with("trame="));
    let name = args.windows(2).find(|w| w[0] == "--crate-name").map(|w| w[1].as_str());
    depends && name != Some("trame")
}

/// Refuse to run if cargo's rustc is not the compiler the driver was built against: the crates
/// the driver compiles would not match the dependencies cargo compiled with that rustc.
fn same_compiler(rustc: &str) -> Result<(), String> {
    let out = Command::new(rustc).arg("-vV").output().map_err(|e| format!("cannot run {rustc}: {e}"))?;
    let got = String::from_utf8_lossy(&out.stdout);
    // `BUILT_FOR` has its lines separated by `|` (build.rs), `rustc -vV` by newlines.
    let pick = |s: &str| {
        s.split(['\n', '|'])
            .filter(|l| l.starts_with("commit-hash") || l.starts_with("release"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if pick(&got) == pick(BUILT_FOR) {
        Ok(())
    } else {
        Err(format!(
            "trame-driver was built for a different rustc than {rustc}:\n  driver: {}\n  cargo:  {}",
            pick(BUILT_FOR),
            pick(&got)
        ))
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().collect();
    let mut rest: &[String] = &argv[1..];
    let rustc = match rest.first() {
        Some(first) if rustc_arg(first) => {
            rest = &rest[1..];
            Some(first.clone())
        }
        _ => None,
    };

    if !is_compile(rest) {
        // Queries (`-vV`, `--print`, probes) are rustc's own business: run it unchanged.
        let program = rustc.unwrap_or_else(|| "rustc".to_string());
        return match Command::new(&program).args(rest).status() {
            Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
            Err(e) => {
                eprintln!("trame-driver: cannot run {program}: {e}");
                ExitCode::FAILURE
            }
        };
    }

    if let Some(rustc) = &rustc
        && let Err(msg) = same_compiler(rustc)
    {
        eprintln!("trame-driver: {msg}");
        return ExitCode::FAILURE;
    }

    let checked = is_checked(rest);
    let mut args = vec!["trame-driver".to_string()];
    args.extend(rest.iter().cloned());
    // Every crate, checked or not, keeps its optimized MIR in metadata: a checked crate reads the
    // MIR of generic trame code from its dependencies, and `--emit=metadata` (cargo check) omits it.
    args.push("-Zalways-encode-mir".into());
    args.push("--cfg".into());
    args.push("trame_checked".into());
    if !rest.iter().any(|a| a == "--sysroot") {
        args.push("--sysroot".into());
        args.push(SYSROOT.into());
    }
    if checked {
        // Eager mono collection: every non-generic item is a root, also under `cargo check`, which
        // emits no code and so has no exported-item roots (the lazy strategy).
        args.push("-Clink-dead-code".into());
        // Known approximation, see README: the checker reads optimized MIR, and with MIR inlining
        // off a `for` loop keeps its call to `Iterator::next`, which the counted-loop rule needs.
        args.push("-Zinline-mir=no".into());
    }

    let mut callbacks = Checker { enabled: checked, errors: 0 };
    rustc_driver::catch_with_exit_code(|| rustc_driver::run_compiler(&args, &mut callbacks))
}
