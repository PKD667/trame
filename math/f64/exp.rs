//! `e^x` for f64, ported from fdlibm e_exp.c (Sun Microsystems; netlib.org/fdlibm/e_exp.c,
//! sha256 04c96195...02ef). Plain arithmetic and word access only; no FMA, no libm.

use super::bits::{hi, lo, with_hi};

pub(super) const HUGE: f64 = 1.0e300;
pub(super) const TWOM1000: f64 = 9.33263618503218878990e-302;
pub(super) const O_THRESHOLD: f64 = 7.09782712893383973096e+02;
pub(super) const U_THRESHOLD: f64 = -7.45133219101941108420e+02;
pub(super) const LN2HI: [f64; 2] = [6.93147180369123816490e-01, -6.93147180369123816490e-01];
pub(super) const LN2LO: [f64; 2] = [1.90821492927058770002e-10, -1.90821492927058770002e-10];
pub(super) const INVLN2: f64 = 1.44269504088896338700e+00;
pub(super) const P1: f64 = 1.66666666666666019037e-01;
pub(super) const P2: f64 = -2.77777777770155933842e-03;
pub(super) const P3: f64 = 6.61375632143793436117e-05;
pub(super) const P4: f64 = -1.65339022054652515390e-06;
pub(super) const P5: f64 = 4.13813679705723846039e-08;

/// `e^x`. Error below 1 ulp against the true value (fdlibm's claim; checked against glibc in
/// `tests/sweep.rs`).
/// Overflows to `+inf` above about 709.78 and underflows to `0` below about -745.13.
pub fn exp(x: f64) -> f64 {
    let mut x = x;
    let mut hx = hi(x);
    let xsb = (hx >> 31) & 1;
    hx &= 0x7fff_ffff;
    let (mut h, mut l, mut k) = (0.0_f64, 0.0_f64, 0_i32);
    if hx >= 0x4086_2e42 {
        if hx >= 0x7ff0_0000 {
            if ((hx & 0xfffff) | lo(x)) != 0 {
                return x + x;
            }
            return if xsb == 0 { x } else { 0.0 };
        }
        if x > O_THRESHOLD {
            return HUGE * HUGE;
        }
        if x < U_THRESHOLD {
            return TWOM1000 * TWOM1000;
        }
    }
    if hx > 0x3fd6_2e42 {
        let s = xsb as usize;
        if hx < 0x3ff0_a2b2 {
            h = x - LN2HI[s];
            l = LN2LO[s];
            k = 1 - 2 * xsb as i32;
        } else {
            let half = if xsb == 0 { 0.5 } else { -0.5 };
            k = (INVLN2 * x + half) as i32;
            let t = f64::from(k);
            h = x - t * LN2HI[0];
            l = t * LN2LO[0];
        }
        x = h - l;
    } else if hx < 0x3e30_0000 && HUGE + x > 1.0 {
        return 1.0 + x;
    }
    let t = x * x;
    let c = x - t * (P1 + t * (P2 + t * (P3 + t * (P4 + t * P5))));
    if k == 0 {
        return 1.0 - ((x * c) / (c - 2.0) - x);
    }
    let y = 1.0 - ((l - (x * c) / (2.0 - c)) - h);
    if k >= -1021 {
        with_hi(y, hi(y).wrapping_add((k << 20) as u32))
    } else {
        with_hi(y, hi(y).wrapping_add(((k + 1000) << 20) as u32)) * TWOM1000
    }
}
