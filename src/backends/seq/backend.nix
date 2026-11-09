# The seq backend as the flake sees it (src/backends/README.md, The descriptor). Its cargo feature is `seq`.
{ pkgs }:
{
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
  # Its own validate verdicts, beyond the generic ones (validate.nix): the step-limited mutants. Called with what lab/validate.nix passes.
  validate = args: import ./validate.nix args;
  # Its own lab experiments, beyond the generic ones (lab/checks.nix, lab/perf.nix): none.
  experiments = _: [ ];
}
