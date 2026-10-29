//! `x * 2^n` by exponent manipulation, ported from fdlibm s_scalbn.c (Sun Microsystems, free use
//! with the notice in the source; netlib.org/fdlibm/s_scalbn.c, sha256 9ad1bb37...d99e). The one
//! rounding of a subnormal result is the final multiplication by 2^-54.

use super::bits::{copysign, hi, lo, with_hi};

pub(super) const TWO54: f64 = 18_014_398_509_481_984.0;
pub(super) const TWOM54: f64 = f64::from_bits(0x3c90_0000_0000_0000);
pub(super) const HUGE: f64 = 1.0e300;
pub(super) const TINY: f64 = 1.0e-300;

pub(super) fn scalbn(x: f64, n: i32) -> f64 {
    let mut x = x;
    let mut hx = hi(x);
    let lx = lo(x);
    let mut k = ((hx & 0x7ff0_0000) >> 20) as i32;
    if k == 0 {
        if (lx | (hx & 0x7fff_ffff)) == 0 {
            return x;
        }
        x *= TWO54;
        hx = hi(x);
        k = ((hx & 0x7ff0_0000) >> 20) as i32 - 54;
        if n < -50_000 {
            return TINY * x;
        }
    }
    if k == 0x7ff {
        return x + x;
    }
    k = k.saturating_add(n);
    if k > 0x7fe {
        return HUGE * copysign(HUGE, x);
    }
    if k > 0 {
        return with_hi(x, (hx & 0x800f_ffff) | ((k as u32) << 20));
    }
    if k <= -54 {
        return if n > 50_000 { HUGE * copysign(HUGE, x) } else { TINY * copysign(TINY, x) };
    }
    k += 54;
    x = with_hi(x, (hx & 0x800f_ffff) | ((k as u32) << 20));
    x * TWOM54
}
