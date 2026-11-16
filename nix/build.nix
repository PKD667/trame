# One build of every backend and its launch. `backends.<name>`: d (the descriptor), src, binsWith patches (a build with
# patches applied, a mutant), bins (without), launch, programs, mutants (a directory or null).
{ pkgs, nixsci, system, tree }:
let
  inherit (tree) lib backendDir descriptors trameSrc programNames benchNames;
  labPy = "${nixsci.packages.${system}.lab-py}/${pkgs.python3.sitePackages}";

  # The programs and benchmarks and the bins its descriptor lists, built with the feature of the backend.
  # Layout: programs/, bench/, bin/, share/expected/.
  binsFor = name: d: patches: pkgs.rustPlatform.buildRustPackage {
    pname = "trame-${name}-bins";
    version = "0.1.0";
    src = trameSrc;
    inherit patches;
    # The patches of src/backends/<b>/mutants are written against src/.
    patchFlags = [ "-p1" "-d" "src" ];
    cargoLock.lockFile = ../Cargo.lock;
    buildNoDefaultFeatures = true;
    buildFeatures = [ name ];
    # Every program is built with the checked cfg (check/programs/run.sh).
    RUSTFLAGS = "--cfg trame_checked";
    nativeBuildInputs = d.packages;
    inherit (d) env;
    cargoBuildFlags = [ "--examples" ] ++ lib.concatMap (b: [ "--bin" b ]) d.bins;
    # Tests run as checks (nix/validate.nix), not in the build.
    doCheck = false;
    inherit (d) postInstall;
    installPhase = ''
      runHook preInstall
      mkdir -p $out/bin $out/programs $out/bench $out/share/expected
      rel=target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release
      for p in ${lib.concatStringsSep " " programNames}; do cp $rel/examples/$p $out/programs/; done
      for p in ${lib.concatStringsSep " " benchNames}; do cp $rel/examples/$p $out/bench/; done
      for b in ${lib.concatStringsSep " " d.bins}; do cp $rel/$b $out/bin/; done
      cp check/programs/*.expected $out/share/expected/
      runHook postInstall
    '';
  };

  # A backend's launch with what it needs on its PATH and the nixsci library, so it runs from anywhere.
  launchFor = name: d: pkgs.runCommand "launch-${name}" { nativeBuildInputs = [ pkgs.makeWrapper pkgs.python3 pkgs.bash ]; } ''
    install -Dm755 ${backendDir + "/${name}/launch"} $out/bin/launch
    patchShebangs $out/bin/launch
    wrapProgram $out/bin/launch --prefix PATH : ${lib.makeBinPath d.launchRuntime} --prefix PYTHONPATH : ${labPy}
  '';
in
{
  inherit labPy;
  backends = lib.mapAttrs (n: d: rec {
    inherit d;
    src = trameSrc;
    binsWith = binsFor n d;
    bins = binsWith [ ];
    launch = launchFor n d;
    programs = programNames;
    mutants = let dir = backendDir + "/${n}/mutants"; in if builtins.pathExists dir then dir else null;
  }) descriptors;
}
