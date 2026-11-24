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
in
{
  inherit suite;
  checks = suite.checks // {
    fmt = pkgs.runCommand "trame-fmt" { nativeBuildInputs = [ pkgs.rustfmt pkgs.findutils ]; } ''
      cd ${tree.fmtSrc}
      find . -name '*.rs' -print0 | xargs -0 rustfmt --check --edition 2024 --config-path rustfmt.toml
      touch $out
    '';
  };
}
