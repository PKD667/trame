# The datasets the lab's benchmark runners record, from lab/columns.json: the columns of each, with the job's common
# columns, and the columns that may differ between replicates (the clocks'). One declaration for lab/perf.py and for
# a backend's own runners that record beside it (src/backends/<b>/experiments.nix).
{ lib }:
let
  cols = builtins.fromJSON (builtins.readFile ../../lab/columns.json);
  noisy = { ns = 1.0; compute_ns = 1.0; user_s = 1.0; sys_s = 1.0; wall_s = 1.0; value = 1.0; };
  dataset = name: {
    columns = (if name == "launch_config" || name == "mpi_config" then { } else cols.common) // cols.${name};
    noisy = lib.filterAttrs (c: _: cols.${name} ? ${c}) noisy;
  };
in
{
  inherit cols;
  # The datasets of one job of a benchmark: its own, the ranks' CPU time and what the launch ran under.
  dataOf = bench: {
    ${bench} = dataset bench;
    cpu = dataset "cpu";
    launch_config = dataset "launch_config";
  };
  dataset = dataset;
}
