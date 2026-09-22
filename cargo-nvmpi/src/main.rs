//! cargo nvmpi: build/run a crate through the cuda-oxide codegen backend.
//!
//! Cargo invokes `cargo-nvmpi nvmpi <sub> <args>`. The subcommand sets
//! CARGO_ENCODED_RUSTFLAGS to point rustc at the backend .so and delegates to
//! cargo. Works on any plain crate that uses `#[kernel]`; no `#[cuda_module]`
//! or cuda-oxide checkout required.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, exit};

use cargo_nvmpi::detect_arch;

use sha2::{Digest, Sha256};

const ENCODED_SEPARATOR: char = '\u{1f}';

fn backend_so() -> PathBuf {
    if let Ok(p) = env::var("CUDA_OXIDE_BACKEND") {
        let path = PathBuf::from(&p);
        if path.exists() {
            return path;
        }
        eprintln!("warning: CUDA_OXIDE_BACKEND={p} does not exist, falling back");
    }
    if let Some(home) = env::var_os("HOME") {
        let cached = PathBuf::from(home).join(".cargo/cuda-oxide/librustc_codegen_cuda.so");
        if cached.exists() {
            return cached;
        }
    }
    eprintln!("error: cuda-oxide backend not found");
    eprintln!("  set CUDA_OXIDE_BACKEND=/path/to/librustc_codegen_cuda.so, or");
    eprintln!("  run `cargo oxide setup` inside a cuda-oxide checkout to publish the shared cache");
    exit(1);
}

fn usage() -> ! {
    eprintln!("usage: cargo nvmpi <build|run> [cargo args...]");
    exit(2);
}

fn digest(path: &PathBuf) -> String {
    let bytes = fs::read(path).unwrap_or_else(|error| {
        eprintln!("error: could not read backend {}: {error}", path.display());
        exit(1);
    });
    format!("{:x}", Sha256::digest(bytes))
}

fn fingerprint(backend: &str, detected_arch: Option<&str>) -> String {
    let mut settings: BTreeMap<OsString, OsString> = env::vars_os()
        .filter(|(key, _)| {
            key.to_string_lossy().starts_with("CUDA_OXIDE_")
                && key != "CUDA_OXIDE_INTERNAL_CODEGEN_FINGERPRINT"
        })
        .collect();
    if let Some(arch) = detected_arch {
        settings.insert("CUDA_OXIDE_DEVICE_ARCH".into(), arch.into());
    }

    let mut hash = Sha256::new();
    update_hash(&mut hash, backend.as_bytes());
    for (key, value) in settings {
        update_hash(&mut hash, key.as_encoded_bytes());
        update_hash(&mut hash, value.as_encoded_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn update_hash(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    // Cargo runs us as `cargo-nvmpi nvmpi <args>`; direct invocation may omit the name.
    let start = if args.get(1).is_some_and(|a| a == "build" || a == "run") {
        1
    } else {
        2
    };
    let Some(sub) = args.get(start) else { usage() };
    if sub != "build" && sub != "run" {
        usage();
    }
    let sub_args = &args[start + 1..];

    let backend = backend_so();
    let digest = digest(&backend);
    // Detected for `build` as well as `run`. The arch is baked into the PTX at compile time, so a
    // binary built without it carries code for the wrong target and fails at load with
    // CUDA_ERROR_INVALID_PTX — and because the arch is part of `fingerprint`, gating it to `run`
    // also made the two subcommands invalidate each other's artifacts. A harness that builds once
    // and launches the binary separately (the shape `trame/bench.sh` uses) depends on this.
    // `detect_arch` answers `None` when `nvidia-smi` is absent or the arch is stated explicitly, so
    // a build on a machine with no GPU is unchanged.
    let detected_arch = detect_arch();
    let fingerprint = fingerprint(&digest, detected_arch.as_deref());
    let mut flags: Vec<String> = Vec::new();
    if let Ok(existing) = env::var("CARGO_ENCODED_RUSTFLAGS") {
        flags.extend(
            existing
                .split(ENCODED_SEPARATOR)
                .filter(|flag| !flag.is_empty())
                .map(String::from),
        );
    } else if let Ok(existing) = env::var("RUSTFLAGS") {
        flags.extend(existing.split_whitespace().map(String::from));
    }
    flags.extend([
        "--cfg".to_string(),
        format!("cuda_oxide_internal_backend_identity=\"{digest}\""),
        format!("-Zcodegen-backend={}", backend.display()),
        "-Zmir-enable-passes=-JumpThreading".to_string(),
        "-Zalways-encode-mir".to_string(),
        "-Csymbol-mangling-version=v0".to_string(),
    ]);
    let encoded = flags.join(&ENCODED_SEPARATOR.to_string());

    let mut command = Command::new("cargo");
    command
        .arg(sub)
        .arg("--release")
        .args(sub_args)
        .env("CARGO_ENCODED_RUSTFLAGS", encoded)
        .env("CUDA_OXIDE_INTERNAL_CODEGEN_FINGERPRINT", fingerprint)
        .env_remove("RUSTFLAGS");
    if let Some(arch) = detected_arch {
        command.env("CUDA_OXIDE_DEVICE_ARCH", arch);
    }
    let status = command.status().expect("failed to run cargo");
    exit(status.code().unwrap_or(1));
}
