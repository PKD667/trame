//! The documents agree with the tree: every public name at the crate root is listed in INTERFACE.md's
//! surface table, and no file cites the removed specification by its old name.

use std::path::Path;

/// The names `lib.rs` exports at the crate root, from its `pub use` lines. Hidden names (`__`) and
/// `bytemuck` are internal; `math` is checked by its functions' rows.
fn exported() -> Vec<String> {
    let root = env!("CARGO_MANIFEST_DIR");
    let lib = std::fs::read_to_string(format!("{root}/src/lib.rs")).expect("lib.rs is readable");
    let mut names = Vec::new();
    // Root-level lines only: an indented `pub use` is inside a hidden module.
    for line in lib.lines() {
        let Some(rest) = line.strip_prefix("pub use ") else { continue };
        let rest = rest.trim_end_matches(';');
        if rest.contains(" as __") {
            continue;
        }
        let tail = rest.rsplit("::").next().unwrap_or(rest);
        let inner = tail.trim_start_matches('{').trim_end_matches('}');
        names.extend(inner.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()));
    }
    names.sort();
    names.dedup();
    names
}

#[test]
fn interface_lists_every_export() {
    let root = env!("CARGO_MANIFEST_DIR");
    let doc = std::fs::read_to_string(format!("{root}/INTERFACE.md")).expect("INTERFACE.md is readable");
    let table: Vec<&str> = doc.lines().filter(|l| l.starts_with("| `")).collect();
    let names = exported();
    assert!(names.len() > 20, "lib.rs exports were not found: {names:?}");
    let missing: Vec<&String> = names
        .iter()
        .filter(|n| {
            let first_cell = |l: &&str| l.split('|').nth(1).unwrap_or("").to_string();
            !table.iter().any(|l| {
                let cell = first_cell(l);
                cell.contains(&format!("`{n}`"))
                    || cell.contains(&format!("`{n}("))
                    || cell.contains(&format!("`{n}!("))
                    || cell.contains(&format!("`{n}::"))
                    || cell.contains(&format!("`{n}<"))
                    || cell.contains(&format!("::{n}]"))
                    || cell.contains(&format!("::{n}("))
                    || cell.contains(&format!("trame::{n}"))
            })
        })
        .collect();
    assert!(missing.is_empty(), "INTERFACE.md's surface table lacks {missing:?}");
}

fn scan(dir: &Path, needle: &str, hits: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name == "target" || name.starts_with('.') || name == "result" {
            continue;
        }
        if p.is_dir() {
            scan(&p, needle, hits);
        } else if let Ok(text) = std::fs::read_to_string(&p) {
            for (i, line) in text.lines().enumerate() {
                if line.contains(needle) {
                    hits.push(format!("{}:{}", p.display(), i + 1));
                }
            }
        }
    }
}

#[test]
fn no_file_cites_the_old_spec() {
    let root = env!("CARGO_MANIFEST_DIR");
    let needle = ["spec", ".md"].concat();
    let mut hits = Vec::new();
    scan(Path::new(root), &needle, &mut hits);
    assert!(hits.is_empty(), "these lines cite {needle}, which is now INTERFACE.md: {hits:?}");
}
