# Case 2 (not run on a device)

A larger case for the one-warp prototype: 100 keys, 300 items, the same order-dependent fold.

- `main.rs`: the host code of `gpu/src/main.rs` with `NKEYS = 100`, 300 inputs (`key = (37*i) % 100`,
  `value = 1 + 13*i`), 100 slots and reports. The device code is the same. Not built: a PTX for it
  needs `cargo oxide build --arch sm_80` in the cuda-oxide devShell, which had to rebuild the CUDA toolkit
  from source during Phase B and was stopped (see `backends/nv/PLAN.md`).
- `cpu_fold.py`: the CPU fold, written independently of the Rust code. It reproduces
  `programs/post_parallel_pod.expected` when run on that program's eight inputs (checked by hand in
  Phase B: `diff` of the two texts is empty).
- `expected.txt`: its output, 101 lines, sha256 `4c327739e11e1eb8271507db101ca9f8020957de4d18386154b914268ecb9e56`.

To run it: build the PTX from `main.rs` (copy it over `gpu/src/main.rs` in a scratch copy), add it and
`expected.txt` as a second experiment in `backends/nv/flake.nix` (the `mkGpu` helper), and run it as the
first case is run.
