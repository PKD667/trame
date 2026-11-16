# The transport's own checks (tests/run.sh) on one host, through mpirun. One local run.
{ lib, labLib, system, closure, b, data, ... }:
{
  name = "transport";
  figures = [ "transport-checks" ];
  runs = [{
    name = "transport-mpi";
    kind = "transport";
    backend = "mpi";
    span = 0;
    exp = labLib.experiment {
      name = "transport-mpi";
      closures.${system} = closure {
        name = "transport-mpi"; inherit b; script = ./run.py;
        path = [ (lib.fileset.toSource { root = ../lib; fileset = ../lib; }) ];
      };
      resources = { provider = "local"; hosts = 1; };
      data.check = {
        columns = { check = "str"; workers = "int"; case_seed = "int"; repeat = "int"; ran = "bool"; ok = "bool"; seconds = "float"; };
        key = [ "check" "workers" "case_seed" "repeat" ];
        noisy.seconds = 1.0;
      };
      data.mpi_config = data.dataset "mpi_config";
    };
  }];
  analysis = ./analysis.py;
}
