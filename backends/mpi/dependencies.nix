{ pkgs }:
let
  mpi = pkgs.openmpi;
  libclang = pkgs.llvmPackages.libclang;
in
{
  packages = [
    mpi
    pkgs.pkg-config
    pkgs.llvmPackages.clang
    libclang
  ];
  env = {
    MPICC = "${mpi}/bin/mpicc";
    LIBCLANG_PATH = "${libclang.lib}/lib";
  };
}
