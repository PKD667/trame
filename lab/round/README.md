# round

The time for ROUNDS rounds of messages through the leader (`lab/bench/round.rs`): the leader sends `Go` to each of W workers,
each answers `Ack`, and the next `Go` goes when all acks are in, after one warm-up round. `round-<b>-s<N>`, N in 1 2 4.

Parameters: `per` (4, 16), `rounds` (10000). The baseline is ROUNDS barrier calls of the messaging library: not equal work (W
ranks agreeing among themselves against W workers agreeing through the leader, 2W messages a round), so it shows the cost of
that choice. No claim is made against it.

Dataset `round`: `workers`, `worker`, `rounds`, `ns`, plus the common columns. Also `cpu`, `launch_config`.

## Claims

None yet.

## Planned (no analysis yet)

- Round growth: the median ns per round grows at most linearly in N (0.99). Refuted by growth faster than linear at fixed load.

## Figures

- `round`: ns per round (median over a run's workers) by series, machines, hosts, workers; median, min and max over runs.

## Run

    nix run .#round-<b>-s1
    nix build .#round
