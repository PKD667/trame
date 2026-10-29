# trame-fuzz: a fuzzer for the compile-time analysis

This directory tests the compile-time analysis of trame: the rustc driver in `driver/` (see
`driver/README.md`) and the rules of spec.md section 3. It is a separate Cargo package with no
dependencies, built by its own Nix flake. It owns only `driver/fuzz/`.

## Object

Let P be a program (defined below). The driver's verdict on P is

    V(P) = { (root, rule, level) : the driver refuses P at that root, with that rule, at that level }

A refusal is one `error: [rule] <level> ...` line of the driver, with the root named in its
`reached from root X` note. A recursion refusal has no note, so its root is empty.

The oracle is R(P): the set of refusals that the spec predicts, computed from the abstract program
alone, by the rules of spec.md section 3, driver/README.md and the lists in `driver/lists/`
(`deny.txt`, `A_I.txt`, `A_C.txt`, `A_W.txt`). R is written in `src/oracle.rs` and is independent of
the driver's code. R can be wrong. A disagreement names a suspect on both sides, and the
disagreement is decided by re-reading the spec, not by trusting either.

## Properties

Let R(P) be the predicted refusals and V(P) the driver's. Levels are compared as the unit of
verdict, rules as attribution.

1. **Verdict soundness** (severe). If R refuses P at level L, then V refuses P at level L. A
   violation is a *verdict miss*: the driver accepts something the spec refuses.
2. **Verdict precision**. If R accepts P at level L, then V accepts P at level L. A violation is an
   *over-refusal*: a usability bug, the driver refuses something the spec allows.
3. **Attribution** (the quality of the diagnostics, which the spec does not require). If R predicts rule
   r at level L, V should name r at L in the program's own code. A miss is an *attribution miss* (V
   refuses at L but not with r) or an *extra rule* (V names a rule R does not predict). Both are
   counted and reported, and neither changes the verdict. A miss is expected where the driver refused
   an earlier call and stopped reading there (property 4).
4. **Metamorphic invariance**. Wrapping a construct in a helper fn, generic fn, closure passed to
   `for_each`, `map` or `Option::map`, a trait method under static dispatch, a `Drop` body, an
   `Add::add` body, a `Deref` body, or a `From` body reached by `.into()`, and placing dead code
   (an uncalled function), must not change the verdict at any level. The set of (rule, level) pairs
   must be equal too when the wrapper adds no refusal of its own. A refused call ends the walk at
   that call: the driver owes a refusal with a true reason, not every reason, so a wrapper that R
   predicts refused at its call site is compared by verdict only.

## Known classifications (decided, not mismatches)

- **Drop glue.** Dropping a value calls its drop glue (spec.md 3, "drop glue counts as a call").
  Glue of a library type is library code, judged by its list entry, so the raw pointer reads inside
  `core::ptr::drop_in_place` are not refusals. Glue that reaches a program's own `Drop` impl is an
  `Own` edge, and that impl is judged in full (rejections: `trusted/` cases).
- **Core internals at a refused level.** A refusal whose span is in core or alloc, at a level R
  already refuses, is attributed to R's refusal and not compared by rule (for example the
  `unsafe` inside `Cell::get`, which is generic and read by the driver).

## Build and run

    driver/fuzz/run.sh <seed> <batches> <K> [--extra]

`run.sh` builds the driver with `nix build "path:$PWD/../..#trame-driver"` (or the path given by
`TRAME_DRIVER_BIN`), builds this package with `nix build "path:$PWD"`, and runs
`trame-fuzz run`. It sets `RUSTUP_TOOLCHAIN=nightly-2026-04-21`, the toolchain pinned in
`driver/rust-toolchain.toml`. It exits 1 on any verdict miss, over-refusal, attribution miss or
extra rule, and 2 on a harness problem. Undecided records do not change the exit status.

    trame-fuzz run   --seed S --batches B --k K [--extra] --driver-bin DIR --out DIR
    trame-fuzz sweep --k K --driver-bin DIR --out DIR
    trame-fuzz show  --seed S --index I [--extra]

`show` prints one generated program with R's expectation, for reading a case.

Each batch is a crate `work/<name>/` with K examples, one per program, and `expect.json` holds R's
expectations. The batch is checked once with `cargo trame check --examples --keep-going`. A fresh
target directory is used for each batch, because a cached example would be skipped silently and
every refusal would show as a miss. A run whose output does not contain `Checking batch` is a
harness problem, not an empty verdict.

Each diagnostic is attached to the example whose file its span names. A span outside every example
(core or alloc) becomes an internal record of the example whose summary follows it.

## Shrinking

On a mismatch, the program is shrunk. A round proposes every single-edit mutant. It drops a statement,
unwraps a wrapper, loop or `if`, inlines a call to a function called once (and drops that function),
drops a root or an uncalled function, or mutates inside a child list. Mutants are checked in one batch.
The first mutant that is strictly smaller and keeps the mismatch key is kept, and the round
repeats. Keys look like `verdict-miss step`, `attr-miss std item`, `over item unsafe`.

The minimal case is written to `found/<seed>-<n>.rs` with R's expectation and the driver's verdict
for the shrunk program (verbatim lines), the difference, and the source.

## Sweep

`trame-fuzz sweep` builds, for every forbidden construct c and every level l at which c is refused, four sets of programs:

- the bare program (root at l holds c),
- the same plus an uncalled function holding a forbidden construct (dead),
- every single normative wrapper of c (depth 1),
- every ordered pair of normative wrappers (depth 2).

Each is compared with R, and each is compared by rule set with its bare program
(metamorphic check). A failure of the metamorphic check names the wrapper.

## What this does not establish

- It checks the driver against the spec as encoded in R. Where R is wrong the report says so.
- It does not check the lists: they are trusted (spec.md section 3), and the fuzzer only exercises
  the constructs above.
- Verification is by testing a generated set of programs. It is not a proof, and a clean run says
  only that no program in the set disagreed.
- The stand-in crate in `support/trame` includes the real `check.rs` by path, but its effect
  functions are empty. A change in the real crate's effect paths must be mirrored there.
