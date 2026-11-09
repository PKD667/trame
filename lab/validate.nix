# The deterministic checks of trame, as nixsci validate verdicts (lab/README.md, "Verdicts").
# Called from flake.nix with the flake's own values. Each check states its property, model, subject,
# checker, scope, and writes $out/outcome.json (validate/README.md).
#
# Generic over the backends of src/backends/ (flake.nix): a backend whose runs are a function of the program
# (`deterministic` in its descriptor) gets `programs-<name>`; a backend's own verdicts come from its descriptor
# (`validate`, src/backends/<name>/validate.nix). `backends.<name>` has: d (the descriptor), bins (the build of its
# examples), src (the source tree), programs (the names of the programs it runs), launch (its launch, wrapped).
{ pkgs, v, backends, rejectionsSrc, driverToolchain, trameDriver }:
let
  lib = pkgs.lib;
  rejectionCases = builtins.length (builtins.filter (lib.hasSuffix ".rs") (lib.filesystem.listFilesRecursive ../check/rejections/cases));
  inherit (import ./validate-cargo.nix { inherit pkgs; }) vendor vendorRejections cargoHomeWith;
  panicsDir = ../check/programs;

  # Every program of check/programs/, run through the backend's launch, against its .expected output.
  programsCheck = name: b: v.check {
    name = "programs-${name}";
    property = "every program of check/programs/*.rs, run on the ${name} backend, prints its .expected stdout and exits 0 (CONTRACT.md, R1)";
    model = "the ${name} backend's launch, one host per program, the checked build (--cfg trame_checked). Not another backend (lab: programs-<backend>), not a split, not timing";
    subject = b.bins;
    checker = pkgs.diffutils;
    runtime = [ pkgs.coreutils pkgs.gnugrep ];
    scope = { programs = builtins.length b.programs; runs = 1; backend = name; wall_guard_s = 120; };
    command = ''
      mkdir -p $out/counterexamples
      holds=true
      items=""
      for p in ${lib.concatStringsSep " " b.programs}; do
        rc=0; panic=""; [ -f "${panicsDir}/$p.panics" ] && panic="$(cat "${panicsDir}/$p.panics")"
        d="$TMPDIR/$p"
        ${b.launch}/bin/launch --timeout 120 --out "$d" -- "$subject/programs/$p" > /dev/null 2> "$TMPDIR/$p.launch" || rc=$?
        got="$(cat "$d/host0.out" 2>/dev/null)"; err="$d/host0.err"; [ -f "$err" ] || err="$TMPDIR/$p.launch"
        if [ "$rc" -eq 124 ]; then why="timeout after 120 s"
        elif [ -n "$panic" ] && { [ "$rc" -eq 0 ] || ! grep -F -x -q -- "$panic" "$err"; }; then why="no panic line on stderr, or exit 0"
        elif [ -z "$panic" ] && [ "$rc" -ne 0 ]; then why="exit status $rc"
        # A program that panics on purpose may have no .expected: then only its panic is judged.
        elif { [ -z "$panic" ] || [ -f "$subject/share/expected/$p.expected" ]; } && ! diff -u "$subject/share/expected/$p.expected" <(if [ -n "$got" ]; then echo "$got"; fi) > "$TMPDIR/$p.diff" 2>&1; then why="stdout differs"
        else why=""; fi
        if [ -n "$why" ]; then
          holds=false
          { echo "program $p: $why"; cat "$TMPDIR/$p.diff" 2>/dev/null; echo "--- stderr"; cat "$err"; } > "$out/counterexamples/$p.txt"
          items="$items{\"id\":\"$p\",\"artifact\":\"counterexamples/$p.txt\"},"
        fi
      done
      items="[''${items%,}]"
      gap='[{"id":"wall-clock","why":"a run over 120 s is failed as a timeout; the bound is a crash guard, and no timing is claimed"}]'
      printf '{"holds":%s,"gaps":%s,"counterexamples":%s}\n' "$holds" "$gap" "$items" > $out/outcome.json
    '';
  };

  exports = b: lib.concatStringsSep "\n" (lib.mapAttrsToList (k: val: "export ${k}=${val}") b.d.env);
  # One failure artifact: the log of the run.
  writeOutcome = ''
    if [ "$rc" -eq 0 ]; then holds=true; items="[]"; else
      holds=false; mkdir -p "$out/counterexamples"; cp "$TMPDIR/log" "$out/counterexamples/log.txt"
      items='[{"id":"log","artifact":"counterexamples/log.txt"}]'
    fi
  '';
  cargoEnv = b: ''
    ${cargoHomeWith vendor b.src "."}
    export RUSTFLAGS='--cfg trame_checked'
    ${exports b}
  '';

  # The root crate's tests on one backend, in the checked build. The test binaries are built once; then each test runs
  # alone (--exact) through the backend's launch, which starts as many hosts and processes as the test's topology needs
  # (the rank count is asked of the test itself, as of any program). A test that launches no topology reports no rank count
  # and the launch reports it (exit 3): that one run is the test. Doctests are not run (cargo test --no-run builds none).
  testsCheck = name: b: v.check {
    name = "tests-${name}";
    property = "every test of the root crate passes on the ${name} backend in the checked build (INTERFACE.md sections 4-5 and CONTRACT.md, as the tests state them)";
    model = "cargo test --no-run of the root crate with the ${name} feature and --cfg trame_checked, then each test of each test binary alone through ${name}'s launch (a test that launches no topology runs once, on the launch's probe, exit 3). The scheduler's exhaustive test is scheduler-exhaustive-${name}. Fixed programs and sampled states, not all executions; no doctests";
    subject = b.src;
    checker = pkgs.cargo;
    runtime = [ pkgs.rustc pkgs.gcc pkgs.coreutils pkgs.gnugrep pkgs.gnused pkgs.gawk pkgs.python3 b.launch ] ++ b.d.packages ++ b.d.launchRuntime;
    scope = { feature = name; cfg = "trame_checked"; launch = "per test, ${name}'s launch, 300 s each"; };
    command = ''
      ${cargoEnv b}
      export HOME="$TMPDIR"
      mkdir -p "$out/counterexamples"
      if ! cargo test --offline --no-run --no-default-features --features ${name} --message-format=json > "$TMPDIR/build.json" 2> "$TMPDIR/build.err"; then
        cp "$TMPDIR/build.err" "$out/counterexamples/build.txt"
        printf '{"holds":false,"gaps":[],"counterexamples":[{"id":"build","artifact":"counterexamples/build.txt"}]}\n' > $out/outcome.json
        exit 0
      fi
      python3 - "$TMPDIR/build.json" > "$TMPDIR/exes" <<'PY'
      import json, sys
      for line in open(sys.argv[1]):
          m = json.loads(line) if line.startswith("{") else {}
          if m.get("reason") == "compiler-artifact" and m.get("profile", {}).get("test") and m.get("executable"):
              print(m["executable"])
      PY
      holds=true; items=""; n=0; failed=0; plain=0
      while read -r exe; do
        for t in $("$exe" --list 2>/dev/null | sed -n 's/: test$//p' | grep -v '^sched::property_tests::start_and_end_exhaustive_up_to_bounds$'); do
          n=$((n + 1)); d="$TMPDIR/run"; rm -rf "$d"; rc=0
          launch --timeout 300 --out "$d" -- "$exe" --exact "$t" --test-threads=1 > /dev/null 2> "$TMPDIR/launch.err" || rc=$?
          # 3: the test launched no topology and passed in its one run (src/backends/README.md, Launch). Any other
          # non-zero status fails the test: there is no fallback that could pass a launching test outside its launch.
          if [ "$rc" -eq 3 ]; then rc=0; plain=$((plain + 1)); fi
          if [ "$rc" -ne 0 ]; then
            failed=$((failed + 1)); holds=false; id="$(basename "$exe")-$n"
            { echo "test $t of $(basename "$exe"): exit $rc"; cat "$d"/host*.out "$d"/host*.err "$TMPDIR/launch.err" 2>/dev/null; } > "$out/counterexamples/$id.txt"
            items="$items{\"id\":\"$id\",\"artifact\":\"counterexamples/$id.txt\"},"
          fi
        done
      done < "$TMPDIR/exes"
      # No test at all is a failure: a harness that lists nothing proves nothing.
      if [ "$n" -eq 0 ]; then holds=false; echo "no test listed" > "$out/counterexamples/none.txt"; items='{"id":"none","artifact":"counterexamples/none.txt"},'; fi
      echo "tests run: $n, failed: $failed, launched no topology: $plain" >&2
      printf '{"holds":%s,"gaps":[{"id":"doctests","why":"doctests are not run"}],"counterexamples":[%s]}\n' "$holds" "''${items%,}" > $out/outcome.json
    '';
  };

  # The exhaustive scheduler test: Start and End of sched.rs against the spec rule, every consistent state up to 2
  # processes and 3 cells, in release. The test is in sched.rs, shared by the backends.
  exhaustiveStates = 22430721;
  exhaustiveEnds = 372787200;
  schedulerCheck = name: let b = backends.${name}; in v.check {
    name = "scheduler-exhaustive-${name}";
    property = "the scheduler's start and end (INTERFACE.md section 5.3; CONTRACT.md S2) agree with the spec rule on every consistent state of up to 2 processes over 3 cells";
    model = "sched.rs start and end, on every state of the scheduler's inputs (process phase, cells, flags, running set) within the bounds. Not the backends' timing, not more processes or cells. Built with the ${name} feature: the crate has no build without a backend, and every backend is checked alike; the test does not touch the backend";
    subject = b.src;
    checker = pkgs.cargo;
    runtime = [ pkgs.rustc pkgs.gcc pkgs.coreutils pkgs.gnugrep pkgs.gnused pkgs.gawk ] ++ b.d.packages;
    scope = { processes = 2; cells = 3; states = exhaustiveStates; end_checks = exhaustiveEnds; profile = "release"; };
    command = ''
      ${cargoEnv b}
      rc=0; cargo test --release --offline --no-default-features --features ${name} start_and_end_exhaustive_up_to_bounds -- --nocapture --exact sched::property_tests::start_and_end_exhaustive_up_to_bounds > "$TMPDIR/log" 2>&1 || rc=$?
      st=$(grep -oE '[0-9]+ consistent states' "$TMPDIR/log" | awk '{s+=$1} END {print s+0}')
      en=$(grep -oE '[0-9]+ end checks' "$TMPDIR/log" | awk '{s+=$1} END {print s+0}')
      echo "states=$st end_checks=$en (scope says ${toString exhaustiveStates} ${toString exhaustiveEnds})" >> "$TMPDIR/log"
      if [ "$rc" -eq 0 ] && { [ "$st" -ne ${toString exhaustiveStates} ] || [ "$en" -ne ${toString exhaustiveEnds} ]; }; then rc=1; echo "enumeration size changed: update scope" >> "$TMPDIR/log"; fi
      ${writeOutcome}
      printf '{"holds":%s,"gaps":[],"counterexamples":%s}\n' "$holds" "$items" > $out/outcome.json
    '';
  };

  # The rejection suite (check/rejections/README.md) on one backend: every case is judged by the pinned driver against
  # the real crate built with that backend's feature (TRAME_BACKEND; the manifest selects none). Each case's
  # diagnostics must be exactly its annotations (a file name cannot match one).
  rejectionsCheck = name: v.check {
    name = "rejections-${name}";
    property = "every rejection case is judged as its annotations say, against the ${name} backend: accepted with no diagnostic, or refused with exactly the diagnostics it annotates (rule, level, message); INTERFACE.md sections 3, 5.3-5.4, 7";
    model = "the driver (crates/driver/, pinned nightly) judging the cases of check/rejections/cases against the root crate built with the ${name} feature, with the lists of crates/driver/lists. It samples the checker on chosen programs; it does not show the rules sound, nor the driver correct";
    subject = rejectionsSrc;
    checker = driverToolchain;
    runtime = [ trameDriver pkgs.bash pkgs.findutils pkgs.gcc pkgs.coreutils pkgs.gnugrep pkgs.gnused pkgs.gawk pkgs.python3 ] ++ backends.${name}.d.packages;
    scope = { cases = rejectionCases; toolchain = "crates/driver/rust-toolchain.toml"; backend = name; };
    command = ''
      ${cargoHomeWith vendorRejections rejectionsSrc "check/rejections"}
      unset RUSTFLAGS
      ${exports backends.${name}}
      export TRAME_DRIVER_BIN=${trameDriver}/bin TRAME_BACKEND=${name}
      rc=0; cargo test --offline --features trame/${name} > "$TMPDIR/run.txt" 2>&1 || rc=$?
      mkdir -p "$out/counterexamples"
      cp "$TMPDIR/run.txt" "$out/counterexamples/run.txt"
      python3 - "$TMPDIR/run.txt" "$out" "$rc" ${toString rejectionCases} <<'PY'
      import json, re, sys
      log, out, rc, cases = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
      # One verdict line per case and configuration, as the harness prints them: "cases/<...>.rs ... ok".
      verdicts = [m for m in (re.match(r"^(cases/\S+\.rs) \.\.\. (ok|FAILED)$", l) for l in open(log).read().split("\n")) if m]
      bad = [m.group(1) for m in verdicts if m.group(2) == "FAILED"]
      # Every case file is judged: a run that judged fewer cases than exist does not hold.
      holds = rc == 0 and not bad and len(verdicts) == cases
      items = [{"id": case, "artifact": "counterexamples/run.txt"} for case in bad]
      if not holds and not items:
          items.append({"id": "run", "artifact": "counterexamples/run.txt"})
      json.dump({"holds": holds, "gaps": [], "counterexamples": items}, open(out + "/outcome.json", "w"))
      PY
    '';
  };

  perBackend = name: b:
    lib.optionalAttrs b.d.deterministic { "programs-${name}" = programsCheck name b; }
    // (b.d.validate or (_: { })) {
      inherit pkgs v vendor cargoHomeWith;
      trameSrc = b.src;
      inherit (b) programs bins;
      launch = b.launch;
    };
in
lib.foldl' (a: n: a // perBackend n backends.${n}) { } (builtins.attrNames backends)
// lib.mapAttrs' (name: b: lib.nameValuePair "tests-${name}" (testsCheck name b)) backends
// lib.mapAttrs' (name: _: lib.nameValuePair "scheduler-exhaustive-${name}" (schedulerCheck name)) backends
// lib.mapAttrs' (name: b: lib.nameValuePair "rejections-${name}" (rejectionsCheck name)) backends
