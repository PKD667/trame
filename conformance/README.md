# Conformance by mutation

A test program `P` shows that a backend follows a rule `R` only if `P` can tell a backend that breaks `R`
from one that does not. Two correct backends that print the same thing prove agreement, not conformance.
So this directory asks the other question: for each rule of `spec.md`, is there a program that fails when
a backend violates it?

## Claim

A program `P` tests rule `R` iff some mutant of a backend that violates `R` makes `P` observably fail:
its stdout differs from `P`'s `.expected`, it exits non-zero (a program with `<name>.panics` must exit
non-zero with that line on stderr), or it times out (a hang counts as a kill).

## Mutants

- A mutant is one patch, `mutants/<backend>/<id>.patch`, applied with `git apply` to the working tree.
- Each backend has its own mutants (seq and mpi are independent implementations). Where the code is shared
  (`sched.rs`, `concurrent.rs`, `io.rs`, `launch.rs`, `cells.rs`, `run.rs`), the same patch is kept under both
  backends and run once per backend. The kill is then a property of the shared code as the backend runs it,
  so both copies carry the same `# expect:` meaning.
- Each patch header states the rule it violates, `# violates: <spec rule>`, and its expectation:
  - `# expect: killed`: some program must fail. If none does, the program set is too weak (a finding).
  - `# expect: equivalent: <argument>`: no output that `T` (spec.md section 5-6) constrains can change. If a
    program fails on it, the argument is wrong.

## Runner

Two records and one quick loop.

- **mpi: the lab experiment `mutants-mpi`** (`experiments/checks.nix`, runner `experiments/mutants.py`). Nix
  builds each mutant with its patch applied, and a job runs the program set against one mutant. Run it with
  `nix run .#mutants-mpi`, and commit `lab.lock`. Its dataset `kill` holds one row per program. No analysis
  judges it yet (experiments/README.md, "Claims"): read the rows.
- **seq: the validate verdict `mutants-seq`** (`experiments/validate-mutants.nix`), deterministic:
  `nix build .#validate.x86_64-linux.verdicts.mutants-seq`. It applies each patch alone, builds the seq
  examples, and runs every program with `TRAME_SEQ_STEP_LIMIT=1000000` (a count of steps, so a looping
  mutant is killed by its count, and the run also has a 60 s guard). It holds when every `killed` mutant fails a
  program and every `equivalent` one passes all. A patch that does not apply is a counterexample, not a skip.
  Its verdict lists the mutants that contradict their headers in `outcome.json`. Read it there, not here.
- **`conformance/mutate.sh [seq|mpi|both] [regex]`**: the quick local loop, not the record. It copies the
  working tree into `/tmp/trame-mutate/<backend>` (or `$TRAME_MUTATE_DIR/<backend>`), kept with its `target/`
  between runs, so builds are incremental. For each patch it restores the copy, applies the patch, builds,
  runs `programs/run.sh <backend>` and records the first failing program. It prints a table and exits
  non-zero when a `killed` mutant survives or an `equivalent` mutant is killed. Its table is not locked, and
  it does not seal the tree it ran.

Both records judge a program the same way, the rule of `programs/run.sh`: a program with `<name>.panics`
must exit non-zero with that line on stderr (`experiments/programs.py`, `judge`, which `mutants.py` imports).
Without that rule every `abort` program would count as a kill on the unpatched build.

A mutant with a `<id>.skip` file instead of a patch cannot be written as one (its body gives the reason).
The file has the same `# violates:` and `# expect:` headers, so the lab experiment records it as `skipped`.

## Making a patch

`conformance/mkpatch.sh <backend> <id> <rule> <expect> <path> <perl-substitution>` writes
`mutants/<backend>/<id>.patch`: the two headers, and the diff of `<path>` before and after a
`perl -0pi -e` substitution. It refuses a substitution that changes nothing, and it checks that the patch
applies with `git apply --check`. A patch that stops applying after its file changes is regenerated this way,
with the same headers.

## Efficiency (not conformance)

`T` does not constrain how fast or how busy a backend is. A busy-wait that changes no output `T` allows is
`equivalent` for conformance. No script measures CPU time, so no number here says how busy such a mutant
is. The header of each such mutant says what it changes beyond output.

## Mutant catalogue and rules

| id | mutant | rule |
|---|---|---|
| m01 | a full route drops the frame and answers `ok` | Full |
| m02 | a route is full at `cap - 1` | Full |
| m03 | a lossy route keeps the oldest frame | Overwrite |
| m04 | `recv` takes the newest frame of a route | Offers, Recv |
| m05 | `recv` ignores the message type (seq) or takes the first inbox type as the leader's owned type (mpi) | Ownership |
| m06 | a frame arrival does not wake its receiver | Progress |
| m07 | the frame condition holds for any queued frame | Enabled |
| m08 | `until` holds at once | Status (until) |
| m09 | `room` always holds | Status (room) |
| m10 | `End` does not set `D` for sharing processes | End |
| m11 | the lowest-priority ready process starts first | Start |
| m12 | an `err` is ignored (`concurrent!` answers `Ok`) | Fail |
| m13 | a barrier fires with one worker missing | Barrier |
| m14 | a publication is never installed | Publish |
| m15 | a barrier answers the old publication to worker 0 | Barrier (one Pub for all) |
| m16 | a returned worker does not release barrier waiters | Deadlock, Join |
| m17 | `join` reports `Deadlock` for a worker waiting on a deadline (seq); skipped on mpi, which reports no Deadlock for an idle worker | Deadlock |
| m18 | a panic does not end the run: the panicking worker counts as returned, the others go on, and the leader does not panic | Abort |
| m19 | a duplicate route is accepted | Launch checks |
| m20 | a route of capacity 0 is accepted | Launch checks |
| m21 | no mutant: the id is unused | - |
| m22 | the leader's `recv` drops every other frame | Take |
| m23 | frames queued when `concurrent!` answers are dropped (skipped: no patch) | Ownership (after concurrent) |
| m24 | `recv` never rotates its route scan | Progress (fairness, section 6) |
| m25 | `Run::wait` returns at once when its counter is not `u64::MAX` (shared `run.rs`): the leader never checks for an abort | Abort: killed by `abort` |
| m26 | `worker_idle` returns at once: a busy wait (equivalent for output; see its header) | Efficiency |

The spec rule names are those of `spec.md`: Full, Overwrite, Offers, Recv, Ownership, Progress, Enabled,
Status, End, Start, Fail, Barrier, Publish, Deadlock, Abort, Launch checks, Take, Join.

## What a kill needs

A program kills a mutant only by making the feature observable. A program must therefore:
- hand the rule's effect to the leader in a form the leader prints (a count, a payload, an order), not a bare
  `Ok`;
- keep the observation deterministic under the correct rules, by ordering the steps through the leader
  (a handshake) rather than by sleeping, except that a host-side `sleep` may separate two events the rules
  do not order;
- still pass `cargo trame check` (no `std` in workers or steps, while the leader prints).

## Limits

- A kill shows that a program can see a mutant. It does not show that every mutant of a rule is caught.
- Mutants are patches written by hand. A survivor only says the catalogue's mutant is not seen. The rule may
  still be violated some other way.
- Hangs are caught by the timeout, which costs its full length.
