# mutants

Conformance by mutation (`check/mutants/README.md`): one build of a backend per mutant, its patch applied, and every program
run on each. The control `none` is the unpatched build. For a backend whose runs are not a function of the program
(`deterministic = false`); the others have the validate verdict `mutants-<b>` (`nix/validate.nix`).

Run: `mutants-<b>`, one local job per mutant (not in the suite). Parameters: `backend`, `mutant` (a patch id, or `none`),
`timeout` (120). The sweep is every id with a `.patch` or a `.skip` in the backend's `mutants/`; a mutant that cannot apply is
one job that records `how` `skipped`. `check/mutants/mutate.sh` is the quick local loop; this run is the record.

Dataset `kill`, one row per program, key `mutant`, `program`: `mutant`, `rule`, `expect`, `program` (`-` when skipped),
`killed` bool (true for `diff`, `exit`, `timeout`), `how` (`ok`, `diff`, `exit`, `timeout`, `skipped`). Also `launch_config`.

## Claims

None yet.

## Planned (no analysis yet)

- Every mutant whose catalogue entry says `killed` and that applies is killed (exact). A `skipped` mutant is a gap in the
  program set, never a kill.

## Figures

- `mutants-kills`: per run name and mutant, its rule, its catalogue expectation, the programs run, how many killed it, and
  `killed`, `survived` or `skipped`. A table.

## Run

    nix run .#mutants-<b>
    nix build .#draft.mutants
