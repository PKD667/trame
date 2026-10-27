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
the driver's code. R can be wrong; a disagreement names a suspect on both sides, and the
disagreement is decided by re-reading the spec, not by trusting either.

## Properties

Let R(P) be the predicted refusals and V(P) the driver's. Levels are compared as the unit of
verdict, rules as attribution.

1. **Verdict soundness** (severe). If R refuses P at level L, then V refuses P at level L. A
   violation is a *verdict miss*: the driver accepts something the spec refuses.
2. **Verdict precision**. If R accepts P at level L, then V accepts P at level L. A violation is an
   *over-refusal*: a usability bug, the driver refuses something the spec allows.
3. **Attribution** (the spec's "same rule" requirement). If R predicts rule r at level L, V should
   name rule r at level L in the program's own code. A violation is an *attribution miss* (V refuses
   at L but not with r) or an *extra rule* (V names a rule at a refused level that R does not
   predict). These are counted and reported; they do not change the verdict.
4. **Metamorphic invariance**. Wrapping a construct in a helper fn, generic fn, closure passed to
   `for_each`, `map` or `Option::map`, a trait method under static dispatch, a `Drop` body, an
   `Add::add` body, a `Deref` body, or a `From` body reached by `.into()`, and placing dead code
   (an uncalled function), must not change V. Concretely: the set of (rule, level) pairs of the
   wrapped or dead program equals that of the bare program.

Property 1 and 2 are checked against R. Property 4 is checked against the bare program in the same
batch, so it does not depend on R being right.

## Abstract language

    Prog   ::= fns, roots
    root   ::= (level, Stmts)         level ∈ {item, step, worker}
    fn     ::= Stmts                  reached only by Call
    Stmt   ::= Forbidden(c)
             | Allowed(a)
             | Call(f)
             | For(0..n, Stmts)
             | While(cond, Stmts)
             | Wrap(w, Stmts)
             | If(cond, Stmts, Stmts)

The roots are emitted as: an item root is `fn r(x: u32)` run through `trame::__check::item`; a
worker root is `fn r(x: u32)` run through `trame::__check::worker`; a step root is an inherent
`step(&mut self)` method of a process type `P`, run through `trame::__check::step`.

Forbidden constructs and the levels where each is refused (the table of spec.md section 3, with the
rule ids the driver prints):

| construct      | levels              | rule(s)                         |
|----------------|---------------------|---------------------------------|
| println        | item, step, worker  | `std`, `unsafe`                 |
| thread::spawn  | item, step, worker  | `std`                           |
| static read    | item, step, worker  | `static`                        |
| Cell           | item, step, worker  | `deny`                          |
| dyn call       | item, step, worker  | `indirect`                      |
| fn-pointer call| item, step, worker  | `indirect`                      |
| unsafe deref   | item, step, worker  | `unsafe`                        |
| trame send/recv/now | item           | `effects`                       |
| barrier, concurrent! | item, step    | `effects`                       |
| Vec::push      | item                | `allocation`                    |
| `(x..).for_each` | item, step        | `deny`                          |

Allowed constructs (refused at no level): wrapping u32 arithmetic, `Option::map`, slice indexing,
`trame::math::exp`, `min`. Vec::push at step and worker is allowed: the table above has it at item only.

Two more rules are structural. A `While` loop that is not counted is refused with `counted-loop` at
item and step. A call-graph cycle is refused with `recursion` at item and step.

**Conditions are not constant-foldable.** Every `if`, `for` bound and `while` condition reads the
parameter `x`. This matters for one reason: R is computed from the source, the driver reads
optimized MIR, and an unreachable branch is removed by the optimizer. With non-constant conditions
the two agree. This is the one place where source-level R and MIR-level V legitimately differ, and
a condition written as a constant would show that difference as a false miss.

**Strictest level.** A function reached from several roots is judged at the strictest of their
levels (item is stricter than step, step stricter than worker). A refusal is attributed to that
level and reported as reached from one of the roots at that level.

**Wrappers** (property 4): `helper` (named fn), `generic` (generic fn, instantiated with u8),
`foreach` (closure to `Iterator::for_each`), `itermap` (closure to `Iterator::map`, consumed by
`count`), `optmap` (closure to `Option::map`), `trait` (static dispatch of a trait method), `drop`
(`Drop::drop` body), `add` (`Add::add` body), `deref` (`Deref::deref` body), `into` (`From::from`
body reached by `.into()`). An extra wrapper outside the theory, `sumforeign`, puts a user
`impl Sum<U> for u32` body behind `Iterator::sum`; it is run only with `--extra`.

**Allow-list calls.** `.into()` on a user type is refused by `allow-list` at every level. The
lists header (`driver/lists/A_I.txt`) states that a default method of core on a user type is not
matched by its trait path. The From impl body is still a node of its own in R.

## Known classifications (decided, not mismatches)

- **Drop glue.** The driver reads drop glue, and `core::ptr::drop_in_place` contains a raw pointer
  dereference, and drop glue of `Arc`, `Thread` and user Drop types calls further internals. spec.md
  says "drop glue counts as a call" and says nothing on whether unsafe code inside glue is refused.
  A core-internal refusal whose reach note passes through `drop_in_place` is therefore
  *undecided*. It is counted and printed, and it does not break agreement. The question for the
  spec is recorded in the results below.
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

On a mismatch, the program is shrunk. A round proposes every single-edit mutant: drop a statement;
unwrap a wrapper, loop or `if`; inline a call to a function called once (and drop that function);
drop a root or an uncalled function; mutate inside a child list. Mutants are checked in one batch.
The first mutant that is strictly smaller and keeps the mismatch key is kept, and the round
repeats. Keys look like `verdict-miss step`, `attr-miss std item`, `over item unsafe`.

The minimal case is written to `found/<seed>-<n>.rs` with R's expectation and the driver's verdict
for the shrunk program (verbatim lines), the difference, and the source.

## Sweep

`trame-fuzz sweep` builds, for every forbidden construct c and every level l at which c is refused:
the bare program (root at l holds c); the same plus an uncalled function holding a forbidden
construct (dead); every single normative wrapper of c (depth 1); and every ordered pair of normative
wrappers (depth 2). Each is compared with R, and each is compared by rule set with its bare program
(metamorphic check). A failure of the metamorphic check names the wrapper.

## What this does not establish

- It checks the driver against the spec as encoded in R. Where R is wrong the report says so.
- It does not check the lists: they are trusted (spec.md section 3), and the fuzzer only exercises
  the constructs above.
- Verification is by testing a generated set of programs. It is not a proof, and a clean run says
  only that no program in the set disagreed.
- The stand-in crate in `support/trame` includes the real `check.rs` by path, but its effect
  functions are empty. A change in the real crate's effect paths must be mirrored there.
