# The mpi backend as the flake sees it (src/backends/README.md, The descriptor). Its cargo feature is `mpi`.
{ pkgs }:
let
  deps = import ./dependencies.nix { inherit pkgs; };
in
{
  # mpicc is in openmpi's dev output, not in the default one that dependencies.nix names, and its MPICC points at
  # the default output, where it is not. Both are set here.
  packages = deps.packages ++ [ pkgs.openmpi.dev ];
  env = deps.env // { MPICC = "${pkgs.openmpi.dev}/bin/mpicc"; };
  launchRuntime = [ pkgs.openmpi pkgs.python3 pkgs.coreutils pkgs.time pkgs.procps pkgs.util-linux pkgs.gnugrep ];
  # One host per machine, or two cut from one (TRAME_MPI_NODE_RANKS, HOSTS.md).
  maxMachines = 1024;
  splits = [ 1 2 ];
  deterministic = false;
  cargoTests = false;
  # The transport's own checks, below the interface (tests/).
  bins = [ "check" ];
  # The plain-MPI baselines of the lab's benchmarks (bench/).
  postInstall = ''
    mpicc -O2 -o $out/bin/mpi_barrier src/backends/mpi/bench/barrier.c
    mpicc -O2 -o $out/bin/mpi_flow src/backends/mpi/bench/flow.c
    mpicc -O2 -o $out/bin/mpi_graph src/backends/mpi/bench/graph.c
  '';
  # The submodule of the real MPI bindings, not part of any build here.
  exclude = [ "mpi-rma" ];
  # The wait protocols, model-checked (validate.nix).
  validate = args: import ./validate.nix args;
  # This backend's own experiments: the transport checks and the baselines (experiments.nix).
  experiments = args: import ./experiments.nix ({ inherit pkgs; } // args);
}
