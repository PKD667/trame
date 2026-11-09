// The driver links rustc's own libraries, so it must run with the sysroot of the compiler it was
// built with. This records that sysroot (the library directory goes into the RUNPATH, so no
// LD_LIBRARY_PATH is needed) and the compiler's version, which `main` compares with cargo's rustc.
use std::process::Command;

fn query(rustc: &str, args: &[&str]) -> String {
    let out = Command::new(rustc).args(args).output().unwrap_or_else(|e| panic!("cannot run {rustc}: {e}"));
    assert!(out.status.success(), "{rustc} {args:?} failed");
    String::from_utf8(out.stdout).expect("rustc output is UTF-8")
}

fn main() {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let sysroot = query(&rustc, &["--print", "sysroot"]).trim().to_string();
    let version = query(&rustc, &["-vV"]);
    println!("cargo:rustc-link-arg=-Wl,-rpath,{sysroot}/lib");
    println!("cargo:rustc-env=TRAME_SYSROOT={sysroot}");
    println!("cargo:rustc-env=TRAME_RUSTC_VERSION={}", version.replace('\n', "|"));
    println!("cargo:rerun-if-env-changed=RUSTC");
    println!("cargo:rerun-if-changed=build.rs");
}
