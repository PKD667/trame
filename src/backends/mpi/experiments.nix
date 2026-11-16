# The mpi backend's own experiments (backend.nix, experiments): the transport's checks, and the plain-MPI baselines of
# the lab's benchmarks, which the suite report sets beside the backends. Called with what flake.nix passes. Answers a
# list of { name, kind, backend, span, exp } (lab/perf.nix); `suite = true` puts one in the suite (lab/suite.nix).
{ pkgs, lib, labLib, system, closure, b, machines, data, ... }:
let
  # The runners of this directory and the columns they record (lab/columns.json), in one directory.
  runner = pkgs.runCommand "mpi-bench-runners" { } ''
    mkdir $out
    cp ${./bench/baselines.py} ${./bench/mpirun.py} ${./bench/xhost.py} ${./tests/transport.py} ${../../../lab/columns.json} $out/
  '';
  # The OSU micro-benchmarks (osu_latency, osu_bw) of nixpkgs, built against the same Open MPI.
  osu = "${pkgs.osu-micro-benchmarks}/libexec/osu-micro-benchmarks/mpi/pt2pt";

  transportData = {
    columns = { check = "str"; workers = "int"; case_seed = "int"; repeat = "int"; ran = "bool"; ok = "bool"; seconds = "float"; };
    key = [ "check" "workers" "case_seed" "repeat" ];
    noisy.seconds = 1.0;
  };
  transport = {
    name = "transport-mpi";
    kind = "transport";
    backend = "mpi";
    span = 0;
    exp = labLib.experiment {
      name = "transport-mpi";
      closures.${system} = closure { name = "transport-mpi"; inherit b; dir = runner; script = "transport.py"; };
      resources = { provider = "local"; hosts = 1; };
      data.check = transportData;
      data.mpi_config = data.dataset "mpi_config";
    };
  };

  # The cross-machine transport microbenchmark (bench/xhost/): a crate of its own, outside the root workspace, so that
  # building it changes neither the root crate's source nor the closures built from it. Two ranks on two hosts
  # (span = 2), one job per mode; one experiment per kind of network, so that runs on different clusters are never
  # replicates of one another. Parameters and columns: bench/xhost.py. `mca` is set after what the probe rows show.
  xhostBin = pkgs.rustPlatform.buildRustPackage {
    pname = "mpi-cross-host";
    version = "0.1.0";
    src = lib.fileset.toSource {
      root = ./bench/xhost;
      fileset = lib.fileset.difference ./bench/xhost (lib.fileset.maybeMissing ./bench/xhost/target);
    };
    cargoLock.lockFile = ./bench/xhost/Cargo.lock;
    nativeBuildInputs = b.d.packages ++ [ pkgs.openmpi.dev ];
    env = b.d.env // { MPICC = "${pkgs.openmpi.dev}/bin/mpicc"; };
    doCheck = false;
  };
  xhostData = {
    columns = {
      mode = "str"; phase = "str"; size = "int"; iters = "int";
      median_ns = "float"; p99_ns = "float"; min_ns = "float"; mean_ns = "float";
      cpu0_ns = "float"; cpu1_ns = "float"; sys0_ns = "float"; sys1_ns = "float";
    };
    key = [ "mode" "phase" "size" ];
    noisy = lib.genAttrs [ "median_ns" "p99_ns" "min_ns" "mean_ns" "cpu0_ns" "cpu1_ns" "sys0_ns" "sys1_ns" ] (_: 1.0);
  };
  xhost = { name, provider }: {
    inherit name;
    kind = "xhost";
    backend = "mpi";
    span = 2;
    exp = labLib.experiment {
      inherit name;
      span = 2;
      closures.${system} = closure {
        inherit name b;
        dir = runner;
        script = "xhost.py";
        env = {
          BIN = "${xhostBin}/bin";
          RUSTC_VERSION = pkgs.rustc.version;
          OPENMPI_VERSION = pkgs.openmpi.version;
        };
      };
      params = { iters = 100000; warmup = 10000; sizes = "8,64,1024,65536"; mca = ""; };
      sweep.mode = [ "direct" "direct_single" "relay" "mixed_self" "mixed_flag" "wake_futex" ];
      replicates = 3;
      resources = { inherit provider; hosts = 2; walltime = 120; opts.stage = "fanout"; };
      data.latency = xhostData;
      data.mpi_config = data.dataset "mpi_config";
    };
  };

  # The baselines: the C programs of bench/ (impl plain) and the OSU benchmarks (impl osu), over the same parameters as
  # the trame family of the same bench. Round and graph are one machine only (the C programs span the world).
  families = {
    flow-bw = {
      bench = "flow"; impl = "plain"; spans = [ 1 2 ];
      params = { pattern = "shift"; per = 4; steps = 20000; timeout = 300; };
      sweep = { size = [ 64 1024 16384 65536 ]; cap = [ 1 8 64 ]; };
    };
    flow-agg = {
      bench = "flow"; impl = "plain"; spans = [ 1 2 4 ];
      params = { pattern = "shift"; per = 4; size = 65536; cap = 8; steps = 20000; timeout = 300; };
      sweep = { };
    };
    osu-bw = {
      bench = "flow"; impl = "osu"; spans = [ 1 2 ];
      params = { samples = 2000; timeout = 300; };
      sweep = { size = [ 64 1024 16384 65536 ]; cap = [ 1 8 64 ]; };
    };
    osu-latency = {
      bench = "ping"; impl = "osu"; spans = [ 1 2 ];
      params = { samples = 6636; timeout = 300; };
      sweep.size = [ 16 1024 65536 ];
    };
    round = {
      bench = "round"; impl = "plain"; spans = [ 1 ];
      params = { rounds = 10000; timeout = 300; };
      sweep.per = [ 4 16 ];
    };
    graph = {
      bench = "graph"; impl = "plain"; spans = [ 1 ];
      params = { graph_steps = 2000; size = 64; degree = 8; graph_iters = 100000; seed = 1; timeout = 600; };
      sweep.per = [ 16 64 ];
    };
  };
  baseline = kind: f: s:
    let name = "baseline-${kind}-mpi-s${toString s}"; in {
      inherit name;
      kind = "baseline-${kind}";
      backend = "mpi";
      span = s;
      suite = true;
      exp = labLib.experiment {
        inherit name;
        closures.${system} = closure {
          inherit name b;
          dir = runner;
          script = "baselines.py";
          env = { OSU = osu; };
        };
        params = { inherit (f) bench impl; } // f.params;
        inherit (f) sweep;
        replicates = 3;
        span = s;
        resources = { provider = "cluster"; hosts = s; walltime = 60; opts.stage = "fanout"; };
        data = {
          ${if f.impl == "osu" then "osu" else f.bench} = data.dataset (if f.impl == "osu" then "osu" else f.bench);
          cpu = data.dataset "cpu";
          mpi_config = data.dataset "mpi_config";
        };
      };
    };
in
[ transport ]
++ lib.optionals (machines >= 2) [
  (xhost { name = "mpi-cross-host"; provider = "cluster"; })
  (xhost { name = "mpi-cross-host-opa"; provider = "cluster-opa"; })
]
++ lib.concatLists (lib.mapAttrsToList (kind: f:
  map (baseline kind f) (lib.filter (s: s <= machines && s <= b.d.maxMachines) f.spans)) families)
