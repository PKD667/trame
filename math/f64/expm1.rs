//! `e^x - 1` for f64, ported from fdlibm s_expm1.c (Sun Microsystems; netlib.org/fdlibm/s_expm1.c,
//! sha256 7b1daee1...). Plain arithmetic and word access only.

use super::bits::{hi, lo, with_hi};

pub(super) const HUGE: f64 = 1.0e300;
pub(super) const TINY: f64 = 1.0e-300;
pub(super) const O_THRESHOLD: f64 = 7.09782712893383973096e+02;
pub(super) const LN2_HI: f64 = 6.93147180369123816490e-01;
pub(super) const LN2_LO: f64 = 1.90821492927058770002e-10;
pub(super) const INVLN2: f64 = 1.44269504088896338700e+00;
pub(super) const Q1: f64 = -3.33333333333331316428e-02;
pub(super) const Q2: f64 = 1.58730158725481460165e-03;
pub(super) const Q3: f64 = -7.93650757867487942473e-05;
pub(super) const Q4: f64 = 4.00821782732936239552e-06;
pub(super) const Q5: f64 = -2.01099218183624371326e-07;

/// `e^x - 1`, accurate for small `x`. Error below 1 ulp against the true value (fdlibm's claim;
/// checked against glibc in `tests/sweep.rs`). `exp_m1(-inf)` is `-1`, `exp_m1(+inf)` is `+inf`.
pub fn exp_m1(x: f64) -> f64 {
    let mut x = x;
    let hx0 = hi(x);
    let xsb = hx0 & 0x8000_0000;
    let hx = hx0 & 0x7fff_ffff;
    if hx >= 0x4043_687a {
        if hx >= 0x4086_2e42 {
            if hx >= 0x7ff0_0000 {
                if ((hx & 0xfffff) | lo(x)) != 0 {
                    return x + x;
                }
                return if xsb == 0 { x } else { -1.0 };
            }
            if x > O_THRESHOLD {
                return HUGE * HUGE;
            }
        }
        if xsb != 0 && x + TINY < 0.0 {
            return TINY - 1.0;
        }
    }
    let mut k: i32 = 0;
    let mut c = 0.0_f64;
    if hx > 0x3fd6_2e42 {
        let (hi_v, lo_v);
        if hx < 0x3ff0_a2b2 {
            if xsb == 0 {
                hi_v = x - LN2_HI;
                lo_v = LN2_LO;
                k = 1;
            } else {
                hi_v = x + LN2_HI;
                lo_v = -LN2_LO;
                k = -1;
            }
        } else {
            let half = if xsb == 0 { 0.5 } else { -0.5 };
            k = (INVLN2 * x + half) as i32;
            let t = f64::from(k);
            hi_v = x - t * LN2_HI;
            lo_v = t * LN2_LO;
        }
        x = hi_v - lo_v;
        c = (hi_v - x) - lo_v;
    } else if hx < 0x3c90_0000 {
        let t = HUGE + x;
        return x - (t - (HUGE + x));
    }
    let hfx = 0.5 * x;
    let hxs = x * hfx;
    let r1 = 1.0 + hxs * (Q1 + hxs * (Q2 + hxs * (Q3 + hxs * (Q4 + hxs * Q5))));
    let t = 3.0 - r1 * hfx;
    let mut e = hxs * ((r1 - t) / (6.0 - x * t));
    if k == 0 {
        return x - (x * e - hxs);
    }
    e = x * (e - c) - c;
    e -= hxs;
    if k == -1 {
        return 0.5 * (x - e) - 0.5;
    }
    if k == 1 {
        return if x < -0.25 { -2.0 * (e - (x + 0.5)) } else { 1.0 + 2.0 * (x - e) };
    }
    if k <= -2 || k > 56 {
        let y = 1.0 - (e - x);
        let y = with_hi(y, hi(y).wrapping_add((k << 20) as u32));
        return y - 1.0;
    }
    if k < 20 {
        let t = with_hi(1.0, 0x3ff0_0000 - (0x20_0000 >> k));
        let y = t - (e - x);
        return with_hi(y, hi(y).wrapping_add((k << 20) as u32));
    }
    let t = with_hi(1.0, ((0x3ff - k) << 20) as u32);
    let y = x - (e + t);
    let y = y + 1.0;
    with_hi(y, hi(y).wrapping_add((k << 20) as u32))
}
