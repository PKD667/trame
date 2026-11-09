//! The driver's test is the rejection suite (check/rejections/README.md), run by its own harness
//! (`check/rejections/tests/ui.rs`) with the driver binaries this build has just made.

use std::path::Path;
use std::process::Command;

#[test]
fn rejection_suite() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // cargo-trame and trame-driver are built side by side; the suite takes the directory.
    let bin = Path::new(env!("CARGO_BIN_EXE_cargo-trame")).parent().expect("bin directory");
    // The suite holds on every backend; the run takes one (check/rejections/tests/ui.rs reads it).
    let backend = std::env::var("TRAME_BACKEND").expect("TRAME_BACKEND must name a backend of src/backends/");
    let status = Command::new(env!("CARGO"))
        .args(["test", "--offline", "--manifest-path"])
        .arg(root.join("check/rejections/Cargo.toml"))
        .arg(format!("--features=trame/{backend}"))
        .env("TRAME_DRIVER_BIN", bin)
        .env("TRAME_BACKEND", backend)
        .status()
        .expect("cannot run cargo test in check/rejections/");
    assert!(status.success(), "the rejection suite failed; its report is above");
}
