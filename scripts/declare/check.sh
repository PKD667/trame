#!/usr/bin/env bash
#
# `#[parallel]`, `#[ordered]` and `concurrent!`, checked against the compiler that
# has to accept and refuse them.
#
#   trame/scripts/declare/check.sh              build trame, then run every stage
#   trame/scripts/declare/check.sh --only key   run only fixtures whose name contains `key`
#   trame/scripts/declare/check.sh --lowering   only the assembly comparison
#   trame/scripts/declare/check.sh --emit DIR   also leave the assembly and the LLVM IR there
#   trame/scripts/declare/check.sh --verbose    print every command and every message
#
# Three stages:
#
#   pass/     every fixture has to compile. These are the accepted grammar.
#   fail/     every fixture has to be refused, and the refusal has to say what the fixture
#             claims it says: a `//~ says:` or `// expect:` line at the top, one per claim. A
#             fixture that fails for the wrong reason counts as a failure — that is what keeps
#             the error messages honest rather than merely present. These are compiled the way
#             `cargo check` compiles, metadata only: a refusal that needs code generation to
#             appear is one an editor and a `cargo check` would not show, so this stage refuses
#             to count it.
#   lowering  an `invoke!` and its hand-written equivalent are compiled with `-O` and their
#             instructions compared. A lowering that started costing something at run time
#             would show up here as a difference.
#
# The fixtures depend on the backend rlib and on nothing else, which is the point: the execution
# surface is a backend surface, and this is the proof it can be checked without an application.

set -o pipefail
set -u

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
root=$(cd -- "$here/../../.." && pwd)
out=${TMPDIR:-/tmp}/declare-check.$$
mkdir -p "$out"
trap 'rm -rf "$out"' EXIT

verbose=0
only=""
emit=""
stages="pass fail lowering"
while [ $# -gt 0 ]; do
    case "$1" in
        --verbose | -v) verbose=1 ;;
        --only) only=$2; shift ;;
        --lowering) stages="lowering" ;;
        --emit) emit=$2; shift ;;
        --fixtures) stages="pass fail" ;;
        -h | --help) sed -n '2,28p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done

say() { printf '%s\n' "$*"; }
loud() { [ "$verbose" = 1 ] && printf '  %s\n' "$*"; return 0; }

# --- the backend the fixtures compile against ----------------------------------------------

cd "$root" || exit 2
say "building trame"
# The rlib cargo names for the default features: `ls -t` over deps would pick whichever feature
# selection was built last.
rlib=$(cargo build -p trame --offline --message-format=json \
    | grep -o '"filenames":\["[^"]*/libtrame\.rlib"' | grep -o '/[^"]*\.rlib') || exit 1
deps="$(dirname "$rlib")/deps"
loud "trame: $rlib"

# `--extern trame` is the whole dependency surface. The proc-macro crate is reached through
# trame's re-export, so it only has to be findable, not named.
compile() {
    rustc --edition 2024 --crate-type lib \
        -L "dependency=$deps" --extern "trame=$rlib" \
        --error-format human --color never \
        -o "$out/$(basename "$1" .rs).rlib" "$1" 2>&1
}

# The same, stopping where `cargo check` stops, so a refusal that needs code generation to appear
# cannot pass the fail stage.
check_only() {
    rustc --edition 2024 --crate-type lib \
        -L "dependency=$deps" --extern "trame=$rlib" \
        --error-format human --color never \
        --emit=metadata -o "$out/$(basename "$1" .rs).rmeta" "$1" 2>&1
}

pass_count=0
fail_count=0
bad=0

wanted() { [ -z "$only" ] || case "$1" in *"$only"*) return 0 ;; *) return 1 ;; esac; }

# --- pass ------------------------------------------------------------------------------------

if [[ " $stages " == *" pass "* ]]; then
    say ""
    say "accepted"
    for f in "$here"/pass/*.rs; do
        name=$(basename "$f" .rs)
        wanted "$name" || continue
        if report=$(compile "$f"); then
            say "  ok      $name"
            pass_count=$((pass_count + 1))
            [ "$verbose" = 1 ] && [ -n "$report" ] && printf '%s\n' "$report"
        else
            say "  FAILED  $name: refused, and should not have been"
            printf '%s\n' "$report" | sed 's/^/          /'
            bad=$((bad + 1))
        fi
    done
fi

# --- fail --------------------------------------------------------------------------------------

if [[ " $stages " == *" fail "* ]]; then
    say ""
    say "refused"
    for f in "$here"/fail/*.rs; do
        name=$(basename "$f" .rs)
        wanted "$name" || continue
        report=$(check_only "$f")
        status=$?
        if [ $status -eq 0 ]; then
            if compile "$f" > /dev/null 2>&1; then
                say "  FAILED  $name: accepted, and should not have been"
            else
                say "  FAILED  $name: only refused by a full build, not by a check"
            fi
            bad=$((bad + 1))
            continue
        fi
        missing=""
        claims=0
        while IFS= read -r want; do
            claims=$((claims + 1))
            case "$report" in
                *"$want"*) ;;
                *) missing="$missing
          wanted to be told: $want" ;;
            esac
        done < <(sed -n -e 's|^//~ says: *||p' -e 's|^// expect: *||p' "$f")
        if [ "$claims" = 0 ]; then
            say "  FAILED  $name: says nothing about why it should be refused"
            say '          add a "// expect: <part of the message>" line at the top'
            bad=$((bad + 1))
        elif [ -z "$missing" ]; then
            say "  ok      $name"
            fail_count=$((fail_count + 1))
            [ "$verbose" = 1 ] && printf '%s\n' "$report" | sed 's/^/          /'
        else
            say "  FAILED  $name: refused for the wrong reason$missing"
            printf '%s\n' "$report" | sed 's/^/          /'
            bad=$((bad + 1))
        fi
    done
fi

# --- lowering ------------------------------------------------------------------------------------

if [[ " $stages " == *" lowering "* ]]; then
    say ""
    say "what invoke! costs"
    asm="$out/lowering.s"
    if ! report=$(rustc --edition 2024 --crate-type lib -O --emit=asm \
        -C debuginfo=0 -C panic=abort \
        -L "dependency=$deps" --extern "trame=$rlib" \
        --error-format human --color never \
        -o "$asm" "$here/lowering.rs" 2>&1); then
        say "  FAILED  lowering.rs did not compile"
        printf '%s\n' "$report" | sed 's/^/          /'
        bad=$((bad + 1))
    else
        # Cut one `.globl`-exported function out of the assembly, and drop the parts that are
        # allowed to differ: the symbol name, local label numbers, and assembler directives.
        cut() {
            awk -v f="$1" '
                $0 ~ "^"f":" { on = 1; next }
                on && /^\t\.(size|cfi_endproc)/ { on = 0 }
                on { print }
            ' "$asm" | sed -e 's/\.L[A-Za-z0-9_$.]*/.L/g' -e "s/$1/FN/g" \
                -e '/^\s*\.\(cfi\|p2align\|section\|type\|size\|globl\|align\)/d' \
                -e '/^\s*#/d' -e '/^\s*$/d'
        }
        declared=$(cut declared_body)
        handwritten=$(cut handwritten_body)
        # The strongest outcome there is: the two bodies were identical, so the compiler folded
        # them into one symbol and made the other an alias for it.
        if grep -qE '^(handwritten_body = declared_body|declared_body = handwritten_body)$' "$asm"; then
            say "  ok      the two bodies are one symbol: the compiler folded them together"
            [ "$verbose" = 1 ] && grep -nE '_body' "$asm" | sed 's/^/          /'
        elif [ -z "$declared" ] || [ -z "$handwritten" ]; then
            say "  FAILED  could not find both bodies in the assembly ($asm)"
            bad=$((bad + 1))
        elif [ "$declared" = "$handwritten" ]; then
            n=$(printf '%s\n' "$declared" | wc -l)
            say "  ok      invoke! and the hand-written loop are the same $n instructions"
            [ "$verbose" = 1 ] && printf '%s\n' "$declared" | sed 's/^/          /'
        else
            say "  FAILED  invoke! changed the code that runs"
            diff <(printf '%s\n' "$handwritten") <(printf '%s\n' "$declared") | sed 's/^/          /'
            bad=$((bad + 1))
        fi
        # Kept for reading rather than for the comparison: the instructions the claim is about,
        # and the IR they came from, so "invoke! is the loop" can be checked by eye.
        if [ -n "$emit" ]; then
            mkdir -p "$emit"
            cp "$asm" "$emit/lowering.s"
            rustc --edition 2024 --crate-type lib -O --emit=llvm-ir \
                -C debuginfo=0 -C panic=abort \
                -L "dependency=$deps" --extern "trame=$rlib" \
                --error-format human --color never \
                -o "$emit/lowering.ll" "$here/lowering.rs" > /dev/null 2>&1 \
                && say "  ok      wrote $emit/lowering.s and $emit/lowering.ll" \
                || say "  note    wrote $emit/lowering.s; the IR emit failed"
        fi
    fi
fi

say ""
if [ "$bad" = 0 ]; then
    say "$pass_count accepted, $fail_count refused as claimed, lowering unchanged"
    exit 0
fi
say "$bad stage(s) wrong"
exit 1
