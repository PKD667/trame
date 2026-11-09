# The performance experiments of the lab (lab/README.md), for every backend, built from its descriptor. Called from
# flake.nix. Each is lab/perf.py over the benchmarks of lab/bench/, through the backend's launch, so one definition
# serves every backend. Answers a list of { name, kind, backend, span, exp } (kind: the family), the experiments of the
# suite (lab/suite.nix); `span` is the machines of a job, the experiment's name ends -s<span>.
#
# A job's trame hosts are machines x split. A backend appears at a span only if it takes that many machines
# (maxMachines) and at a split only if the descriptor lists it (splits); a job the launch still refuses (exit 2) records
# nothing (lab/perf.py). Nothing here names a backend.
{ pkgs, lib, labLib, system, closure, backends, machines, q }:
let
  inherit (import ./data.nix { inherit lib; }) dataOf;

  # The families. Each: the bench, the spans (machines per job) it runs on, the sweep over its parameters, the fixed
  # parameters, and the replicates. Sizes are a subset of lab/bench/common.rs SIZES.
  families = {
    # Q1 and Q2 (CONTRACT.md, Latency): round trips of one frame size, with and without a random gap before each send.
    # Same host is one trame host; cross-host is a job of two machines. The bounds are lab/q.nix's.
    latency = {
      bench = "ping"; spans = [ 1 2 ]; replicates = 5;
      params = { samples = 6636; cap = 1; deployment = q.reference; timeout = 300; };
      sweep = { size = q.sizes; gap_ns = [ 0 500000 ]; };
    };
    # Bandwidth of one stream per worker: frame size by route capacity, one host and across two machines.
    flow-bw = {
      bench = "flow"; spans = [ 1 2 ]; replicates = 3;
      params = { pattern = "shift"; per = 4; steps = 20000; timeout = 300; };
      sweep = { size = [ 64 1024 16384 65536 ]; cap = [ 1 8 64 ]; };
    };
    # Aggregate bandwidth against the number of hosts: machines (the span) times the descriptor's splits.
    flow-agg = {
      bench = "flow"; spans = [ 1 2 4 ]; replicates = 3; splits = true;
      params = { pattern = "shift"; per = 4; size = 65536; cap = 8; steps = 20000; timeout = 300; };
      sweep = { };
    };
    # What a step that does nothing costs, with one step and with four.
    step = {
      bench = "step"; spans = [ 1 ]; replicates = 3;
      params = { n = 1000000; timeout = 300; };
      sweep = { per = [ 1 4 ]; k = [ 1 4 ]; };
    };
    # CPU time of a worker that waits: GNU time per rank (cpu dataset).
    idle = {
      bench = "idle"; spans = [ 1 ]; replicates = 3;
      params = { seconds = 3; timeout = 300; };
      sweep.per = [ 1 4 16 ];
    };
    round = {
      bench = "round"; spans = [ 1 2 4 ]; replicates = 3;
      params = { rounds = 10000; timeout = 300; };
      sweep.per = [ 4 16 ];
    };
    fanin = {
      bench = "fanin"; spans = [ 1 2 ]; replicates = 3;
      params = { msgs = 10000; timeout = 300; };
      sweep.per = [ 4 16 ];
    };
    graph = {
      bench = "graph"; spans = [ 1 2 4 ]; replicates = 3;
      params = { graph_steps = 2000; size = 64; degree = 8; graph_iters = 100000; seed = 1; timeout = 600; };
      sweep.per = [ 16 64 ];
    };
  };

  # The spans of a family a backend takes, with the machines the suite has.
  spansOf = b: f: lib.filter (s: s <= machines && s <= b.d.maxMachines) f.spans;

  one = kind: f: n: b: s: {
    inherit kind;
    span = s;
    suite = true;
    backend = n;
    name = "${kind}-${n}-s${toString s}";
    exp = labLib.experiment {
      name = "${kind}-${n}-s${toString s}";
      closures.${system} = closure { name = "${kind}-${n}-s${toString s}"; script = "perf.py"; inherit b; env = { BOUNDS = "${builtins.toFile "q.json" (builtins.toJSON q.q)}"; }; };
      params = { backend = n; bench = f.bench; } // f.params;
      sweep = f.sweep // lib.optionalAttrs (f.splits or false) { split = b.d.splits; };
      inherit (f) replicates;
      span = s;
      # The lease is the job's machines and no more, so the jobs of one experiment never share the network.
      resources = { provider = "cluster"; hosts = s; walltime = 60; opts.stage = "fanout"; };
      data = dataOf f.bench;
    };
  };
in
lib.concatLists (lib.mapAttrsToList (kind: f:
  lib.concatLists (lib.mapAttrsToList (n: b: map (one kind f n b) (spansOf b f)) backends)) families)
