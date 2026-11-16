# Every program of check/programs/ against its .expected output, on every backend:
#   programs-<b>       one machine, locally, over the descriptor's splits
#   programs-<b>-s<S>  the same on S machines (S in 1 2 4, within the descriptor's maxMachines), on the cluster
# A deterministic backend's programs are also a validate verdict (nix/validate.nix): the same rule without the lab.
{ lib, labLib, system, closure, backends, machines, data, ... }:
let
  runData = {
    columns = { program = "str"; repeat = "int"; ran = "bool"; ok = "bool"; verdict = "str"; seconds = "float"; stdout_sha256 = "str"; };
    key = [ "program" "repeat" ];
    noisy.seconds = 1.0;
  };
  run = n: b: span:
    let name = if span == 0 then "programs-${n}" else "programs-${n}-s${toString span}"; in {
      inherit name span;
      kind = "programs";
      backend = n;
      suite = span != 0;
      exp = labLib.experiment ({
        inherit name;
        closures.${system} = closure { inherit name b; script = ./run.py; };
        params = { backend = n; repeats = 3; };
        sweep.split = b.d.splits;
        data = { run = runData; launch_config = data.dataset "launch_config"; };
      } // (if span == 0
        then { resources = { provider = "local"; hosts = 1; }; }
        else { inherit span; resources = { provider = "cluster"; hosts = span; walltime = 60; opts.stage = "fanout"; }; }));
    };
  spans = b: lib.filter (s: s <= machines && s <= b.d.maxMachines) [ 1 2 4 ];
  runs = lib.concatLists (lib.mapAttrsToList (n: b: [ (run n b 0) ] ++ map (run n b) (spans b)) backends);
in
{
  name = "programs";
  figures = [ "programs-verdicts" ];
  inherit runs;
  claimIds = [ "programs/<backend>/s<span>" ];
  claims = builtins.listToAttrs (map (r: {
    name = "programs/${r.backend}/s${toString r.span}";
    value = { kind = "exact"; of = { use = r.name; dataset = "run"; column = "ok"; }; value = true; };
  }) (lib.filter (r: r.span != 0) runs));
  analysis = ./analysis.py;
}
