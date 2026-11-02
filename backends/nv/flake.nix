{
  description = "NVIDIA backend spike: the one-warp GPU prototype as a nixsci lab experiment (PLAN.md, gate D)";

  # Its own flake, not trame's root flake: the root pins what trame's own checks need, and this
  # experiment needs a PTX built outside Nix (gpu/README.md) and its own lab.lock. The flake source
  # is backends/nv, so programs/ (the expected output) is in it.
  # nixpkgs follows nixsci's lock: nixsci asserts pkgsStatic.nix is 2.34.8 (deploy/nix-bootstrap.nix),
  # and an unpinned nixos-unstable had moved to 2.34.9 (run failed at eval, /tmp/run1.log).
  inputs.nixpkgs.follows = "nixsci/nixpkgs";
  # The same nixsci revision trame's flake.lock records (fff7977), pinned here by revision.
  inputs.nixsci.url = "github:PKD667/nixsci/fff797708f5d07be224b78c35665b33432a305b9";

  outputs = { self, nixpkgs, nixsci, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      # The host harness: rustc only. The CUDA driver is found at run time (gpu/host/main.rs).
      host = pkgs.stdenv.mkDerivation {
        pname = "nv-post-parallel-host";
        version = "0.0.0";
        src = ./gpu/host;
        nativeBuildInputs = [ pkgs.rustc ];
        buildPhase = ''
          runHook preBuild
          rustc --edition 2024 -O -o nv-post-parallel-host main.rs
          runHook postBuild
        '';
        installPhase = ''
          runHook preInstall
          install -Dm755 nv-post-parallel-host $out/bin/nv-post-parallel-host
          runHook postInstall
        '';
      };
      # The device code, as PTX: built by cargo-oxide from gpu/src/main.rs, outside Nix (gpu/README.md).
      # Its hash is recorded in every row, so a run names the PTX it loaded.
      ptx = ./gpu/nv_post_parallel_gpu.ptx;
      expected = ./programs/post_parallel_pod.expected;
      # nixsci's driver.sh: binds the host's libcuda.so.1 to one command (gpu/driver.sh in nixsci).
      driver = (nixsci.lib.gpuProbe pkgs).driver;
      gpuClosure = nixsci.lib.mkExperiment pkgs {
        name = "post-parallel-gpu";
        program = "${pkgs.python3}/bin/python3";
        args = [ "${./gpu/experiment.py}" ];
        env = {
          PYTHONPATH = "${nixsci.packages.${system}.lab-py}/${pkgs.python3.sitePackages}";
          HOST = "${host}/bin/nv-post-parallel-host";
          PTX = "${ptx}";
          EXPECTED = "${expected}";
          DRIVER = "${driver}/bin/nixsci-gpu-driver";
        };
        runtime = [ pkgs.coreutils ];
        # a target must declare driver = "nvidia" to take this job (nixsci deploy Backend._admit)
        resources.driver = "nvidia";
        metadata.post-parallel-gpu = { ptx_sha256 = builtins.hashFile "sha256" ptx; };
      };
      labLib = nixsci.lib.lab { inherit pkgs; project = self; };
      experiments = {
        post-parallel-gpu = labLib.experiment {
          name = "post-parallel-gpu";
          closures.${system} = gpuClosure;
          # three finished runs: three fresh processes, each with its own driver context
          replicates = 3;
          # provider gruss3 (~/.config/nixsci/providers.json): a targets file that declares driver = "nvidia"
          resources = { provider = "gruss3"; hosts = 1; };
          data.run = {
            columns = {
              ran = "bool"; ok = "bool"; verdict = "str"; device = "str"; compute_capability = "str";
              driver_version = "int?"; ptx_sha256 = "str"; stdout_sha256 = "str"; seq_sha256 = "str";
              exit_code = "int";
            };
          };
          data.smi = {
            columns = { ran = "bool"; name = "str"; driver_version = "str"; compute_cap = "str"; memory_total = "str"; };
          };
        };
      };
    in
    {
      packages.${system} = {
        inherit host gpuClosure;
      };
      lab.${system}.experiments = experiments;
      apps.${system} = builtins.mapAttrs (_: e: e.run) experiments;
    };
}
