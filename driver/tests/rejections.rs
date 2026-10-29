//! The driver's test is the rejection suite (rejections/README.md), run by its own harness
//! (`rejections/tests/ui.rs`) with the driver binaries this build has just made.

use std::path::Path;
use std::process::Command;

#[test]
fn rejection_suite() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    // cargo-trame and trame-driver are built side by side; the suite takes the directory.
    let bin = Path::new(env!("CARGO_BIN_EXE_cargo-trame")).parent().expect("bin directory");
    let status = Command::new(env!("CARGO"))
        .args(["test", "--offline", "--manifest-path"])
        .arg(root.join("rejections/Cargo.toml"))
        .env("TRAME_DRIVER_BIN", bin)
        .status()
        .expect("cannot run cargo test in rejections/");
    assert!(status.success(), "the rejection suite failed; its report is above");
}
