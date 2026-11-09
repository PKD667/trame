{
  description = "Backend-local trame development environments";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  # Nightly toolchains by date, for the driver only (see crates/driver/rust-toolchain.toml).
  inputs.rust-overlay = {
    url = "github:oxalica/rust-overlay";
    inputs.nixpkgs.follows = "nixpkgs";
  };
  # The lab experiments (lab/README.md) and their deployment to hosts. Pinned; nixsci's only input is nixpkgs.
  # nixsci main (health check, target-side cache fetch, --hosts).
  inputs.nixsci = {
    url = "github:PKD667/nixsci/main";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, rust-overlay, nixsci, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      # The driver's toolchain: the nightly pinned in crates/driver/rust-toolchain.toml, with rustc-dev.
      driverPkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
      driverToolchain = driverPkgs.rust-bin.fromRustupToolchainFile ./crates/driver/rust-toolchain.toml;
      driverPlatform = driverPkgs.makeRustPlatform {
        cargo = driverToolchain;
        rustc = driverToolchain;
      };
      trameDriver = driverPlatform.buildRustPackage {
        pname = "trame-driver";
        version = "0.1.0";
        src = ./crates/driver;
        cargoLock.lockFile = ./crates/driver/Cargo.lock;
        # Tests compile fixture crates and need the tree; run them with `cargo test` (crates/driver/README.md).
        doCheck = false;
      };
      lib = pkgs.lib;

      # The backends of the tree: every directory of src/backends/ with a backend.nix (src/backends/README.md, The
      # descriptor). Nothing below names one. The cargo feature of a backend is the name of its directory.
      backendDir = ./src/backends;
      backendNames = builtins.attrNames (lib.filterAttrs
        (n: t: t == "directory" && builtins.pathExists (backendDir + "/${n}/backend.nix"))
        (builtins.readDir backendDir));
      descriptors = lib.genAttrs backendNames (n: import (backendDir + "/${n}/backend.nix") { inherit pkgs; });

      # The root crate and what its builds read: never crates/driver/ or target/, and not what a descriptor excludes.
      trameFiles = lib.fileset.unions ([
          ./Cargo.toml
          ./Cargo.lock
          ./README.md
          ./INTERFACE.md
          ./crates/macros
          ./src/math
          ./check/programs
          ./tests
          ./lab/bench
          ./examples
          (lib.fileset.difference backendDir (lib.fileset.unions (lib.concatMap
            (n: map (e: lib.fileset.maybeMissing (backendDir + "/${n}/${e}")) descriptors.${n}.exclude) backendNames)))
        ] ++ map (n: ./src + "/${n}") (
          builtins.filter (n: lib.hasSuffix ".rs" n) (builtins.attrNames (builtins.readDir ./src))
        ));
      trameSrc = lib.fileset.toSource { root = ./.; fileset = trameFiles; };
      # The rejection suite (check/rejections/README.md) is a separate package that depends on the root crate by path.
      rejectionsSrc = lib.fileset.toSource {
        root = ./.;
        fileset = lib.fileset.unions [ trameFiles (lib.fileset.difference ./check/rejections (lib.fileset.maybeMissing ./check/rejections/target)) ];
      };
      fmtSrc = lib.fileset.toSource {
        root = ./.;
        fileset = lib.fileset.unions [
          ./rustfmt.toml
          (lib.fileset.difference (lib.fileset.fileFilter (f: f.hasExt "rs") ./.) (lib.fileset.unions ([
            ./check/rejections
          ] ++ lib.concatMap
            (n: map (e: lib.fileset.maybeMissing (backendDir + "/${n}/${e}")) descriptors.${n}.exclude) backendNames)))
        ];
      };

      # The programs of check/programs/ and the benchmarks of lab/bench/: the examples of Cargo.toml by path.
      examples = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).example;
      programNames = map (n: lib.removeSuffix ".rs" n)
        (builtins.filter (n: lib.hasSuffix ".rs" n) (builtins.attrNames (builtins.readDir ./check/programs)));
      benchNames = map (e: e.name) (builtins.filter (e: lib.hasPrefix "lab/bench/" e.path) examples);

      # One build of a backend: its feature, the programs and benchmarks, and the bins its descriptor lists, with the
      # patches given (a mutant). Layout: programs/, bench/, bin/, share/expected/.
      binsFor = name: d: patches: pkgs.rustPlatform.buildRustPackage {
        pname = "trame-${name}-bins";
        version = "0.1.0";
        src = trameSrc;
        inherit patches;
        # The patches of src/backends/<b>/mutants are written against src/.
        patchFlags = [ "-p1" "-d" "src" ];
        cargoLock.lockFile = ./Cargo.lock;
        buildNoDefaultFeatures = true;
        buildFeatures = [ name ];
        # Every program is built with the checked cfg (check/programs/run.sh).
        RUSTFLAGS = "--cfg trame_checked";
        nativeBuildInputs = d.packages;
        inherit (d) env;
        cargoBuildFlags = [ "--examples" ] ++ lib.concatMap (b: [ "--bin" b ]) d.bins;
        # Tests run as checks (lab/validate.nix), not in the build.
        doCheck = false;
        inherit (d) postInstall;
        installPhase = ''
          runHook preInstall
          mkdir -p $out/bin $out/programs $out/bench $out/share/expected
          rel=target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release
          for p in ${lib.concatStringsSep " " programNames}; do cp $rel/examples/$p $out/programs/; done
          for p in ${lib.concatStringsSep " " benchNames}; do cp $rel/examples/$p $out/bench/; done
          for b in ${lib.concatStringsSep " " d.bins}; do cp $rel/$b $out/bin/; done
          cp check/programs/*.expected $out/share/expected/
          runHook postInstall
        '';
      };

      labPy = "${nixsci.packages.${system}.lab-py}/${pkgs.python3.sitePackages}";
      # A backend's launch with what it needs on its PATH and the nixsci library, so it runs from anywhere.
      launchFor = name: d: pkgs.runCommand "launch-${name}" { nativeBuildInputs = [ pkgs.makeWrapper pkgs.python3 pkgs.bash ]; } ''
        install -Dm755 ${backendDir + "/${name}/launch"} $out/bin/launch
        patchShebangs $out/bin/launch
        wrapProgram $out/bin/launch --prefix PATH : ${lib.makeBinPath d.launchRuntime} --prefix PYTHONPATH : ${labPy}
      '';

      backends = lib.mapAttrs (n: d: rec {
        inherit d;
        src = trameSrc;
        binsWith = binsFor n d;
        bins = binsWith [ ];
        launch = launchFor n d;
        programs = programNames;
        mutants = let dir = backendDir + "/${n}/mutants"; in if builtins.pathExists dir then dir else null;
      }) descriptors;

      # The lab (lab/README.md). Runners are the .py files of lab/ (and the columns they record) only.
      runners = lib.fileset.toSource {
        root = ./lab;
        fileset = lib.fileset.unions [ (lib.fileset.fileFilter (f: f.hasExt "py") ./lab) ./lab/columns.json ];
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
          PANICS = "${./check/programs}";
        } // env;
        runtime = b.d.launchRuntime;
      };
      # The machines of the suite (lab/suite.nix): the most a job of the suite spans.
      machines = 4;
      qm = import ./lab/q.nix { inherit lib; };
      labArgs = { inherit pkgs lib labLib system closure backends machines; };
      catalogue =
        import ./lab/checks.nix labArgs
        ++ import ./lab/perf.nix (labArgs // { q = qm; })
        ++ lib.concatMap (n: backends.${n}.d.experiments (labArgs // { b = backends.${n}; data = import ./lab/data.nix { inherit lib; }; }))
          backendNames;
      experiments = builtins.listToAttrs (map (i: { inherit (i) name; value = i.exp; }) catalogue);
      labSuite = import ./lab/suite.nix {
        inherit pkgs lib labLib catalogue machines qm;
        python = nixsci.packages.${system}.python;
      };

      # The deterministic checks: nixsci validate verdicts (lab/validate.nix).
      v = nixsci.lib.validate { inherit pkgs; };
      suite = v.suite (import ./lab/validate.nix {
        inherit pkgs v backends rejectionsSrc driverToolchain trameDriver;
      });
      mkBackendShell = name: d:
        pkgs.mkShell {
          packages = [ pkgs.rustc pkgs.cargo pkgs.clippy pkgs.rustfmt ] ++ d.packages ++ d.launchRuntime;
          shellHook = builtins.concatStringsSep "\n" (
            [ "export TRAME_BACKEND=${name}" ] ++ lib.mapAttrsToList (k: val: "export ${k}=${val}") d.env
          );
        };
      # `trame build|run` (crates/cli/trame): the backend as a flag, the program started by that backend's launch.
      launches = pkgs.linkFarm "trame-launches" (lib.mapAttrsToList (n: b: { name = n; path = "${b.launch}/bin"; }) backends);
      trameCli = pkgs.runCommand "trame-cli" { nativeBuildInputs = [ pkgs.makeWrapper ]; } ''
        install -Dm755 ${./crates/cli/trame} $out/bin/trame
        patchShebangs $out/bin/trame
        wrapProgram $out/bin/trame \
          --prefix PYTHONPATH : ${labPy} \
          --prefix PATH : ${trameDriver}/bin \
          --set TRAME_TOOLCHAIN ${driverToolchain}/bin \
          --set TRAME_BACKENDS ${launches}
      '';
    in {
      validate.${system} = suite;
      # The verdicts, and one style for the Rust sources (rustfmt.toml). check/rejections/ is left out: its cases
      # carry line-anchored annotations.
      checks.${system} = suite.checks // {
        fmt = pkgs.runCommand "trame-fmt" { nativeBuildInputs = [ pkgs.rustfmt pkgs.findutils ]; } ''
          cd ${fmtSrc}
          find . -name '*.rs' -print0 | xargs -0 rustfmt --check --edition 2024 --config-path rustfmt.toml
          touch $out
        '';
      };
      packages.${system} = {
        trame = trameCli;
        trame-driver = trameDriver;
      } // lib.concatMapAttrs (n: b: {
        "trame-${n}-bins" = b.bins;
        "launch-${n}" = b.launch;
      }) backends;
      # An analysis is a derivation over runs in lab.lock, which a fresh clone may not have, and `nix flake check`
      # evaluates packages: so the report is not under `packages` (nixsci's convention).
      legacyPackages.${system} = {
        suite-report = labSuite.report;
        suite-report-draft = labSuite.reportDraft;
      };
      lab.${system} = {
        inherit experiments;
        # The suite's jobs per backend: experiment, kind, machines of a job, and its jobs (sweep x seeds x replicates).
        suiteMatrix = labSuite.matrix;
      };
      # The dev shell of each backend as a nixsci deployment, for `nixsci deploy shell ... dev-<backend> -- CMD` on a
      # cluster node: tests run there, not on a laptop. Same packages and env as devShells.<backend>.
      deployments.${system} = lib.mapAttrs' (n: d: lib.nameValuePair "dev-${n}" (nixsci.lib.mkDeployment pkgs {
        name = "trame-dev-${n}";
        program = "${pkgs.bashInteractive}/bin/bash";
        runtime = [ pkgs.rustc pkgs.cargo pkgs.clippy pkgs.rustfmt pkgs.gcc pkgs.git pkgs.gnumake pkgs.time pkgs.bashInteractive pkgs.gnugrep pkgs.gnused pkgs.gawk pkgs.findutils pkgs.diffutils ] ++ d.packages;
        env = d.env;
      })) descriptors;
      apps.${system} = builtins.mapAttrs (_: e: e.run) experiments // {
        trame = { type = "app"; program = "${trameCli}/bin/trame"; };
        # Every experiment of the suite, for every backend (lab/suite.nix), one after another.
        suite = labSuite.app;
      };
      devShells.${system} = {
        # No backend: the toolchain only. A cargo build names its backend (--no-default-features --features <name>).
        default = pkgs.mkShell { packages = [ pkgs.rustc pkgs.cargo pkgs.clippy pkgs.rustfmt ]; };
        # The pinned nightly, for building crates/driver/ with `cargo` (see crates/driver/README.md). The shell
        # does not depend on the package, so entering it does not build the driver.
        driver = driverPkgs.mkShell {
          packages = [ driverToolchain ];
        };
      } // lib.mapAttrs mkBackendShell descriptors;
    };
}
