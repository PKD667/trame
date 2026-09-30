# trame

## Conformance

`bash trame/scripts/conform.sh local` runs `conformance/claims.rs` on `none`, nv's host model, mpi
and lossy, adds X1 (`cargo test`), X2 (declaration fixtures) and P1 (public surface), and
prints a table of claim × backend. A pass means every participant ran the claim and it held; a FAIL
means one refuted it, never reported it, or was inside it when its launch timed out. It exits 0
only when every cell passes, and writes one `<backend>.<launch>.jsonl`/`.log` pair per launch into
the output directory it prints (`<out>` below).

## Contract and implementation

[backend.md](backend.md) is the target function-and-declaration contract: logical units,
communication and shared memory, independent of physical scheduling. [execution.md](execution.md)
records its migration and implementation evidence. Proposed interfaces are marked; the current
suite does not yet check the entire target surface. No implementation is certified by definition.

The 2026-09-25 run on MIST exited 1 against an unchanged dirty-tree source fingerprint. Main-body
claims passed on nv's host model, mpi and lossy; unit tests passed on the selections tested.
The previously listed RMA L2/L3 and multi-leader D1/E1/R1 failures did not recur in this run.
Evidence: `/tmp/trame-freeze-20260925T200027/`.

- M5 still fails on mpi and lossy: after `Full` at 2,047 accepted frames, every worker
  timed out without a `done` verdict. Buffer detachment is the first implementation point to
  investigate; no stack trace was collected.
- P1 fails. The none/nv `Io` unwind-safety reports differ; its `Send` difference is confounded by
  the scraper missing explicit implementations. The none/mpi `Leader::Freeze` comparison also
  uses different compilers. The verifier needs correction, not weaker portability requirements.
- Invocation error selection is uncovered: nv returns lane-local errors where the contract
  promises the first error in list order. A correct lowering preserves one logical result owner;
  the public error type need not become a warp-transfer value.
- The CUDA step vote checks outcome discriminants, not error payload agreement.
- Device transport, discovery and declared dispatch have been reported on a graffiti RTX 2080 Ti;
  the full public surface has not run on hardware. This verification ran no CUDA jobs.
