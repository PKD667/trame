# fanin

Every worker sends MSGS 64-byte messages to the leader (`lab/bench/fanin.rs`). The leader's receive time runs from its first
`recv` to its last; each worker reports its own send span. The leader times itself with `std::time::Instant` (`trame::now`
panics outside a worker): a host clock, outside the interface's contract. `fanin-<b>-s<N>`, N in 1 2.

Parameters: `per` (4, 16), `msgs` (10000).

Dataset `fanin`: `workers`, `worker` (-1: the leader's total), `msgs`, `ns`, plus the common columns. Also `cpu`, `launch_config`.

## Claims

None yet.

## Planned (no analysis yet)

- The leader is a single sink, so its receive rate is a ceiling on the total message rate. Refuted by a leader time that grows
  faster than the number of messages it receives, at fixed load. No threshold is set.

## Figures

- `fanin`: ns per message (median over a run's rows) by series, machines, hosts, workers; median, min and max over runs.

## Run

    nix run .#fanin-<b>-s1
    nix build .#fanin
