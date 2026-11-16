# xhost

The cross-machine transport microbenchmark (`main.rs`, a crate of its own outside the root workspace): two ranks on two
machines, one job per mode, so that a mode is one transport path. One run per kind of network, because runs on different
networks are not replicates: `mpi-cross-host` (provider `cluster`) and `mpi-cross-host-opa` (provider `cluster-opa`; what they
mean is in `~/.config/nixsci/providers.json`). Not in the suite. A job whose ranks land on one machine is refused.

Parameters: `mode` (direct, direct_single, relay, mixed_self, mixed_flag, wake_futex), `iters` (100000), `warmup` (10000),
`sizes` (8,64,1024,65536 bytes), `mca` (Open MPI settings, none), 3 replicates.

Dataset `latency`, key `mode`, `phase`, `size`: times in ns: `median_ns`, `p99_ns`, `min_ns`, `mean_ns`, `cpu0_ns`, `cpu1_ns`
(user plus system per iteration, rank 0 and 1), `sys0_ns`, `sys1_ns`. Phase `rtt`: half the round trip; phase `wake`: a local
wake (size 1; the rows of mixed_self, mixed_flag, wake_futex). Also `launch_config` (the network interfaces, devices, compiler and
Open MPI, the MCA settings, the MPI library string) and `transport` rows (the selection lines of a verbose probe run).

## Claims

None yet.

## Figures

- `xhost-latency`: per run name, mode, phase and size, the runs, the median of medians with its min and max over runs, and
  the p99. A table.

## Run

    nix run .#mpi-cross-host
    nix build .#draft.xhost
