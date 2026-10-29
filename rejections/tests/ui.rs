//! The rejection suite (rejections/README.md). Every case under cases/ is compiled, and its diagnostics
//! must be exactly the ones it annotates (`//~ ERROR: text` on the refused line, `//@check-pass` for an
//! accepted case, `//@error-in-other-file: text` for a refusal whose span is in another file). A file
//! name cannot satisfy an annotation: ui_test matches the text of the message.
//!
//! Two configurations run the cases. The driver one compiles them with trame-driver, which passes
//! `--cfg trame_checked` and runs the checks. The plain one compiles them with rustc alone, for the cases
//! whose header says `runner: plain cargo`. Each configuration builds the dependencies of Cargo.toml once.

use std::path::{Path, PathBuf};
use std::{env, fs};
use ui_test::custom_flags::edition::Edition;
use ui_test::dependencies::DependencyBuilder;
use ui_test::status_emitter::StatusEmitter;
use ui_test::{Config, default_file_filter, default_per_file_config, ignore_output_conflict, run_tests_generic};

/// The driver's directory, from TRAME_DRIVER_BIN (validate.nix sets it; `cargo build` in driver/ makes it).
fn driver_dir() -> PathBuf {
    let dir = env::var_os("TRAME_DRIVER_BIN")
        .expect("TRAME_DRIVER_BIN must name the directory of trame-driver (see rejections/README.md)");
    let dir = PathBuf::from(dir);
    assert!(dir.join("trame-driver").is_file(), "no trame-driver in {}", dir.display());
    dir.join("trame-driver")
}

/// The rustc that the driver wraps. The driver was built for one rustc (`rustc -vV` at build time, driver/build.rs),
/// so RUSTC must be that one.
fn rustc() -> String {
    env::var("RUSTC").unwrap_or_else(|_| "rustc".into())
}

/// Settings shared by both configurations: the crate edition, no stderr files (the annotations are the
/// verdict), no warnings (the verdict is about refusals, not lints), and metadata only, as `cargo check`
/// was before this harness: the verdict is the checks, not a link. `dir` is the build directory.
fn common(dir: &str) -> Config {
    let mut config = Config::rustc("cases");
    config.out_dir = PathBuf::from(dir);
    config.output_conflict_handling = ignore_output_conflict;
    config.comment_defaults.base().set_custom("edition", Edition("2024".into()));
    config.program.args.extend(["--emit=metadata".into(), "-Awarnings".into()]);
    config
}

/// The driver configuration: `trame-driver <rustc> <rustc's arguments>` (driver/src/main.rs). The driver
/// checks a crate that has `--extern trame=`, and it needs `--crate-name` to tell a compile from a query.
fn driver_config(driver: &Path, rustc: &str) -> Config {
    let mut config = common("target/ui-driver");
    config.program.program = driver.into();
    config.program.args.insert(0, rustc.into());
    config.program.args.extend(["--crate-name".into(), "case".into()]);
    let mut deps = DependencyBuilder::default();
    deps.crate_manifest_path = PathBuf::from("Cargo.toml");
    // Cargo builds the dependencies with the driver as its compiler, as `cargo trame` does.
    deps.program.envs.push(("RUSTC_WRAPPER".into(), Some(driver.into())));
    config.comment_defaults.base().set_custom("dependencies", deps);
    config
}

/// The plain configuration: rustc alone, for the cases whose header says `runner: plain cargo`.
fn plain_config() -> Config {
    let mut config = common("target/ui-plain");
    config.program.args.extend(["--crate-name".into(), "case".into()]);
    let mut deps = DependencyBuilder::default();
    deps.crate_manifest_path = PathBuf::from("Cargo.toml");
    deps.program.envs.push(("RUSTC_WRAPPER".into(), None));
    config.comment_defaults.base().set_custom("dependencies", deps);
    config
}

/// A case whose header says `//! runner: plain cargo` runs in the plain configuration.
fn is_plain(path: &Path) -> bool {
    let text = fs::read_to_string(path).unwrap_or_default();
    text.lines().take(5).any(|l| l == "//! runner: plain cargo")
}

fn main() -> ui_test::Result<()> {
    let driver = driver_dir();
    let rustc = rustc();
    let mut configs = vec![driver_config(&driver, &rustc), plain_config()];
    let args = ui_test::Args::test()?;
    let emitter: Box<dyn StatusEmitter> = args.format.into();
    for config in &mut configs {
        config.with_args(&args);
    }
    // Each file runs in one configuration: the driver one, unless its header says plain cargo.
    let filter = |path: &Path, config: &Config| {
        let plain_config = config.program.program != driver;
        default_file_filter(path, config).map(|keep| keep && is_plain(path) == plain_config)
    };
    run_tests_generic(configs, filter, default_per_file_config, (emitter,))
}
