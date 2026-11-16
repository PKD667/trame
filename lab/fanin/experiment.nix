{ bench, ... }:
{
  name = "fanin";
  figures = [ "fanin" ];
  runs = bench.runs {
    kind = "fanin"; script = ./run.py; bench = "fanin"; spans = [ 1 2 ]; replicates = 3;
    params = { msgs = 10000; timeout = 300; };
    sweep = { per = [ 4 16 ]; };
  };
  analysis = ./analysis.py;
}
