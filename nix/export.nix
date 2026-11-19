# What a program's own flake takes from trame (src/backends/README.md, The export): `lib.<system>.backend NAME`, NAME
# being a backend's own name. A program names a backend by that string and nothing else: its cargo feature, its build
# inputs, its environment, its runtime and its launcher come from here, from the backend's descriptor.
{ pkgs, tree, build }:
let
  inherit (tree) lib backendDir descriptors;
  known = builtins.attrNames descriptors;

  # A derivation with bin/NAME that serves `binary` on the backend: the backend's `serve`, with its runtime on PATH.
  launcherFor = backend: d: { name, binary }: pkgs.runCommand name { nativeBuildInputs = [ pkgs.makeWrapper pkgs.python3 pkgs.bash ]; } ''
    install -Dm755 ${backendDir + "/${backend}/serve"} $out/libexec/serve
    patchShebangs $out/libexec/serve
    makeWrapper $out/libexec/serve $out/bin/${name} \
      --prefix PATH : ${lib.makeBinPath d.launchRuntime} --prefix PYTHONPATH : ${build.labPy} \
      --add-flags "-- ${binary}"
  '';
in
backend: { checked ? true }:
  if !(builtins.elem backend known) then
    throw "trame: no backend `${backend}`; the backends are: ${lib.concatStringsSep ", " known}"
  else
    let d = descriptors.${backend}; in {
      # The cargo features to build with, on the program's package: `buildNoDefaultFeatures = true` and these.
      # `trame/<name>` names the feature of the dependency, so the program's Cargo.toml need not.
      feature = backend;
      buildFeatures = [ "trame/${backend}" ];
      buildNoDefaultFeatures = true;
      # What the build needs (nativeBuildInputs) and sets (env). A checked build compiles with `--cfg trame_checked`
      # (INTERFACE.md 3.1): without it `#[trame::state]` and `parallel!` do not compile. A program with its own
      # RUSTFLAGS appends to `env.RUSTFLAGS`, not replaces it.
      nativeBuildInputs = d.packages;
      env = d.env // lib.optionalAttrs checked { RUSTFLAGS = "--cfg trame_checked"; };
      # What a launched program needs on its PATH.
      runtime = d.launchRuntime;
      # launcher { name, binary }: bin/NAME starts `binary` in the foreground on the backend, ranks and hosts decided
      # by the backend. stdin, stdout and stderr are the program's leader's.
      launcher = launcherFor backend d;
    }
