# The seq backend's own verdicts, added to the generic ones of lab/validate.nix through the descriptor (backend.nix,
# validate). Called with the flake's values: the source tree, the vendored crates and cargo's offline setup, the
# programs, and this backend's launch. The root crate's tests, the rejection suite and the programs are generic
# (tests-<b>, rejections-<b>, programs-<b>); what is seq's own is the mutants, which need seq's step limit.
{ pkgs, v, trameSrc, cargoHomeWith, vendor, programs, launch, ... }:
{
  # The seq mutants: every patch of ./mutants is caught or spared as its header says, with a step limit.
  mutants-seq = import ./validate-mutants.nix {
    inherit pkgs v cargoHomeWith vendor trameSrc programs launch;
    mutantsDir = ./mutants;
  };
}
