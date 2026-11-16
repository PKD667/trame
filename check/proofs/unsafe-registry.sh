#!/bin/sh
# The unsafe registry check (check/proofs/README.md). Reads source only; runs nothing.
#   sh check/proofs/unsafe-registry.sh ROOT
# Every `unsafe` keyword in the Rust code under ROOT, outside directories named driver, rejections, tests,
# experiments, proofs, target and mpi-rma (at any depth), must have a comment block directly above its line
# (attributes may sit between) or a trailing comment on its line that holds `SAFETY:` or `# Safety`, and cites
# at least one name that ROOT/CONTRACT.md defines (a clause, `**S1 Exclusion.**`) or ROOT/PROOFS.md defines as
# an obligation (`**O-seg-1 Mapped.**`, if any). A cited name that is not defined fails. A defined obligation that
# no site cites fails. Exit 0 when all hold; every failure is printed as `file:line: reason`. The last line
# counts the unsafe sites and the names cited.
set -eu
root=${1:?usage: unsafe-registry.sh ROOT}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

grep -oE '^\*\*[A-Z][0-9]+ ' "$root/CONTRACT.md" | tr -d '* ' | sort -u > "$tmp/clauses"
grep -oE '^\*\*O-[a-z]+-[0-9]+ ' "$root/PROOFS.md" | tr -d '* ' | sort -u > "$tmp/obligations"
cat "$tmp/clauses" "$tmp/obligations" > "$tmp/defined"

find "$root" \( -name driver -o -name rejections -o -name tests -o -name experiments -o -name proofs \
  -o -name target -o -name mpi-rma -o -name .git \) -prune -o -name '*.rs' -print | sort > "$tmp/files"

: > "$tmp/cited"
: > "$tmp/fail"
: > "$tmp/sites"
: > "$tmp/ok"
while read -r f; do
  awk -v file="${f#"$root"/}" -v defined="$tmp/defined" -v cited="$tmp/cited" -v sites="$tmp/sites" -v ok="$tmp/ok" '
    BEGIN { while ((getline d < defined) > 0) def[d] = 1 }
    { L[NR] = $0 }
    function code(s) {
      gsub(/"([^"\\]|\\.)*"/, "\"\"", s)
      sub(/\/\/.*/, "", s)
      return s
    }
    function iscomment(s) { return s ~ /^[ \t]*\/\// }
    function isattr(s) { return s ~ /^[ \t]*#\[/ }
    function names(s,   out, m) {
      out = ""
      while (match(s, /(^|[^A-Za-z0-9_-])([RLSTPME][0-9]+|O-[a-z]+-[0-9]+)([^A-Za-z0-9_-]|$)/)) {
        m = substr(s, RSTART, RLENGTH)
        sub(/^[^A-Za-z0-9]/, "", m)
        sub(/[^0-9]$/, "", m)
        out = out " " m
        s = substr(s, RSTART + RLENGTH - 1)
      }
      return out
    }
    END {
      for (i = 1; i <= NR; i++) {
        c = code(L[i])
        if (c !~ /(^|[^A-Za-z0-9_])unsafe([^A-Za-z0-9_]|$)/) continue
        print file ":" i >> sites
        text = ""
        if (L[i] ~ /\/\/.*(SAFETY:|# Safety)/) text = L[i]
        j = i - 1
        while (j >= 1 && isattr(L[j])) j--
        while (j >= 1 && iscomment(L[j])) { text = L[j] " " text; j-- }
        if (text !~ /SAFETY:|# Safety/) { print file ":" i ": unsafe without a SAFETY comment or # Safety section"; continue }
        n = split(names(text), nm, " ")
        good = 0
        for (k = 1; k <= n; k++) {
          if (nm[k] in def) { good = 1; print nm[k] >> cited }
          else print file ":" i ": cites " nm[k] ", which CONTRACT.md and PROOFS.md do not define"
        }
        if (good) print file ":" i >> ok
        else print file ":" i ": SAFETY names no clause or obligation"
      }
    }
  ' "$f" >> "$tmp/fail"
done < "$tmp/files"

sort -u "$tmp/cited" > "$tmp/cited.u"
while read -r o; do
  grep -qx "$o" "$tmp/cited.u" || echo "PROOFS.md: obligation $o is cited by no unsafe site" >> "$tmp/fail"
done < "$tmp/obligations"

sites=$(wc -l < "$tmp/sites")
safe=$(wc -l < "$tmp/ok")
echo "unsafe sites: $sites; SAFETY names a defined name: $safe; not: $((sites - safe)); names cited: $(wc -l < "$tmp/cited.u") of $(wc -l < "$tmp/defined") defined"
if [ -s "$tmp/fail" ]; then cat "$tmp/fail"; exit 1; fi
