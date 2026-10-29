//! README.md quotes `examples/ring.rs` whole. This keeps the quote equal to the file.

#[test]
fn readme_quotes_examples_ring_rs() {
    let root = env!("CARGO_MANIFEST_DIR");
    let readme = std::fs::read_to_string(format!("{root}/README.md")).expect("README.md is readable");
    let ring = std::fs::read_to_string(format!("{root}/examples/ring.rs")).expect("examples/ring.rs is readable");
    assert!(readme.contains(&format!("```rust\n{ring}```")), "README.md's example differs from examples/ring.rs");
}
