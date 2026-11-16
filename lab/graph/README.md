# graph

The proxy benchmark (`lab/bench/graph.rs`, `graph_proc.rs`, `graph_net.rs`): each step, ITERS xorshift rounds, one SIZE-byte
message to each of DEGREE out-neighbours on a seeded circulant graph, one from each in-neighbour. `graph-<b>-s<N>`, N in 1 2 4.
A job with `per` at most DEGREE records nothing.

Parameters: `per` (16, 64), `size` (64), `degree` (8), `graph_steps` (2000), `graph_iters` (100000), `seed` (1). The baseline's
`graph.c` does the same with nonblocking receives, sends and a wait-all, and must print the same checksum per rank.
Weak scaling keeps ITERS; strong scaling divides it by the nodes.

Dataset `graph`: `workers`, `worker`, `steps`, `size`, `degree`, `iters`, `ns`, `compute_ns` (the recurrence alone),
`checksum` (text: a u64, the xorshift state XOR the sum of every frame received), plus the common columns. Also `cpu`.

## Claims

None yet.

## Planned (no analysis yet)

Judged at 0.99 against the baseline (`impl` plain), as the proxy benchmark's spec states them:

- Overhead: at most 1.10 times the baseline.
- Weak scaling: the curve within 1.05 of the baseline's.
- Strong scaling: speedup at least 0.9 of the baseline's.

The spec's locality claim is about a relay the tree no longer has, so it is not listed.

## Figures

- `graph`: ns per step and the compute share (`compute_ns / ns`) by series, machines, hosts, workers, size.

## Run

    nix run .#graph-<b>-s1
    nix build .#graph
