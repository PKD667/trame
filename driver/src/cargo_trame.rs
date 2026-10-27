//! `cargo trame check|build|test [cargo args]`: the single entry point for trame's checks.
//!
//! Runs cargo with `trame-driver` as the workspace wrapper, so every crate of the workspace is
//! compiled by the driver: crates that depend on trame are checked (driver/README.md).
//! `TRAME_DRIVER=<path>` overrides the driver; by default it is the binary next to this one.

use std::env;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: cargo trame <check|build|test> [cargo arguments...]\n\
    runs cargo with trame's compile-time checks (spec.md, Compile time)";

fn driver() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("TRAME_DRIVER") {
        return Ok(PathBuf::from(path));
    }
    let exe = env::current_exe().map_err(|e| format!("cannot find cargo-trame: {e}"))?;
    let path = exe.with_file_name("trame-driver");
    if path.exists() { Ok(path) } else { Err(format!("no trame-driver next to {}", exe.display())) }
}

/// The target directory for checked builds: `<target>/trame`, apart from plain cargo's `<target>`.
/// Cargo's fingerprints do not include the wrapper's extra flags (`--cfg trame_checked`), so a
/// shared directory would let plain cargo reuse artifacts the driver built, or the reverse.
fn checked_target_dir() -> Result<PathBuf, String> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err("cargo metadata failed".into());
    }
    let json = String::from_utf8_lossy(&out.stdout);
    let key = "\"target_directory\":\"";
    let start = json.find(key).ok_or("cargo metadata has no target_directory")? + key.len();
    let end = json[start..].find('"').ok_or("malformed target_directory")?;
    Ok(PathBuf::from(&json[start..start + end]).join("trame"))
}

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();
    // `cargo trame ...` runs `cargo-trame trame ...`; accept both spellings.
    if args.first().map(String::as_str) == Some("trame") {
        args.remove(0);
    }
    let Some(sub) = args.first().cloned() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if !matches!(sub.as_str(), "check" | "build" | "test") {
        eprintln!("cargo trame: unknown subcommand `{sub}`\n{USAGE}");
        return ExitCode::from(2);
    }
    let driver = match driver() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cargo trame: {e}");
            return ExitCode::FAILURE;
        }
    };
    let target = match checked_target_dir() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cargo trame: {e}");
            return ExitCode::FAILURE;
        }
    };
    let status = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(&args)
        .env("CARGO_TARGET_DIR", &target)
        // All crates, not only workspace members: see main.rs on `-Zalways-encode-mir`.
        .env("RUSTC_WRAPPER", &driver)
        .status();
    match status {
        Ok(s) => ExitCode::from(s.code().unwrap_or(1) as u8),
        Err(e) => {
            eprintln!("cargo trame: cannot run cargo: {e}");
            ExitCode::FAILURE
        }
    }
}
