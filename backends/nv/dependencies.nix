{ pkgs }:
{
  # CUDA device builds use the site-approved CUDA/NVVM toolchain and pinned
  # libclang described by backends/nv/build/tests/lib.sh. The host model needs
  # no backend-specific Nix package; do not replace that device environment here.
  packages = [ ];
  env = { };
}
