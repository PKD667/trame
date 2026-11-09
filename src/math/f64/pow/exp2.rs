//! The second stage of fdlibm's pow: given `log2(|x|) = t1 + t2` (from `log2.rs`), form
//! `y * log2(|x|)`, check the result against the overflow and underflow limits, and compute
//! `2^z` for `z = p_h + p_l` as `2^n * e^(r)` with `|r|` small (fdlibm e_pow.c).

use super::super::bits::{hi, lo, with_hi, with_lo};
use super::super::scalbn::scalbn;

pub(in crate::math::f64) const HUGE: f64 = 1.0e300;
pub(in crate::math::f64) const TINY: f64 = 1.0e-300;
pub(in crate::math::f64) const P1: f64 = 1.66666666666666019037e-01;
pub(in crate::math::f64) const P2: f64 = -2.77777777770155933842e-03;
pub(in crate::math::f64) const P3: f64 = 6.61375632143793436117e-05;
pub(in crate::math::f64) const P4: f64 = -1.65339022054652515390e-06;
pub(in crate::math::f64) const P5: f64 = 4.13813679705723846039e-08;
pub(in crate::math::f64) const LG2: f64 = 6.93147180559945286227e-01;
pub(in crate::math::f64) const LG2_H: f64 = 6.93147182464599609375e-01;
pub(in crate::math::f64) const LG2_L: f64 = -1.90465429995776804525e-09;
pub(in crate::math::f64) const OVT: f64 = 8.0085662595372944372e-17;

/// Finish `pow`: `s` is the sign of the result, `(t1, t2)` is `log2(|x|)`, `y` the exponent.
pub(super) fn finish(y: f64, s: f64, t1: f64, t2: f64) -> f64 {
    let y1 = with_lo(y, 0);
    let p_l0 = (y - y1) * t1 + y * t2;
    let mut p_h = y1 * t1;
    let z = p_l0 + p_h;
    let j = hi(z) as i32;
    let i = lo(z);
    if j >= 0x4090_0000 {
        if (((j - 0x4090_0000) as u32) | i) != 0 {
            return s * HUGE * HUGE;
        }
        if p_l0 + OVT > z - p_h {
            return s * HUGE * HUGE;
        }
    } else if (j & 0x7fff_ffff) >= 0x4090_cc00 {
        if ((j as u32).wrapping_sub(0xc090_cc00) | i) != 0 {
            return s * TINY * TINY;
        }
        if p_l0 <= z - p_h {
            return s * TINY * TINY;
        }
    }
    let ij = j & 0x7fff_ffff;
    let mut k = (ij >> 20) - 0x3ff;
    let mut n: i32 = 0;
    if ij > 0x3fe0_0000 {
        n = j + (0x0010_0000 >> (k + 1));
        k = ((n & 0x7fff_ffff) >> 20) - 0x3ff;
        let t = with_hi(0.0, (n & !(0x000f_ffff >> k)) as u32);
        n = ((n & 0x000f_ffff) | 0x0010_0000) >> (20 - k);
        if j < 0 {
            n = -n;
        }
        p_h -= t;
    }
    let t = with_lo(p_l0 + p_h, 0);
    let u = t * LG2_H;
    let v = (p_l0 - (t - p_h)) * LG2 + t * LG2_L;
    let z = u + v;
    let w = v - (z - u);
    let t = z * z;
    let t1 = z - t * (P1 + t * (P2 + t * (P3 + t * (P4 + t * P5))));
    let r = (z * t1) / (t1 - 2.0) - (w + z * w);
    let z = 1.0 - (r - z);
    let jz = hi(z).wrapping_add((n << 20) as u32) as i32;
    let z = if (jz >> 20) <= 0 { scalbn(z, n) } else { with_hi(z, hi(z).wrapping_add((n << 20) as u32)) };
    s * z
}
