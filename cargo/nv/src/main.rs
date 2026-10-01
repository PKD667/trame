//! cargo nv: build/run a crate through the cuda-oxide codegen backend.
//!
//! Cargo invokes `cargo-nv nv <sub> <args>`, `<sub>` one of build, run, test. The
//! subcommand sets CARGO_ENCODED_RUSTFLAGS to point rustc at the backend .so and delegates to
//! cargo. Works on any plain crate that uses `#[kernel]`; no `#[cuda_module]`
//! or cuda-oxide checkout required.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, exit};

use nv_cargo::detect_arch;

use sha2::{Digest, Sha256};

const ENCODED_SEPARATOR: char = '\u{1f}';

fn backend_so() -> PathBuf {
    let value = match env::var("CUDA_OXIDE_BACKEND") {
        Ok(value) if !value.is_empty() => value,
        Ok(_) => {
            eprintln!("error: CUDA_OXIDE_BACKEND is empty");
            exit(1);
        }
        Err(_) => {
            eprintln!("error: CUDA_OXIDE_BACKEND is unset");
            exit(1);
        }
    };
    let path = PathBuf::from(&value);
    let canonical = match fs::canonicalize(&path) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("error: CUDA_OXIDE_BACKEND={value} cannot be canonicalized: {error}");
            exit(1);
        }
    };
    if path != canonical {
        eprintln!(
            "error: CUDA_OXIDE_BACKEND={value} is not canonical; use {}",
            canonical.display()
        );
        exit(1);
    }
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => {
            eprintln!("error: CUDA_OXIDE_BACKEND={value} cannot be inspected: {error}");
            exit(1);
        }
    };
    if !metadata.is_file() {
        eprintln!("error: CUDA_OXIDE_BACKEND={value} is not a regular file");
        exit(1);
    }
    if let Err(error) = fs::File::open(&path) {
        eprintln!("error: CUDA_OXIDE_BACKEND={value} is not readable: {error}");
        exit(1);
    }
    path
}

fn usage() -> ! {
    eprintln!("usage: cargo nv <build|run|test> [cargo args...]");
    exit(2);
}

fn digest(path: &PathBuf) -> String {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("error: could not read backend {}: {error}", path.display());
            exit(1);
        }
    };
    format!("{:x}", Sha256::digest(bytes))
}

fn fingerprint(backend: &str, detected_arch: &str) -> String {
    let mut settings: BTreeMap<OsString, OsString> = env::vars_os()
        .filter(|(key, _)| {
            key.to_string_lossy().starts_with("CUDA_OXIDE_")
                && key != "CUDA_OXIDE_INTERNAL_CODEGEN_FINGERPRINT"
        })
        .collect();
    settings.insert("CUDA_OXIDE_DEVICE_ARCH".into(), detected_arch.into());

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
    // Cargo runs us as `cargo-nv nv <args>`; direct invocation may omit the name.
    let start = if args
        .get(1)
        .is_some_and(|a| a == "build" || a == "run" || a == "test")
    {
        1
    } else {
        2
    };
    let Some(sub) = args.get(start) else { usage() };
    if sub != "build" && sub != "run" && sub != "test" {
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
    let detected_arch = match detect_arch() {
        Ok(arch) => arch,
        Err(error) => {
            eprintln!("error: {error}");
            exit(1);
        }
    };
    let identity = format!("backend-sha256={digest} arch={detected_arch}\n");
    eprintln!("[cargo-nv] {identity}");
    let manifest_dir = match env::var_os("NVMPI_MANIFEST_DIR") {
        Some(path) => PathBuf::from(path),
        None => {
            eprintln!("error: NVMPI_MANIFEST_DIR is unset");
            exit(1);
        }
    };
    let mut log = match OpenOptions::new()
        .create(true)
        .append(true)
        .open(manifest_dir.join("cargo-nv-identity.log"))
    {
        Ok(log) => log,
        Err(error) => {
            eprintln!(
                "error: cannot append cargo-nv identity to {}: {error}",
                manifest_dir.display()
            );
            exit(1);
        }
    };
    if let Err(error) = log.write_all(identity.as_bytes()) {
        eprintln!("error: cannot write cargo-nv identity log: {error}");
        exit(1);
    }
    let fingerprint = fingerprint(&digest, &detected_arch);
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
    command.env("CUDA_OXIDE_DEVICE_ARCH", detected_arch);
    let status = match command.status() {
        Ok(status) => status,
        Err(error) => {
            eprintln!("error: failed to run cargo: {error}");
            exit(1);
        }
    };
    exit(match status.code() {
        Some(code) => code,
        None => 1,
    });
}
