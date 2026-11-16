# step

What a step that does nothing costs, with one step per call and with four (`lab/bench/step.rs`). `step-<b>-s1`, one machine.

Parameters: `per` (1, 4), `k` (steps per call: 1, 4), `n` (1000000 calls). 3 replicates.

Dataset `step`: `host`, `per`, `k`, `n`, `worker`, `step`, `ns`, plus the common columns. Also `cpu`, `launch_config`.

## Claims

None yet.

## Figures

- `step-overhead`: ns per call (`ns / n`, median over a run's steps) by series, machines, hosts, per, k; median, min and max
  over runs.

## Run

    nix run .#step-<b>-s1
    nix build .#step
