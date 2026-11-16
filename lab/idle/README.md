# idle

CPU of a worker that waits (`lab/bench/idle.rs`): every worker waits SECONDS with nothing sent to it. The wall time is the
wait; the cost is CPU, which the runner reads per rank from GNU time. `idle-<b>-s1`, one machine.

Parameters: `per` (1, 4, 16), `seconds` (3). A rank also pays the messaging library's start and finish, so a control with
`seconds = 0` is needed to read the wait cost off the total; the sweep has none yet.

Dataset `idle`: `workers`, `worker`, `seconds`, `ns`, plus the common columns. Dataset `cpu` (`rank`, `host`, `machine`,
`user_s`, `sys_s`, `wall_s`): the one the analysis reads.

## Claims

None yet.

## Planned (no analysis yet)

- Idle workers sleep: a rank uses below 2% of its wall time in user plus system time (0.99). Refuted by CPU seconds that
  grow with SECONDS rather than staying at the `seconds = 0` level. With several nodes a relay-like thread may be the only
  steady CPU cost; a one-machine run cannot show it.

## Figures

- `idle-cpu`: CPU seconds per wall second of a rank (user plus system over wall), by series, machines, hosts and workers per
  host; median, min and max over runs.

## Run

    nix run .#idle-<b>-s1
    nix build .#idle
