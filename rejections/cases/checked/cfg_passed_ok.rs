//! expect: accepted
// spec: Requirement (Checked builds): the driver compiles with --cfg trame_checked. This case fails to compile
// unless the driver passed the flag, so it tests that the driver sets it (the old fixture cfg_ok).
#[cfg(not(trame_checked))]
compile_error!("trame-driver did not pass --cfg trame_checked");

fn main() {}
