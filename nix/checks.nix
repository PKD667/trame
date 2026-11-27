# The deterministic checks: nixsci validate verdicts (nix/validate.nix), and one style for the Rust sources
# (rustfmt.toml). check/rejections/ is left out of the style check: its cases carry line-anchored annotations.
{ pkgs, nixsci, tree, build, tc }:
let
  v = nixsci.lib.validate { inherit pkgs; };
  suite = v.suite (import ./validate.nix {
    inherit pkgs v;
    inherit (build) backends;
    inherit (tree) rejectionsSrc;
    inherit (tree) guaranteesSrc;
    inherit (tc) driverToolchain trameDriver;
  });

  # The unsafe registry, as a ratchet: every `unsafe` site must carry a SAFETY comment naming a clause, and the tree
  # adopts the backlog it has (check/proofs/unsafe-baseline.txt) while refusing to grow it. A new site without a
  # comment, or with one that names nothing, fails the build.
  unsafeSrc = pkgs.lib.fileset.toSource {
    root = ../.;
    fileset = pkgs.lib.fileset.unions [
      ../CONTRACT.md
      ../PROOFS.md
      ../check/proofs/unsafe-baseline.txt
      ../check/proofs/unsafe-registry.sh
      (pkgs.lib.fileset.maybeMissing ../.gitmodules)
      (pkgs.lib.fileset.difference (pkgs.lib.fileset.fileFilter (f: f.hasExt "rs") ../.)
        (pkgs.lib.fileset.maybeMissing ../check/rejections))
    ];
  };
  unsafe-registry = pkgs.runCommand "trame-unsafe-registry" { nativeBuildInputs = [ pkgs.bash pkgs.gnugrep pkgs.gnused pkgs.coreutils pkgs.findutils ]; } ''
    UNSAFE_BASELINE=${unsafeSrc}/check/proofs/unsafe-baseline.txt \
      sh ${unsafeSrc}/check/proofs/unsafe-registry.sh ${unsafeSrc} > "$TMPDIR/log" 2>&1 || {
        mkdir -p $out/counterexamples; cp "$TMPDIR/log" $out/counterexamples/log.txt; exit 1; }
    mkdir -p $out; cp "$TMPDIR/log" $out/log.txt
  '';
in
{
  inherit suite;
  checks = suite.checks // {
    inherit unsafe-registry;
    fmt = pkgs.runCommand "trame-fmt" { nativeBuildInputs = [ pkgs.rustfmt pkgs.findutils ]; } ''
      cd ${tree.fmtSrc}
      find . -name '*.rs' -print0 | xargs -0 rustfmt --check --edition 2024 --config-path rustfmt.toml
      touch $out
    '';
  };
}
