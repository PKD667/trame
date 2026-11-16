# The SIMD benchmark of the lab (lab/simd/, lab/simd_bench.py, lab/analysis/simd_process.py). Called from flake.nix.
# Its experiments are not the suite's (`suite = false`): each runs on one host of a CPU type, through its own
# provider (lab/README.md, Hosts), and measures the lane kernels of lab/simd/ against the scalar reference
# trame::__rt::parallel. Answers { experiments, bin, analysis, figures }.
#
# The analysis is a draft (nixsci's `draft`): the locked runs were measured by earlier code, so it is not clean and
# its outputs carry `-nixsci-draft` in their name. Remove `draft` only after the runs are measured again.
{ pkgs, lib, labLib, system, nixsci, runners, labPy, trameFiles }:
let
  # The benchmark binary: the crate of lab/simd/ with trame as a path dependency, so the source is trame's files plus
  # that directory. Built from the subdirectory (its own Cargo.lock and workspace).
  bin = pkgs.rustPlatform.buildRustPackage {
    pname = "trame-simd-bench";
    version = "0.1.0";
    src = lib.fileset.toSource {
      root = ../../.;
      fileset = lib.fileset.unions [
        trameFiles
        (lib.fileset.difference ../../lab/simd (lib.fileset.maybeMissing ../../lab/simd/target))
      ];
    };
    sourceRoot = "source/lab/simd";
    cargoLock.lockFile = ../../lab/simd/Cargo.lock;
    RUSTFLAGS = "-C overflow-checks=on";
    cargoBuildFlags = [ "--bin" "bench" ];
    doCheck = false;
  };

  # One job: the runner on one host, through its provider's lease. The runner reads BIN and RUSTC_VERSION.
  closure = nixsci.lib.mkExperiment pkgs {
    name = "simd-bench";
    program = "${pkgs.python3}/bin/python3";
    args = [ "${runners}/simd_bench.py" ];
    env = {
      PYTHONPATH = "${runners}:${labPy}";
      BIN = "${bin}/bin";
      RUSTC_VERSION = pkgs.rustc.version;
    };
    runtime = [ pkgs.coreutils pkgs.procps pkgs.util-linux pkgs.gnugrep pkgs.python3 ];
  };

  # What the runner records (simd_bench.py): one row per case of the benchmark, and one machine row per job.
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

  experiment = name: cpuType: provider: labLib.experiment {
    inherit name;
    closures.${system} = closure;
    params = { cpu_type = cpuType; };
    resources = { inherit provider; hosts = 1; walltime = 1800; };
    replicates = 3;
    data = { bench = benchData; machine = machineData; };
  };

  experiments = {
    "simd-bench-zen1" = experiment "simd-bench-zen1" "zen1" "simd-zen1";
    "simd-bench-avx512" = experiment "simd-bench-avx512" "intel-avx512" "simd-avx512";
  };

  analysis = labLib.analysis {
    name = "simd-analysis";
    draft = true;
    use = {
      zen1 = experiments."simd-bench-zen1";
      avx512 = experiments."simd-bench-avx512";
    };
    pipelines.process = {
      script = ../../lab/analysis/simd_process.py;
      packages = [ pkgs.python3Packages.matplotlib pkgs.python3Packages.pandas ];
    };
  };

  # A combined sheet of the figures, with the TSVs and PDFs of the analysis beside it.
  figures = pkgs.runCommand "simd-figures" {
    nativeBuildInputs = [ pkgs.texlive.combined.scheme-basic pkgs.ghostscript ];
  } ''
    mkdir -p $out
    cp ${analysis}/*.tsv $out/ || true
    cp ${analysis}/*.pdf $out/ || true
    cp ${analysis}/*.png $out/ || true

    cat > $out/simd_figures_sheet.tex <<'LATEX'
\documentclass[a4paper]{article}
\usepackage[margin=1cm]{geometry}
\usepackage{graphicx}

\begin{document}

\section*{SIMD Benchmark Figures}

\subsection*{Figure 1: Speedup at 1M items}
\includegraphics[width=0.9\textwidth]{simd_speedup.pdf}

\newpage

\subsection*{Figure 2: Throughput vs Item Count (log scale)}
\includegraphics[width=0.9\textwidth]{simd_throughput.pdf}

\end{document}
LATEX

    if command -v pdflatex > /dev/null 2>&1; then
      cd $out
      pdflatex -interaction=nonstopmode simd_figures_sheet.tex 2>&1 | grep -E "(! |Output written|Error)" || true
      if [ -f simd_figures_sheet.pdf ]; then
        echo "Combined sheet PDF created successfully"
      fi
    fi
  '';
in
{
  inherit bin analysis figures;
  # The catalogue's items (lab/perf.nix, lab/checks.nix): name and experiment; not in the suite.
  experiments = lib.mapAttrsToList (name: exp: { inherit name exp; suite = false; }) experiments;
}
