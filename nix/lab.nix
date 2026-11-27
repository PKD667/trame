# The lab (lab/README.md): the experiments, their runs, the suite over them, and one analysis per experiment.
#
# An experiment is a directory with an experiment.nix: every lab/<name>/ and every src/backends/<b>/lab/<name>/, found the way
# nix/tree.nix finds backends. It answers { name, figures, runs, claims, claimIds, analysis, packages }; lab/README.md is the
# contract. `catalogue` is the list of the runs of every experiment: { name, kind, backend, span, suite, exp, experiment }.
# Nothing here names a backend.
{ pkgs, nixsci, system, self, tree, build, manifest }:
let
  inherit (tree) lib backendNames backendDir trameFiles;
  inherit (build) backends labPy;

  # The modules of the runners (lab/lib): the closure's PYTHONPATH. Not the analyses' (lab/tables.py).
  runnerLib = lib.fileset.toSource { root = ../lab/lib; fileset = lib.fileset.fileFilter (f: f.hasExt "py") ../lab/lib; };
  labLib = nixsci.lib.lab { inherit pkgs; project = self; };

  # One closure: a runner over one backend's build and launch. The environment is the runners' contract: LAUNCH (the
  # backend's launch), PROGRAMS, BENCH, BIN, EXPECTED (its build), PANICS (check/programs), COLUMNS (lab/columns.json).
  # `script` is the experiment's run.py; `path` adds directories to PYTHONPATH (a backend's own modules).
  closure = { name, script, b, env ? { }, path ? [ ] }: nixsci.lib.mkExperiment pkgs {
    inherit name;
    program = "${pkgs.python3}/bin/python3";
    args = [ "${script}" ];
    env = {
      PYTHONPATH = lib.concatStringsSep ":" ([ labPy "${runnerLib}" ] ++ map toString path);
      LAUNCH = "${b.launch}/bin/launch";
      PROGRAMS = "${b.bins}/programs";
      BENCH = "${b.bins}/bench";
      BIN = "${b.bins}/bin";
      EXPECTED = "${b.bins}/share/expected";
      PANICS = "${../check/programs}";
      COLUMNS = "${../lab/columns.json}";
    } // env;
    runtime = b.d.launchRuntime;
  };

  # The machines of the suite: the most a job of the suite spans.
  machines = 4;
  data = import ./lab/data.nix { inherit lib; };
  args = { inherit pkgs lib labLib system closure backends machines data nixsci labPy trameFiles manifest; bench = import ./lab/bench.nix { inherit lib labLib system closure backends machines data; }; };

  # The directories under `base` that hold an experiment.nix.
  experimentDirs = base:
    lib.optionals (builtins.pathExists base) (lib.filter (d: builtins.pathExists (d + "/experiment.nix"))
      (map (n: base + "/${n}") (builtins.attrNames (lib.filterAttrs (_: t: t == "directory") (builtins.readDir base)))));
  load = extra: dir:
    let e = import (dir + "/experiment.nix") (args // extra); in
    { claims = { }; claimIds = [ ]; figures = [ ]; packages = { }; } // e // {
      inherit dir;
      readme = dir + "/README.md";
      # A run belongs to the experiment of its directory unless it names another (a baseline names the family it joins).
      runs = map (r: { experiment = e.name; } // r) e.runs;
    };
  experiments = map (load { }) (experimentDirs ../lab)
    ++ lib.concatMap (n: map (load { b = backends.${n}; }) (experimentDirs (backendDir + "/${n}/lab"))) backendNames;

  # The catalogue in one order: programs, then the other families by name, then the baselines (the suite runs, and the matrix
  # lists, in this order). The sort is stable: a family's runs stay in the order of its spans.
  rank = r: if r.kind == "programs" then 0 else if lib.hasPrefix "baseline-" r.kind then 2 else 1;
  catalogue = lib.sort (a: b: rank a < rank b || rank a == rank b && a.kind < b.kind) (lib.concatMap (e: e.runs) experiments);

  # ANALYSES. One per experiment that has an analysis, over its runs and the runs of other experiments that name it.
  lockFile = self + "/lab.lock";
  lock = if builtins.pathExists lockFile then builtins.fromJSON (builtins.readFile lockFile) else { };
  locked = builtins.attrNames (lock.runs or lock);
  analysisOf = draft: e:
    let
      mine = lib.filter (r: r.experiment == e.name) catalogue;
      # A draft takes the runs that exist; the strict analysis fails, naming the runs that do not.
      used = if draft then lib.filter (r: builtins.elem r.name locked) mine else mine;
      use = builtins.listToAttrs (map (r: { inherit (r) name; value = r.exp; }) used);
      # The pipeline's directory: the analysis, the shared module and the figures it must write (lab/tables.py `done`).
      pipeline = pkgs.runCommand "${e.name}-pipeline" { } ''
        mkdir $out
        cp ${e.analysis} $out/analysis.py
        cp ${../lab/tables.py} $out/tables.py
        echo ${lib.escapeShellArg (builtins.toJSON e.figures)} > $out/figures.json
      '';
    in
    labLib.analysis {
      name = e.name;
      inherit draft use;
      claims = lib.filterAttrs (_: c: use ? ${c.of.use}) e.claims;
      pipelines.tables.script = "${pipeline}/analysis.py";
      confidence = 0.99;
    };
  withAnalysis = lib.filter (e: e ? analysis) experiments;
  analyses = draft: lib.listToAttrs (map (e: { name = e.name; value = analysisOf draft e; }) withAnalysis);
  report = draft: pkgs.linkFarm (if draft then "report-draft" else "report") (lib.mapAttrsToList (name: path: { inherit name path; }) (analyses draft));

  # THE CONTRACT of an experiment directory, checked in evaluation: its README has the claims and the figures it declares.
  # The claims of a README are the lines "- `id` ..." under "## Claims"; an id may hold <placeholders>, which stand for the
  # parts that vary (backend, size).
  readmeClaims = text:
    (lib.foldl' (acc: l:
      if lib.hasPrefix "## " l then acc // { on = l == "## Claims"; }
      else if acc.on && builtins.match "- `([^`]+)`.*" l != null then acc // { ids = acc.ids ++ builtins.match "- `([^`]+)`.*" l; }
      else acc) { on = false; ids = [ ]; } (lib.splitString "\n" text)).ids;
  regexOf = template: lib.concatStrings (map (p: if builtins.isList p then "[^/]+" else lib.escapeRegex p) (builtins.split "<[^>]*>" template));
  problems = lib.concatMap (e:
    let
      text = if builtins.pathExists e.readme then builtins.readFile e.readme else "";
      ids = readmeClaims text;
      matches = id: lib.any (t: builtins.match (regexOf t) id != null) e.claimIds;
    in
    lib.optional (!builtins.pathExists e.readme) "${e.name}: no README.md"
    ++ map (t: "${e.name}: claim ${t} is not in the README's Claims") (lib.subtractLists ids e.claimIds)
    ++ map (t: "${e.name}: README claim ${t} is not in claimIds") (lib.subtractLists e.claimIds ids)
    ++ map (id: "${e.name}: claim ${id} matches no claimIds template") (lib.filter (id: !matches id) (builtins.attrNames e.claims))
    ++ map (f: "${e.name}: figure ${f} is not named in the README") (lib.filter (f: !lib.hasInfix "`${f}`" text) e.figures)
    ++ lib.optional (e ? analysis && e.figures == [ ]) "${e.name}: an analysis with no figure"
  ) experiments;
  contract = assert lib.assertMsg (problems == [ ]) "lab experiments break their contract:\n${lib.concatStringsSep "\n" problems}";
    pkgs.runCommand "lab-contract" { } "touch $out";

  # Every runner and analysis compiles.
  python = pkgs.runCommand "lab-python" { nativeBuildInputs = [ pkgs.python3 ]; src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      (lib.fileset.fileFilter (f: f.hasExt "py") ../lab)
      (lib.fileset.fileFilter (f: f.hasExt "py") ../check)
      (lib.fileset.fileFilter (f: f.hasExt "py") backendDir)
    ];
  }; } ''
    cd $src
    find . -name '*.py' -print0 | xargs -0 python3 -c 'import sys; [compile(open(f).read(), f, "exec") for f in sys.argv[1:]]'
    touch $out
  '';
in
{
  inherit catalogue experiments contract python;
  # name -> experiment of nixsci, of every run: the apps (`nix run .#<name>`).
  runs = builtins.listToAttrs (map (r: { inherit (r) name; value = r.exp; }) catalogue);
  packages = lib.foldl' (a: e: a // e.packages) { } experiments;
  # name -> analysis. Strict ones fail naming the runs they lack; drafts take the runs that exist.
  analyses = analyses false // { report = report false; };
  drafts = analyses true // { report = report true; };
  suite = import ./lab/suite.nix { inherit pkgs lib catalogue; };
}
