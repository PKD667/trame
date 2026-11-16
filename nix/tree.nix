# The tree as the flake reads it: the backends (every directory of src/backends/ with a backend.nix; its descriptor is
# in src/backends/README.md), the sources a build reads, and the names of the programs and benchmarks. Nothing here
# names a backend: the cargo feature of a backend is the name of its directory.
{ pkgs }:
let
  lib = pkgs.lib;
  backendDir = ../src/backends;
  backendNames = builtins.attrNames (lib.filterAttrs
    (n: t: t == "directory" && builtins.pathExists (backendDir + "/${n}/backend.nix"))
    (builtins.readDir backendDir));
  descriptors = lib.genAttrs backendNames (n: import (backendDir + "/${n}/backend.nix") { inherit pkgs; });
  # What a descriptor excludes from the sources (paths under its directory).
  excluded = lib.concatMap (n: map (e: lib.fileset.maybeMissing (backendDir + "/${n}/${e}")) descriptors.${n}.exclude) backendNames;

  # The root crate and what its builds read: never crates/driver/ or target/, and not what a descriptor excludes.
  trameFiles = lib.fileset.unions ([
      ../Cargo.toml
      ../Cargo.lock
      ../README.md
      ../INTERFACE.md
      ../crates/macros
      ../src/math
      ../src/parallel
      ../check/programs
      ../tests
      ../lab/bench
      ../examples
      (lib.fileset.difference backendDir (lib.fileset.unions excluded))
    ] ++ map (n: ../src + "/${n}") (
      builtins.filter (n: lib.hasSuffix ".rs" n) (builtins.attrNames (builtins.readDir ../src))
    ));

  # The programs of check/programs/ and the benchmarks of lab/bench/: the examples of Cargo.toml by path.
  examples = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).example;
in
{
  inherit lib backendDir backendNames descriptors trameFiles;
  trameSrc = lib.fileset.toSource { root = ../.; fileset = trameFiles; };
  # The rejection suite (check/rejections/README.md) is a separate package that depends on the root crate by path.
  rejectionsSrc = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [ trameFiles (lib.fileset.difference ../check/rejections (lib.fileset.maybeMissing ../check/rejections/target)) ];
  };
  fmtSrc = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../rustfmt.toml
      (lib.fileset.difference (lib.fileset.fileFilter (f: f.hasExt "rs") ../.) (lib.fileset.unions ([ ../check/rejections ] ++ excluded)))
    ];
  };
  programNames = map (n: lib.removeSuffix ".rs" n)
    (builtins.filter (n: lib.hasSuffix ".rs" n) (builtins.attrNames (builtins.readDir ../check/programs)));
  benchNames = map (e: e.name) (builtins.filter (e: lib.hasPrefix "lab/bench/" e.path) examples);
}
