# The development shells, and the same environments as nixsci deployments (for a cluster node, where tests run).
{ pkgs, nixsci, tree, tc }:
let
  inherit (tree) lib descriptors;
  base = [ pkgs.rustc pkgs.cargo pkgs.clippy pkgs.rustfmt ];
  backendShell = name: d: pkgs.mkShell {
    packages = base ++ d.packages ++ d.launchRuntime;
    shellHook = builtins.concatStringsSep "\n" (
      [ "export TRAME_BACKEND=${name}" ] ++ lib.mapAttrsToList (k: val: "export ${k}=${val}") d.env
    );
  };
in
{
  devShells = {
    # No backend: the toolchain only. A cargo build names its backend (--no-default-features --features <name>).
    default = pkgs.mkShell { packages = base; };
    # The checks of the numeric library (src/math, check/math/README.md): cargo-mutants for the mutation run, and
    # python with mpmath, the arbitrary-precision oracle that writes the test vectors. Both pinned by flake.lock.
    math = pkgs.mkShell { packages = base ++ [ pkgs.cargo-mutants (pkgs.python3.withPackages (p: [ p.mpmath ])) ]; };
    # The pinned nightly, for building crates/driver/ with `cargo` (see crates/driver/README.md). The shell does not
    # depend on the package, so entering it does not build the driver.
    driver = tc.driverPkgs.mkShell { packages = [ tc.driverToolchain ]; };
    # Miri on the driver's nightly: `sh check/proofs/miri.sh` (cargo miri under Tree Borrows). Miri builds std from
    # rust-src into a cache directory, so the shell needs a writable HOME or XDG_CACHE_HOME.
    miri = tc.driverPkgs.mkShell { packages = [ tc.miriToolchain pkgs.gcc ]; };
  } // lib.mapAttrs backendShell descriptors;
  # `nixsci deploy shell ... dev-<backend> -- CMD`. Same packages and env as devShells.<backend>.
  deployments = lib.mapAttrs' (n: d: lib.nameValuePair "dev-${n}" (nixsci.lib.mkDeployment pkgs {
    name = "trame-dev-${n}";
    program = "${pkgs.bashInteractive}/bin/bash";
    runtime = base ++ [ pkgs.gcc pkgs.git pkgs.gnumake pkgs.time pkgs.bashInteractive pkgs.gnugrep pkgs.gnused pkgs.gawk pkgs.findutils pkgs.diffutils ] ++ d.packages;
    env = d.env;
  })) descriptors;
}
