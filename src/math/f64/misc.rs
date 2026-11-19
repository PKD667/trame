//! log2, log10, exp2, cbrt, hypot — ported from fdlibm (netlib.org) and FreeBSD msun.
//! Ported from: fdlibm e_log10.c (sha256 0d636f23...), e_hypot.c (87425adc...), 
//! s_cbrt.c (7ce1c729...), and FreeBSD msun e_log2.c (d198050a...), k_log.h (ebab5a01...), 
//! s_exp2.c (f8c8bd96...).
//! Plain IEEE arithmetic and word access only; no libm, no FMA, no platform transcendental.

use super::bits::{hi, lo, with_hi, with_lo};
use super::ln;
use super::sqrt;

// ============================================================================
// log10: fdlibm e_log10.c
// ============================================================================

/// Base-10 logarithm, fdlibm `e_log10.c`: `n = ilogb(x)` (with `n+1` for `n < 0`), `x` scaled to
/// `[1,2)` or `[1/2,1)`, then `n*log10_2hi + (n*log10_2lo + ivln10*ln(x))`.
/// Error below 1 ulp (fdlibm's claim). Special cases: log10(0)=-inf, log10(-x)=NaN, log10(+inf)=+inf.
pub fn log10(x: f64) -> f64 {
    const TWO54: f64 = 1.80143985094819840000e+16;
    const IVLN10: f64 = 4.34294481903251816668e-01;
    const LOG10_2HI: f64 = 3.01029995663611771306e-01;
    const LOG10_2LO: f64 = 3.69423907715893078616e-13;

    let mut x = x;
    let mut hx = hi(x) as i32;
    let lx = lo(x);
    let mut k = 0_i32;

    if hx < 0x0010_0000 {
        if (((hx & 0x7fff_ffff) as u32) | lx) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k -= 54;
        x *= TWO54;
        hx = hi(x) as i32; // C re-reads the high word after scaling
    }
    if hx >= 0x7ff0_0000 {
        return x + x;
    }
    k += (hx >> 20) - 1023;
    let i = ((k as u32) & 0x8000_0000) >> 31; // 1 if k < 0
    let hx_new = ((hx as u32) & 0x000f_ffff) | ((0x3ff - i) << 20);
    let y = f64::from(k + i as i32);
    x = with_hi(x, hx_new);
    // The fdlibm form `z + y*LOG10_2HI` with `z = y*LOG10_2LO + IVLN10*ln(x)` rounds ln(x) to 53
    // bits before the multiply by 1/ln10, which is 0.86 ulp of log10 at 0.59 (measured). Here ln(x)
    // is kept as an unrounded pair and the sum is taken in double-double.
    let (lh, ll) = ln_dd(x);
    let (p, pe) = two_prod(IVLN10, lh);
    let (s, e) = two_sum(y * LOG10_2HI, p);
    let tail = e + pe + (IVLN10_LO * lh + IVLN10 * ll + y * LOG10_2LO);
    s + tail
}

/// `IVLN10_LO` = 1/ln10 - IVLN10, pinned to bits 0x3C695355BAAAFAD3.
const IVLN10_LO: f64 = f64::from_bits(0x3c69_5355_baaa_fad3);

/// `a + b` as `(s, e)` with `s = fl(a + b)` and `s + e = a + b` exactly (Knuth, no FMA).
pub(super) fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

/// `a * b` as `(p, e)` with `p = fl(a * b)` and `p + e = a * b` exactly (Dekker, no FMA).
pub(super) fn two_prod(a: f64, b: f64) -> (f64, f64) {
    fn split(a: f64) -> (f64, f64) {
        let c = 134_217_729.0 * a; // 2^27 + 1
        let h = c - (c - a);
        (h, a - h)
    }
    let p = a * b;
    let (ah, al) = split(a);
    let (bh, bl) = split(b);
    (p, ((ah * bh - p) + ah * bl + al * bh) + al * bl)
}

/// `ln(x)` for `x` in `[1/2, 2)`, as an unrounded pair `(hi, lo)` with `hi + lo = ln(x)` to about
/// 2^-60 relative. The steps are those of `ln.rs`; each branch returns its final sum before the
/// last rounding (`ln` returns `hi - lo`, rounded once).
fn ln_dd(x: f64) -> (f64, f64) {
    let hx0 = hi(x) as i32 & 0x000f_ffff;
    let i0 = (hx0 + 0x95f64) & 0x0010_0000;
    let xm = with_hi(x, (hx0 | (i0 ^ 0x3ff0_0000)) as u32);
    // Exponent of x (-1 or 0 for x in [1/2, 2)) plus the sqrt(2) split of ln.rs (0 or 1).
    let k = ((hi(x) as i32 >> 20) - 1023) + (i0 >> 20);
    let dk = f64::from(k);
    let f = xm - 1.0;
    if (0x000f_ffff & (2 + hx0)) < 3 {
        if f == 0.0 {
            return (dk * ln::LN2_HI, dk * ln::LN2_LO);
        }
        let r = f * f * (0.5 - 0.333_333_333_333_333_33 * f);
        if k == 0 {
            return (f, -r);
        }
        return (dk * ln::LN2_HI, -((r - dk * ln::LN2_LO) - f));
    }
    let s = f / (2.0 + f);
    let z = s * s;
    let w = z * z;
    let t1 = w * (ln::LG2 + w * (ln::LG4 + w * ln::LG6));
    let t2 = z * (ln::LG1 + w * (ln::LG3 + w * (ln::LG5 + w * ln::LG7)));
    let r = t2 + t1;
    let i = (hx0 - 0x6147a) | (0x6b851 - hx0);
    if i > 0 {
        let hfsq = 0.5 * f * f;
        if k == 0 {
            return (f, -(hfsq - s * (hfsq + r)));
        }
        return (dk * ln::LN2_HI, -((hfsq - (s * (hfsq + r) + dk * ln::LN2_LO)) - f));
    }
    if k == 0 {
        return (f, -(s * (f - r)));
    }
    (dk * ln::LN2_HI, -((s * (f - r) - dk * ln::LN2_LO) - f))
}

// ============================================================================
// cbrt: cube root via Newton iteration
// ============================================================================

/// Cube root of x. Exact for cbrt(-8)=-2 and cbrt(27)=3.
/// Error below 1 ulp. Special cases: cbrt(0)=0 with sign preserved.
pub fn cbrt(x: f64) -> f64 {
    const B1: u32 = 715_094_163;
    const B2: u32 = 696_219_795;
    const C: f64 = 5.42857142857142815906e-01;
    const D: f64 = -7.05306122448979611050e-01;
    const E: f64 = 1.41428571428571436819e+00;
    const F: f64 = 1.60714285714285720630e+00;
    const G: f64 = 3.57142857142857150787e-01;

    let hx = hi(x) as i32;
    let sign = (hx as u32) & 0x8000_0000;
    let hx_abs = (hx as u32) ^ sign;

    if hx_abs >= 0x7ff0_0000 {
        return x + x;
    }
    if (hx_abs | lo(x)) == 0 {
        return x;
    }

    let xx = if sign != 0 { -x } else { x };
    let xx_hi = hi(xx);

    let mut t = if xx_hi < 0x0010_0000 {
        let mut temp = with_hi(0.0, 0x4350_0000);
        temp *= xx;
        with_hi(temp, hi(temp).wrapping_div(3).wrapping_add(B2))
    } else {
        with_hi(0.0, xx_hi.wrapping_div(3).wrapping_add(B1))
    };

    // Newton iteration
    let r = t * t / xx;
    let s = C + r * t;
    t *= G + F / (s + E + D / s);

    // Increase precision
    t = with_lo(t, 0);
    t = with_hi(t, hi(t).wrapping_add(1));

    // Final Newton step
    let s = t * t;
    let r = xx / s;
    let w = t + t;
    let r = (r - t) / (w + r);
    t = t + t * r;

    with_hi(t, hi(t) | sign)
}

// ============================================================================
// hypot: fdlibm e_hypot.c
// ============================================================================

/// Euclidean norm `sqrt(x^2 + y^2)`, fdlibm `e_hypot.c`: the larger argument is `a`, both high words
/// are taken in absolute value, the operands are scaled by `2^±600` (or `2^-1022` for a subnormal
/// `b`) out of the overflow and underflow ranges, and the final `sqrt` is evaluated on an exact
/// split `t1 + t2` of `a`. Error below 1 ulp (fdlibm's claim).
/// Special cases: hypot(±inf, y)=+inf even if y is NaN, hypot(x, NaN)=NaN, hypot(0,0)=0.
pub fn hypot(x: f64, y: f64) -> f64 {
    // a is the argument with the larger |high word|; ha, hb are the (absolute) high words.
    let mut ha = (hi(x) & 0x7fff_ffff) as i32;
    let mut hb = (hi(y) & 0x7fff_ffff) as i32;
    let (mut a, mut b) = if hb > ha {
        core::mem::swap(&mut ha, &mut hb);
        (y, x)
    } else {
        (x, y)
    };
    a = with_hi(a, ha as u32); // a <- |a|
    b = with_hi(b, hb as u32); // b <- |b|

    if ha - hb > 0x3c00000 {
        return a + b; // x/y > 2^60
    }

    let mut k = 0_i32;
    if ha > 0x5f30_0000 {
        // a > 2^500
        if ha >= 0x7ff0_0000 {
            // Inf or NaN
            let mut w = a + b;
            if (((ha & 0xf_ffff) as u32) | lo(a)) == 0 {
                w = a;
            }
            if (((hb ^ 0x7ff0_0000) as u32) | lo(b)) == 0 {
                w = b;
            }
            return w;
        }
        // scale a and b by 2^-600
        ha -= 0x2580_0000;
        hb -= 0x2580_0000;
        k += 600;
        a = with_hi(a, ha as u32);
        b = with_hi(b, hb as u32);
    }
    if hb < 0x20b0_0000 {
        // b < 2^-500
        if hb <= 0x000f_ffff {
            // subnormal b or 0
            if ((hb as u32) | lo(b)) == 0 {
                return a;
            }
            let t1 = with_hi(0.0, 0x7fd0_0000); // 2^1022
            b *= t1;
            a *= t1;
            k -= 1022;
        } else {
            // scale a and b by 2^600
            ha += 0x2580_0000;
            hb += 0x2580_0000;
            k -= 600;
            a = with_hi(a, ha as u32);
            b = with_hi(b, hb as u32);
        }
    }

    // medium size a and b
    let mut w = a - b;
    if w > b {
        let t1 = with_hi(0.0, ha as u32);
        let t2 = a - t1;
        w = sqrt(t1 * t1 - (b * (-b) - t2 * (a + t1)));
    } else {
        a += a;
        let y1 = with_hi(0.0, hb as u32);
        let y2 = b - y1;
        let t1 = with_hi(0.0, (ha + 0x0010_0000) as u32);
        let t2 = a - t1;
        w = sqrt(t1 * y1 - (w * (-w) - (t1 * y2 + t2 * b)));
    }

    if k != 0 {
        // t1 = 2^k: the high word of 1.0 plus k << 20
        let t1 = with_hi(1.0, (0x3ff0_0000_i32.wrapping_add(k << 20)) as u32);
        t1 * w
    } else {
        w
    }
}

// ============================================================================
// log2: FreeBSD e_log2.c (extra-precision ln(x)/ln 2 with ivln2hi having trailing zero bits)
// ============================================================================

/// Base-2 logarithm, FreeBSD msun `e_log2.c`. Error below 0.55 ulp (FreeBSD's claim).
/// log2(1) = 0 exactly, log2(2^n) = n exactly. Special cases: log2(0)=-inf, log2(-x)=NaN,
/// log2(+inf)=+inf.
pub fn log2(x: f64) -> f64 {
    const TWO54: f64 = 1.80143985094819840000e+16;
    // 1/ln2 split: ivln2hi has 21 trailing zero bits, so hi * ivln2hi is exact.
    const IVLN2HI: f64 = 1.44269504072144627571e+00;
    const IVLN2LO: f64 = 1.67517131648865118353e-10;

    const LG1: f64 = 6.666666666666735130e-01;
    const LG2: f64 = 3.999999999940941908e-01;
    const LG3: f64 = 2.857142874366239149e-01;
    const LG4: f64 = 2.222219843214978396e-01;
    const LG5: f64 = 1.818357216161805012e-01;
    const LG6: f64 = 1.531383769920937332e-01;
    const LG7: f64 = 1.479819860511658591e-01;

    let mut x = x;
    let mut hx = hi(x) as i32;
    let lx = lo(x);
    let mut k = 0_i32;

    if hx < 0x0010_0000 {
        if (((hx & 0x7fff_ffff) as u32) | lx) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k -= 54;
        x *= TWO54;
        hx = hi(x) as i32; // C re-reads the high word after scaling
    }
    if hx >= 0x7ff0_0000 {
        return x + x;
    }
    if hx == 0x3ff0_0000 && lx == 0 {
        return 0.0;
    }
    k += (hx >> 20) - 1023;
    let hxm = (hx as u32) & 0x000f_ffff;
    let i = hxm.wrapping_add(0x0009_5f64) & 0x0010_0000;
    x = with_hi(x, hxm | (i ^ 0x3ff0_0000));
    k += (i >> 20) as i32;

    let y = f64::from(k);
    let f = x - 1.0;
    let hfsq = 0.5 * f * f;

    // k_log1p(f), inlined as in FreeBSD k_log.h
    let s = f / (2.0 + f);
    let z = s * s;
    let w = z * z;
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    let r = t2 + t1;
    let r = s * (hfsq + r);

    // f - hfsq in extra precision: hi has its low word cleared
    let h = with_lo(f - hfsq, 0);
    let l = (f - h) - hfsq + r;
    let val_hi = h * IVLN2HI;
    let mut val_lo = (l + h) * IVLN2LO + l * IVLN2HI;

    // spadd(val_hi, val_lo, y)
    let w = y + val_hi;
    val_lo += (y - w) + val_hi;
    let val_hi = w;

    val_lo + val_hi
}

// ============================================================================
// exp2: FreeBSD s_exp2.c (table of 2^(i/256) with correction, degree-5 polynomial)
// ============================================================================

const HUGE: f64 = f64::from_bits(0x7e70_0000_0000_0000); // 2^1000
const TWOM1000: f64 = f64::from_bits(0x0170_0000_0000_0000); // 2^-1000
const REDUX: f64 = f64::from_bits(0x42b8_0000_0000_0000); // 0x1.8p52 / 256 = 0x1.8p44
const P1: f64 = f64::from_bits(0x3fe6_2e42_fefa_39ef);
const P2: f64 = f64::from_bits(0x3fce_bfbd_ff82_c575);
const P3: f64 = f64::from_bits(0x3fac_6b08_d704_a0a6);
const P4: f64 = f64::from_bits(0x3f83_b2ab_88f7_0400);
const P5: f64 = f64::from_bits(0x3f55_d880_0387_5c74);

/// Base-2 exponential, FreeBSD msun `s_exp2.c`: `x = k + y` with `|y| <= 1/2`, `y` reduced to
/// `i/256 + z - eps[i]` via table lookup, `2^(i/256)` from the table and `2^z` from a polynomial,
/// then scaled by `2^k`. Error below 0.503 ulp for normal results (FreeBSD's claim).
/// exp2(-inf)=0, exp2(+inf)=inf, exp2(n) exact for integers n in range.
pub fn exp2(x: f64) -> f64 {
    let hx = hi(x);
    let ix = hx & 0x7fff_ffff;

    // Filter out exceptional cases.
    if ix >= 0x4090_0000 {
        // |x| >= 1024
        if ix >= 0x7ff0_0000 {
            let lx = lo(x);
            if ((ix & 0xf_ffff) | lx) != 0 || (hx & 0x8000_0000) == 0 {
                return x + x; // NaN or +Inf
            }
            return 0.0; // -Inf
        }
        if x >= 1024.0 {
            return HUGE * HUGE; // overflow
        }
        if x <= -1075.0 {
            return TWOM1000 * TWOM1000; // underflow
        }
    } else if ix < 0x3c90_0000 {
        // |x| < 2^-54
        return 1.0 + x;
    }

    // Reduce x, computing z, i0, and k.
    let t = x + REDUX;
    let i0 = lo(t).wrapping_add(128); // TBLSIZE / 2
    let k = ((i0 >> 8) << 20) as i32;
    let idx = ((i0 & 255) << 1) as usize;
    let t = t - REDUX;
    let z = x - t;

    // Compute r = exp2(y) = exp2t[i0] * p(z - eps[i]).
    let tt = f64::from_bits(EXP2_TBL[idx]); // exp2t[i0]
    let z = z - f64::from_bits(EXP2_TBL[idx + 1]); // eps[i0]
    let r = tt + tt * z * (P1 + z * (P2 + z * (P3 + z * (P4 + z * P5))));

    // Scale by 2^(k >> 20).
    if k >= -(1021 << 20) {
        if k == 1024 << 20 {
            return r * 2.0 * f64::from_bits(0x7fe0_0000_0000_0000); // 2^1023
        }
        let twopk = with_hi(1.0, 0x3ff0_0000_i32.wrapping_add(k) as u32);
        r * twopk
    } else {
        let twopkp1000 = with_hi(1.0, 0x3ff0_0000_i32.wrapping_add(k).wrapping_add(1000 << 20) as u32);
        r * twopkp1000 * TWOM1000
    }
}

/// FreeBSD s_exp2.c `tbl` (sha256 f8c8bd96...): pairs (exp2t[i], eps[i]) for i = 0..256 as f64
/// bit patterns, interleaved as in the C. Mechanical copy: the hex-float text of the C is in the comments.
static EXP2_TBL: [u64; 512] = [
    0x3fe6a09e667f3d5d, 0x3d39880000000000, // 0x1.6a09e667f3d5dp-1, 0x1.9880p-44
    0x3fe6b052fa751744, 0x3cd8000000000000, // 0x1.6b052fa751744p-1, 0x1.8000p-50
    0x3fe6c012750bd9fe, 0xbd28780000000000, // 0x1.6c012750bd9fep-1, -0x1.8780p-45
    0x3fe6cfdcddd476bf, 0x3d1ec00000000000, // 0x1.6cfdcddd476bfp-1, 0x1.ec00p-46
    0x3fe6dfb23c651a29, 0xbcd8000000000000, // 0x1.6dfb23c651a29p-1, -0x1.8000p-50
    0x3fe6ef9298593ae3, 0xbcbc000000000000, // 0x1.6ef9298593ae3p-1, -0x1.c000p-52
    0x3fe6ff7df9519386, 0xbd2fd80000000000, // 0x1.6ff7df9519386p-1, -0x1.fd80p-45
    0x3fe70f7466f42da3, 0xbd2c880000000000, // 0x1.70f7466f42da3p-1, -0x1.c880p-45
    0x3fe71f75e8ec5fc3, 0x3d13c00000000000, // 0x1.71f75e8ec5fc3p-1, 0x1.3c00p-46
    0x3fe72f8286eacf05, 0xbd38300000000000, // 0x1.72f8286eacf05p-1, -0x1.8300p-44
    0x3fe73f9a48a58152, 0xbd00c00000000000, // 0x1.73f9a48a58152p-1, -0x1.0c00p-47
    0x3fe74fbd35d7ccfc, 0x3d2f880000000000, // 0x1.74fbd35d7ccfcp-1, 0x1.f880p-45
    0x3fe75feb564267f1, 0x3d03e00000000000, // 0x1.75feb564267f1p-1, 0x1.3e00p-47
    0x3fe77024b1ab6d48, 0xbd27d00000000000, // 0x1.77024b1ab6d48p-1, -0x1.7d00p-45
    0x3fe780694fde5d38, 0xbcdd000000000000, // 0x1.780694fde5d38p-1, -0x1.d000p-50
    0x3fe790b938ac1d00, 0x3ce3000000000000, // 0x1.790b938ac1d00p-1, 0x1.3000p-49
    0x3fe7a11473eb0178, 0xbced000000000000, // 0x1.7a11473eb0178p-1, -0x1.d000p-49
    0x3fe7b17b0976d060, 0x3d20400000000000, // 0x1.7b17b0976d060p-1, 0x1.0400p-45
    0x3fe7c1ed0130c133, 0x3ca0000000000000, // 0x1.7c1ed0130c133p-1, 0x1.0000p-53
    0x3fe7d26a62ff8636, 0xbd26900000000000, // 0x1.7d26a62ff8636p-1, -0x1.6900p-45
    0x3fe7e2f336cf4e3b, 0xbd02e00000000000, // 0x1.7e2f336cf4e3bp-1, -0x1.2e00p-47
    0x3fe7f3878491c3e8, 0xbd24580000000000, // 0x1.7f3878491c3e8p-1, -0x1.4580p-45
    0x3fe80427543e1b4e, 0x3d33000000000000, // 0x1.80427543e1b4ep-1, 0x1.3000p-44
    0x3fe814d2add1071a, 0x3d0f000000000000, // 0x1.814d2add1071ap-1, 0x1.f000p-47
    0x3fe82589994ccd7e, 0xbd21c00000000000, // 0x1.82589994ccd7ep-1, -0x1.1c00p-45
    0x3fe8364c1eb942d0, 0x3d29d00000000000, // 0x1.8364c1eb942d0p-1, 0x1.9d00p-45
    0x3fe8471a4623cab5, 0x3d47100000000000, // 0x1.8471a4623cab5p-1, 0x1.7100p-43
    0x3fe857f4179f5bbc, 0x3d22600000000000, // 0x1.857f4179f5bbcp-1, 0x1.2600p-45
    0x3fe868d99b4491af, 0xbd32c40000000000, // 0x1.868d99b4491afp-1, -0x1.2c40p-44
    0x3fe879cad931a395, 0xbd23000000000000, // 0x1.879cad931a395p-1, -0x1.3000p-45
    0x3fe88ac7d98a65b8, 0xbd2a800000000000, // 0x1.88ac7d98a65b8p-1, -0x1.a800p-45
    0x3fe89bd0a4785800, 0xbced000000000000, // 0x1.89bd0a4785800p-1, -0x1.d000p-49
    0x3fe8ace5422aa223, 0x3d33280000000000, // 0x1.8ace5422aa223p-1, 0x1.3280p-44
    0x3fe8be05bad619fa, 0x3d42b40000000000, // 0x1.8be05bad619fap-1, 0x1.2b40p-43
    0x3fe8cf3216b54383, 0xbd2ed00000000000, // 0x1.8cf3216b54383p-1, -0x1.ed00p-45
    0x3fe8e06a5e08664c, 0xbd20500000000000, // 0x1.8e06a5e08664cp-1, -0x1.0500p-45
    0x3fe8f1ae99157807, 0x3d28280000000000, // 0x1.8f1ae99157807p-1, 0x1.8280p-45
    0x3fe902fed0282c0e, 0xbd1cb00000000000, // 0x1.902fed0282c0ep-1, -0x1.cb00p-46
    0x3fe9145b0b91ff96, 0xbd05e00000000000, // 0x1.9145b0b91ff96p-1, -0x1.5e00p-47
    0x3fe925c353aa2ff9, 0x3cf5400000000000, // 0x1.925c353aa2ff9p-1, 0x1.5400p-48
    0x3fe93737b0cdc64a, 0x3d17200000000000, // 0x1.93737b0cdc64ap-1, 0x1.7200p-46
    0x3fe948b82b5f98ae, 0xbd09000000000000, // 0x1.948b82b5f98aep-1, -0x1.9000p-47
    0x3fe95a44cbc852cb, 0x3d25680000000000, // 0x1.95a44cbc852cbp-1, 0x1.5680p-45
    0x3fe96bdd9a766f21, 0xbd36d00000000000, // 0x1.96bdd9a766f21p-1, -0x1.6d00p-44
    0x3fe97d829fde4e2a, 0xbd01000000000000, // 0x1.97d829fde4e2ap-1, -0x1.1000p-47
    0x3fe98f33e47a23a3, 0x3d2d000000000000, // 0x1.98f33e47a23a3p-1, 0x1.d000p-45
    0x3fe9a0f170ca0604, 0xbd38a40000000000, // 0x1.9a0f170ca0604p-1, -0x1.8a40p-44
    0x3fe9b2bb4d53ff89, 0x3d355c0000000000, // 0x1.9b2bb4d53ff89p-1, 0x1.55c0p-44
    0x3fe9c49182a3f15b, 0x3d26b80000000000, // 0x1.9c49182a3f15bp-1, 0x1.6b80p-45
    0x3fe9d674194bb8c5, 0xbcec000000000000, // 0x1.9d674194bb8c5p-1, -0x1.c000p-49
    0x3fe9e86319e3238e, 0x3d17d00000000000, // 0x1.9e86319e3238ep-1, 0x1.7d00p-46
    0x3fe9fa5e8d07f302, 0x3d16400000000000, // 0x1.9fa5e8d07f302p-1, 0x1.6400p-46
    0x3fea0c667b5de54d, 0xbcf5000000000000, // 0x1.a0c667b5de54dp-1, -0x1.5000p-48
    0x3fea1e7aed8eb8f6, 0x3d09e00000000000, // 0x1.a1e7aed8eb8f6p-1, 0x1.9e00p-47
    0x3fea309bec4a2e27, 0x3d2ad80000000000, // 0x1.a309bec4a2e27p-1, 0x1.ad80p-45
    0x3fea42c980460a5d, 0xbd1af00000000000, // 0x1.a42c980460a5dp-1, -0x1.af00p-46
    0x3fea5503b23e259b, 0x3d0b600000000000, // 0x1.a5503b23e259bp-1, 0x1.b600p-47
    0x3fea674a8af46213, 0x3d38880000000000, // 0x1.a674a8af46213p-1, 0x1.8880p-44
    0x3fea799e1330b3a7, 0x3d11200000000000, // 0x1.a799e1330b3a7p-1, 0x1.1200p-46
    0x3fea8bfe53c12e8d, 0x3d06c00000000000, // 0x1.a8bfe53c12e8dp-1, 0x1.6c00p-47
    0x3fea9e6b5579fcd2, 0xbd29b80000000000, // 0x1.a9e6b5579fcd2p-1, -0x1.9b80p-45
    0x3feab0e521356fb8, 0x3d2b700000000000, // 0x1.ab0e521356fb8p-1, 0x1.b700p-45
    0x3feac36bbfd3f381, 0x3cd9000000000000, // 0x1.ac36bbfd3f381p-1, 0x1.9000p-50
    0x3fead5ff3a3c2780, 0x3ce4000000000000, // 0x1.ad5ff3a3c2780p-1, 0x1.4000p-49
    0x3feae89f995ad2a3, 0xbd2c900000000000, // 0x1.ae89f995ad2a3p-1, -0x1.c900p-45
    0x3feafb4ce622f367, 0x3d16500000000000, // 0x1.afb4ce622f367p-1, 0x1.6500p-46
    0x3feb0e07298db790, 0x3d2fd40000000000, // 0x1.b0e07298db790p-1, 0x1.fd40p-45
    0x3feb20ce6c9a89a9, 0x3d12700000000000, // 0x1.b20ce6c9a89a9p-1, 0x1.2700p-46
    0x3feb33a2b84f1a4b, 0x3d4d470000000000, // 0x1.b33a2b84f1a4bp-1, 0x1.d470p-43
    0x3feb468415b747e7, 0xbd38380000000000, // 0x1.b468415b747e7p-1, -0x1.8380p-44
    0x3feb59728de5593a, 0x3c98000000000000, // 0x1.b59728de5593ap-1, 0x1.8000p-54
    0x3feb6c6e29f1c56a, 0x3d0ad00000000000, // 0x1.b6c6e29f1c56ap-1, 0x1.ad00p-47
    0x3feb7f76f2fb5e50, 0x3cde800000000000, // 0x1.b7f76f2fb5e50p-1, 0x1.e800p-50
    0x3feb928cf22749b2, 0xbd04c00000000000, // 0x1.b928cf22749b2p-1, -0x1.4c00p-47
    0x3feba5b030a10603, 0xbd0d700000000000, // 0x1.ba5b030a10603p-1, -0x1.d700p-47
    0x3febb8e0b79a6f66, 0x3d0d900000000000, // 0x1.bb8e0b79a6f66p-1, 0x1.d900p-47
    0x3febcc1e904bc1ff, 0x3d02a00000000000, // 0x1.bcc1e904bc1ffp-1, 0x1.2a00p-47
    0x3febdf69c3f3a16f, 0xbd1f780000000000, // 0x1.bdf69c3f3a16fp-1, -0x1.f780p-46
    0x3febf2c25bd71db8, 0xbd10a00000000000, // 0x1.bf2c25bd71db8p-1, -0x1.0a00p-46
    0x3fec06286141b2e9, 0xbd11400000000000, // 0x1.c06286141b2e9p-1, -0x1.1400p-46
    0x3fec199bdd8552e0, 0x3d0be00000000000, // 0x1.c199bdd8552e0p-1, 0x1.be00p-47
    0x3fec2d1cd9fa64ee, 0xbd09400000000000, // 0x1.c2d1cd9fa64eep-1, -0x1.9400p-47
    0x3fec40ab5fffd02f, 0xbd0ed00000000000, // 0x1.c40ab5fffd02fp-1, -0x1.ed00p-47
    0x3fec544778fafd15, 0x3d39660000000000, // 0x1.c544778fafd15p-1, 0x1.9660p-44
    0x3fec67f12e57d0cb, 0xbd1a100000000000, // 0x1.c67f12e57d0cbp-1, -0x1.a100p-46
    0x3fec7ba88988c1b6, 0xbd58458000000000, // 0x1.c7ba88988c1b6p-1, -0x1.8458p-42
    0x3fec8f6d9406e733, 0xbd1a480000000000, // 0x1.c8f6d9406e733p-1, -0x1.a480p-46
    0x3feca3405751c4df, 0x3ccb000000000000, // 0x1.ca3405751c4dfp-1, 0x1.b000p-51
    0x3fecb720dcef9094, 0x3d01400000000000, // 0x1.cb720dcef9094p-1, 0x1.1400p-47
    0x3feccb0f2e6d1689, 0x3cf0200000000000, // 0x1.ccb0f2e6d1689p-1, 0x1.0200p-48
    0x3fecdf0b555dc412, 0x3cf3600000000000, // 0x1.cdf0b555dc412p-1, 0x1.3600p-48
    0x3fecf3155b5bab3b, 0xbd06900000000000, // 0x1.cf3155b5bab3bp-1, -0x1.6900p-47
    0x3fed072d4a0789bc, 0x3d09a00000000000, // 0x1.d072d4a0789bcp-1, 0x1.9a00p-47
    0x3fed1b532b08c8fa, 0xbd15e00000000000, // 0x1.d1b532b08c8fap-1, -0x1.5e00p-46
    0x3fed2f87080d8a85, 0x3d1d280000000000, // 0x1.d2f87080d8a85p-1, 0x1.d280p-46
    0x3fed43c8eacaa203, 0x3d01a00000000000, // 0x1.d43c8eacaa203p-1, 0x1.1a00p-47
    0x3fed5818dcfba491, 0x3cdf000000000000, // 0x1.d5818dcfba491p-1, 0x1.f000p-50
    0x3fed6c76e862e6a1, 0xbd03a00000000000, // 0x1.d6c76e862e6a1p-1, -0x1.3a00p-47
    0x3fed80e316c9834e, 0xbd0cd80000000000, // 0x1.d80e316c9834ep-1, -0x1.cd80p-47
    0x3fed955d71ff6090, 0x3cf4c00000000000, // 0x1.d955d71ff6090p-1, 0x1.4c00p-48
    0x3feda9e603db32ae, 0x3cff900000000000, // 0x1.da9e603db32aep-1, 0x1.f900p-48
    0x3fedbe7cd63a8325, 0x3ce9800000000000, // 0x1.dbe7cd63a8325p-1, 0x1.9800p-49
    0x3fedd321f301b445, 0xbcf5200000000000, // 0x1.dd321f301b445p-1, -0x1.5200p-48
    0x3fede7d5641c05bf, 0xbd1d700000000000, // 0x1.de7d5641c05bfp-1, -0x1.d700p-46
    0x3fedfc97337b9aec, 0xbd16140000000000, // 0x1.dfc97337b9aecp-1, -0x1.6140p-46
    0x3fee11676b197d5e, 0x3d0b480000000000, // 0x1.e11676b197d5ep-1, 0x1.b480p-47
    0x3fee264614f5a3e7, 0x3d40ce0000000000, // 0x1.e264614f5a3e7p-1, 0x1.0ce0p-43
    0x3fee3b333b16ee5c, 0x3d0c680000000000, // 0x1.e3b333b16ee5cp-1, 0x1.c680p-47
    0x3fee502ee78b3fb4, 0xbd09300000000000, // 0x1.e502ee78b3fb4p-1, -0x1.9300p-47
    0x3fee653924676d68, 0xbce5000000000000, // 0x1.e653924676d68p-1, -0x1.5000p-49
    0x3fee7a51fbc74c44, 0xbd07f80000000000, // 0x1.e7a51fbc74c44p-1, -0x1.7f80p-47
    0x3fee8f7977cdb726, 0xbcf3700000000000, // 0x1.e8f7977cdb726p-1, -0x1.3700p-48
    0x3feea4afa2a490e8, 0x3ce5d00000000000, // 0x1.ea4afa2a490e8p-1, 0x1.5d00p-49
    0x3feeb9f4867ccae4, 0x3d161a0000000000, // 0x1.eb9f4867ccae4p-1, 0x1.61a0p-46
    0x3feecf482d8e680d, 0x3cf5500000000000, // 0x1.ecf482d8e680dp-1, 0x1.5500p-48
    0x3feee4aaa2188514, 0x3cc6400000000000, // 0x1.ee4aaa2188514p-1, 0x1.6400p-51
    0x3feefa1bee615a13, 0xbcee800000000000, // 0x1.efa1bee615a13p-1, -0x1.e800p-49
    0x3fef0f9c1cb64106, 0xbcfa880000000000, // 0x1.f0f9c1cb64106p-1, -0x1.a880p-48
    0x3fef252b376bb963, 0xbd2c900000000000, // 0x1.f252b376bb963p-1, -0x1.c900p-45
    0x3fef3ac948dd7275, 0x3caa000000000000, // 0x1.f3ac948dd7275p-1, 0x1.a000p-53
    0x3fef50765b6e4524, 0xbcf4f00000000000, // 0x1.f50765b6e4524p-1, -0x1.4f00p-48
    0x3fef6632798844fd, 0x3cca800000000000, // 0x1.f6632798844fdp-1, 0x1.a800p-51
    0x3fef7bfdad9cbe38, 0x3cfabc0000000000, // 0x1.f7bfdad9cbe38p-1, 0x1.abc0p-48
    0x3fef91d802243c82, 0xbcd4600000000000, // 0x1.f91d802243c82p-1, -0x1.4600p-50
    0x3fefa7c1819e908e, 0xbd0b0c0000000000, // 0x1.fa7c1819e908ep-1, -0x1.b0c0p-47
    0x3fefbdba3692d511, 0xbcc0e00000000000, // 0x1.fbdba3692d511p-1, -0x1.0e00p-51
    0x3fefd3c22b8f7194, 0xbd10de8000000000, // 0x1.fd3c22b8f7194p-1, -0x1.0de8p-46
    0x3fefe9d96b2a23ee, 0x3cee430000000000, // 0x1.fe9d96b2a23eep-1, 0x1.e430p-49
    0x3ff0000000000000, 0x0000000000000000, // 0x1.0000000000000p+0, 0x0.0000p+0
    0x3ff00b1afa5abcbe, 0xbcb3400000000000, // 0x1.00b1afa5abcbep+0, -0x1.3400p-52
    0x3ff0163da9fb3303, 0xbd12170000000000, // 0x1.0163da9fb3303p+0, -0x1.2170p-46
    0x3ff02168143b0282, 0x3cba400000000000, // 0x1.02168143b0282p+0, 0x1.a400p-52
    0x3ff02c9a3e77806c, 0x3cef980000000000, // 0x1.02c9a3e77806cp+0, 0x1.f980p-49
    0x3ff037d42e11bbca, 0xbcc7400000000000, // 0x1.037d42e11bbcap+0, -0x1.7400p-51
    0x3ff04315e86e7f89, 0x3cd8300000000000, // 0x1.04315e86e7f89p+0, 0x1.8300p-50
    0x3ff04e5f72f65467, 0xbd1a3f0000000000, // 0x1.04e5f72f65467p+0, -0x1.a3f0p-46
    0x3ff059b0d315855a, 0xbd02840000000000, // 0x1.059b0d315855ap+0, -0x1.2840p-47
    0x3ff0650a0e3c1f95, 0x3cf1600000000000, // 0x1.0650a0e3c1f95p+0, 0x1.1600p-48
    0x3ff0706b29ddf71a, 0x3d15240000000000, // 0x1.0706b29ddf71ap+0, 0x1.5240p-46
    0x3ff07bd42b72a82d, 0xbce9a00000000000, // 0x1.07bd42b72a82dp+0, -0x1.9a00p-49
    0x3ff0874518759bd0, 0x3ce6400000000000, // 0x1.0874518759bd0p+0, 0x1.6400p-49
    0x3ff092bdf66607c8, 0xbd00780000000000, // 0x1.092bdf66607c8p+0, -0x1.0780p-47
    0x3ff09e3ecac6f383, 0xbc98000000000000, // 0x1.09e3ecac6f383p+0, -0x1.8000p-54
    0x3ff0a9c79b1f3930, 0x3cffa00000000000, // 0x1.0a9c79b1f3930p+0, 0x1.fa00p-48
    0x3ff0b5586cf988fc, 0xbcfac80000000000, // 0x1.0b5586cf988fcp+0, -0x1.ac80p-48
    0x3ff0c0f145e46c8a, 0x3cd9c00000000000, // 0x1.0c0f145e46c8ap+0, 0x1.9c00p-50
    0x3ff0cc922b724816, 0x3d05200000000000, // 0x1.0cc922b724816p+0, 0x1.5200p-47
    0x3ff0d83b23395dd8, 0xbcfad00000000000, // 0x1.0d83b23395dd8p+0, -0x1.ad00p-48
    0x3ff0e3ec32d3d1f3, 0x3d1bac0000000000, // 0x1.0e3ec32d3d1f3p+0, 0x1.bac0p-46
    0x3ff0efa55fdfa9a6, 0xbd04e80000000000, // 0x1.0efa55fdfa9a6p+0, -0x1.4e80p-47
    0x3ff0fb66affed2f0, 0xbd0d300000000000, // 0x1.0fb66affed2f0p+0, -0x1.d300p-47
    0x3ff1073028d7234b, 0x3cf1500000000000, // 0x1.1073028d7234bp+0, 0x1.1500p-48
    0x3ff11301d0125b5b, 0x3cec000000000000, // 0x1.11301d0125b5bp+0, 0x1.c000p-49
    0x3ff11edbab5e2af9, 0x3d16bc0000000000, // 0x1.11edbab5e2af9p+0, 0x1.6bc0p-46
    0x3ff12abdc06c31d5, 0x3ce8400000000000, // 0x1.12abdc06c31d5p+0, 0x1.8400p-49
    0x3ff136a814f2047d, 0xbd0ed00000000000, // 0x1.136a814f2047dp+0, -0x1.ed00p-47
    0x3ff1429aaea92de9, 0x3ce8e00000000000, // 0x1.1429aaea92de9p+0, 0x1.8e00p-49
    0x3ff14e95934f3138, 0x3ceb400000000000, // 0x1.14e95934f3138p+0, 0x1.b400p-49
    0x3ff15a98c8a58e71, 0x3d05300000000000, // 0x1.15a98c8a58e71p+0, 0x1.5300p-47
    0x3ff166a45471c3df, 0x3d03380000000000, // 0x1.166a45471c3dfp+0, 0x1.3380p-47
    0x3ff172b83c7d5211, 0x3d28d40000000000, // 0x1.172b83c7d5211p+0, 0x1.8d40p-45
    0x3ff17ed48695bb9f, 0xbd05d00000000000, // 0x1.17ed48695bb9fp+0, -0x1.5d00p-47
    0x3ff18af9388c8d93, 0xbd1c880000000000, // 0x1.18af9388c8d93p+0, -0x1.c880p-46
    0x3ff1972658375d66, 0x3d11f00000000000, // 0x1.1972658375d66p+0, 0x1.1f00p-46
    0x3ff1a35beb6fcba7, 0x3d10480000000000, // 0x1.1a35beb6fcba7p+0, 0x1.0480p-46
    0x3ff1af99f81387e3, 0xbd47390000000000, // 0x1.1af99f81387e3p+0, -0x1.7390p-43
    0x3ff1bbe084045d54, 0x3d24e40000000000, // 0x1.1bbe084045d54p+0, 0x1.4e40p-45
    0x3ff1c82f95281c43, 0xbd0a200000000000, // 0x1.1c82f95281c43p+0, -0x1.a200p-47
    0x3ff1d4873168b9b2, 0x3ce3800000000000, // 0x1.1d4873168b9b2p+0, 0x1.3800p-49
    0x3ff1e0e75eb44031, 0x3ceac00000000000, // 0x1.1e0e75eb44031p+0, 0x1.ac00p-49
    0x3ff1ed5022fcd938, 0x3d01900000000000, // 0x1.1ed5022fcd938p+0, 0x1.1900p-47
    0x3ff1f9c18438cdf7, 0xbd1b780000000000, // 0x1.1f9c18438cdf7p+0, -0x1.b780p-46
    0x3ff2063b88628d8f, 0x3d2d940000000000, // 0x1.2063b88628d8fp+0, 0x1.d940p-45
    0x3ff212be3578a81e, 0x3cd8000000000000, // 0x1.212be3578a81ep+0, 0x1.8000p-50
    0x3ff21f49917ddd41, 0x3d2b340000000000, // 0x1.21f49917ddd41p+0, 0x1.b340p-45
    0x3ff22bdda2791323, 0x3d19f80000000000, // 0x1.22bdda2791323p+0, 0x1.9f80p-46
    0x3ff2387a6e7561e7, 0xbd19c80000000000, // 0x1.2387a6e7561e7p+0, -0x1.9c80p-46
    0x3ff2451ffb821427, 0x3d02300000000000, // 0x1.2451ffb821427p+0, 0x1.2300p-47
    0x3ff251ce4fb2a602, 0xbd13480000000000, // 0x1.251ce4fb2a602p+0, -0x1.3480p-46
    0x3ff25e85711eceb0, 0x3d12700000000000, // 0x1.25e85711eceb0p+0, 0x1.2700p-46
    0x3ff26b4565e27d16, 0x3d11d00000000000, // 0x1.26b4565e27d16p+0, 0x1.1d00p-46
    0x3ff2780e341de00f, 0x3d31ee0000000000, // 0x1.2780e341de00fp+0, 0x1.1ee0p-44
    0x3ff284dfe1f5633e, 0xbd14c00000000000, // 0x1.284dfe1f5633ep+0, -0x1.4c00p-46
    0x3ff291ba7591bb30, 0xbd13d80000000000, // 0x1.291ba7591bb30p+0, -0x1.3d80p-46
    0x3ff29e9df51fdf09, 0x3d08b00000000000, // 0x1.29e9df51fdf09p+0, 0x1.8b00p-47
    0x3ff2ab8a66d10e9b, 0xbd227c0000000000, // 0x1.2ab8a66d10e9bp+0, -0x1.27c0p-45
    0x3ff2b87fd0dada3a, 0x3d2a340000000000, // 0x1.2b87fd0dada3ap+0, 0x1.a340p-45
    0x3ff2c57e39771af9, 0xbd10800000000000, // 0x1.2c57e39771af9p+0, -0x1.0800p-46
    0x3ff2d285a6e402d9, 0xbd0ed00000000000, // 0x1.2d285a6e402d9p+0, -0x1.ed00p-47
    0x3ff2df961f641579, 0xbcf4200000000000, // 0x1.2df961f641579p+0, -0x1.4200p-48
    0x3ff2ecafa93e2ecf, 0xbd24980000000000, // 0x1.2ecafa93e2ecfp+0, -0x1.4980p-45
    0x3ff2f9d24abd8822, 0xbd16300000000000, // 0x1.2f9d24abd8822p+0, -0x1.6300p-46
    0x3ff306fe0a31b625, 0xbd32360000000000, // 0x1.306fe0a31b625p+0, -0x1.2360p-44
    0x3ff31432edeea50b, 0xbd70df8000000000, // 0x1.31432edeea50bp+0, -0x1.0df8p-40
    0x3ff32170fc4cd7b8, 0xbd22480000000000, // 0x1.32170fc4cd7b8p+0, -0x1.2480p-45
    0x3ff32eb83ba8e9a2, 0xbd25980000000000, // 0x1.32eb83ba8e9a2p+0, -0x1.5980p-45
    0x3ff33c08b2641766, 0x3d1ed00000000000, // 0x1.33c08b2641766p+0, 0x1.ed00p-46
    0x3ff3496266e3fa27, 0xbcdc000000000000, // 0x1.3496266e3fa27p+0, -0x1.c000p-50
    0x3ff356c55f929f0f, 0xbd30d80000000000, // 0x1.356c55f929f0fp+0, -0x1.0d80p-44
    0x3ff36431a2de88b9, 0x3d22c80000000000, // 0x1.36431a2de88b9p+0, 0x1.2c80p-45
    0x3ff371a7373aaa39, 0x3d20600000000000, // 0x1.371a7373aaa39p+0, 0x1.0600p-45
    0x3ff37f26231e74fe, 0xbd16600000000000, // 0x1.37f26231e74fep+0, -0x1.6600p-46
    0x3ff38cae6d05d838, 0xbd0ae00000000000, // 0x1.38cae6d05d838p+0, -0x1.ae00p-47
    0x3ff39a401b713ec3, 0xbd44720000000000, // 0x1.39a401b713ec3p+0, -0x1.4720p-43
    0x3ff3a7db34e5a020, 0x3d08200000000000, // 0x1.3a7db34e5a020p+0, 0x1.8200p-47
    0x3ff3b57fbfec6e95, 0x3d3e800000000000, // 0x1.3b57fbfec6e95p+0, 0x1.e800p-44
    0x3ff3c32dc313a8f2, 0x3cef800000000000, // 0x1.3c32dc313a8f2p+0, 0x1.f800p-49
    0x3ff3d0e544ede122, 0xbd17a00000000000, // 0x1.3d0e544ede122p+0, -0x1.7a00p-46
    0x3ff3dea64c1234bb, 0x3d26300000000000, // 0x1.3dea64c1234bbp+0, 0x1.6300p-45
    0x3ff3ec70df1c4ecc, 0xbd48a60000000000, // 0x1.3ec70df1c4eccp+0, -0x1.8a60p-43
    0x3ff3fa4504ac7e8c, 0xbd3cdc0000000000, // 0x1.3fa4504ac7e8cp+0, -0x1.cdc0p-44
    0x3ff40822c367a0bb, 0x3d25b80000000000, // 0x1.40822c367a0bbp+0, 0x1.5b80p-45
    0x3ff4160a21f72e95, 0x3d1ec00000000000, // 0x1.4160a21f72e95p+0, 0x1.ec00p-46
    0x3ff423fb27094646, 0xbd13600000000000, // 0x1.423fb27094646p+0, -0x1.3600p-46
    0x3ff431f5d950a920, 0x3d23980000000000, // 0x1.431f5d950a920p+0, 0x1.3980p-45
    0x3ff43ffa3f84b9eb, 0x3cfa000000000000, // 0x1.43ffa3f84b9ebp+0, 0x1.a000p-48
    0x3ff44e0860618919, 0xbcf6c00000000000, // 0x1.44e0860618919p+0, -0x1.6c00p-48
    0x3ff45c2042a7d201, 0xbd0bc00000000000, // 0x1.45c2042a7d201p+0, -0x1.bc00p-47
    0x3ff46a41ed1d0016, 0xbd12800000000000, // 0x1.46a41ed1d0016p+0, -0x1.2800p-46
    0x3ff4786d668b3326, 0x3d30e00000000000, // 0x1.4786d668b3326p+0, 0x1.0e00p-44
    0x3ff486a2b5c13c00, 0xbd2d400000000000, // 0x1.486a2b5c13c00p+0, -0x1.d400p-45
    0x3ff494e1e192af04, 0x3d0c200000000000, // 0x1.494e1e192af04p+0, 0x1.c200p-47
    0x3ff4a32af0d7d372, 0xbd1e500000000000, // 0x1.4a32af0d7d372p+0, -0x1.e500p-46
    0x3ff4b17dea6db801, 0x3d07800000000000, // 0x1.4b17dea6db801p+0, 0x1.7800p-47
    0x3ff4bfdad53629e1, 0xbd13800000000000, // 0x1.4bfdad53629e1p+0, -0x1.3800p-46
    0x3ff4ce41b817c132, 0x3d00800000000000, // 0x1.4ce41b817c132p+0, 0x1.0800p-47
    0x3ff4dcb299fddddb, 0x3d2c700000000000, // 0x1.4dcb299fddddbp+0, 0x1.c700p-45
    0x3ff4eb2d81d8ab96, 0xbd1ce00000000000, // 0x1.4eb2d81d8ab96p+0, -0x1.ce00p-46
    0x3ff4f9b2769d2d02, 0x3d19200000000000, // 0x1.4f9b2769d2d02p+0, 0x1.9200p-46
    0x3ff508417f4531c1, 0xbd08c00000000000, // 0x1.508417f4531c1p+0, -0x1.8c00p-47
    0x3ff516daa2cf662a, 0xbcfa000000000000, // 0x1.516daa2cf662ap+0, -0x1.a000p-48
    0x3ff5257de83f51ea, 0x3d4a080000000000, // 0x1.5257de83f51eap+0, 0x1.a080p-43
    0x3ff5342b569d4eda, 0xbd26d80000000000, // 0x1.5342b569d4edap+0, -0x1.6d80p-45
    0x3ff542e2f4f6ac1a, 0xbd32440000000000, // 0x1.542e2f4f6ac1ap+0, -0x1.2440p-44
    0x3ff551a4ca5d94db, 0x3d483c0000000000, // 0x1.551a4ca5d94dbp+0, 0x1.83c0p-43
    0x3ff56070dde9116b, 0x3d24b00000000000, // 0x1.56070dde9116bp+0, 0x1.4b00p-45
    0x3ff56f4736b529de, 0x3d415a0000000000, // 0x1.56f4736b529dep+0, 0x1.15a0p-43
    0x3ff57e27dbe2c40e, 0xbd29e00000000000, // 0x1.57e27dbe2c40ep+0, -0x1.9e00p-45
    0x3ff58d12d497c76f, 0xbd23080000000000, // 0x1.58d12d497c76fp+0, -0x1.3080p-45
    0x3ff59c0827ff0b4c, 0x3d4dec0000000000, // 0x1.59c0827ff0b4cp+0, 0x1.dec0p-43
    0x3ff5ab07dd485427, 0xbcc4000000000000, // 0x1.5ab07dd485427p+0, -0x1.4000p-51
    0x3ff5ba11fba87af4, 0x3d30080000000000, // 0x1.5ba11fba87af4p+0, 0x1.0080p-44
    0x3ff5c9268a59460b, 0xbd26c80000000000, // 0x1.5c9268a59460bp+0, -0x1.6c80p-45
    0x3ff5d84590998e3f, 0x3d469a0000000000, // 0x1.5d84590998e3fp+0, 0x1.69a0p-43
    0x3ff5e76f15ad20e1, 0xbd1b400000000000, // 0x1.5e76f15ad20e1p+0, -0x1.b400p-46
    0x3ff5f6a320dcebca, 0x3d17700000000000, // 0x1.5f6a320dcebcap+0, 0x1.7700p-46
    0x3ff605e1b976dcb8, 0x3d26f80000000000, // 0x1.605e1b976dcb8p+0, 0x1.6f80p-45
    0x3ff6152ae6cdf715, 0x3d01000000000000, // 0x1.6152ae6cdf715p+0, 0x1.1000p-47
    0x3ff6247eb03a5531, 0xbd15d00000000000, // 0x1.6247eb03a5531p+0, -0x1.5d00p-46
    0x3ff633dd1d1929b5, 0xbd12d00000000000, // 0x1.633dd1d1929b5p+0, -0x1.2d00p-46
    0x3ff6434634ccc313, 0xbcea800000000000, // 0x1.6434634ccc313p+0, -0x1.a800p-49
    0x3ff652b9febc8efa, 0xbd28600000000000, // 0x1.652b9febc8efap+0, -0x1.8600p-45
    0x3ff6623882553397, 0x3d71fe0000000000, // 0x1.6623882553397p+0, 0x1.1fe0p-40
    0x3ff671c1c708328e, 0xbd37200000000000, // 0x1.671c1c708328ep+0, -0x1.7200p-44
    0x3ff68155d44ca97e, 0x3ce6800000000000, // 0x1.68155d44ca97ep+0, 0x1.6800p-49
    0x3ff690f4b19e9471, 0xbd29780000000000, // 0x1.690f4b19e9471p+0, -0x1.9780p-45
];
