//! The first stage of fdlibm's pow: `log2(|x|)` as an unevaluated pair `t1 + t2`, with `t1` having
//! its low word cleared so that `y * t1` is exact in the second stage (fdlibm e_pow.c).

use super::super::bits::{hi, with_hi, with_lo};

pub(in crate::math::f64) const TWO53: f64 = 9_007_199_254_740_992.0;
pub(in crate::math::f64) const BP: [f64; 2] = [1.0, 1.5];
pub(in crate::math::f64) const DP_H: [f64; 2] = [0.0, 5.84962487220764160156e-01];
pub(in crate::math::f64) const DP_L: [f64; 2] = [0.0, 1.35003920212974897128e-08];
pub(in crate::math::f64) const L1: f64 = 5.99999999999994648725e-01;
pub(in crate::math::f64) const L2: f64 = 4.28571428578550184252e-01;
pub(in crate::math::f64) const L3: f64 = 3.33333329818377432918e-01;
pub(in crate::math::f64) const L4: f64 = 2.72728123808534006489e-01;
pub(in crate::math::f64) const L5: f64 = 2.30660745775561754067e-01;
pub(in crate::math::f64) const L6: f64 = 2.06975017800338417784e-01;
pub(in crate::math::f64) const CP: f64 = 9.61796693925975554329e-01;
pub(in crate::math::f64) const CP_H: f64 = 9.61796700954437255859e-01;
pub(in crate::math::f64) const CP_L: f64 = -7.02846165095275826516e-09;
pub(in crate::math::f64) const IVLN2: f64 = 1.44269504088896338700e+00;
pub(in crate::math::f64) const IVLN2_H: f64 = 1.44269502162933349609e+00;
pub(in crate::math::f64) const IVLN2_L: f64 = 1.92596299112661746887e-08;

/// `log2(|x|)` for `|x|` close to 1 (`|x - 1| <= 2^-20`): the series `x - x^2/2 + ...`.
pub(super) fn near_one(ax: f64) -> (f64, f64) {
    let t = ax - 1.0;
    let w = (t * t) * (0.5 - t * (0.333_333_333_333_333_333_333_3 - t * 0.25));
    let u = IVLN2_H * t;
    let v = t * IVLN2_L - w * IVLN2;
    let t1 = with_lo(u + v, 0);
    let t2 = v - (t1 - u);
    (t1, t2)
}

/// `log2(ax)` for a positive finite `ax` with high word `ix` (`ix = hi(ax) & 0x7fffffff`), by
/// reduction to `[sqrt(2/3), sqrt(3/2))` and a rational approximation of `log`.
pub(super) fn split(ax0: f64, ix0: i32) -> (f64, f64) {
    let mut n: i32 = 0;
    let mut ax = ax0;
    let mut ix = ix0;
    if ix < 0x0010_0000 {
        ax *= TWO53;
        n -= 53;
        ix = hi(ax) as i32;
    }
    n += (ix >> 20) - 0x3ff;
    let j = ix & 0x000f_ffff;
    ix = j | 0x3ff0_0000;
    let k: i32 = if j <= 0x3988e {
        0
    } else if j < 0xbb67a {
        1
    } else {
        n += 1;
        ix -= 0x0010_0000;
        0
    };
    let kk = k as usize;
    ax = with_hi(ax, ix as u32);
    let u = ax - BP[kk];
    let v = 1.0 / (ax + BP[kk]);
    let ss = u * v;
    let s_h = with_lo(ss, 0);
    let t_h = with_hi(0.0, (((ix >> 1) | 0x2000_0000) + 0x0008_0000 + (k << 18)) as u32);
    let t_l = ax - (t_h - BP[kk]);
    let s_l = v * ((u - s_h * t_h) - s_h * t_l);
    let s2 = ss * ss;
    let mut r = s2 * s2 * (L1 + s2 * (L2 + s2 * (L3 + s2 * (L4 + s2 * (L5 + s2 * L6)))));
    r += s_l * (s_h + ss);
    let s2 = s_h * s_h;
    let t_h = with_lo(3.0 + s2 + r, 0);
    let t_l = r - ((t_h - 3.0) - s2);
    let u = s_h * t_h;
    let v = s_l * t_h + t_l * ss;
    let p_h = with_lo(u + v, 0);
    let p_l = v - (p_h - u);
    let z_h = CP_H * p_h;
    let z_l = CP_L * p_h + p_l * CP + DP_L[kk];
    let t = f64::from(n);
    let t1 = with_lo(((z_h + z_l) + DP_H[kk]) + t, 0);
    let t2 = z_l - (((t1 - t) - DP_H[kk]) - z_h);
    (t1, t2)
}
