{
  description = "trame-fuzz: fuzzer for the trame compile-time driver (driver/fuzz/README.md)";

  # Same nixpkgs as the repository's flake.nix; pinned by this directory's flake.lock.
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      fuzz = pkgs.rustPlatform.buildRustPackage {
        pname = "trame-fuzz";
        version = "0.1.0";
        # Only the sources: work/, found/ and target/ are not inputs.
        src = pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions [ ./src ./Cargo.toml ./Cargo.lock ];
        };
        cargoLock.lockFile = ./Cargo.lock;
        # The unit tests run here: a failing unit test fails the build.
        doCheck = true;
      };
    in {
      packages.${system} = {
        trame-fuzz = fuzz;
        default = fuzz;
      };
    };
}
