# Conformance by mutation

A test program `P` shows that a backend follows a rule `R` only if `P` can tell a backend that breaks `R`
from one that does not. Two correct backends that print the same thing prove agreement, not conformance.
So this directory asks the other question: for each rule of `spec.md`, is there a program that fails when
a backend violates it?

## Claim

A program `P` tests rule `R` iff some mutant of a backend that violates `R` makes `P` observably fail:
its stdout differs from `P`'s `.expected`, it exits non-zero, or it times out (a hang counts as a kill).

## Mutants

- A mutant is one patch, `mutants/<backend>/<id>.patch`, applied with `git apply` to the working tree.
- Each backend has its own mutants (seq and mpi are independent implementations). Where the code is shared
  (`sched.rs`, `concurrent.rs`, `io.rs`, `launch.rs`, `cells.rs`), the same patch is kept under both backends
  and run once per backend. The kill is then a property of the shared code as the backend runs it.
- Each patch header states the rule it violates, `# violates: <spec rule>`, and its expectation:
  - `# expect: killed`: some program must fail. If none does, the program set is too weak (a finding).
  - `# expect: equivalent: <argument>`: no output that `T` (spec.md section 5-6) constrains can change. If a
    program fails on it, the argument is wrong.

## Runner

The record is the lab experiment `mutants-seq` or `mutants-mpi` (`experiments/checks.nix`, runner
`experiments/mutants.py`; experiments/README.md, "mutants"). Nix builds each mutant with its patch applied,
and a job runs the program set against one mutant. Its dataset `kill` holds one row per program, and the
analysis `mutants` judges it. Run it with `nix run .#mutants-seq` (or `.#mutants-mpi`), and commit `lab.lock`.

`mutate.sh [seq|mpi]` is the quick local loop. It copies the repository into a fixed directory per backend
(`/tmp/trame-mutate/<backend>`, so Cargo fingerprints match and builds are incremental; `target/` is kept
there). For each patch it restores the copy from the repository, applies the patch, runs
`programs/run.sh <backend>` and records the first failing program. It prints a table and exits non-zero when
a `killed` mutant survives or an `equivalent` mutant is killed. Timeouts are those of `run.sh`. It is not
the record: its table is not locked, and it does not seal the tree it ran.

A mutant with a `<id>.skip` file instead of a patch cannot be written as one (its body gives the reason).
The file has the same `# violates:` and `# expect:` headers, so the lab experiment records it as `skipped`.

## Efficiency (not conformance)

`T` does not constrain how fast or how busy a backend is, so a busy-wait cannot change any output `T` allows.
Such mutants are `equivalent` for conformance. They are caught by `efficiency.sh`, a separate CPU-time check
(`utime + stime` of the whole run, from `/usr/bin/time`) against a stated bound. It is reported beside the
table and never changes the conformance verdict.

## Mutant catalogue and rules

| id | mutant | rule |
|---|---|---|
| m01 | a full route drops the frame and answers `ok` | Full |
| m02 | a route is full at `cap - 1` | Full |
| m03 | a lossy route keeps the oldest frame | Overwrite |
| m04 | `recv` takes the newest frame of a route | Offers, Recv |
| m05a | `recv` ignores the message type | Ownership (Recv) |
| m05b | a process receives a type it does not own | Ownership |
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
| m17 | `join` reports `Deadlock` for a worker waiting on a deadline | Deadlock |
| m18 | a panic does not abort the other workers | Abort |
| m19 | a duplicate route is accepted | Launch checks |
| m20 | a route of capacity 0 is accepted | Launch checks |
| m22 | the leader's `recv` drops every other frame | Take |
| m23 | frames queued when `concurrent!` answers are dropped | Ownership (after concurrent) |
| m24 | `recv` never rotates its route scan | Progress (fairness, section 6) |
| m25 | `Run::wait` returns at once | efficiency only (equivalent) |
| m26 | `worker_idle` returns at once | efficiency only (equivalent) |

The spec rule names are those of `spec.md`: Full, Overwrite, Offers, Recv, Ownership, Progress, Enabled,
Status, End, Start, Fail, Barrier, Publish, Deadlock, Abort, Launch checks, Take, Join.

## What a kill needs

A program kills a mutant only by making the feature observable. A program must therefore:
- hand the rule's effect to the leader in a form the leader prints (a count, a payload, an order), not a bare
  `Ok`;
- keep the observation deterministic under the correct rules, by ordering the steps through the leader
  (a handshake) rather than by sleeping, except that a host-side `sleep` may separate two events the rules
  do not order;
- still pass `cargo trame check` (no `std` in workers or steps; the leader prints).

## Limits

- A kill shows that a program can see a mutant. It does not show that every mutant of a rule is caught.
- Mutants are patches written by hand. A survivor only says the catalogue's mutant is not seen; the rule may
  still be violated some other way.
- Hangs are caught by the timeout, which costs its full length.
