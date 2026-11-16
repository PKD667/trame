{ bench, ... }:
{
  name = "graph";
  figures = [ "graph" ];
  runs = bench.runs {
    kind = "graph"; script = ./run.py; bench = "graph"; spans = [ 1 2 4 ]; replicates = 3;
    params = { graph_steps = 2000; size = 64; degree = 8; graph_iters = 100000; seed = 1; timeout = 600; };
    sweep = { per = [ 16 64 ]; };
  };
  analysis = ./analysis.py;
}
