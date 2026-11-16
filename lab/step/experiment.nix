{ bench, ... }:
{
  name = "step";
  figures = [ "step-overhead" ];
  runs = bench.runs {
    kind = "step"; script = ./run.py; bench = "step"; spans = [ 1 ]; replicates = 3;
    params = { n = 1000000; timeout = 300; };
    sweep = { per = [ 1 4 ]; k = [ 1 4 ]; };
  };
  analysis = ./analysis.py;
}
