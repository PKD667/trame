{
  description = "Backend-local trame development environments";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  # Nightly toolchains by date, for the driver only (see crates/driver/rust-toolchain.toml).
  inputs.rust-overlay = {
    url = "github:oxalica/rust-overlay";
    inputs.nixpkgs.follows = "nixpkgs";
  };
  # The lab experiments (nix/lab.nix, lab/README.md) and their deployment to hosts. Pinned; nixsci's only input is nixpkgs.
  # nixsci main (health check, target-side cache fetch, --hosts).
  inputs.nixsci = {
    url = "github:PKD667/nixsci/main";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, rust-overlay, nixsci, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      lib = pkgs.lib;
      # Each piece is a file of nix/ and takes what it reads; nothing here names a backend.
      tc = import ./nix/toolchain.nix { inherit nixpkgs rust-overlay system; };
      tree = import ./nix/tree.nix { inherit pkgs; };
      build = import ./nix/build.nix { inherit pkgs nixsci system tree; };
      lab = import ./nix/lab.nix { inherit pkgs nixsci system self tree build; };
      verdicts = import ./nix/checks.nix { inherit pkgs nixsci tree build tc; };
      shells = import ./nix/shells.nix { inherit pkgs nixsci tree tc; };
      cli = import ./nix/cli.nix { inherit pkgs build; inherit (tc) driverToolchain trameDriver; };
    in {
      validate.${system} = verdicts.suite;
      checks.${system} = verdicts.checks // {
        lab-contract = lab.contract;
        lab-python = lab.python;
        no-backend-names = import ./nix/names.nix { inherit pkgs self; inherit (tree) backendNames; };
      };
      packages.${system} = {
        trame = cli;
        trame-driver = tc.trameDriver;
      } // lab.packages // lib.concatMapAttrs (n: b: {
        "trame-${n}-bins" = b.bins;
        "launch-${n}" = b.launch;
      }) build.backends;
      # An analysis is a derivation over runs in lab.lock, which a fresh clone may not have, and `nix flake check`
      # evaluates packages: so the analyses are not under `packages` (nixsci's convention). One per experiment, named
      # for it, and `report` over all of them; `draft.<name>` takes the runs that exist.
      legacyPackages.${system} = lab.analyses // { draft = lab.drafts; };
      lab.${system} = {
        experiments = lab.runs;
        # The suite's jobs per backend: experiment, kind, machines of a job, and its jobs (sweep x seeds x replicates).
        suiteMatrix = lab.suite.matrix;
      };
      deployments.${system} = shells.deployments;
      apps.${system} = builtins.mapAttrs (_: e: e.run) lab.runs // {
        trame = { type = "app"; program = "${cli}/bin/trame"; };
        # Every experiment of the suite, for every backend (nix/lab/suite.nix), one after another.
        suite = lab.suite.app;
      };
      devShells.${system} = shells.devShells;
    };
}
