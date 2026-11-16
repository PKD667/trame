# `trame build|run` (crates/cli/trame): the backend as a flag, the program started by that backend's launch.
{ pkgs, driverToolchain, trameDriver, build }:
let
  launches = pkgs.linkFarm "trame-launches"
    (pkgs.lib.mapAttrsToList (n: b: { name = n; path = "${b.launch}/bin"; }) build.backends);
in
pkgs.runCommand "trame-cli" { nativeBuildInputs = [ pkgs.makeWrapper ]; } ''
  install -Dm755 ${../crates/cli/trame} $out/bin/trame
  patchShebangs $out/bin/trame
  wrapProgram $out/bin/trame \
    --prefix PYTHONPATH : ${build.labPy} \
    --prefix PATH : ${trameDriver}/bin \
    --set TRAME_TOOLCHAIN ${driverToolchain}/bin \
    --set TRAME_BACKENDS ${launches}
''
