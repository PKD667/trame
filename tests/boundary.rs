// The dependency direction, checked rather than trusted (backend.md).
//
// The backend owns primitives and generic mechanisms; NERVE is an application expressed above
// it. So the arrow points one way — `NERVE → backend API` — and this file fails if it ever turns
// round: no manifest entry naming a crate from above the backend, and no source file importing
// one.
//
// Scope, deliberately small. It reads this crate's own manifest and its own `.rs` files. It is
// not a repository-wide grep harness, and it does not need to be: naming a NERVE type requires a
// dependency, a dependency requires a manifest entry, and a manifest entry fails the first test
// here. The second test catches the intermediate step — an import written against a dependency
// that has not been added yet — while the message still says what the rule is.

use std::fs;
use std::path::{Path, PathBuf};

/// Crates that live above the backend. A backend that names one has inverted the dependency.
/// Written as both the package name and the module name an import would use.
const ABOVE: [&str; 6] = [
    "nerve",
    "nerve-ffi",
    "nerve_ffi",
    "pn",
    "nerve-core",
    "nerve_core",
];

/// This crate's directory.
fn here() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every Rust source file this crate owns.
fn sources(from: &Path, found: &mut Vec<PathBuf>) {
    let listing = fs::read_dir(from).expect("the backend's own directory is readable");
    for entry in listing.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            found.push(path);
        }
    }
}

/// The crate a dependency line names, if that crate is above the backend. A path dependency
/// reaching into the application tree is the same inversion under a different name.
fn depended(line: &str) -> Option<&str> {
    let (name, rest) = line.split_once('=')?;
    let name = name.trim().trim_matches('"');
    (ABOVE.contains(&name) || rest.contains("../src/")).then_some(name)
}

/// The first segment of the path a `use` declaration names, if the line is one.
fn imported(line: &str) -> Option<&str> {
    let line = line.trim();
    let path = line
        .strip_prefix("use ")
        .or_else(|| line.strip_prefix("pub use "))
        .or_else(|| line.strip_prefix("pub(crate) use "))?;
    let path = path.trim_start().trim_start_matches("::");
    Some(path.split([':', ' ', ';', '{', ',']).next().unwrap_or(path))
}

#[test]
fn the_manifest_depends_on_nothing_above_the_backend() {
    let manifest = fs::read_to_string(here().join("Cargo.toml")).expect("the backend manifest");
    let mut section = String::new();
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|at| at.strip_suffix(']')) {
            section = name.to_string();
            continue;
        }
        if !section.contains("dependencies") {
            continue;
        }
        assert!(
            depended(line).is_none(),
            "trame/Cargo.toml depends on something above the backend: `{line}`. The \
             dependency direction is NERVE -> backend API"
        );
    }
}

#[test]
fn the_two_checks_recognise_an_inversion_when_they_see_one() {
    // Without this, a matcher that quietly stopped recognising anything would still pass both
    // tests above, and they would be checking nothing at all.
    assert_eq!(
        depended("nerve = { path = \"../src/nerve\" }"),
        Some("nerve")
    );
    assert_eq!(depended("nerve_ffi = \"0.1\""), Some("nerve_ffi"));
    assert_eq!(
        depended("model = { path = \"../src/nerve/model\" }"),
        Some("model")
    );
    assert_eq!(depended("libc = \"0.2\""), None);
    assert_eq!(depended("mpi-rma = { path = \"../mpi-rma\" }"), None);

    assert_eq!(imported("use nerve::message::BACKENDS;"), Some("nerve"));
    assert_eq!(imported("pub use pn::watch::Probe;"), Some("pn"));
    assert_eq!(
        imported("    use crate::cpu::sync::turn::Exclusive;"),
        Some("crate")
    );
    assert_eq!(imported("use std::sync::Mutex;"), Some("std"));
    assert_eq!(imported("let nerve = 1;"), None);
}

/// One source file, as its lines of code with the prose and the literals removed.
fn code_of(file: &Path) -> Vec<(usize, String)> {
    let text = fs::read_to_string(file).expect("a source file of this crate");
    text.lines()
        .enumerate()
        .map(|(at, line)| (at + 1, code(line)))
        .collect()
}

/// The line with a trailing `//` comment removed. Prose is where the reasons live, and the
/// reasons here name the thing they are about — a doc comment saying "this backend has no
/// `cpu::sync`" would otherwise be a failure of the check that keeps `cpu::sync` out.
fn code(line: &str) -> String {
    match line.find("//") {
        Some(at) => line[..at].to_string(),
        None => line.to_string(),
    }
}

#[test]
fn the_device_backend_names_none_of_the_host_mechanisms() {
    // `cpu/` is what a backend whose participants are *host threads* answers: a thread to start,
    // a mutex to take, a clock to read. The device backend is not that kind of backend, and the
    // moment it names one of those mechanisms it has either grown a host dependency it cannot
    // honour or lost the isolation that makes it a separate backend at all. So the rule is a
    // check rather than a convention: nothing under `nv/` names `cpu`.
    let dir = here().join("nv");
    let mut files = Vec::new();
    sources(&dir, &mut files);
    assert!(
        !files.is_empty(),
        "the walk found no source under nv/, so it would check nothing"
    );
    for file in files {
        for (at, line) in code_of(&file) {
            let names_cpu = line.contains("crate::cpu")
                || line.contains("cpu::")
                || imported(&line) == Some("cpu");
            assert!(
                !names_cpu,
                "{}:{} names `cpu`, which is a host mechanism: the device backend answers the \
                 transport surface without it, and `nv/` holds its own equivalents",
                file.display(),
                at
            );
        }
    }
}

#[test]
fn no_source_imports_a_crate_above_the_backend() {
    let mut files = Vec::new();
    sources(&here(), &mut files);
    assert!(files.len() > 10, "the source walk found almost nothing");
    for file in files {
        let text = fs::read_to_string(&file).expect("a source file of this crate");
        for (at, line) in text.lines().enumerate() {
            let Some(crate_name) = imported(line) else {
                continue;
            };
            assert!(
                !ABOVE.contains(&crate_name),
                "{}:{} imports `{crate_name}`, which is above the backend: the backend owns \
                 primitives and generic mechanisms, NERVE owns the model and operations",
                file.display(),
                at + 1
            );
        }
    }
}
