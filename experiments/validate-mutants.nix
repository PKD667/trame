# The mutants-seq check (conformance/README.md): each patch of conformance/mutants/seq is applied alone to
# the seq backend, built, and run on every program with a step limit. A `killed` mutant must fail some
# program; an `equivalent` one must not. Deterministic: the step limit (TRAME_SEQ_STEP_LIMIT) counts steps,
# not seconds, so a mutant that loops is killed by its count. The wall clock (120 s per run) is a crash guard.
{ pkgs, v, cargoHomeWith, vendor, trameSrc, mutantsDir, programNames }:
let
  lib = pkgs.lib;
  limit = "1000000"; # the largest minimal step count of the programs is 6004 (ping_pong); 160x margin
  patches = builtins.filter (n: lib.hasSuffix ".patch" n) (builtins.attrNames (builtins.readDir mutantsDir));
  skips = builtins.filter (n: lib.hasSuffix ".skip" n) (builtins.attrNames (builtins.readDir mutantsDir));
in
v.check {
  name = "mutants-seq";
  property = "every mutant of the seq backend that the catalogue expects killed fails some program, and every one expected equivalent passes all (conformance/README.md; spec.md section 5 rules named in each patch header)";
  model = "one patch at a time, applied to the seq backend of the checked build (--cfg trame_checked). A kill is a program's stdout differing, exit non-zero, or a step limit of ${limit} steps. It shows the catalogue detects these violations in these programs, not that the programs cover every rule";
  subject = trameSrc;
  checker = pkgs.cargo;
  runtime = [ pkgs.rustc pkgs.gcc pkgs.coreutils pkgs.gnugrep pkgs.gnused pkgs.gawk pkgs.git pkgs.diffutils pkgs.bash pkgs.findutils ];
  scope = {
    mutants = builtins.length patches;
    skipped = builtins.length skips;
    programs = builtins.length programNames;
    step_limit = limit;
    backend = "seq";
  };
  command = ''
    ${cargoHomeWith vendor trameSrc "."}
    export RUSTFLAGS='--cfg trame_checked' TRAME_SEQ_STEP_LIMIT=${limit}
    export HOME="$TMPDIR"
    mkdir -p "$out/counterexamples"
    tsv="$out/kills.tsv"; : > "$tsv"
    holds=true; items=""; gaps=""
    for f in ${lib.concatStringsSep " " (map (p: "${mutantsDir}/${p}") patches)}; do
      id="$(basename "$f" .patch)"
      expect="$(grep -m1 '^# expect:' "$f" | sed 's/^# expect: *//')"
      rm -rf "$TMPDIR/src"; cp -r ${trameSrc} "$TMPDIR/src"; chmod -R u+w "$TMPDIR/src"
      cd "$TMPDIR/src"
      why=""
      if ! git apply "$f" 2> "$TMPDIR/$id.apply"; then verdict=APPLY; why="$(head -c 300 "$TMPDIR/$id.apply")"
      elif ! cargo build -q --examples > "$TMPDIR/$id.build" 2>&1; then verdict=BUILD; why="$(grep -m1 error "$TMPDIR/$id.build")"
      else
        verdict=survived
        for p in ${lib.concatStringsSep " " programNames}; do
          rc=0
          got="$(timeout -k 5 60 "$TMPDIR/target/debug/examples/$p" 2>/dev/null)" || rc=$?
          if [ "$rc" -ne 0 ] || ! diff -q "$TMPDIR/src/programs/$p.expected" <(echo "$got") >/dev/null 2>&1; then
            verdict=killed; why="program $p: exit $rc, stdout differs or step limit"; break
          fi
        done
      fi
      printf '%s\t%s\t%s\t%s\n' "$id" "$expect" "$verdict" "$why" >> "$tsv"
      kind="''${expect%%:*}"
      case "$kind:$verdict" in
        killed:killed|equivalent:survived) ;;
        *) holds=false
           printf 'mutant %s: expects %s, got %s\n%s\n' "$id" "$expect" "$verdict" "$why" > "$out/counterexamples/$id.txt"
           items="$items{\"id\":\"$id\",\"artifact\":\"counterexamples/$id.txt\"},"
           ;;
      esac
      cd "$TMPDIR"
    done
    for s in ${lib.concatStringsSep " " (map (p: "${mutantsDir}/${p}") skips)}; do
      id="$(basename "$s" .skip)"
      gaps="$gaps{\"id\":\"$id\",\"why\":\"no patch: $(grep -m1 '^# skip:' "$s" | sed 's/^# skip: *//' | tr -d '"\\' | head -c 200)\"},"
    done
    items="[''${items%,}]"; gaps="[''${gaps%,}]"
    printf '{"holds":%s,"gaps":%s,"counterexamples":%s}\n' "$holds" "$gaps" "$items" > "$out/outcome.json"
  '';
}
