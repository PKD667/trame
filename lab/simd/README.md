# simd

Can `parallel!` (INTERFACE.md 5.4) be lowered on CPU to explicit SIMD with results bit-identical to the scalar fold, and does it pay?
The code is hand-written lane versions of five item bodies against the scalar reference `trame::__rt::parallel`. The prototype,
its design, its checks and the numbers of one hand run are in `REPORT.md`. This experiment re-measures the benchmark through
the lab, one run per CPU type.

Runs: `simd-bench-zen1` (provider `simd-zen1`) and `simd-bench-avx512` (provider `simd-avx512`); one host each, 3 replicates.
Not in the suite. Parameter: `cpu_type`. The runner pins `taskset -c 4` and runs `bench` and `bench knee`. The binary is
`packages.<system>.trame-simd-bench`, built from this directory with trame as a path dependency (own `Cargo.lock`).

Datasets. `bench`: `machine`, `table` (`bench` or `bench_knee`), `case`, `dist`, `slots`, `variant`, `items`, `median_mitems_s`,
`min_mitems_s`, `max_mitems_s`, `eq` (`DIFF` if a row differs bit for bit from the scalar result). `machine`: `hostname`, `model`,
`avx2`, `avx512f`, `kernel`, `rustc`.

## Claims

None yet.

## Planned (no analysis yet)

- Every row has `eq` other than `DIFF` (exact).

## Figures

- `simd-throughput`: Mitems/s by machine, table, case, distribution, slots, variant and item count; median, min and max over runs.
- `simd-speedup`: the ratio of a lane variant to the scalar row of the same run at 1e6 items, by machine, case, distribution
  and variant; median, min and max over runs. Both are tables; the figure source is `usi-design`.

## Run

    nix run .#simd-bench-zen1
    nix build .#draft.simd
    cargo test --release --offline     # in this directory, the exhaustive and property tests of REPORT.md section 4

Caveat: the scalar reduce loop has two speeds on one node (`REPORT.md` 2.1), so a reduce speedup is quoted against both or neither.
