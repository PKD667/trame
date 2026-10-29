{
  description = "Backend-local trame development environments";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  # Nightly toolchains by date, for the driver only (see driver/rust-toolchain.toml).
  inputs.rust-overlay = {
    url = "github:oxalica/rust-overlay";
    inputs.nixpkgs.follows = "nixpkgs";
  };
  # The lab experiments (experiments/README.md) and their deployment to hosts. Pinned; nixsci's only input is nixpkgs.
  # The validate branch: nixsci.lib.validate (validate/README.md) is not on main yet.
  inputs.nixsci = {
    url = "github:PKD667/nixsci/validate";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, rust-overlay, nixsci, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      # The driver's toolchain: the nightly pinned in driver/rust-toolchain.toml, with rustc-dev.
      driverPkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
      driverToolchain = driverPkgs.rust-bin.fromRustupToolchainFile ./driver/rust-toolchain.toml;
      driverPlatform = driverPkgs.makeRustPlatform {
        cargo = driverToolchain;
        rustc = driverToolchain;
      };
      trameDriver = driverPlatform.buildRustPackage {
        pname = "trame-driver";
        version = "0.1.0";
        src = ./driver;
        cargoLock.lockFile = ./driver/Cargo.lock;
        # Tests compile fixture crates and need the tree; run them with `cargo test` (driver/README.md).
        doCheck = false;
      };
      # The root crate with the MPI backend, its examples and the check and latency bins. Only the
      # files the build reads: never driver/ or target/, and not the mpi-rma submodule.
      trameFiles = pkgs.lib.fileset.unions ([
          ./Cargo.toml
          ./Cargo.lock
          ./README.md
          ./macros
          ./math
          ./backends/seq
          ./programs
          ./tests
          ./scale
          ./examples
          (pkgs.lib.fileset.difference ./backends/mpi (pkgs.lib.fileset.maybeMissing ./backends/mpi/mpi-rma))
        ] ++ map (n: ./. + "/${n}") (
          builtins.filter (n: pkgs.lib.hasSuffix ".rs" n) (builtins.attrNames (builtins.readDir ./.))
        ));
      trameMpiSrc = pkgs.lib.fileset.toSource { root = ./.; fileset = trameFiles; };
      # The rejection suite (rejections/README.md) is a separate package that depends on the root crate by path.
      rejectionsSrc = pkgs.lib.fileset.toSource {
        root = ./.;
        fileset = pkgs.lib.fileset.unions [ trameFiles (pkgs.lib.fileset.difference ./rejections (pkgs.lib.fileset.maybeMissing ./rejections/target)) ];
      };
      fmtSrc = pkgs.lib.fileset.toSource {
        root = ./.;
        fileset = pkgs.lib.fileset.unions [
          ./rustfmt.toml
          (pkgs.lib.fileset.difference (pkgs.lib.fileset.fileFilter (f: f.hasExt "rs") ./.) (pkgs.lib.fileset.unions [
            ./rejections
            (pkgs.lib.fileset.maybeMissing ./backends/nv)
            (pkgs.lib.fileset.maybeMissing ./backends/mpi/mpi-rma)
          ]))
        ];
      };
      mpiDeps = import ./backends/mpi/dependencies.nix { inherit pkgs; };
      trameMpiBinsWith = patches: pkgs.rustPlatform.buildRustPackage {
        pname = "trame-mpi-bins";
        version = "0.1.0";
        src = trameMpiSrc;
        patches = patches;
        cargoLock.lockFile = ./Cargo.lock;
        buildNoDefaultFeatures = true;
        buildFeatures = [ "mpi" ];
        # Every program is built with the checked cfg (programs/run.sh).
        RUSTFLAGS = "--cfg trame_checked";
        # mpicc is in openmpi's dev output, not in the default one that dependencies.nix names; and its
        # MPICC points at the default output, where it is not. Both are set here.
        nativeBuildInputs = mpiDeps.packages ++ [ pkgs.openmpi.dev ];
        env = mpiDeps.env // { MPICC = "${pkgs.openmpi.dev}/bin/mpicc"; };
        cargoBuildFlags = [ "--examples" "--bins" ];
        # Tests run in the devshell (backends/mpi/tests/run.sh), not in the build.
        doCheck = false;
        installPhase = ''
          runHook preInstall
          mkdir -p $out/bin $out/share/expected
          rel=target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release
          for p in ${pkgs.lib.concatStringsSep " " mpiPrograms} scale_barrier scale_ring scale_idle scale_fanin scale_graph; do
            cp $rel/examples/$p $out/bin/
          done
          cp $rel/check $rel/latency $out/bin/
          mpicc -O2 -o $out/bin/mpi_barrier scale/baseline/barrier.c
          mpicc -O2 -o $out/bin/mpi_ring scale/baseline/ring.c
          mpicc -O2 -o $out/bin/mpi_graph scale/baseline/graph.c
          cp programs/*.expected $out/share/expected/
          runHook postInstall
        '';
      };
      trameMpiBins = trameMpiBinsWith [ ];
      # The two program lists (programs/run.sh says the same in words). seq runs every programs/*.rs; mpi runs
      # the rows of backends/mpi/deploy/ranks.txt (`<program> <ranks>` per line), so a program without a row is seq only.
      seqPrograms = map (n: pkgs.lib.removeSuffix ".rs" n)
        (builtins.filter (n: pkgs.lib.hasSuffix ".rs" n) (builtins.attrNames (builtins.readDir ./programs)));
      mpiPrograms = map (l: builtins.head (pkgs.lib.splitString " " l))
        (builtins.filter (l: l != "" && !pkgs.lib.hasPrefix "#" l)
          (pkgs.lib.splitString "\n" (builtins.readFile ./backends/mpi/deploy/ranks.txt)));
      # The seq backend: the default build, one process per program (programs/run.sh, seq).
      trameSeqBinsWith = patches: pkgs.rustPlatform.buildRustPackage {
        pname = "trame-seq-bins";
        version = "0.1.0";
        src = trameMpiSrc;
        patches = patches;
        cargoLock.lockFile = ./Cargo.lock;
        # Every program is built with the checked cfg (programs/run.sh).
        RUSTFLAGS = "--cfg trame_checked";
        cargoBuildFlags = map (p: "--example ${p}") seqPrograms;
        doCheck = false;
        installPhase = ''
          runHook preInstall
          mkdir -p $out/bin $out/share/expected
          rel=target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release
          for p in ${pkgs.lib.concatStringsSep " " seqPrograms}; do
            cp $rel/examples/$p $out/bin/
          done
          cp programs/*.expected $out/share/expected/
          runHook postInstall
        '';
      };
      trameSeqBins = trameSeqBinsWith [ ];
      # The lab experiments (experiments/README.md). Runners are the .py files of experiments/ only.
      runners = pkgs.lib.fileset.toSource {
        root = ./experiments;
        fileset = pkgs.lib.fileset.fileFilter (f: f.hasExt "py") ./experiments;
      };
      labRuntime = [ pkgs.openmpi pkgs.coreutils pkgs.time pkgs.procps pkgs.util-linux pkgs.gnugrep ];
      # One closure: a runner over one set of binaries (BIN, EXPECTED, RANKS_FILE in its environment).
      runnerClosure = name: script: bins: nixsci.lib.mkExperiment pkgs {
        inherit name;
        program = "${pkgs.python3}/bin/python3";
        args = [ "${runners}/${script}" ];
        env = {
          PYTHONPATH = "${nixsci.packages.${system}.lab-py}/${pkgs.python3.sitePackages}";
          BIN = "${bins}/bin";
          EXPECTED = "${bins}/share/expected";
          RANKS_FILE = "${./backends/mpi/deploy/ranks.txt}";
          PANICS = "${./programs}";
        };
        runtime = labRuntime;
      };
      programsMpiClosure = runnerClosure "programs-mpi" "programs.py" trameMpiBins;
      transportClosure = runnerClosure "transport-mpi" "transport.py" trameMpiBins;
      scaleClosure = runnerClosure "scale-local" "scale.py" trameMpiBins;
      labLib = nixsci.lib.lab { inherit pkgs; project = self; };
      # MPI transport settings are machine configuration, recorded per job as mpi_config (experiments/mpirun.py).
      mpiConfigData = { columns = { source = "str"; text = "str"; }; };
      runData = {
        columns = { program = "str"; repeat = "int"; ok = "bool"; verdict = "str"; seconds = "float"; stdout_sha256 = "str"; };
        key = [ "program" "repeat" ];
        noisy.seconds = 1.0;
      };
      transportData = {
        columns = { check = "str"; workers = "int"; seed = "int"; repeat = "int"; ok = "bool"; seconds = "float"; };
        key = [ "check" "workers" "seed" "repeat" ];
        noisy.seconds = 1.0;
      };
      scaleCols = { bench = "str"; impl = "str"; nodes = "int"; workers = "int"; per_node = "int"; };
      scaleData = {
        barrier = { columns = scaleCols // { worker = "int"; rounds = "int"; ns = "int"; }; noisy.ns = 1.0; };
        ring = { columns = scaleCols // { worker = "int"; steps = "int"; size = "int"; cap = "int?"; ns = "int"; }; noisy.ns = 1.0; };
        idle = { columns = scaleCols // { worker = "int"; seconds = "int"; ns = "int"; }; noisy.ns = 1.0; };
        fanin = { columns = scaleCols // { worker = "int"; msgs = "int"; ns = "int"; }; noisy.ns = 1.0; };
        graph = {
          columns = scaleCols // { worker = "int"; steps = "int"; size = "int"; degree = "int"; iters = "int"; ns = "int"; compute_ns = "int"; checksum = "str"; };
          noisy = { ns = 1.0; compute_ns = 1.0; };
        };
        cpu = {
          columns = scaleCols // { rank = "int"; host = "str"; user_s = "float"; sys_s = "float"; wall_s = "float"; };
          noisy = { user_s = 1.0; sys_s = 1.0; wall_s = 1.0; };
        };
      };
      scaleExperiment = { name, nodes, span, resources }: labLib.experiment {
        inherit name span resources;
        closures.${system} = scaleClosure;
        params = { inherit nodes; rounds = 10000; steps = 10000; msgs = 10000; seconds = 3; cap = 8; graph_iters = 100000; graph_steps = 2000; degree = 8; seed = 1; };
        sweep = { bench = [ "barrier" "ring64" "ring4096" "idle" "fanin" "graph64" "graph1024" "graph1024s" ]; impl = [ "trame" "mpi" ]; fill = [ "half" "full" ]; };
        replicates = 3;
        data = scaleData // { mpi_config = mpiConfigData; };
      };
      experiments = {
        programs-mpi = labLib.experiment {
          name = "programs-mpi";
          closures.${system} = programsMpiClosure;
          params = { backend = "mpi"; repeats = 3; };
          sweep.node_ranks = [ 0 1 2 ];
          resources = { provider = "local"; hosts = 1; };
          data.run = runData;
          data.mpi_config = mpiConfigData;
        };
        transport-mpi = labLib.experiment {
          name = "transport-mpi";
          closures.${system} = transportClosure;
          sweep.node_ranks = [ 0 1 2 ];
          resources = { provider = "local"; hosts = 1; };
          data.check = transportData;
          data.mpi_config = mpiConfigData;
        };
        programs-cluster = labLib.experiment {
          name = "programs-cluster";
          span = 2;
          closures.${system} = programsMpiClosure;
          params = { backend = "mpi"; repeats = 3; node_ranks = 0; };
          resources = { provider = "cluster"; hosts = 2; walltime = 60; opts.stage = "fanout"; };
          data.run = runData;
          data.mpi_config = mpiConfigData;
        };
        transport-cluster = labLib.experiment {
          name = "transport-cluster";
          span = 2;
          closures.${system} = transportClosure;
          params = { node_ranks = 0; };
          resources = { provider = "cluster"; hosts = 2; walltime = 60; opts.stage = "fanout"; };
          data.check = transportData;
          data.mpi_config = mpiConfigData;
        };
        scale-local = scaleExperiment {
          name = "scale-local";
          nodes = 1;
          span = 1;
          resources = { provider = "local"; hosts = 1; };
        };
      } // builtins.listToAttrs (map (n: {
        name = "scale-n${toString n}";
        value = scaleExperiment {
          name = "scale-n${toString n}";
          nodes = n;
          span = n;
          resources = { provider = "cluster"; hosts = n; walltime = 60; opts.stage = "fanout"; };
        };
      }) [ 1 2 4 8 16 32 ]) // import ./experiments/checks.nix {
        inherit pkgs system labLib trameSeqBinsWith trameMpiBinsWith runners labRuntime nixsci;
        lib = pkgs.lib;
      };
      # The deterministic checks: nixsci validate verdicts (experiments/validate.nix).
      v = nixsci.lib.validate { inherit pkgs; };
      suite = v.suite (import ./experiments/validate.nix {
        inherit pkgs v trameSeqBins seqPrograms driverToolchain trameDriver;
        trameSrc = trameMpiSrc;
        rejectionsSrc = rejectionsSrc;
      });
      mkBackendShell = backend: extra:
        let
          dependencies = import (./backends + "/${backend}/dependencies.nix") { inherit pkgs; };
        in
        pkgs.mkShell {
          packages = [ pkgs.rustc pkgs.cargo pkgs.clippy pkgs.rustfmt ] ++ dependencies.packages ++ extra;
          shellHook = builtins.concatStringsSep "\n" (
            builtins.attrValues (builtins.mapAttrs (name: value: "export ${name}=${value}") dependencies.env)
          );
        };
    in {
      validate.${system} = suite;
      # The verdicts, and one style for the Rust sources (rustfmt.toml). rejections/ is left out: its cases
      # carry line-anchored annotations.
      checks.${system} = suite.checks // {
        fmt = pkgs.runCommand "trame-fmt" { nativeBuildInputs = [ pkgs.rustfmt pkgs.findutils ]; } ''
          cd ${fmtSrc}
          find . -name '*.rs' -print0 | xargs -0 rustfmt --check --edition 2024 --config-path rustfmt.toml
          touch $out
        '';
      };
      packages.${system} = {
        trame-driver = trameDriver;
        trame-mpi-bins = trameMpiBins;
        trame-seq-bins = trameSeqBins;
      };
      lab.${system}.experiments = experiments;
      apps.${system} = builtins.mapAttrs (_: e: e.run) experiments;
      devShells.${system} = {
        default = pkgs.mkShell { packages = [ pkgs.rustc pkgs.cargo pkgs.clippy pkgs.rustfmt ]; };
        # GNU time: experiments/scale.py measures per-rank CPU with it.
        mpi = mkBackendShell "mpi" [ pkgs.time ];
        # The pinned nightly, for building driver/ with `cargo` (see driver/README.md). The shell
        # does not depend on the package, so entering it does not build the driver.
        driver = driverPkgs.mkShell {
          packages = [ driverToolchain ];
        };
      };
    };
}
