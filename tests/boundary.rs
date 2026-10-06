// The dependency direction, checked rather than trusted (backend.md).
//
// The backend owns primitives and generic mechanisms; applications are expressed above it. So
// the arrow points one way — application → backend API — and this file fails if it ever turns
// round. Nothing here names an application: the check is an allow-list. Every dependency a
// manifest in this tree declares must be one listed below, from the source listed below, and
// every import must start at the standard library, one of those dependencies, this tree's own
// packages, or a name this tree binds itself.
//
// Scope, deliberately small. It reads the manifests and `.rs` files under this crate's
// directory. Naming an application type requires a dependency, a dependency requires a manifest
// entry, and a manifest entry fails the first test here. The second test catches the
// intermediate step — an import written against a dependency that has not been added yet —
// while the message still says what the rule is.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The dependencies this tree may declare, each with the only `path` it may take (`None`: a
/// registry or git source). Admitting a crate is an edit here, made where the direction is stated.
const DECLARED: [(&str, Option<&str>); 9] = [
    ("bytemuck", None),
    ("libc", None),
    ("trame-macros", Some("macros")),
    ("cuda-device", None),
    ("cuda-core", None),
    ("cuda-host", None),
    ("mpi", None),
    ("mpi-rma", Some("../mpi-rma")),
    ("sha2", None),
];

/// Import roots that need no manifest entry.
const LANGUAGE: [&str; 8] = [
    "std",
    "core",
    "alloc",
    "proc_macro",
    "crate",
    "self",
    "super",
    "$crate",
];

/// This crate's directory.
fn here() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every file under `from` with the given name suffix.
fn files(from: &Path, suffix: &str, found: &mut Vec<PathBuf>) {
    let listing = fs::read_dir(from).expect("the backend's own directory is readable");
    for entry in listing.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, suffix, found);
        } else if path.to_string_lossy().ends_with(suffix) {
            found.push(path);
        }
    }
}

/// Every Rust source file this crate owns.
fn sources(from: &Path, found: &mut Vec<PathBuf>) {
    files(from, ".rs", found);
}

/// The `path` a dependency entry names, if it names one.
fn path_of(rest: &str) -> Option<&str> {
    let (_, after) = rest.split_once("path")?;
    let after = after
        .trim_start()
        .strip_prefix('=')?
        .trim_start()
        .strip_prefix('"')?;
    after.split('"').next()
}

/// The crate a dependency line names, if the allow-list does not admit it from that source. A
/// `package` rename would let an admitted name stand for another crate, so it is never admitted.
fn depended(line: &str) -> Option<&str> {
    let (name, rest) = line.split_once('=')?;
    let name = name.trim().trim_matches('"');
    let admitted = DECLARED
        .iter()
        .any(|&(known, path)| known == name && path_of(rest) == path)
        && !rest.contains("package");
    (!admitted).then_some(name)
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

/// Every dependency entry of every manifest in this tree, with the manifest it is in.
fn dependency_lines() -> Vec<(PathBuf, String)> {
    let mut manifests = Vec::new();
    files(&here(), "Cargo.toml", &mut manifests);
    let mut found = Vec::new();
    for manifest in manifests {
        let text = fs::read_to_string(&manifest).expect("a manifest of this tree");
        let mut section = String::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|at| at.strip_suffix(']')) {
                section = name.to_string();
                continue;
            }
            if section.contains("dependencies") {
                found.push((manifest.clone(), line.to_string()));
            }
        }
    }
    found
}

/// The import roots this tree may use: the language's own, every admitted dependency, this
/// tree's packages, and every name the tree binds with `mod` or `as`.
fn admitted() -> BTreeSet<String> {
    let mut names: BTreeSet<String> = LANGUAGE.iter().map(|name| name.to_string()).collect();
    names.extend(DECLARED.iter().map(|(name, _)| name.replace('-', "_")));
    let mut manifests = Vec::new();
    files(&here(), "Cargo.toml", &mut manifests);
    for manifest in manifests {
        let text = fs::read_to_string(&manifest).expect("a manifest of this tree");
        let mut section = "";
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                section = line;
                continue;
            }
            if section != "[package]" && section != "[lib]" {
                continue;
            }
            if let Some(("name", value)) = line.split_once('=').map(|(k, v)| (k.trim(), v)) {
                names.insert(value.trim().trim_matches('"').replace('-', "_"));
            }
        }
    }
    let mut found = Vec::new();
    sources(&here(), &mut found);
    for file in found {
        for (_, line) in code_of(&file) {
            let words: Vec<&str> = line
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .filter(|word| !word.is_empty())
                .collect();
            for pair in words.windows(2) {
                if pair[0] == "mod" || pair[0] == "as" {
                    names.insert(pair[1].to_string());
                }
            }
        }
    }
    names
}

#[test]
fn the_manifests_declare_only_admitted_dependencies() {
    let lines = dependency_lines();
    assert!(
        lines.iter().any(|(_, line)| line.starts_with("libc")),
        "the manifest walk found no dependency, so it would check nothing"
    );
    for (manifest, line) in lines {
        assert!(
            depended(&line).is_none(),
            "{} declares a dependency the backend does not admit: `{line}`. The dependency \
             direction is application -> backend API",
            manifest.display()
        );
    }
}

#[test]
fn the_two_checks_recognise_an_inversion_when_they_see_one() {
    // Without this, a matcher that quietly stopped recognising anything would still pass both
    // tests around it, and they would be checking nothing at all.
    assert_eq!(depended("app = { path = \"../src/app\" }"), Some("app"));
    assert_eq!(depended("app_ffi = \"0.1\""), Some("app_ffi"));
    assert_eq!(
        depended("model = { path = \"../src/app/model\" }"),
        Some("model")
    );
    assert_eq!(
        depended("mpi-rma = { path = \"../src/app\" }"),
        Some("mpi-rma")
    );
    assert_eq!(depended("libc = { path = \"../libc\" }"), Some("libc"));
    assert_eq!(
        depended("libc = { package = \"app\", version = \"1\" }"),
        Some("libc")
    );
    assert_eq!(depended("libc = \"0.2\""), None);
    assert_eq!(
        depended("mpi-rma = { path = \"../mpi-rma\", optional = true }"),
        None
    );
    assert_eq!(depended("trame-macros = { path = \"macros\" }"), None);

    assert_eq!(imported("use app::message::BACKENDS;"), Some("app"));
    assert_eq!(imported("pub use pn::watch::Probe;"), Some("pn"));
    assert_eq!(
        imported("    use crate::host::sync::turn::Exclusive;"),
        Some("crate")
    );
    assert_eq!(imported("use std::sync::Mutex;"), Some("std"));
    assert_eq!(imported("let app = 1;"), None);

    let roots = admitted();
    for root in ["std", "crate", "trame", "mpi_rma", "trame_macros", "wire"] {
        assert!(
            roots.contains(root),
            "`{root}` should be an admitted import root"
        );
    }
    for root in ["app", "pn", "model_ffi"] {
        assert!(
            !roots.contains(root),
            "`{root}` should not be an admitted import root"
        );
    }
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
    let dir = here().join("backends/nv");
    let mut files = Vec::new();
    sources(&dir, &mut files);
    assert!(
        !files.is_empty(),
        "the walk found no source under nv/, so it would check nothing"
    );
    for file in files {
        for (at, line) in code_of(&file) {
            let names_cpu = line.contains("crate::host")
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
fn no_source_imports_a_crate_the_backend_does_not_admit() {
    let roots = admitted();
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
                roots.contains(crate_name),
                "{}:{} imports `{crate_name}`, which the backend does not admit: the backend \
                 owns primitives and generic mechanisms, the application owns the model and \
                 operations",
                file.display(),
                at + 1
            );
        }
    }
}
