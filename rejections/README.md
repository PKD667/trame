# Rejection suite

This suite tests what trame's compile-time checks accept and refuse. Each case is a small program checked against the real `trame` crate (the repository root, by path, `seq` feature). The checks are the driver (`driver/`, a rustc driver that judges MIR) and the macros (`macros/`, proc macros). The rules are in `spec.md`, Section 3 (Compile time), Section 5.3 and 5.4 (`concurrent!`, `parallel!`), and Section 7 (the Rust surface).

## Theory

1. **Every refusal has a case.** Each refusal that `spec.md` Section 3 or Section 7 promises has at least one case that must be refused, with its rule named in the case.
2. **Every boundary has a matched pair.** A boundary is a construct that is legal in one place and refused in another, or legal in one form and refused in a near form. Each boundary has a minimal accepted case and the smallest change that makes it refused. The accepted half is named `*_ok.rs`.
3. **Over-refusal is a bug.** A construct the spec admits, which the checks refuse, is a finding. It is not a safe default. Findings are listed below with the verbatim message. They are not fixed here.
4. **Expectations come from the spec.** An expectation states what `spec.md` says. Where the checks disagree, the case still states the spec, and the disagreement is recorded as a known gap (`known:`), so the suite stays green and the gap stays visible. A known gap that starts to match the spec fails the suite, so the list cannot go stale.

What the suite does and does not establish: it samples the checker's behaviour on chosen programs. It shows that each listed case is judged as its expectation says. It does not show that the rules are sound, and it does not prove the checker correct. A refusal is a claim about the driver's model of MIR and of the trusted lists (`driver/lists/`), not about the machine code that runs.

Out of scope: run-time checks (aborts, overflow, floating point). They are covered by `programs/`.

## Layout

    rejections/
      Cargo.toml        package `rejections`, depends on trame by path (seq feature)
      src/lib.rs        empty; cargo needs a target for the examples
      cases/<rule>/<name>.rs
      run.sh            the runner
      README.md         this file

Each case is one example of the `rejections` package. `run.sh` writes the `[[example]]` entries into `Cargo.toml` from the files under `cases/` before each run, so a new case needs no manifest edit. The example name is `<rule>__<name>`, with `-` changed to `_` in the directory.

The first line of each case is its expectation. A second line of the form `//! runner: plain cargo` runs the case under plain `cargo check` instead of `cargo trame check`. A third line `//! known: <reason>` marks a known gap. Only the first five lines are read.

| Expectation | Meaning |
|---|---|
| `//! expect: accepted` | `cargo trame check` exits 0. |
| `//! expect: refused [<rule>] <level>` | Refused, and the driver's refusals are exactly this set. Several rules are a comma-separated list: `refused [unsafe] step, [std] step`. The set must match exactly. |
| `//! expect: error <substring>` | Refused by a macro or by rustc, with no driver refusal, and the compiler output contains the substring. |

Rule names are the driver's (`driver/src/check/`): `unsafe`, `ffi`, `indirect`, `static`, `std`, `deny`, `effects`, `allow-list`, `allocation`, `counted-loop`, `recursion`, `unresolved`. Levels are `item`, `step`, `worker`. The driver's refusal line is `error: [<rule>] <level> ...`; a root-level message has no level and is not used here.

## The runner

`run.sh [substring]` runs every case whose path contains the substring, or all of them.

1. It re-enters the pinned driver shell, `nix develop path:<root>#driver`, unless `TRAME_REJECTIONS_INNER=1` is set. The driver refuses to run under another rustc.
2. It unsets `RUSTFLAGS`. A plain-cargo case must see no `--cfg`, and the driver adds `--cfg trame_checked` itself.
3. It finds the driver binaries: `nix build path:<root>#trame-driver`, or the directory in `TRAME_DRIVER_BIN` when set. `driver/tests/rejections.rs` sets it to the binaries of the running test build.
4. Before each case it runs `cargo clean -p rejections`, both in the default target directory and in `target/trame`. Cargo does not fingerprint the driver binary, so a cached result could otherwise replay a verdict the current driver never gave.
5. It runs `cargo trame check --example <case>` (or `cargo check` for a plain-cargo case) and parses `error: [rule] level` lines from the output.
6. It compares the result with the expectation and prints a table. Full output of each case goes to `rejections/target/rejections-logs/`.

Exit status is 0 when every case matches or is a known gap. It is 1 on any other mismatch, and on a known gap that now matches.

As a verdict, `nix build .#validate.x86_64-linux.verdicts.rejections` runs the same script with the driver
and the vendored crates from the flake (`experiments/validate.nix`). It holds when the run exits 0 and its
summary counts every case file (95 at the time of writing; the scope records the count). Known gaps are
reported in `outcome.json` as gaps. It does not show the rules sound: it judges the listed cases only.

## Catalogue

`*` marks a boundary with a matched pair.

| Item | Cases |
|---|---|
| All levels: unsafe block* | `unsafe/block_item`, `unsafe/block_item_ok`, `unsafe/block_step`, `unsafe/block_worker`; `unsafe/unsafe_fn_step` (rustc, see E1) |
| All levels: extern fn call | `ffi/extern_item`, `ffi/extern_item_ok` |
| All levels: std::println | `std/println_item`, `std/println_step`, `std/println_worker` |
| All levels: std::thread::spawn | `std/thread_spawn_item`, `std/thread_spawn_step`, `std/thread_spawn_worker` |
| All levels: static read* | `static/read_item`, `static/read_step` (`_ok`), `static/read_worker` (`_ok`) |
| All levels: static mut | `static/mut_step` |
| All levels: dyn Fn call* | `indirect/dyn_fn_worker`, `indirect/dyn_fn_worker_ok`, `indirect/dyn_trait_step` |
| All levels: fn pointer call | `indirect/fn_pointer_worker`, `indirect/fn_pointer_worker_ok` |
| All levels: Cell/RefCell* | `deny/cell_item`, `deny/cell_worker`, `deny/cell_worker_ok`, `deny/refcell_step` |
| All levels: AtomicU32 | `deny/atomic_worker` |
| Effects: send in item* | `effects/send_item`, `effects/send_step_ok`, `effects/send_item_worker_ok` |
| Effects: now in item | `effects/now_item`, `effects/now_step_ok` |
| Effects: barrier in step* | `effects/barrier_step`, `effects/barrier_worker_ok`, `effects/barrier_item` |
| Effects: concurrent! in step | `effects/concurrent_step`, `effects/concurrent_worker_ok`, `effects/concurrent_item` |
| Effects: parallel! in item | `effects/parallel_item`, `effects/parallel_step_ok` |
| Effects: recv in item | `effects/recv_item` |
| Counted loops: `for` over `0..n` accepted | `counted-loop/for_range_step_ok`, `counted-loop/item_loop_ok` |
| Counted loops: while* | `counted-loop/while_item`, `counted-loop/while_step`, `counted-loop/for_range_step_ok` |
| Counted loops: loop | `counted-loop/loop_step` |
| Counted loops: `for` over a Vec iterator mutated in the body* | `counted-loop/vec_iter_mutated_step` (refused by rustc's borrow check, see G1); `counted-loop/vec_iter_ok` (known gap C1) |
| Counted loops: `(0..).for_each`* | `deny/unbounded_range_step`, `deny/unbounded_range_step_ok` |
| Counted loops: `iter::repeat` | `deny/repeat_step` |
| Recursion: step | `recursion/recursion_step`, `recursion/recursion_step_mutual`, `recursion/recursion_worker_ok` |
| Recursion: item | `recursion/recursion_item`, `recursion/recursion_item_worker_ok` |
| Allocation: `Vec::push` in item* | `allocation/vec_push_item`, `allocation/vec_push_step_ok`, `allocation/vec_push_worker_ok`, `allocation/vec_with_capacity_item` |
| Allocation: helper reached from an item* | `allocation/helper_alloc_from_item`, `allocation/helper_pure_ok` |
| Allocation: `Vec::new` and `len` in item | `allocation/vec_new_len_item` (see A1) |
| Closure to `for_each` calling std | `std/foreach_step`, `std/foreach_worker`, `std/foreach_worker_ok` |
| Strictest level | `effects/strictest_level_item`, `effects/strictest_level_worker_ok` |
| Allow-list | `allowlist/forget_item`, `allowlist/swap_item_ok` |
| Abort via panic is not std | `std/abort_item` (a call to `process::abort`), `std/option_expect_step_ok` |
| Drop glue counts as a call | `std/drop_glue_item`, `std/thread_spawn_step` |
| Trame's own functions are trusted | `trusted/math_exp_item_ok` |
| Messages: padding* | `message/padding`, `message/padding_ok` |
| Messages: reference field | `message/reference_field`, `message/reference_field_nested` |
| Messages: raw pointer | `message/raw_pointer` |
| Messages: fn pointer | `message/fn_pointer` |
| Messages: generic | `message/generic` |
| Messages: repr(packed) | `message/repr_packed` |
| Messages: user derive(Clone) | `message/derive_clone` |
| Messages: plain struct accepted | `message/plain_ok` |
| Processes: primitive or core/alloc cell* | `process/cell_primitive_u32`, `process/cell_vec_u32`, `process/cell_newtype_ok` |
| Checked builds: no cfg | `checked/concurrent_no_cfg`, `checked/parallel_no_cfg` (plain cargo); `checked/cfg_passed_ok` |

## Migration of the earlier fixtures

The earlier suite was `driver/tests/fail` and `driver/tests/pass`, checked against a stand-in crate in `driver/tests/support/trame`. Both are removed. Each fixture is now a case with the same rule.

| Earlier fixture | Case |
|---|---|
| fail/alloc_item | allocation/vec_with_capacity_item |
| fail/allowlist_item | allowlist/forget_item |
| fail/cell_worker | deny/cell_worker |
| fail/effect_item | effects/send_item |
| fail/effect_step | effects/barrier_step |
| fail/ffi_item | ffi/extern_item |
| fail/foreach_step | std/foreach_step |
| fail/foreach_worker | std/foreach_worker |
| fail/indirect_worker | indirect/fn_pointer_worker |
| fail/recursion_item | recursion/recursion_item |
| fail/static_step | static/read_step |
| fail/std_item | std/abort_item |
| fail/strictest_level | effects/strictest_level_item |
| fail/unbounded_range_step | deny/unbounded_range_step |
| fail/uncounted_loop_step | counted-loop/while_step |
| fail/unsafe_item | unsafe/block_item |
| fail/unsafe_step | unsafe/unsafe_fn_step (now a rustc error, see E1; the driver rule is covered by unsafe/block_step) |
| fail/vec_push_item | allocation/vec_push_item |
| pass/cfg_ok | checked/cfg_passed_ok |
| pass/item_ok | counted-loop/item_loop_ok |
| pass/option_expect_step | std/option_expect_step_ok |
| pass/step_ok | effects/step_mixed_ok |
| pass/trame_internal_item | trusted/math_exp_item_ok |
| pass/vec_push_step | allocation/vec_push_step_ok |
| pass/worker_ok | recursion/recursion_worker_ok, effects/barrier_worker_ok, effects/concurrent_worker_ok |

The old fixtures called a stand-in API (`send(1)`, `trame::__check::item(...)`). Each case here uses the real surface: `send<M: Message>(to, msg)`, `#[trame::item]` with `trame::parallel!`, `#[trame::process]` with `trame::concurrent!`, and `Topology::worker`.

## Findings

Each finding is a disagreement between the checks and the spec, with the verbatim message. None was fixed here.

- **C1, over-refusal (known gap).** `counted-loop/vec_iter_ok`. A `for` over a slice in a step is a counted loop (`spec.md`, Definition (Counted loop)). The driver refuses it:
  `error: [unsafe] step contains dereference of a raw pointer: no unsafe code (spec.md, Requirement (All levels))`
  with the second line `error: [unsafe] step contains access to a union field: no unsafe code`. The raw pointer is inside core's slice iterator, which the driver reads because it is generic. `driver/README.md` says slice `Iter` is counted, so the checks disagree with their own design. The likely fix is to treat the slice iterator's `next` as a trusted counted source, which is a change to the lists or to scan.rs, not to this suite.
- **A1, strict reading of the allocation rule (open question).** `allocation/vec_new_len_item`. Requirement (Item) says an item "neither allocates nor calls alloc". The driver refuses `Vec::new`, `Vec::len` and the `Vec` drop glue, with, for example:
  `error: [allocation] item calls alloc::vec::Vec::len: items neither allocate nor call alloc (spec.md, Requirement (Item))`.
  `Vec::new` and `len` do not allocate. The letter of the rule refuses them, so the driver follows the letter. Whether the rule means "allocates" or "calls alloc" decides whether this is over-refusal. The case expects the strict reading. Changing it is a spec decision.
- **D1, extra rule (not a disagreement in outcome).** `println!` in any level is refused under `[std]` and also `[unsafe]`, because the `format_args!` expansion calls core's `unsafe fn fmt::Arguments::new`. The refusal is correct; the second rule is noise. The expectations list both.
- **E1, rustc before the driver.** A process step declared `unsafe fn` is called by the generated impl of the `concurrent!` expansion, outside any unsafe block. rustc refuses it first:
  `error[E0133]: call to unsafe function `P::step` is unsafe and requires unsafe block`.
  An unsafe block around the `concurrent!` call does not help, because the call is inside the generated impl. So the driver's rule for an unsafe signature is not reached through the public macro. `unsafe/unsafe_fn_step` asserts the rustc error. The driver's `unsafe` rule for the body is still tested by `unsafe/block_step`.
- **M1, message generics.** A message with a lifetime parameter, `struct Borrowed<'a> { s: &'a str }`, is refused with `error: a message type cannot be generic`. The reference rule is not reached. Both refusals are correct; the reference case therefore uses `&'static str`.
- **W1, cfg warning in user crates.** Every `concurrent!` and `parallel!` in a user crate prints an `unexpected_cfgs` warning, because the macro emits `#[cfg(trame_checked)]` and the user crate does not declare the cfg. The warning names `trame::concurrent` as its origin. It is noise, and it reaches every user.
- **S1, stale reference outside this suite.** `backends/seq/tests_concurrent_refuse.rs`, line 4, cites a `compile_fail` doc test in `lib.rs` that has moved here. Its owner should update it.
- **G1, the driver's loop-mutation test is a backstop.** `counted-loop/vec_iter_mutated_step` is refused by rustc's borrow check (`error[E0502]: cannot borrow `v` as mutable`) before the driver runs. The driver's own mutation test (`driver/README.md`, Known approximations) is shallow, and this case does not exercise it.

## Open questions

- Whether `Vec::len` at item level is over-refusal (A1). The spec must say which reading it means.
- Whether a counted `for` over a slice in a step is admitted (C1). The spec admits it; the driver does not.
- `iter::repeat` in a step is refused (`deny/repeat_step`), which answers the earlier open question. `(0..).for_each` is refused in a step too (`deny/unbounded_range_step`).

## Run

    nix develop .#mpi --command rejections/run.sh          # the table; exit 0 when every row matches
    cd driver && nix develop "path:$PWD/..#driver" --command cargo test   # runs the suite via tests/rejections.rs
    RUSTFLAGS='--cfg trame_checked' nix develop -c cargo test             # the trame crate's own tests, checked

The third line tests the trame crate (its unit and doc tests), not this suite. The suite always runs with `RUSTFLAGS` unset (see the runner).
