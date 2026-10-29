Cross-node is untested: every result here is one node, MIST, Open MPI 5.0.11, two to six ranks sharing memory.

# MPI transport: results (single node)

Machine: MIST, AMD Ryzen 5 PRO 4650U (12 threads), Linux 7.2.7, x86_64, 14 GB. Open MPI 5.0.11
from the `mpi` Nix shell. Rust 1.98.1. The checks run the debug build (`cargo build --bins`); the
latency record is a release build. `mpirun --oversubscribe`.
Load average during runs: 2 to 3. The machine is shared, so timings vary between runs (see below).

Command: `OUT=<dir> backends/mpi/tests/run.sh` inside `nix develop .#mpi`. Last full run: exit 0,
1 min 19 s, on the final tree. Every check below is in that run.

## Checks

| check | command | what it shows | result |
|---|---|---|---|
| FIFO per route | `check fifo --seed S` then `check verify DIR`, n = 3, 4, 6, S = 1..5 | seeded interleavings; each route in FIFO order; every Full has exactly `cap` outstanding | 15 of 15 pass |
| Fair receive | `check fair --msgs 16`, n = 4 | two routes pre-filled; the rotating receive takes them in turns | alternates 0,1,0,1,... pass |
| Overwrite | `check overwrite --seed 7 --cap 4 --msgs 20000` | lossy route never answers Full; receiver sees increasing frames, newest kept | 19940 to 19995 of 20000 seen (varies by run), newest 19999 |
| Overwrite, late | `... --late` | receiver starts after the sender finished; it reads the newest `cap` frames | `[19996 .. 19999]` |
| Barrier + publish | `check barrier --seed 3 --rounds 300`, 5 workers + leader (n = 6) | 300 rounds; every worker sees every publication | pass (15 reruns, all pass) |
| Deadlock | `check deadlock`, 4 workers + leader (n = 5) | one worker returns early; the others get `Deadlock`; no hang at finalization | pass (8 reruns, all pass) |
| Stability | 25 reruns of `fifo --seed 4`, n = 6 | the case that failed once before the fence change | 25 of 25 pass |

The FIFO verifier reads the per-rank logs written by the test and checks the order of each
route. It does not read the transport's internal state.

### Defects found and fixed during this work

- Barrier hang: rank 0 took the non-leader branch and waited for a release that nothing stored.
  That design was replaced: the leader is now the last rank and the barrier is rootless (`gate.rs`).
  The barrier run passes; the old design timed out at 120 s.
- Verify failure, fifo n = 6 seed = 4 (one run): the checker saw a consume that finished before
  a send that still read `C` as its old value. The trace stamps do not prove visibility. Fix:
  `SeqCst` fences on the traced path only (`ring.rs`). The old code was not rebuilt to confirm
  this was the cause. That case has passed every run since (25 reruns in a row, then the matrix and full runs).
- Receive ordering: `recv` used a per-frame stamp and an oldest-offer search across routes, with a
  retry when a frame was overtaken. Removed. Frames are FIFO per route only; `recv` takes the head of
  the first owned route with a frame, scanning from a rotating start. Cross-route order is not a
  guarantee.
- Barrier spin: the barrier polled with `sched_yield`. Replaced by `FUTEX_WAIT` on GEN. Checked with
  `strace -f`: the barrier run makes 661 futex calls and no `sched_yield` from trame frames (the
  15,373 `sched_yield` calls are UCX's `ucm` at MPI startup).

## Latency

`latency` writes raw round trips, one JSON line each, in nixsci record format (`kind: value`, name
`rtt`): `ns` is A's round trip on A's clock (CLOCK_MONOTONIC), `bytes` is `size_of::<Probe>()`,
plus the drawn and measured gap before the send, the seed and `seq`. No verdict is computed here:
an importer states its claim over the records with nixsci.stats.

No record is kept in this tree. The first records came from unpinned runs on a loaded laptop, and
identical runs of the same binary had medians about 2.4 times apart. Placement is the suspected
cause, so a kept record must state where each rank ran.

## Not checked

- Several real nodes. Multi-node runs are tested on one machine split into nodes (NODES.md,
  `TRAME_MPI_NODE_RANKS`); runs on separate machines are lab experiments (experiments/).
- Other MPI implementations and other CPUs. Timings are on this machine only.
- The traced build adds SeqCst fences, so trace checks exercise a more strongly ordered variant than production, which relies on the standard SPSC acquire/release and seqlock (Boehm 2012) argument.
