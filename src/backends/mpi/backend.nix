# The mpi backend as the flake sees it (src/backends/README.md, The descriptor). Its cargo feature is `mpi`.
{ pkgs }:
let
  deps = import ./dependencies.nix { inherit pkgs; };
in
{
  # mpicc is in openmpi's dev output, not in the default one that dependencies.nix names, and its MPICC points at
  # the default output, where it is not. Both are set here.
  # libffi-sys (under the mpi crate) configures and builds its bundled libffi with sh and make: a build outside
  # stdenv (a validate check) needs them on PATH.
  packages = deps.packages ++ [ pkgs.openmpi.dev pkgs.bash pkgs.gnumake pkgs.diffutils pkgs.findutils pkgs.which pkgs.file ];
  env = deps.env // { MPICC = "${pkgs.openmpi.dev}/bin/mpicc"; };
  launchRuntime = [ pkgs.openmpi pkgs.python3 pkgs.coreutils pkgs.time pkgs.procps pkgs.util-linux pkgs.gnugrep ];
  # One host per machine, or two cut from one (TRAME_MPI_NODE_RANKS, transport/README.md).
  maxMachines = 1024;
  splits = [ 1 2 ];
  deterministic = false;
  cargoTests = false;
  # The transport's own checks, below the interface (tests/).
  bins = [ "check" ];
  # The plain-MPI baselines of the lab's benchmarks (lab/baselines/).
  postInstall = ''
    mpicc -O2 -o $out/bin/mpi_barrier src/backends/mpi/lab/baselines/barrier.c
    mpicc -O2 -o $out/bin/mpi_flow src/backends/mpi/lab/baselines/flow.c
    mpicc -O2 -o $out/bin/mpi_graph src/backends/mpi/lab/baselines/graph.c
  '';
  # mpi-rma/: a crate of its own (a submodule: typed one-sided communication over rsmpi), not part of any build here.
  exclude = [ "mpi-rma" ];
  # What Cargo.toml needs of this backend (nix/manifest.nix): the dependencies only it uses, optional and enabled by its
  # feature, and the binaries built from its own sources.
  cargo = {
    dependencies = {
      mpi = { version = "=0.8.2"; optional = true; };
      libc = { version = "=0.2.191"; optional = true; };
    };
    bins = [ { name = "check"; path = "src/backends/mpi/tests/check.rs"; } ];
  };
  # The wait protocols, model-checked (validate.nix).
  validate = args: import ./validate.nix args;
}
