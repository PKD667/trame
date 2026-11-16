# The suite (`nix run .#suite`) and its matrix. Called from nix/lab.nix.
#
# The suite is every run of the catalogue marked `suite`: for every discovered backend, programs on 1, 2 and 4 machines, latency,
# flow, step, idle, round, fan-in and graph, and the baselines a backend declares (src/backends/<b>/lab/baselines). A combination a
# backend does not take (more machines than its maxMachines, a split it does not list) is not in the catalogue; one the launch
# still refuses (exit 2) records nothing. The suite runs one after another, each leasing its own machines (a job never shares the
# network with another), and goes on after one fails. What it measured is read by the analyses (nix/lab.nix), not here.
{ pkgs, lib, catalogue }:
let
  members = lib.filter (i: i.suite or false) catalogue;

  app = {
    type = "app";
    program = toString (pkgs.writeShellScript "suite" ''
      failed=0
      ${lib.concatMapStrings (i: ''
        echo "== ${i.name}" >&2
        ${i.exp.run.program} "$@" || { echo "suite: ${i.name} failed" >&2; failed=$((failed + 1)); }
      '') members}
      echo "suite: $failed experiments failed" >&2
      [ "$failed" -eq 0 ]
    '');
  };

  # Jobs of a run: every point of its sweep, every seed, each as many times as it has replicates.
  jobs = e: lib.foldl' (a: v: a * builtins.length v) 1 (builtins.attrValues e.spec.sweep)
    * (if e.spec.seeds == [ ] then 1 else builtins.length e.spec.seeds) * e.spec.replicates;
  matrix = lib.mapAttrs (_: is: map (i: { inherit (i) name kind span; jobs = jobs i.exp; }) is)
    (lib.groupBy (i: i.backend) members);
in
{ inherit app matrix; }
