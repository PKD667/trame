{ bench, ... }:
{
  name = "idle";
  figures = [ "idle-cpu" ];
  runs = bench.runs {
    kind = "idle"; script = ./run.py; bench = "idle"; spans = [ 1 ]; replicates = 3;
    params = { seconds = 3; timeout = 300; };
    sweep = { per = [ 1 4 16 ]; };
  };
  analysis = ./analysis.py;
}
