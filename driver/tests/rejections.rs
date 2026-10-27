//! The driver's test is the rejection suite (rejections/README.md). It runs rejections/run.sh against the
//! real trame crate, with the driver binaries this build has just made, so there is one suite and one
//! expectation format.

use std::path::Path;
use std::process::Command;

#[test]
fn rejection_suite() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    // cargo-trame and trame-driver are built side by side; run.sh takes the directory.
    let bin = Path::new(env!("CARGO_BIN_EXE_cargo-trame")).parent().expect("bin directory");
    let status = Command::new("bash")
        .arg(root.join("rejections/run.sh"))
        .env("TRAME_REJECTIONS_INNER", "1")
        .env("TRAME_DRIVER_BIN", bin)
        .status()
        .expect("cannot run rejections/run.sh");
    assert!(
        status.success(),
        "rejections/run.sh reported a mismatch; the table is above (run with --nocapture), logs in rejections/target/rejections-logs/"
    );
}
