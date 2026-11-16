{ bench, ... }:
{
  name = "round";
  figures = [ "round" ];
  runs = bench.runs {
    kind = "round"; script = ./run.py; bench = "round"; spans = [ 1 2 4 ]; replicates = 3;
    params = { rounds = 10000; timeout = 300; };
    sweep = { per = [ 4 16 ]; };
  };
  analysis = ./analysis.py;
}
