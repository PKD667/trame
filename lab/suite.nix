# The suite (`nix run .#suite`) and its report (`nix build .#suite-report`). Called from flake.nix.
#
# The suite is every experiment of the catalogue marked `suite`: for every discovered backend, programs on 1, 2 and 4
# machines, latency, flow bandwidth by size and capacity, aggregate bandwidth by host count, step overhead, idle CPU,
# round, fan-in and graph; and the plain baselines a backend declares (src/backends/<b>/experiments.nix). A combination
# a backend does not take (more machines than its maxMachines, a split it does not list) is not in the catalogue; one the
# launch still refuses (exit 2) records nothing. The suite runs the experiments one after another, each leasing its own
# machines (a job never shares the network with another), and goes on after one fails.
#
# The report is a nixsci analysis over the locked runs, in two parts that never mix:
#   VALIDATION  claims. Q1 and Q2 per backend, placement and size, as the proportion of round trips over the bound,
#               one sample per run (lab/q.nix); and every program ok. Verdicts: supported, contradicted, inconclusive.
#               A bound that is null is no claim: validation.tsv says "no bound".
#   COMPARISON  tables (TSV) and figure sources per metric, backends side by side. No pass or fail on performance.
{ pkgs, lib, labLib, python, catalogue, machines, qm }:
let
  members = lib.filter (i: i.suite or false) catalogue;
  exps = builtins.listToAttrs (map (i: { inherit (i) name; value = i.exp; }) members);

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

  # Jobs of an experiment: every point of its sweep, every seed, each as many times as it has replicates.
  jobs = e: lib.foldl' (a: v: a * builtins.length v) 1 (builtins.attrValues e.spec.sweep)
    * (if e.spec.seeds == [ ] then 1 else builtins.length e.spec.seeds) * e.spec.replicates;
  matrix = lib.mapAttrs (_: is: map (i: { inherit (i) name kind span; jobs = jobs i.exp; }) is)
    (lib.groupBy (i: i.backend) members);

  # VALIDATION. The reference deployment's bounds (lab/q.nix) become claims; a null bound is listed, not claimed.
  latency = lib.filter (i: i.kind == "latency") members;
  placementOf = i: if i.span == 1 then "same-host" else "cross-host";
  cells = lib.concatMap (i: lib.concatMap (size: map (key: {
    inherit i size key;
    placement = placementOf i;
    bound = qm.q.${qm.reference}.${placementOf i}.${toString size}.${key};
    id = "latency/${i.backend}/${placementOf i}/${toString size}/${key}";
  }) [ "p50" "p99" ]) qm.sizes) latency;
  latencyClaims = builtins.listToAttrs (map (c: {
    name = c.id;
    # Q1: at most half of the round trips exceed the median bound. Q2: at most one in a hundred exceeds the p99 bound.
    # `over_<key>` is a bool column; a proportion per run, the runs the samples (unit run is the default).
    value = {
      kind = "below";
      of = { use = c.i.name; dataset = "ping"; column = "over_${c.key}"; stat = "proportion"; where = { size = c.size; warm = false; }; };
      than = if c.key == "p50" then 0.5 else 0.01;
    };
  }) (lib.filter (c: c.bound != null) cells));
  programs = lib.filter (i: i.kind == "programs") members;
  programClaims = builtins.listToAttrs (map (i: {
    name = "programs/${i.backend}/s${toString i.span}";
    value = { kind = "exact"; of = { use = i.name; dataset = "run"; column = "ok"; }; value = true; };
  }) programs);
  claims = latencyClaims // programClaims;
  # What validation.py needs beyond the claim report: the cells with no bound, and the claims' meaning.
  cellsFile = pkgs.writeText "suite-cells.json" (builtins.toJSON {
    deployment = qm.reference;
    cells = map (c: { inherit (c) id placement size key bound; backend = c.i.backend; claimed = c.bound != null; }) cells;
    programs = map (i: { id = "programs/${i.backend}/s${toString i.span}"; backend = i.backend; machines = i.span; }) programs;
  });

  analysis = draft: labLib.analysis {
    name = "suite-report";
    inherit draft claims;
    use = exps;
    pipelines.compare.script = ./analysis/compare.py;
    confidence = 0.99;
  };
  report = draft:
    let a = analysis draft; in
    pkgs.runCommand "suite-report" { nativeBuildInputs = [ python ]; } ''
      mkdir $out
      cp -rL ${a}/. $out/
      chmod -R u+w $out
      python ${./analysis/validation.py} ${cellsFile} $out/claims-report.json $out/validation.tsv
    '';
in
{
  inherit app matrix exps members;
  # Strict: evaluation fails, naming the jobs without runs, until the suite has run and lab.lock has been committed.
  report = report false;
  # The same over whatever has run.
  reportDraft = report true;
}
