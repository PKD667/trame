# The mpi backend's own experiments (backend.nix, experiments): the transport's checks, and the plain-MPI baselines of
# the lab's benchmarks, which the suite report sets beside the backends. Called with what flake.nix passes. Answers a
# list of { name, kind, backend, span, exp } (lab/perf.nix); `suite = true` puts one in the suite (lab/suite.nix).
{ pkgs, lib, labLib, system, closure, b, machines, data, ... }:
let
  # The runners of this directory and the columns they record (lab/columns.json), in one directory.
  runner = pkgs.runCommand "mpi-bench-runners" { } ''
    mkdir $out
    cp ${./bench/baselines.py} ${./bench/mpirun.py} ${./tests/transport.py} ${../../../lab/columns.json} $out/
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
++ lib.concatLists (lib.mapAttrsToList (kind: f:
  map (baseline kind f) (lib.filter (s: s <= machines && s <= b.d.maxMachines) f.spans)) families)
