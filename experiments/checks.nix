# Conformance by mutation as lab experiments (conformance/README.md; experiments/README.md, "mutants").
# Called from flake.nix with the flake's own values. One experiment per backend. Its closure holds one
# build of the backend per mutant (the patch applied with the other sources unchanged), and the runner.
{ pkgs, lib, labLib, system, trameSeqBinsWith, trameMpiBinsWith, runners, labRuntime, nixsci }:
let
  backends = {
    seq = { name = "seq"; dir = ../conformance/mutants/seq; bins = trameSeqBinsWith; };
    mpi = { name = "mpi"; dir = ../conformance/mutants/mpi; bins = trameMpiBinsWith; };
  };
  # The ids of the files of `dir` with the given suffix: `.patch` (applied) or `.skip` (no patch, a reason).
  idsWith = dir: suffix:
    map (n: lib.removeSuffix suffix n)
      (builtins.filter (n: lib.hasSuffix suffix n) (builtins.attrNames (builtins.readDir dir)));
  # Every mutant of a backend, in order: the patched ones and the skipped ones.
  mutantIds = dir: lib.sort lib.lessThan (lib.unique (idsWith dir ".patch" ++ idsWith dir ".skip"));
  # One build per patched mutant, plus `none`: the unpatched build, the control. A skipped mutant has no build.
  mutantFarm = b: pkgs.linkFarm "mutants-${b.name}" (
    [ { name = "none"; path = b.bins [ ]; } ]
    ++ map (id: { name = id; path = b.bins [ (b.dir + "/${id}.patch") ]; }) (idsWith b.dir ".patch")
  );
  mpiConfigData = { columns = { source = "str"; text = "str"; }; };
  killData = {
    columns = { mutant = "str"; rule = "str"; expect = "str"; program = "str"; killed = "bool"; how = "str"; };
    key = [ "mutant" "program" ];
  };
  experiment = name: b: labLib.experiment {
    inherit name;
    closures.${system} = nixsci.lib.mkExperiment pkgs {
      name = "mutants-${b.name}";
      program = "${pkgs.python3}/bin/python3";
      args = [ "${runners}/mutants.py" ];
      env = {
        PYTHONPATH = "${nixsci.packages.${system}.lab-py}/${pkgs.python3.sitePackages}";
        MUTANTS = "${mutantFarm b}";
        PATCHES = "${b.dir}";
        RANKS_FILE = "${../backends/mpi/deploy/ranks.txt}";
      };
      runtime = labRuntime;
    };
    params = { backend = b.name; timeout = 120; };
    sweep.mutant = mutantIds b.dir;
    resources = { provider = "local"; hosts = 1; };
    data = { kill = killData; } // lib.optionalAttrs (b.name == "mpi") { mpi_config = mpiConfigData; };
  };
in {
  mutants-seq = experiment "mutants-seq" backends.seq;
  mutants-mpi = experiment "mutants-mpi" backends.mpi;
}
