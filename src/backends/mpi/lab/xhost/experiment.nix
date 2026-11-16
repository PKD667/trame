# The cross-machine transport microbenchmark (main.rs): a crate of its own, outside the root workspace, so that building it
# changes neither the root crate's source nor the closures built from it. Two ranks on two hosts (span 2), one job per mode;
# one run per kind of network, so that runs on different clusters are never replicates of one another. `mca` is set after
# what the probe rows show.
{ pkgs, lib, labLib, system, closure, b, machines, data, ... }:
let
  bin = pkgs.rustPlatform.buildRustPackage {
    pname = "mpi-cross-host";
    version = "0.1.0";
    src = lib.fileset.toSource { root = ./.; fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./main.rs ]; };
    cargoLock.lockFile = ./Cargo.lock;
    nativeBuildInputs = b.d.packages ++ [ pkgs.openmpi.dev ];
    env = b.d.env // { MPICC = "${pkgs.openmpi.dev}/bin/mpicc"; };
    doCheck = false;
  };
  latencyData = {
    columns = {
      mode = "str"; phase = "str"; size = "int"; iters = "int";
      median_ns = "float"; p99_ns = "float"; min_ns = "float"; mean_ns = "float";
      cpu0_ns = "float"; cpu1_ns = "float"; sys0_ns = "float"; sys1_ns = "float";
    };
    key = [ "mode" "phase" "size" ];
    noisy = lib.genAttrs [ "median_ns" "p99_ns" "min_ns" "mean_ns" "cpu0_ns" "cpu1_ns" "sys0_ns" "sys1_ns" ] (_: 1.0);
  };
  run = { name, provider }: {
    inherit name;
    kind = "xhost";
    backend = "mpi";
    span = 2;
    exp = labLib.experiment {
      inherit name;
      span = 2;
      closures.${system} = closure {
        inherit name b;
        script = ./run.py;
        path = [ (lib.fileset.toSource { root = ../lib; fileset = ../lib; }) ];
        env = {
          BIN = "${bin}/bin";
          RUSTC_VERSION = pkgs.rustc.version;
          OPENMPI_VERSION = pkgs.openmpi.version;
        };
      };
      params = { iters = 100000; warmup = 10000; sizes = "8,64,1024,65536"; mca = ""; };
      sweep.mode = [ "direct" "direct_single" "relay" "mixed_self" "mixed_flag" "wake_futex" ];
      replicates = 3;
      resources = { inherit provider; hosts = 2; walltime = 120; opts.stage = "fanout"; };
      data.latency = latencyData;
      data.mpi_config = data.dataset "mpi_config";
    };
  };
in
{
  name = "xhost";
  figures = [ "xhost-latency" ];
  runs = lib.optionals (machines >= 2) [
    (run { name = "mpi-cross-host"; provider = "cluster"; })
    (run { name = "mpi-cross-host-opa"; provider = "cluster-opa"; })
  ];
  analysis = ./analysis.py;
}
