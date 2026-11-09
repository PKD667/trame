//! Natural logarithm for f64, ported from fdlibm e_log.c (Sun Microsystems; netlib.org/fdlibm/
//! e_log.c, sha256 a2b1bd1b...). Plain arithmetic and word access only.

use super::bits::{hi, lo, with_hi};

pub(super) const LN2_HI: f64 = 6.93147180369123816490e-01;
pub(super) const LN2_LO: f64 = 1.90821492927058770002e-10;
pub(super) const TWO54: f64 = 1.80143985094819840000e+16;
pub(super) const LG1: f64 = 6.666666666666735130e-01;
pub(super) const LG2: f64 = 3.999999999940941908e-01;
pub(super) const LG3: f64 = 2.857142874366239149e-01;
pub(super) const LG4: f64 = 2.222219843214978396e-01;
pub(super) const LG5: f64 = 1.818357216161805012e-01;
pub(super) const LG6: f64 = 1.531383769920937332e-01;
pub(super) const LG7: f64 = 1.479819860511658591e-01;

/// Natural logarithm. Error below 1 ulp against the true value (fdlibm's claim; checked against
/// glibc in `tests/sweep.rs`). `ln(0)` is `-inf`, `ln(x)` for `x < 0` is NaN, `ln(+inf)` is `+inf`.
pub fn ln(x: f64) -> f64 {
    let mut x = x;
    let mut hx = hi(x) as i32;
    let lx = lo(x);
    let mut k: i32 = 0;
    if hx < 0x0010_0000 {
        if (((hx & 0x7fff_ffff) as u32) | lx) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k -= 54;
        x *= TWO54;
        hx = hi(x) as i32;
    }
    if hx >= 0x7ff0_0000 {
        return x + x;
    }
    k += (hx >> 20) - 1023;
    hx &= 0x000f_ffff;
    let i = (hx + 0x95f64) & 0x0010_0000;
    x = with_hi(x, (hx | (i ^ 0x3ff0_0000)) as u32);
    k += i >> 20;
    let f = x - 1.0;
    if (0x000f_ffff & (2 + hx)) < 3 {
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            }
            let dk = f64::from(k);
            return dk * LN2_HI + dk * LN2_LO;
        }
        let r = f * f * (0.5 - 0.333_333_333_333_333_33 * f);
        if k == 0 {
            return f - r;
        }
        let dk = f64::from(k);
        return dk * LN2_HI - ((r - dk * LN2_LO) - f);
    }
    let s = f / (2.0 + f);
    let dk = f64::from(k);
    let z = s * s;
    let mut i = hx - 0x6147a;
    let w = z * z;
    let j = 0x6b851 - hx;
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    i |= j;
    let r = t2 + t1;
    if i > 0 {
        let hfsq = 0.5 * f * f;
        if k == 0 {
            return f - (hfsq - s * (hfsq + r));
        }
        return dk * LN2_HI - ((hfsq - (s * (hfsq + r) + dk * LN2_LO)) - f);
    }
    if k == 0 {
        return f - s * (f - r);
    }
    dk * LN2_HI - ((s * (f - r) - dk * LN2_LO) - f)
}
