# The lab (lab/README.md): the closures of the runners, the experiments of every backend and the suite over them, the
# SIMD benchmark. `catalogue` is the list of { name, kind, backend, span, exp } the apps and the suite are made of.
{ pkgs, nixsci, system, self, tree, build }:
let
  inherit (tree) lib backendNames trameFiles;
  inherit (build) backends labPy;
  # Runners are the .py files of lab/ (and the columns they record) only.
  runners = lib.fileset.toSource {
    root = ../lab;
    fileset = lib.fileset.unions [ (lib.fileset.fileFilter (f: f.hasExt "py") ../lab) ../lab/columns.json ];
  };
  labLib = nixsci.lib.lab { inherit pkgs; project = self; };
  # One closure: a runner over one backend's build and launch. The environment is the runners' contract: LAUNCH (the
  # backend's launch), PROGRAMS, BENCH, BIN, EXPECTED (its build), PANICS (check/programs).
  closure = { name, script, b, dir ? runners, env ? { } }: nixsci.lib.mkExperiment pkgs {
    inherit name;
    program = "${pkgs.python3}/bin/python3";
    args = [ "${dir}/${script}" ];
    env = {
      PYTHONPATH = labPy;
      LAUNCH = "${b.launch}/bin/launch";
      PROGRAMS = "${b.bins}/programs";
      BENCH = "${b.bins}/bench";
      BIN = "${b.bins}/bin";
      EXPECTED = "${b.bins}/share/expected";
      PANICS = "${../check/programs}";
    } // env;
    runtime = b.d.launchRuntime;
  };
  # The machines of the suite (nix/lab/suite.nix): the most a job of the suite spans.
  machines = 4;
  q = import ./lab/q.nix { inherit lib; };
  labArgs = { inherit pkgs lib labLib system closure backends machines; };
  # The SIMD benchmark (nix/lab/simd.nix): experiments and their analysis, not in the suite.
  simd = import ./lab/simd.nix { inherit pkgs lib labLib system nixsci runners labPy trameFiles; };
  catalogue =
    import ./lab/checks.nix labArgs
    ++ import ./lab/perf.nix (labArgs // { inherit q; })
    ++ lib.concatMap (n: backends.${n}.d.experiments (labArgs // { b = backends.${n}; data = import ./lab/data.nix { inherit lib; }; }))
      backendNames
    ++ simd.experiments;
in
{
  inherit simd catalogue;
  experiments = builtins.listToAttrs (map (i: { inherit (i) name; value = i.exp; }) catalogue);
  suite = import ./lab/suite.nix {
    inherit pkgs lib labLib catalogue machines;
    qm = q;
    python = nixsci.packages.${system}.python;
  };
}
