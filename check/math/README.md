# Correctly rounded reference values for `trame::math::f64`

**What it is.** `vectors.tsv` lists inputs of each f64 function with the result rounded once from
mpmath at 400 bits (nearest, ties to even, subnormals and overflow included). It does not use libm or
Rust std, which are themselves wrong for some functions (std's `atanh`, `acosh`, `asinh` are formulas
that lose hundreds of ulp near the ends). `src/math/f64/tests/vectors.rs` reads it.

**Regenerate.** From the repository root:

    nix develop "path:$PWD#math" -c python3 check/math/vectors.py > check/math/vectors.tsv

The seed is fixed, so the output is byte-identical on every run (sha256 `ea40712e…f34a` at the time of
writing). About 5 s, 1.7 MB, 24 functions, 1028 to 2560 lines each.

**A line.** `name TAB arg_bits [TAB arg_bits] TAB result_bits`. Each value is 16 hex digits of its
IEEE 754 binary64 bit pattern. One or two arguments, depending on the function. The last field is the
correctly rounded result. Inputs the function does not accept (e.g. `ln` of 0 or a negative) are not
written. Results that are NaN or infinite in the reference are not written; infinite results from
overflow are written as the infinity bits.

**What the test measures.** For each line, the ulp distance between trame's result and the result
field, using the bit-ordered integers of `tests.rs::ulps` (`+0` and `-0` are 0 apart; a NaN result is
an infinite distance). The test fails a function whose maximum exceeds `BOUND = 1`, and prints the table
with `--nocapture`.

**The claim.** For each listed input, the result field is the binary64 value nearest the exact value of the
function. Checked: the 400-bit reference equals the 600-bit one on 3500 sampled rows
(`check/math/recheck600.py`, 0 mismatches). The two rows that fail below (`log10`, `tanh`) also agree at
400, 600 and 1000 bits and with glibc.
Hand-checked: `exp(0)`, `exp(1)`, `ln(2)`, `ln(1)`, `exp2` at the
subnormal, underflow and overflow edges, `sqrt(4)`, `cbrt(±8)`.

**The limits.**

- The inputs are sampled (fixed seed, 512 per regime), not exhaustive. A bound holding here says
  nothing about an input that was not drawn; the regimes were chosen by hand.
- mpmath at 400 bits is assumed correct. A wrong rounding needs the exact value within about 2^-340 of
  a midpoint between two binary64 values; none was seen, and the 600-bit check agrees.
- The reference is correctly rounded, but the bound is 1 ulp (faithful rounding), not 0. A function
  can be off by 1 ulp on many inputs and pass. The table's `inexact` column counts those.
- The sign of a zero result is not compared (`ulps` treats `±0` as equal).
- Only the bit patterns in the file are tested. Special values (NaN, signed zero, infinities as inputs)
  are the job of `tests/special.rs`.

## Lanes benchmark (`check/math/bench.rs`)

`trame::math::f64` scalar loop against `trame::math::lanes::avx2` slice functions, 2^20 inputs, ns per element, minimum of
9 repetitions after 2 warmups, lane output checked bit-equal to scalar first. The lanes are opt-in: without
`--features lanes-avx2` the module does not exist, and the example prints the scalar column only.

Measured 2026-11-19 on an AMD Ryzen 5 PRO 4650U (AVX2 and FMA, no AVX-512), rustc 1.98.1, `RUSTFLAGS` unset, release
build, pinned with `taskset -c 3`, five alternating runs of each binary, median (range). Load average was 2 to 3 during
the runs, not idle. Scalar from the lanes binary and from the baseline binary agree within 10%.

| function | scalar | lanes | speedup |
|---|---|---|---|
| exp | 9.8 (8.9-13.3) | 12.7 (11.0-16.3) | 0.80 |
| ln | 9.5 (7.9-10.5) | 4.3 (3.8-4.6) | 2.26 |
| sqrt | 384 (344-394) | 1.73 (1.63-1.78) | 220 |
| floor | 2.7 (2.3-2.9) | 1.75 (1.7-1.9) | 1.5 |
| ceil | 3.1 (2.5-3.3) | 1.8 (1.7-2.0) | 1.7 |
| trunc | 2.0 (1.9-2.4) | 1.7 (1.7-2.1) | 1.2 |
| round_ties_even | 1.7 | 1.7 | 1.0 |
| max | 7.9 (6.2-9.1) | 2.1 | 3.7 |
| min | 8.2 (6.3-8.5) | 2.15 | 3.8 |
| mul_add | 33.7 (29-36) | 2.4 | 13.8 |

What the table does and does not say:
- `sqrt` and `mul_add` are large because the scalar versions are integer-arithmetic software (correctly rounded sqrt;
  fused multiply-add without FMA, `src/math/f64/fma.rs`). The lane versions use `vsqrtpd` and `vfmadd`, which are
  correctly rounded by hardware and so bit-identical. The speedup there is hardware against software, not width.
  A scalar `sqrt` through `sqrtsd` would give most of the 220 without lanes.
- `exp` is slower in lanes (0.80): not explained yet. Probably the guard or the table lookup per lane.
- Scalar functions are not `#[inline]`, so each scalar call pays a call, which the slice loops do not. Not separated.
- Bit-identity is tested (`src/math/lanes/tests.rs`), speed is not: sin and cos lanes cover only |x| <= pi/4, so on random
  arguments their speedup is near 1 and they are not in the table.
