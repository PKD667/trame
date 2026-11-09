//! `x^y` for f64, ported from fdlibm e_pow.c (Sun Microsystems; netlib.org/fdlibm/e_pow.c,
//! sha256 763b86ba...). Plain arithmetic and word access only; `sqrt` is `super::sqrt`, which is
//! correctly rounded, and `scalbn` is fdlibm's s_scalbn.c. The special cases follow the C code.

pub(super) mod exp2;
pub(super) mod log2;

use super::bits::{hi, lo};
use super::sqrt::sqrt;

const HUGE: f64 = 1.0e300;
const TINY: f64 = 1.0e-300;

/// `x^y`. Error below 1 ulp against the true value, as fdlibm claims (checked against glibc in `tests/sweep.rs`).
/// Follows the C special cases: `powf(x, 0) = 1`, `powf(NaN, y) = NaN` unless `y = 0`, a negative
/// `x` with a non-integer `y` gives NaN.
pub fn powf(x: f64, y: f64) -> f64 {
    let hx = hi(x) as i32;
    let lx = lo(x);
    let hy = hi(y) as i32;
    let ly = lo(y);
    let ix = hx & 0x7fff_ffff;
    let iy = hy & 0x7fff_ffff;
    if (iy as u32 | ly) == 0 {
        return 1.0;
    }
    if ix > 0x7ff0_0000 || (ix == 0x7ff0_0000 && lx != 0) || iy > 0x7ff0_0000 || (iy == 0x7ff0_0000 && ly != 0) {
        return x + y;
    }
    let yisint = yisint(hx, ly, iy);
    if ly == 0 {
        if iy == 0x7ff0_0000 {
            if (((ix - 0x3ff0_0000) as u32) | lx) == 0 {
                return f64::NAN; // y is +-inf here: y - y is NaN
            } else if ix >= 0x3ff0_0000 {
                return if hy >= 0 { y } else { 0.0 };
            }
            return if hy < 0 { -y } else { 0.0 };
        }
        if iy == 0x3ff0_0000 {
            return if hy < 0 { 1.0 / x } else { x };
        }
        if hy == 0x4000_0000 {
            return x * x;
        }
        if hy == 0x3fe0_0000 && hx >= 0 {
            return sqrt(x);
        }
    }
    let ax = x.abs();
    if lx == 0 && (ix == 0x7ff0_0000 || ix == 0 || ix == 0x3ff0_0000) {
        let mut z = ax;
        if hy < 0 {
            z = 1.0 / z;
        }
        if hx < 0 {
            if (((ix - 0x3ff0_0000) as u32) | (yisint as u32)) == 0 {
                z = f64::NAN; // z is 0, 1 or inf here: (z - z) / (z - z) is NaN
            } else if yisint == 1 {
                z = -z;
            }
        }
        return z;
    }
    let n = (hx >> 31) + 1;
    if (n | yisint) == 0 {
        return f64::NAN; // x is finite and non-zero here: (x - x) / (x - x) is NaN
    }
    let s = if (n | (yisint - 1)) == 0 { -1.0 } else { 1.0 };
    let (t1, t2) = if iy > 0x41e0_0000 {
        if iy > 0x43f0_0000 {
            if ix <= 0x3fef_ffff {
                return if hy < 0 { HUGE * HUGE } else { TINY * TINY };
            }
            if ix >= 0x3ff0_0000 {
                return if hy > 0 { HUGE * HUGE } else { TINY * TINY };
            }
        }
        if ix < 0x3fef_ffff {
            return if hy < 0 { s * HUGE * HUGE } else { s * TINY * TINY };
        }
        if ix > 0x3ff0_0000 {
            return if hy > 0 { s * HUGE * HUGE } else { s * TINY * TINY };
        }
        log2::near_one(ax)
    } else {
        log2::split(ax, ix)
    };
    exp2::finish(y, s, t1, t2)
}

/// fdlibm's `yisint`: 0 when `y` is not an integer, 1 when it is an odd integer, 2 when it is an
/// even integer. Only consulted when `x < 0`.
fn yisint(hx: i32, ly: u32, iy: i32) -> i32 {
    if hx >= 0 {
        return 0;
    }
    if iy >= 0x4340_0000 {
        return 2;
    }
    if iy >= 0x3ff0_0000 {
        let k = (iy >> 20) - 0x3ff;
        if k > 20 {
            let j = ly >> (52 - k);
            if (j << (52 - k)) == ly {
                return 2 - (j & 1) as i32;
            }
        } else if ly == 0 {
            let j = iy >> (20 - k);
            if (j << (20 - k)) == iy {
                return 2 - (j & 1);
            }
        }
    }
    0
}
