# The SIMD benchmark: the lane kernels of this crate against the scalar reference trame::__rt::parallel. Each run is one host
# of a CPU type, through its own provider (lab/README.md, Hosts). Not in the suite (`suite = false`).
{ pkgs, lib, system, nixsci, labLib, labPy, trameFiles, ... }:
let
  # The benchmark binary: this crate with trame as a path dependency, so the source is trame's files plus the crate's own
  # (Cargo files and src/: not the runner, the analysis or the README). Built from the subdirectory.
  bin = pkgs.rustPlatform.buildRustPackage {
    pname = "trame-simd-bench";
    version = "0.1.0";
    src = lib.fileset.toSource {
      root = ../../.;
      fileset = lib.fileset.unions [ trameFiles ./Cargo.toml ./Cargo.lock ./src ];
    };
    sourceRoot = "source/lab/simd";
    cargoLock.lockFile = ./Cargo.lock;
    RUSTFLAGS = "-C overflow-checks=on";
    cargoBuildFlags = [ "--bin" "bench" ];
    doCheck = false;
  };

  # One job: the runner on one host, through its provider's lease. The runner reads BIN and RUSTC_VERSION.
  closure = nixsci.lib.mkExperiment pkgs {
    name = "simd-bench";
    program = "${pkgs.python3}/bin/python3";
    args = [ "${./run.py}" ];
    env = {
      PYTHONPATH = labPy;
      BIN = "${bin}/bin";
      RUSTC_VERSION = pkgs.rustc.version;
    };
    runtime = [ pkgs.coreutils pkgs.procps pkgs.util-linux pkgs.gnugrep pkgs.python3 ];
  };

  # What the runner records: one row per case of the benchmark, and one machine row per job.
  benchData = {
    columns = {
      machine = "str"; table = "str"; case = "str"; dist = "str"; slots = "int";
      variant = "str"; items = "int"; median_mitems_s = "float"; min_mitems_s = "float";
      max_mitems_s = "float"; eq = "str";
    };
    key = [ "machine" "table" "case" "dist" "slots" "variant" "items" ];
  };
  machineData = {
    columns = { hostname = "str"; model = "str"; avx2 = "bool"; avx512f = "bool"; kernel = "str"; rustc = "str"; };
  };

  run = name: cpuType: provider: {
    inherit name;
    kind = "simd";
    backend = null;
    span = 1;
    suite = false;
    exp = labLib.experiment {
      inherit name;
      closures.${system} = closure;
      params = { cpu_type = cpuType; };
      resources = { inherit provider; hosts = 1; walltime = 1800; };
      replicates = 3;
      data = { bench = benchData; machine = machineData; };
    };
  };
in
{
  name = "simd";
  figures = [ "simd-throughput" "simd-speedup" ];
  runs = [
    (run "simd-bench-zen1" "zen1" "simd-zen1")
    (run "simd-bench-avx512" "intel-avx512" "simd-avx512")
  ];
  analysis = ./analysis.py;
  packages.trame-simd-bench = bin;
}
