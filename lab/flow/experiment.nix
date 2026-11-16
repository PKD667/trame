# Two families of one benchmark: bandwidth by frame size and route capacity (flow-bw), and aggregate bandwidth against the
# number of hosts (flow-agg). Their runs share parameter points (size 65536, cap 8), so one analysis pools them.
{ bench, ... }:
{
  name = "flow";
  figures = [ "bandwidth-size" "aggregate-hosts" ];
  runs =
    # One stream per worker: frame size by route capacity, one host and across two machines.
    bench.runs {
      kind = "flow-bw"; script = ./run.py; bench = "flow"; spans = [ 1 2 ]; replicates = 3;
      params = { pattern = "shift"; per = 4; steps = 20000; timeout = 300; };
      sweep = { size = [ 64 1024 16384 65536 ]; cap = [ 1 8 64 ]; };
    }
    # Aggregate bandwidth against the hosts: machines (the span) times the descriptor's splits.
    ++ bench.runs {
      kind = "flow-agg"; script = ./run.py; bench = "flow"; spans = [ 1 2 4 ]; replicates = 3; splits = true;
      params = { pattern = "shift"; per = 4; size = 65536; cap = 8; steps = 20000; timeout = 300; };
      sweep = { };
    };
  analysis = ./analysis.py;
}
