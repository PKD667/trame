# One-warp GPU prototype: post_parallel on sm_80

Question (PLAN.md, gate D): can a CUDA warp be one logical worker, with `parallel!` run lane-parallel
over keys, and the per-key state still canonical after the region? The prototype is one warp
(32 lanes) folding the inputs of `programs/post_parallel_pod.rs`, then lane 0 reporting each slot.
Its stdout must equal `programs/post_parallel_pod.expected`.

## Layout

| File | What it is | Built by |
|---|---|---|
| `src/main.rs` | device kernel `post_parallel` and the cuda-oxide host code | `cargo oxide build --arch sm_80` (outside Nix) |
| `nv_post_parallel_gpu.ptx` | the PTX the build produced, committed | cargo-oxide |
| `host/main.rs` | host harness: loads the PTX through `libcuda.so.1` (dlopen), one block of 32 threads | Nix (`backends/nv/flake.nix`, package `host`), `rustc` only |
| `experiment.py` | the lab runner: one launch per job, rows in `run` and `smi` | nixsci (`backends/nv/flake.nix`, `lab.experiments.post-parallel-gpu`) |

## Build (device side, not in Nix)

cuda-oxide's own devShell provides the toolchain. Its own comment says `cargo oxide` caches
`librustc_codegen_cuda.so` outside the store, so this step is impure by design. The PTX it writes is
committed and its sha256 is recorded in every row.

```
nix develop --no-write-lock-file "path:/home/pkd/code/cuda-oxide?dir=cuda-oxide" -c bash -c 'cargo oxide build --arch sm_80'
```

Pinned: cuda-oxide 9df9e7cef, rustc 1.99.1 (`rust-toolchain.toml`), CUDA 13.2.51, cargo-oxide 0.2.1.
`ptxas -arch=sm_80` and `-arch=sm_86` both assemble the committed PTX (static check, no GPU).

## Run (on a node, through nixsci)

```
cd backends/nv && nix run .#post-parallel-gpu
```

Needs a target with `driver = "nvidia"` and a `gpu` provider. Each job is one process with its own
driver context; `replicates = 3` asks for three finished runs. Rows:

- `run`: `ran` (true only when the kernel launched and copied its output back), `ok`, `verdict`
  (`ok`, `diff`, `no-driver`, `cuda-error`, `timeout`), `device`, `compute_capability`, `driver_version`,
  `ptx_sha256`, `stdout_sha256`, `seq_sha256`, `exit_code`.
- `smi`: `nvidia-smi` identity; the raw `nvidia-smi -q` goes to `smi_raw` as an artifact.

No timing is recorded. This is a semantics test.

## What was checked, and where

| Check | Command | Result |
|---|---|---|
| PTX assembles for sm_80 and sm_86 | `ptxas -arch=sm_80 ...` (cuda-oxide shell, CUDA 13.2.51) | exit 0 both |
| Host harness builds | `nix build .#host` in `backends/nv` | ok |
| Harness with no driver | `nv-post-parallel-host PTX` | exit 3, nothing on stdout, message on stderr |
| Runner, no driver (local machine) | `$(nix build .#gpuClosure)/bin/run` | three rows `ran=false`, `verdict=no-driver`, job exit 0 |
| Runner, stand-in driver | `driver.sh` binds a C stand-in (CPU code) | `verdict=ok`, `stdout_sha256` = `seq_sha256`; `ran=true` |
| Runner, deliberately wrong expected | same, with a wrong `.expected` | `verdict=diff`, both texts printed |
| Lab spec | `nix build .#lab.x86_64-linux.experiments.post-parallel-gpu.specFile` | evaluates, `replicates` 3 |
| Heap variant on device | `cargo oxide build --arch sm_80` on a copy with `Vec` in the kernel | refused by the toolchain: "heap allocation is not supported in kernels" |

The stand-in driver is a C file of CPU code that does what the kernel does. It shows that the
harness passes the seven kernel parameters in the PTX's order and that the output format equals the
expected text. It is not a GPU result, and no timing or behaviour of the device is claimed from it.

## Not checked

- The device run. No GPU is on this machine, and the reserve for a node is blocked (see PLAN.md, step 5).
- Whether the warp barrier and the lane split give the seq result on hardware.
- Any multi-warp or multi-block behaviour; the prototype is one block of 32 threads.
- Whether `nvidia-smi` on the node reports what the driver reports to the harness: the two are recorded side by side, not compared.
