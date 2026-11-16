# The pinned nightly of crates/driver/rust-toolchain.toml, as a toolchain, the driver built with it, and the Miri variant.
{ nixpkgs, rust-overlay, system }:
let
  driverPkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
  driverToolchain = driverPkgs.rust-bin.fromRustupToolchainFile ../crates/driver/rust-toolchain.toml;
  driverPlatform = driverPkgs.makeRustPlatform {
    cargo = driverToolchain;
    rustc = driverToolchain;
  };
in
{
  inherit driverPkgs driverToolchain;
  # Miri: the same nightly, with the miri and rust-src components (check/proofs/miri.sh).
  miriToolchain = driverPkgs.rust-bin.fromRustupToolchain {
    channel = (builtins.fromTOML (builtins.readFile ../crates/driver/rust-toolchain.toml)).toolchain.channel;
    profile = "minimal";
    components = [ "miri" "rust-src" ];
  };
  trameDriver = driverPlatform.buildRustPackage {
    pname = "trame-driver";
    version = "0.1.0";
    src = ../crates/driver;
    cargoLock.lockFile = ../crates/driver/Cargo.lock;
    # Tests compile fixture crates and need the tree; run them with `cargo test` (crates/driver/README.md).
    doCheck = false;
  };
}
