# Q1 and Q2 (CONTRACT.md, Latency): round trips of one frame size, with and without a random gap before each send. Same host
# is one trame host (span 1); cross-host is a job of two machines (span 2). The bounds are q.nix's.
{ lib, bench, ... }:
let
  q = import ./q.nix { inherit lib; };
  runs = bench.runs {
    kind = "latency"; script = ./run.py; bench = "ping"; spans = [ 1 2 ]; replicates = 5;
    params = { samples = 6636; cap = 1; deployment = q.reference; timeout = 300; };
    sweep = { size = q.sizes; gap_ns = [ 0 500000 ]; };
    env.BOUNDS = "${builtins.toFile "q.json" (builtins.toJSON q.q)}";
  };
  placementOf = r: if r.span == 1 then "same-host" else "cross-host";
  cells = lib.concatMap (r: lib.concatMap (size: map (key: {
    inherit r size key;
    bound = q.q.${q.reference}.${placementOf r}.${toString size}.${key};
    id = "latency/${r.backend}/${placementOf r}/${toString size}/${key}";
  }) [ "p50" "p99" ]) q.sizes) runs;
in
{
  name = "latency";
  figures = [ "latency" ];
  inherit runs;
  # A bound that is null is no claim: the table lists it as empty.
  claimIds = [ "latency/<backend>/<placement>/<size>/p50" "latency/<backend>/<placement>/<size>/p99" ];
  claims = builtins.listToAttrs (map (c: {
    name = c.id;
    # Q1: at most half of the round trips exceed the median bound. Q2: at most one in a hundred exceeds the p99 bound.
    # `over_<key>` is a bool column: a proportion per run, the runs the samples.
    value = {
      kind = "below";
      of = { use = c.r.name; dataset = "ping"; column = "over_${c.key}"; stat = "proportion"; where = { size = c.size; warm = false; }; };
      than = if c.key == "p50" then 0.5 else 0.01;
    };
  }) (lib.filter (c: c.bound != null) cells));
  analysis = ./analysis.py;
}
