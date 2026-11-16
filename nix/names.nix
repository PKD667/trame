# The check `no-backend-names`: outside src/backends/ nothing names a backend (src/backends/README.md). The names are the
# directories of src/backends/ that have a backend.nix. A name is searched as a word, case-insensitively, except the ones
# that are also ordinary identifiers (`seq` is a field and a loop variable): those only as a path, `<name> backend`, or
# a feature. The exceptions are the files that must say it: Cargo.toml's feature list and the manifests that select a
# feature, the locks, the submodule list, and the proof scripts (check/proofs/README.md).
{ pkgs, self, backendNames }:
let
  inherit (pkgs) lib;
  ordinary = [ "seq" ];
  words = lib.filter (n: !lib.elem n ordinary) backendNames;
  alt = names: lib.concatStringsSep "|" names;
  structural = "backends/(${alt backendNames})|(${alt backendNames}) backend|--features[ =]\"?(${alt backendNames})|features = \\[\"(${alt backendNames})\"|feature = \"(${alt backendNames})\"";
  allowed = [ "src/backends" "check/proofs" "lab/simd" "Cargo.toml" "Cargo.lock" "flake.lock" "lab.lock" ".gitmodules" "nix/names.nix" ];
in
pkgs.runCommand "no-backend-names" { src = self; nativeBuildInputs = [ pkgs.gnugrep ]; } ''
  cd $src
  hits=$(grep -rniIE '(^|[^[:alnum:]_])(${alt words})([^[:alnum:]_]|$)|${structural}' . | grep -v -E '^\./(${lib.concatStringsSep "|" (map lib.escapeRegex allowed)})' || true)
  if [ -n "$hits" ]; then echo "backend names outside src/backends/:"; echo "$hits"; exit 1; fi
  touch $out
''
