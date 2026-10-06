{
  description = "Backend-local trame development environments";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      trameVerify = pkgs.rustPlatform.buildRustPackage {
        pname = "trame-verify";
        version = "0.1.0";
        src = ./verify;
        cargoLock.lockFile = ./verify/Cargo.lock;
        nativeBuildInputs = [ pkgs.stdenv.cc ];
      };
      mkBackendShell = backend:
        let
          dependencies = import (./backends + "/${backend}/dependencies.nix") { inherit pkgs; };
        in
        pkgs.mkShell {
          packages = [ pkgs.rustc pkgs.cargo trameVerify ] ++ dependencies.packages;
          shellHook = builtins.concatStringsSep "\n" (
            builtins.attrValues (builtins.mapAttrs (name: value: "export ${name}=${value}") dependencies.env)
          );
        };
    in {
      devShells.${system} = {
        default = mkBackendShell "none";
        none = mkBackendShell "none";
        mpi = mkBackendShell "mpi";
        nv = mkBackendShell "nv";
      };
    };
}
