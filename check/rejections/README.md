# Rejection suite

Each case is a small program checked against the real `trame` crate (the repository root, by path, with
the reference backend's feature). The checks are the driver (`crates/driver/`, a rustc driver that judges MIR) and the macros (`crates/macros/`).
The rules are in `INTERFACE.md`, Section 3 (Compile time), Sections 5.3-5.4, and Section 7. The cases are listed
in `CATALOGUE.md`.

## Theory

1. **Every refusal has a case.** Each refusal that `INTERFACE.md` promises has a case that must be refused, with its rule named.
2. **Every boundary has a matched pair.** A construct legal in one place and refused in another has an accepted case
   (`*_ok.rs`) and the smallest change that refuses it.
3. **Over-refusal is a bug.** A construct the spec admits, which the checks refuse, is a finding. It is not a safe default.
4. **The verdict is the diagnostics.** A case states, on each line the checks refuse, the rule and the message. The
   harness compares the parsed diagnostics (line, level, message substring) with those annotations. A file name cannot
   satisfy a match, and an unannotated diagnostic fails the case.

What the suite establishes: each listed case is judged as its annotations say. It does not show that the rules are
sound, and it does not prove the driver correct. A refusal is a claim about the driver's model of MIR and the trusted
lists (`crates/driver/lists/`), not about the machine code that runs. Run-time checks (aborts, overflow, floating point) are
covered by `check/programs/`, not here.

## Layout

    check/rejections/
      Cargo.toml        package `rejections`, trame by path, `ui_test` as the harness
      tests/ui.rs       the harness: the driver configuration and the plain-cargo configuration
      cases/<rule>/<name>.rs
      CATALOGUE.md      every case, by directory
      README.md         this file

`cargo test` builds the cases' dependencies (trame, bytemuck) once, from `Cargo.toml`, with the driver as their
compiler. Cases are compiled with `--emit=metadata` (a check, not a link), as `cargo check` did before.

## Annotations

Each case's header comment (the lines starting `//`) says why the case exists, and states each rule it expects in one
line: `Refused [rule] level: why`. The annotations below are what the harness reads.

| Form | Meaning |
|---|---|
| `//@check-pass` | Accepted: compiles with no diagnostic. |
| `//~ ERROR: [rule] level` | A refusal on this line. The text after `ERROR:` must occur in the message. |
| `//~^ ERROR: ...`, `//~\| ERROR: ...` | The same, for the line above, or a second diagnostic on the line of the first. |
| `//@error-in-other-file: [rule] level` | A refusal whose span is in another file (std, a dependency). |
| `//! runner: plain cargo` | The case runs under plain rustc, with no driver and no `--cfg trame_checked`. |

Warnings are not verdicts (`-Awarnings`). A rustc or macro error is annotated like a driver refusal, with its own text
(`ERROR: call to unsafe function ...`, or the macro's message).

The driver is one configuration: `TRAME_DRIVER_BIN` is the directory holding `trame-driver`, and cases with no runner line
are compiled through it. The plain configuration runs the cases whose header says `runner: plain cargo`.

## Run one case, add one

    cd check/rejections && TRAME_DRIVER_BIN=<dir> cargo test          # all cases, <dir> holds trame-driver
    TRAME_DRIVER_BIN=<dir> cargo test -- padding                 # the cases whose path contains "padding"

`<dir>` is `result/bin` of `nix build .#trame-driver`, and `RUSTC` must be the rustc it was built for. The nix check,
`nix build .#validate.x86_64-linux.verdicts.rejections`, runs the same suite offline (`lab/validate.nix`).

To add a case:

1. create `cases/<rule>/<name>.rs` with a header naming each rule;
2. annotate each refused line (`//~ ERROR: [rule] level`), or write `//@check-pass`;
3. run `cargo test -- <name>`;
4. list it in `CATALOGUE.md`.

## Rules

Levels are `item`, `step`, `worker`. Rules, as the driver names them (`crates/driver/src/check/`): `unsafe`, `ffi`, `indirect`,
`static`, `std`, `deny`, `effects`, `allow-list`, `allocation`, `counted-loop`, `recursion`, `unbounded` (a user iterator
passed to core code). The driver also emits
`unresolved` for a call it cannot check. No case refuses by it yet.
A root-level message (`[root]`) has no level and is not used here.

## Roots in the cases

A step case is a `#[trame::state]` struct with a `#[trame::step]` function (a Step root, emitted by the macro). An item case is
`parallel!` with `#[trame::item]`. No library code emits a worker marker yet, so a worker case calls
`trame::__check::worker(&|| { ... })` itself; it tests the level, not a library path. A step allocates nothing (CONTRACT.md, M4):
`allocation/*_step*` hold the pairs, a Vec rewritten in place (accepted) and a Vec grown, made or dropped (refused).

## Findings

Each finding is a disagreement between the checks and the spec, with the verbatim message. None is fixed here.

- **A1, strict reading of the allocation rule (open).** `allocation/vec_new_len_item`. Requirement (Item) says an item
  "neither allocates nor calls alloc". The driver refuses `Vec::new`, `Vec::len` and the `Vec` drop glue:
  `[allocation] item calls alloc::vec::Vec::len: items neither allocate nor call alloc`. `Vec::new` and `len` do not
  allocate, but the letter of the rule refuses them. Whether this is over-refusal is a spec decision. The case expects the
  strict reading.
- **D1, println! (corrected twice).** `println!` in a worker, an item or a step is refused under `[std]` only
  (`std/println_worker`, `std/println_item`, `std/println_step`). It was once also refused under `[unsafe]`, because the
  `format_args!` expansion calls core's `unsafe fn fmt::Arguments::new` and the `fmt::rt` argument constructors. Those are
  hidden and emitted only by the expansion, so the driver no longer judges them (`fn_use`, scan.rs). The visible effect:
  `panic!("{}", x)` with a runtime `x` is accepted in a step, as `panic!("full")` was (`std/panic_fmt_*_step_ok`; nerve,
  at trame 8c8a53d). A constant argument is folded by MIR inlining and never reached the rule.
- **E1, the macro before the driver.** A step declared `unsafe fn` is refused by `#[trame::step]` itself (`state/unsafe_fn_step`).
  The driver's `unsafe` rule for the body is covered by `unsafe/block_step`.  What a state holds is not the driver's rule but the type
  `trame::Plain`, which rustc checks (`plain/`).
- **M1, message generics.** A message with a lifetime parameter is refused by the macro, `a message type cannot be
  generic`. The reference rule is not reached; the reference cases therefore use `&'static str`.
- **W1, cfg warning in user crates.** Each `#[trame::state]` and `parallel!` in a user crate prints an `unexpected_cfgs`
  warning, because the macro emits `#[cfg(trame_checked)]`. It is noise, and it reaches every user.
- **G1, the loop-mutation backstop.** `counted-loop/vec_iter_mutated_step` is refused by rustc's borrow check (E0502)
  before the driver runs. The driver's own mutation test (`crates/driver/README.md`) is shallow, and this case does not exercise it.
- **C1, resolved.** `counted-loop/vec_iter_ok` was listed as a slice-iterator over-refusal. Its [unsafe] came from the
  `vec!` in the case, not from the iterator. It is accepted.

Open: whether `Vec::len` at item level is over-refusal (A1). `iter::repeat` and `(0..).for_each` are refused in a step
(`deny/repeat_step`, `deny/unbounded_range_step`). The earlier fixtures (`crates/driver/tests/fail`, `pass`) were migrated to
cases. The mapping is in the git history.
