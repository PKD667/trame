# Results of `run.sh` (single node)

Machine: MIST, AMD Ryzen 5 PRO 4650U (12 threads), Linux 7.2.7, x86_64, 14 GB. Open MPI 5.0.11
from the `mpi` Nix shell. Rust 1.98.1. The checks run the debug build (`cargo build --bins`) under `mpirun --oversubscribe`.
Load average during runs: 2 to 3. The machine is shared, so timings vary between runs (see below).

Command: `OUT=<dir> src/backends/mpi/tests/run.sh` inside `nix develop .#mpi`. Every check below is in that run except Stability, which is a loop over `fifo --seed 4`.

## Checks

| check | command | what it shows | result |
|---|---|---|---|
| FIFO per route | `check fifo --seed S` then `check verify DIR`, n = 3, 4, 6, S = 1..5 | seeded interleavings; each route in FIFO order; every Full has exactly `cap` outstanding | 15 of 15 pass |
| Fair receive | `check fair --msgs 16`, n = 4 | two routes pre-filled; the rotating receive takes them in turns | alternates 0,1,0,1,... pass |
| Overwrite | `check overwrite --seed 7 --cap 4 --msgs 20000` | lossy route never answers Full; receiver sees increasing frames, newest kept | 19940 to 19995 of 20000 seen (varies by run), newest 19999 |
| Overwrite, late | `... --late` | receiver starts after the sender finished; it reads the newest `cap` frames | `[19996 .. 19999]` |
| Stability | 25 reruns of `fifo --seed 4`, n = 6 | one seed, repeated | 25 of 25 pass |

The FIFO verifier reads the per-rank logs written by the test and checks the order of each
route. It does not read the transport's internal state.

## Latency

`check` has no latency subcommand. Round trips are measured by `lab/bench/ping.rs`, which runs on every backend.
Unpinned runs on a loaded laptop gave medians about 2.4 times apart for the same binary, so a kept record must
state where each rank ran. No record is kept in this tree.

What these runs do not cover is in `README.md`, Not checked.
