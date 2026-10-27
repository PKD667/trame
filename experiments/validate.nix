# The deterministic checks of trame, as nixsci validate verdicts (experiments/README.md, "Verdicts").
# Called from flake.nix with the flake's own values. Each check states its property, model, subject,
# checker, scope, and writes $out/outcome.json (validate/README.md).
{ pkgs, v, trameSeqBins, trameSrc, rejectionsSrc, programNames, driverToolchain, trameDriver }:
let
  lib = pkgs.lib;
  rejectionCases = builtins.length (builtins.filter (lib.hasSuffix ".rs") (lib.filesystem.listFilesRecursive ../rejections/cases));
  seqTests = 57; # tests of seq-tests, counted by its command
  # The exhaustive scheduler test (sched.rs): states and end checks it reports for 2 processes, 3 cells.
  exhaustiveStates = 22430721;
  exhaustiveEnds = 372787200;
  inherit (import ./validate-cargo.nix { inherit pkgs; }) vendor vendorRejections cargoHomeWith;
  cargoHome = cargoHomeWith vendor trameSrc ".";
  # One failure artifact: the log of the run.
  writeOutcome = ''
    if [ "$rc" -eq 0 ]; then holds=true; items="[]"; else
      holds=false; mkdir -p "$out/counterexamples"; cp "$TMPDIR/log" "$out/counterexamples/log.txt"
      items='[{"id":"log","artifact":"counterexamples/log.txt"}]'
    fi
  '';
in
{
  # (a) every program, on seq, against its .expected output.
  programs-seq = v.check {
    name = "programs-seq";
    property = "every program of backends/mpi/deploy/ranks.txt, run on the seq backend, prints its .expected stdout and exits 0 (spec.md section 6)";
    model = "the seq backend's scheduler and launch, one process per program, the checked build (--cfg trame_checked). Not the mpi backend (lab: programs-mpi), not a node split, not timing";
    subject = trameSeqBins;
    checker = pkgs.diffutils;
    runtime = [ pkgs.coreutils ];
    scope = { programs = builtins.length programNames; runs = 1; backend = "seq"; wall_guard_s = 120; };
    command = ''
      mkdir -p $out/counterexamples
      holds=true
      items=""
      for p in ${lib.concatStringsSep " " programNames}; do
        rc=0
        got="$(timeout -k 5 120 "$subject/bin/$p" 2>"$TMPDIR/$p.stderr")" || rc=$?
        if [ "$rc" -eq 124 ]; then why="timeout after 120 s"
        elif [ "$rc" -ne 0 ]; then why="exit status $rc"
        elif ! diff -u "$subject/share/expected/$p.expected" <(echo "$got") > "$TMPDIR/$p.diff" 2>&1; then why="stdout differs"
        else why=""; fi
        if [ -n "$why" ]; then
          holds=false
          { echo "program $p: $why"; cat "$TMPDIR/$p.diff" 2>/dev/null; echo "--- stderr"; cat "$TMPDIR/$p.stderr"; } > "$out/counterexamples/$p.txt"
          items="$items{\"id\":\"$p\",\"artifact\":\"counterexamples/$p.txt\"},"
        fi
      done
      items="[''${items%,}]"
      gap='[{"id":"wall-clock","why":"a run over 120 s is failed as a timeout; the bound is a crash guard, and no timing is claimed"}]'
      printf '{"holds":%s,"gaps":%s,"counterexamples":%s}\n' "$holds" "$gap" "$items" > $out/outcome.json
    '';
  };

  # (b) the root crate's tests on seq, checked build, offline. The exhaustive scheduler test is (c).
  seq-tests = v.check {
    name = "seq-tests";
    property = "every test of the root crate's seq backend passes in the checked build (spec.md sections 4-6, as the tests state them)";
    model = "cargo test of the root crate, seq feature, --cfg trame_checked, the exhaustive scheduler test excluded. The tests check the seq backend and its launch and scheduler with fixed programs and sampled states, not all executions";
    subject = trameSrc;
    checker = pkgs.cargo;
    runtime = [ pkgs.rustc pkgs.gcc pkgs.coreutils pkgs.gnugrep pkgs.gnused pkgs.gawk ];
    scope = { tests = seqTests; features = "seq"; cfg = "trame_checked"; };
    command = ''
      ${cargoHome}
      export RUSTFLAGS='--cfg trame_checked'
      rc=0; cargo test --offline -- --skip start_and_end_exhaustive_up_to_bounds > "$TMPDIR/log" 2>&1 || rc=$?
      n=$(grep -E '^test result: ' "$TMPDIR/log" | sed -E 's/^test result: [a-z]+\. ([0-9]+) passed.*/\1/' | awk '{s+=$1} END {print s+0}')
      echo "tests passed: $n (scope says ${toString seqTests})" >> "$TMPDIR/log"
      # The scope is the count measured when the check was written: a suite that gains or loses tests fails here.
      if [ "$rc" -eq 0 ] && [ "$n" -ne ${toString seqTests} ]; then rc=1; echo "test count changed: update scope.tests" >> "$TMPDIR/log"; fi
      ${writeOutcome}
      printf '{"holds":%s,"gaps":[],"counterexamples":%s}\n' "$holds" "$items" > $out/outcome.json
    '';
  };

  # (c) the exhaustive scheduler test: Start and End of sched.rs against the spec rule, every consistent
  # state up to 2 processes and 3 cells, in release. The test is in sched.rs (shared by the backends).
  scheduler-exhaustive = v.check {
    name = "scheduler-exhaustive";
    property = "the scheduler's start and end (spec.md section 5.3 and 6) agree with the spec rule on every consistent state of up to 2 processes over 3 cells";
    model = "sched.rs start and end, on every state of the scheduler's inputs (process phase, cells, flags, running set) within the bounds. Not the backends' timing, not more processes or cells";
    subject = trameSrc;
    checker = pkgs.cargo;
    runtime = [ pkgs.rustc pkgs.gcc pkgs.coreutils pkgs.gnugrep pkgs.gnused pkgs.gawk ];
    scope = { processes = 2; cells = 3; states = exhaustiveStates; end_checks = exhaustiveEnds; profile = "release"; };
    command = ''
      ${cargoHome}
      export RUSTFLAGS='--cfg trame_checked'
      rc=0; cargo test --release --offline start_and_end_exhaustive_up_to_bounds -- --nocapture --exact sched::property_tests::start_and_end_exhaustive_up_to_bounds > "$TMPDIR/log" 2>&1 || rc=$?
      st=$(grep -oE '[0-9]+ consistent states' "$TMPDIR/log" | awk '{s+=$1} END {print s+0}')
      en=$(grep -oE '[0-9]+ end checks' "$TMPDIR/log" | awk '{s+=$1} END {print s+0}')
      echo "states=$st end_checks=$en (scope says ${toString exhaustiveStates} ${toString exhaustiveEnds})" >> "$TMPDIR/log"
      if [ "$rc" -eq 0 ] && { [ "$st" -ne ${toString exhaustiveStates} ] || [ "$en" -ne ${toString exhaustiveEnds} ]; }; then rc=1; echo "enumeration size changed: update scope" >> "$TMPDIR/log"; fi
      ${writeOutcome}
      printf '{"holds":%s,"gaps":[],"counterexamples":%s}\n' "$holds" "$items" > $out/outcome.json
    '';
  };

  # (e) the seq mutants: every patch of conformance/mutants/seq is caught or spared as its header says.
  mutants-seq = import ./validate-mutants.nix {
    inherit pkgs v cargoHomeWith vendor trameSrc programNames;
    mutantsDir = ../conformance/mutants/seq;
  };
  # (d) the rejection suite (rejections/README.md): every case is judged by the pinned driver against the
  # real crate. Known gaps are gaps; a case that differs from its expectation is a counterexample.
  rejections = v.check {
    name = "rejections";
    property = "every rejection case is judged as its expectation says: accepted, or refused by exactly the rule and level it names (spec.md sections 3, 5.3-5.4, 7); a known gap is reported, not counted as a failure";
    model = "the driver (driver/, pinned nightly) judging the cases of rejections/cases against the root crate's seq feature, with the lists of driver/lists. It samples the checker on chosen programs; it does not show the rules sound, nor the driver correct";
    subject = rejectionsSrc;
    checker = driverToolchain;
    runtime = [ trameDriver pkgs.bash pkgs.findutils pkgs.gcc pkgs.coreutils pkgs.gnugrep pkgs.gnused pkgs.gawk pkgs.python3 ];
    scope = { cases = rejectionCases; toolchain = "driver/rust-toolchain.toml"; };
    command = ''
      ${cargoHomeWith vendorRejections rejectionsSrc "rejections"}
      export TRAME_REJECTIONS_INNER=1 TRAME_DRIVER_BIN=${trameDriver}/bin
      rc=0; bash ./run.sh > "$TMPDIR/run.txt" 2>&1 || rc=$?
      mkdir -p "$out/counterexamples/cases"
      cp "$TMPDIR/run.txt" "$out/counterexamples/run.txt"
      python3 - "$TMPDIR/run.txt" "$out" "$rc" ${toString rejectionCases} <<'PY'
      import json, re, shutil, sys
      log, out, rc, cases = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
      counted = [int(m) for m in re.findall(r"^(\d+) pass, (\d+) known gap, (\d+) fail", open(log).read(), re.M)[0]] if re.search(r"^\d+ pass, ", open(log).read(), re.M) else None
      lines = open(log).read().split("\n")
      gaps, bad = [], []
      for i, line in enumerate(lines):
          parts = line.split()
          if line.startswith("GAP ") and len(parts) > 1:
              why = lines[i + 1].strip().removeprefix("known: ") if i + 1 < len(lines) else ""
              gaps.append({"id": parts[1], "why": why})
          if (line.startswith("FAIL ") or line.startswith("XPASS ")) and len(parts) > 1:
              bad.append(parts[1])
      items = []
      for case in bad:
          art = "counterexamples/cases/" + case.replace("/", "__") + ".log"
          src = "target/rejections-logs/" + case.replace("/", "__") + ".log"
          shutil.copy(src, out + "/" + art)
          items.append({"id": case, "artifact": art})
      # Every case file is counted: a run that judged fewer cases than exist does not hold.
      complete = counted is not None and sum(counted) == cases
      holds = rc == 0 and not bad and complete
      if not holds and not items:
          items.append({"id": "run", "artifact": "counterexamples/run.txt"})
      json.dump({"holds": holds, "gaps": gaps, "counterexamples": items}, open(out + "/outcome.json", "w"))
      PY
    '';
  };
}
