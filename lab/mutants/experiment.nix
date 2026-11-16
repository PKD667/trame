# Conformance by mutation (check/mutants/): one build per patch of a backend's own set, every program run on each. Only for a
# backend whose runs are not a function of the program (`deterministic = false`): the others have the validate verdict
# mutants-<b> (nix/validate.nix). Not in the suite; one local job per mutant.
{ pkgs, lib, labLib, system, closure, backends, data, ... }:
let
  killData = {
    columns = { mutant = "str"; rule = "str"; expect = "str"; program = "str"; killed = "bool"; how = "str"; };
    key = [ "mutant" "program" ];
  };
  # The ids of the files of `dir` with the given suffix: `.patch` (applied) or `.skip` (no patch, a reason).
  idsWith = dir: suffix:
    map (n: lib.removeSuffix suffix n)
      (builtins.filter (n: lib.hasSuffix suffix n) (builtins.attrNames (builtins.readDir dir)));
  mutantIds = dir: lib.sort lib.lessThan (lib.unique (idsWith dir ".patch" ++ idsWith dir ".skip"));
  # One build per patched mutant, plus `none`: the unpatched build, the control. A skipped mutant has no build.
  mutantFarm = n: b: pkgs.linkFarm "mutants-${n}" (
    [ { name = "none"; path = b.bins; } ]
    ++ map (id: { name = id; path = b.binsWith [ (b.mutants + "/${id}.patch") ]; }) (idsWith b.mutants ".patch")
  );
  run = n: b: {
    name = "mutants-${n}";
    kind = "mutants";
    backend = n;
    span = 0;
    exp = labLib.experiment {
      name = "mutants-${n}";
      closures.${system} = closure {
        name = "mutants-${n}"; inherit b; script = ./run.py;
        env = { MUTANTS = "${mutantFarm n b}"; PATCHES = "${b.mutants}"; };
      };
      params = { backend = n; timeout = 120; };
      sweep.mutant = mutantIds b.mutants;
      resources = { provider = "local"; hosts = 1; };
      data = { kill = killData; launch_config = data.dataset "launch_config"; };
    };
  };
in
{
  name = "mutants";
  figures = [ "mutants-kills" ];
  runs = lib.concatLists (lib.mapAttrsToList (n: b: lib.optional (!b.d.deterministic && b.mutants != null) (run n b)) backends);
  analysis = ./analysis.py;
}
