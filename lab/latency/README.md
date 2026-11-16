# latency

Round trips between two workers, with random gaps before each send, at three frame sizes. Same host (`latency-<b>-s1`, both
workers on one trame host) or cross host (`latency-<b>-s2`, two machines). Round trips in one run are autocorrelated, so a
run is one sample and the claims count runs. Program `lab/bench/ping.rs`; runner `run.py`.

Parameters: `size` (16, 1024, 65536), `gap_ns` (0, 500000: the bound of the drawn gaps), `samples` (6636), `cap` (1),
`deployment` (the key of `q.nix`), 5 replicates. `q.nix` holds the bounds of CONTRACT.md Latency, per deployment, placement and
size; a bound that is `null` is measured and not claimed. No bound is set yet.

Dataset `ping`: `seq`, `size`, `cap`, `warm` bool, `gap_ns`, `max_gap_ns`, `ns`, `placement` (`same-host`, `cross-host`),
`bound_p50`, `bound_p99`, `over_p50`, `over_p99` (bool, null without a bound), plus the common columns. The OSU latency
baselines of a backend add `osu` rows (`value` in us, one-way: twice it is a mean round trip, no tail).

## Claims

- `latency/<backend>/<placement>/<size>/p50`: at most half of the round trips exceed the Q1 bound (below 0.5, proportion per run).
- `latency/<backend>/<placement>/<size>/p99`: at most one in a hundred exceeds the Q2 bound (below 0.01).

A backend is accepted only if its latency claims are supported (confidence 0.99, `nixsci.stats`).

## Figures

- `latency`: median and 99th percentile of a run's round trips (warm-up dropped) by series, placement, size and gap, with the
  bounds; median, min and max over runs.

## Run

    nix run .#latency-<b>-s2
    nix build .#latency
