# The seq backend as the flake sees it (src/backends/README.md, The descriptor). Its cargo feature is `seq`.
{ pkgs }:
{
  # `seq` is also an ordinary identifier: the check no-backend-names looks for it only as a path, a feature or `seq backend`.
  ordinaryWord = true;
  # The one the editor and benchmarks that need some backend take (nix/manifest.nix, lab/simd): the reference of this README.
  reference = true;
  packages = [ ];
  env = { };
  launchRuntime = [ pkgs.bash pkgs.coreutils pkgs.time pkgs.gnugrep ];
  # One host, on one machine.
  maxMachines = 1;
  splits = [ 1 ];
  # A run is a function of the program: its programs and mutants are validate verdicts, not lab runs.
  deterministic = true;
  # The root crate's cargo tests run on it.
  cargoTests = true;
  bins = [ ];
  postInstall = "";
  # Directories of this backend's tree that are not built (git submodules).
  exclude = [ ];
  # What Cargo.toml needs of this backend (nix/manifest.nix): nothing beyond its feature.
  cargo = { dependencies = { }; bins = [ ]; };
  # Its own validate verdicts, beyond the generic ones (validate.nix): the step-limited mutants. Called with what lab/validate.nix passes.
  validate = args: import ./validate.nix args;
}
