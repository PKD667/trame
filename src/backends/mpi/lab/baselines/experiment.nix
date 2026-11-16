# The plain-MPI baselines of the lab's benchmarks: the C programs of this directory (impl plain) and the OSU micro-benchmarks
# (impl osu), over the same parameters as the trame experiment of the same bench. They are runs of the experiments named in
# `experiment` (their analyses set them beside the backends) and have no analysis of their own. Round and graph are one
# machine only (the C programs span the world).
{ pkgs, lib, labLib, system, closure, b, machines, data, ... }:
let
  # The OSU micro-benchmarks (osu_latency, osu_bw) of nixpkgs, built against the same Open MPI.
  osu = "${pkgs.osu-micro-benchmarks}/libexec/osu-micro-benchmarks/mpi/pt2pt";
  families = {
    flow-bw = {
      experiment = "flow"; bench = "flow"; impl = "plain"; spans = [ 1 2 ];
      params = { pattern = "shift"; per = 4; steps = 20000; timeout = 300; };
      sweep = { size = [ 64 1024 16384 65536 ]; cap = [ 1 8 64 ]; };
    };
    flow-agg = {
      experiment = "flow"; bench = "flow"; impl = "plain"; spans = [ 1 2 4 ];
      params = { pattern = "shift"; per = 4; size = 65536; cap = 8; steps = 20000; timeout = 300; };
      sweep = { };
    };
    osu-bw = {
      experiment = "flow"; bench = "flow"; impl = "osu"; spans = [ 1 2 ];
      params = { samples = 2000; timeout = 300; };
      sweep = { size = [ 64 1024 16384 65536 ]; cap = [ 1 8 64 ]; };
    };
    osu-latency = {
      experiment = "latency"; bench = "ping"; impl = "osu"; spans = [ 1 2 ];
      params = { samples = 6636; timeout = 300; };
      sweep.size = [ 16 1024 65536 ];
    };
    round = {
      experiment = "round"; bench = "round"; impl = "plain"; spans = [ 1 ];
      params = { rounds = 10000; timeout = 300; };
      sweep.per = [ 4 16 ];
    };
    graph = {
      experiment = "graph"; bench = "graph"; impl = "plain"; spans = [ 1 ];
      params = { graph_steps = 2000; size = 64; degree = 8; graph_iters = 100000; seed = 1; timeout = 600; };
      sweep.per = [ 16 64 ];
    };
  };
  run = kind: f: s:
    let name = "baseline-${kind}-mpi-s${toString s}"; in {
      inherit name;
      kind = "baseline-${kind}";
      backend = "mpi";
      span = s;
      suite = true;
      inherit (f) experiment;
      exp = labLib.experiment {
        inherit name;
        closures.${system} = closure {
          inherit name b;
          script = ./run.py;
          path = [ (lib.fileset.toSource { root = ../lib; fileset = ../lib; }) ];
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
{
  name = "baselines";
  figures = [ ];
  runs = lib.concatLists (lib.mapAttrsToList (kind: f:
    map (run kind f) (lib.filter (s: s <= machines && s <= b.d.maxMachines) f.spans)) families);
}
