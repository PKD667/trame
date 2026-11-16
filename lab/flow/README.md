# flow

Bandwidth of one stream per worker (`lab/bench/flow.rs`). Two families of runs, pooled by one analysis because they share
parameter points (size 65536, cap 8): `flow-bw-<b>-s<N>` (frame size by route capacity, N in 1 2)
and `flow-agg-<b>-s<N>` (aggregate bandwidth against hosts: N in 1 2 4 machines times the backend's splits).

Parameters: `pattern` (shift; a ring on one host), `per` (4), `steps` (20000); bw sweeps `size` (64, 1024, 16384, 65536) and
`cap` (1, 8, 64); agg fixes size 65536, cap 8 and sweeps `split`. Workers time themselves on their own clocks; no duration
crosses hosts.

Dataset `flow`: `pattern`, `host`, `per`, `g`, `size`, `cap`, `steps`, `ns`, plus the common columns (`bench`, `impl`,
`backend`, `machines`, `hosts`, `per_host`). Also `cpu`, `launch_config`. Baseline runs (`impl` plain and osu) add `flow` and `osu`
rows to the same tables.

## Claims

None yet.

## Planned (no analysis yet)

- Cost against the baseline: the median ns of the flow ring is at most 3 times the baseline's at the same `nodes`, `per_node`
  and size (0.99). The ring bench was `ring`; its dataset is not in `lab/columns.json`, `flow` with pattern ring succeeds it.
- Weak scaling of neighbour exchange: at a fixed `per_node`, the median ns per step at N nodes is at most 1.5 times the median
  at one node, for every N measured (0.99).

## Figures

- `bandwidth-size`: aggregate bandwidth (bytes per ns = GB/s, over the workers of a run) by series, machines, hosts, size, cap.
- `aggregate-hosts`: the same at size 65536 and cap 8, by series, machines, hosts.

## Run

    nix run .#flow-bw-<b>-s2
    nix build .#flow
