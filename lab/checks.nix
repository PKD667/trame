# Conformance as lab experiments, for every backend, built from its descriptor (lab/README.md, "programs" and
# "mutants"). Called from flake.nix. Nothing here names a backend.
#
#   programs-<b>        every program of check/programs/ on backend <b>, one machine, locally, over the descriptor's splits
#   programs-<b>-s<S>   the same on S machines (S in 1 2 4 within the descriptor's maxMachines), on the cluster
#   mutants-<b>         conformance by mutation (src/backends/<b>/mutants/README via check/mutants/README.md): one build
#                       per patch of the backend's own set, every program run on each. Only for a backend whose runs are
#                       not a function of the program (`deterministic = false`): the others have the validate verdict
#                       mutants-<b> (lab/validate.nix, the backend's validate.nix).
# Answers a list of { name, kind, backend, span, exp } (as lab/perf.nix); span 0 is the local experiment.
{ pkgs, lib, labLib, system, closure, backends, machines }:
let
  # What the launch ran under (lab/launch.py record_config), recorded by every runner that launches.
  launchConfig = (import ./data.nix { inherit lib; }).dataset "launch_config";
  runData = {
    columns = { program = "str"; repeat = "int"; ran = "bool"; ok = "bool"; verdict = "str"; seconds = "float"; stdout_sha256 = "str"; };
    key = [ "program" "repeat" ];
    noisy.seconds = 1.0;
  };
  killData = {
    columns = { mutant = "str"; rule = "str"; expect = "str"; program = "str"; killed = "bool"; how = "str"; };
    key = [ "mutant" "program" ];
  };

  programs = n: b: span:
    let name = if span == 0 then "programs-${n}" else "programs-${n}-s${toString span}"; in {
      inherit name;
      kind = "programs";
      backend = n;
      inherit span;
      suite = span != 0;
      exp = labLib.experiment ({
        inherit name;
        closures.${system} = closure { inherit name b; script = "programs.py"; };
        params = { backend = n; repeats = 3; };
        sweep.split = b.d.splits;
        data = { run = runData; launch_config = launchConfig; };
      } // (if span == 0
        then { resources = { provider = "local"; hosts = 1; }; }
        else { inherit span; resources = { provider = "cluster"; hosts = span; walltime = 60; opts.stage = "fanout"; }; }));
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
  mutants = n: b: {
    name = "mutants-${n}";
    kind = "mutants";
    backend = n;
    span = 0;
    exp = labLib.experiment {
      name = "mutants-${n}";
      closures.${system} = closure {
        name = "mutants-${n}"; inherit b; script = "mutants.py";
        env = { MUTANTS = "${mutantFarm n b}"; PATCHES = "${b.mutants}"; };
      };
      params = { backend = n; timeout = 120; };
      sweep.mutant = mutantIds b.mutants;
      resources = { provider = "local"; hosts = 1; };
      data = { kill = killData; launch_config = launchConfig; };
    };
  };

  spans = b: lib.filter (s: s <= machines && s <= b.d.maxMachines) [ 1 2 4 ];
in
lib.concatLists (lib.mapAttrsToList (n: b:
  [ (programs n b 0) ] ++ map (programs n b) (spans b)
  ++ lib.optional (!b.d.deterministic && b.mutants != null) (mutants n b)) backends)
