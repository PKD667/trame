# The runs of a benchmark experiment (lab/<experiment>/experiment.nix, README.md "Contract"): one run per backend and per span
# the backend takes. `runs` answers a list of { name, kind, backend, span, suite, exp }, named <kind>-<backend>-s<span>.
#
# A job's trame hosts are machines x split. A backend appears at a span only if it takes that many machines (maxMachines)
# and at a split only if its descriptor lists it (splits); a job the launch still refuses (exit 2) records nothing.
# Nothing here names a backend.
{ lib, labLib, system, closure, backends, machines, data }:
{
  # kind        the family: the run's name starts with it
  # script      the experiment's run.py
  # bench       the benchmark's name (the dataset it records, the program bench_<bench>)
  # spans       the machines of a job; replicates; params; sweep; splits (sweep the descriptor's splits too)
  # env         the runner's environment beside the closure's
  runs = { kind, script, bench, spans, replicates, params, sweep, splits ? false, env ? { } }:
    let
      one = n: b: s:
        let name = "${kind}-${n}-s${toString s}"; in {
          inherit kind;
          span = s;
          suite = true;
          backend = n;
          inherit name;
          exp = labLib.experiment {
            inherit name replicates;
            closures.${system} = closure { inherit name script b env; };
            params = { backend = n; inherit bench; } // params;
            sweep = sweep // lib.optionalAttrs splits { split = b.d.splits; };
            span = s;
            # The lease is the job's machines and no more, so the jobs of one experiment never share the network.
            resources = { provider = "cluster"; hosts = s; walltime = 60; opts.stage = "fanout"; };
            data = data.dataOf bench;
          };
        };
      spansOf = b: lib.filter (s: s <= machines && s <= b.d.maxMachines) spans;
    in
    lib.concatLists (lib.mapAttrsToList (n: b: map (one n b) (spansOf b)) backends);
}
