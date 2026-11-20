//! Generates the backend selection of src/backends/mod.rs. The backends are the directories of src/backends/ that hold
//! a mod.rs; a build selects one by the cargo feature of the same name. No backend is named here or in mod.rs: this
//! reads the directory, and writes $OUT_DIR/backends.rs, which mod.rs includes. Only the selected backend is declared,
//! so the others are not compiled. Cargo itself still wants each feature declared in Cargo.toml (a manifest is not
//! generated); a directory without its feature is an error here, not a backend that can never be chosen.

use std::{env, fs, path::Path, process};

fn main() {
    let root = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let dir = Path::new(&root).join("src/backends");
    println!("cargo:rerun-if-changed=src/backends");
    println!("cargo:rerun-if-changed=Cargo.toml");

    let mut names: Vec<String> = fs::read_dir(&dir)
        .expect("read src/backends")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("mod.rs").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();

    let manifest = fs::read_to_string(Path::new(&root).join("Cargo.toml")).expect("read Cargo.toml");
    for n in &names {
        let ident = !n.is_empty()
            && !n.starts_with(|c: char| c.is_ascii_digit())
            && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ident {
            fail(&format!("src/backends/{n}/ is not a module name"));
        }
        if !manifest.lines().any(|l| l.split('=').next().is_some_and(|k| k.trim() == n) && l.contains('=')) {
            fail(&format!("src/backends/{n}/ has a mod.rs but Cargo.toml declares no feature `{n}`"));
        }
    }

    let chosen: Vec<&String> =
        names.iter().filter(|n| env::var_os(format!("CARGO_FEATURE_{}", n.to_ascii_uppercase())).is_some()).collect();
    let list = names.join(", ");

    let code = match chosen.as_slice() {
        [] => format!(
            "compile_error!(\"select a backend: --no-default-features --features <name>, where <name> is one of: {list}\");\n"
        ),
        [n] => {
            let path = dir.join(n).join("mod.rs");
            // Absolute: a `mod` in an included file is looked up beside the included file, which is in OUT_DIR.
            // `self::`: the crate may also depend on an external crate with the backend's name, and a bare name is ambiguous.
            format!(
                "#[path = {path:?}]\npub(crate) mod {n};\npub(crate) use self::{n} as selected;\n/// The name of the selected backend: its directory.\npub const NAME: &str = {n:?};\n"
            )
        }
        many => {
            let got: Vec<&str> = many.iter().map(|s| s.as_str()).collect();
            format!("compile_error!(\"features {} are exclusive: one backend per build\");\n", got.join(", "))
        }
    };

    let out = Path::new(&env::var("OUT_DIR").expect("OUT_DIR")).join("backends.rs");
    if fs::read_to_string(&out).ok().as_deref() != Some(&code) {
        fs::write(&out, code).expect("write backends.rs");
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    process::exit(1)
}
