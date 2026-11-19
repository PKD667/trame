//! Trigonometric functions for f64, ported from fdlibm.
//!
//! # Sources
//! - `s_sin.c`: sha256 4d5edb753..., Sun Microsystems fdlibm
//! - `s_cos.c`: sha256 bdcbea3c8..., Sun Microsystems fdlibm
//! - `s_tan.c`: sha256 f86caed50..., Sun Microsystems fdlibm
//! - `k_sin.c`: sha256 75d391f3c..., Sun Microsystems fdlibm
//! - `k_cos.c`: sha256 841086c75..., Sun Microsystems fdlibm
//! - `k_tan.c`: sha256 9dfd4c84f..., Sun Microsystems fdlibm
//! - `e_rem_pio2.c`: sha256 3a08dd4a3..., Sun Microsystems fdlibm
//! - `k_rem_pio2.c`: sha256 5796209ea..., Sun Microsystems fdlibm
//!
//! Plain arithmetic and word access only; no libm, no FMA, no std transcendental.
//! Argument reduction includes the full 2/pi table and handles f64::MAX correctly.
//! Results are bit-identical on every IEEE 754 target.

use super::bits::{hi, lo, with_hi, with_lo};
use super::floor::floor;
use super::scalbn::scalbn;

// ============================================================================
// CONSTANTS
// ============================================================================

const ONE: f64 = 1.0;
const ZERO: f64 = 0.0;
const HALF: f64 = 5.00000000000000000000e-01;

// kernel_sin coefficients
const S1: f64 = -1.66666666666666324348e-01;
const S2: f64 = 8.33333333332248946124e-03;
const S3: f64 = -1.98412698298579493134e-04;
const S4: f64 = 2.75573137070700676789e-06;
const S5: f64 = -2.50507602534068634195e-08;
const S6: f64 = 1.58969099521155010221e-10;

// kernel_cos coefficients
const C1: f64 = 4.16666666666666019037e-02;
const C2: f64 = -1.38888888888741095749e-03;
const C3: f64 = 2.48015872894767294178e-05;
const C4: f64 = -2.75573143513906633035e-07;
const C5: f64 = 2.08757232129817482790e-09;
const C6: f64 = -1.13596475577881948265e-11;

// kernel_tan coefficients (the T array from k_tan.c)
const T_COEFFS: [f64; 13] = [
    3.33333333333334091986e-01,     // T[0]
    1.33333333333201242699e-01,     // T[1]
    5.39682539762260521377e-02,     // T[2]
    2.18694882948595424599e-02,     // T[3]
    8.86323982359930005737e-03,     // T[4]
    3.59207910759131235356e-03,     // T[5]
    1.45620945432529025516e-03,     // T[6]
    5.88041240820264096874e-04,     // T[7]
    2.46463134818469906812e-04,     // T[8]
    7.81794442939557092300e-05,     // T[9]
    7.14072491382608190305e-05,     // T[10]
    -1.85586374855275456654e-05,    // T[11]
    2.59073051863633712884e-05,     // T[12]
];
const TAN_PIO4: f64 = 7.85398163397448278999e-01;
const TAN_PIO4LO: f64 = 3.06161699786838301793e-17;

// rem_pio2 constants
const INVPIO2: f64 = 6.36619772367581382433e-01;
const PIO2_1: f64 = 1.57079632673412561417e+00;
const PIO2_1T: f64 = 6.07710050650619224932e-11;
const PIO2_2: f64 = 6.07710050630396597660e-11;
const PIO2_2T: f64 = 2.02226624879595063154e-21;
const PIO2_3: f64 = 2.02226624871116645580e-21;
const PIO2_3T: f64 = 8.47842766036889956997e-32;

// 2/pi in 24-bit chunks (from e_rem_pio2.c)
const TWO_OVER_PI: &[u32] = &[
    0xA2F983, 0x6E4E44, 0x1529FC, 0x2757D1, 0xF534DD, 0xC0DB62,
    0x95993C, 0x439041, 0xFE5163, 0xABDEBB, 0xC561B7, 0x246E3A,
    0x424DD2, 0xE00649, 0x2EEA09, 0xD1921C, 0xFE1DEB, 0x1CB129,
    0xA73EE8, 0x8235F5, 0x2EBB44, 0x84E99C, 0x7026B4, 0x5F7E41,
    0x3991D6, 0x398353, 0x39F49C, 0x845F8B, 0xBDF928, 0x3B1FF8,
    0x97FFDE, 0x05980F, 0xEF2F11, 0x8B5A0A, 0x6D1F6D, 0x367ECF,
    0x27CB09, 0xB74F46, 0x3F669E, 0x5FEA2D, 0x7527BA, 0xC7EBE5,
    0xF17B3D, 0x0739F7, 0x8A5292, 0xEA6BFB, 0x5FB11F, 0x8D5D08,
    0x560330, 0x46FC7B, 0x6BABF0, 0xCFBC20, 0x9AF436, 0x1DA9E3,
    0x91615E, 0xE61B08, 0x659985, 0x5F14A0, 0x68408D, 0xFFD880,
    0x4D7327, 0x310606, 0x1556CA, 0x73A8C9, 0x60E27B, 0xC08C6B,
];

const NPIO2_HW: &[u32] = &[
    0x3FF921FB, 0x400921FB, 0x4012D97C, 0x401921FB, 0x401F6A7A, 0x4022D97C,
    0x4025FDBB, 0x402921FB, 0x402C463A, 0x402F6A7A, 0x4031475C, 0x4032D97C,
    0x40346B9C, 0x4035FDBB, 0x40378FDB, 0x403921FB, 0x403AB41B, 0x403C463A,
    0x403DD85A, 0x403F6A7A, 0x40407E4C, 0x4041475C, 0x4042106C, 0x4042D97C,
    0x4043A28C, 0x40446B9C, 0x404534AC, 0x4045FDBB, 0x4046C6CB, 0x40478FDB,
    0x404858EB, 0x404921FB,
];

// kernel_rem_pio2 pi/2 table
const PIO2_TABLE: &[f64] = &[
    1.57079625129699707031e+00,   // 0x3FF921FB, 0x40000000
    7.54978941586159635335e-08,   // 0x3E74442D, 0x00000000
    5.39030252995776476554e-15,   // 0x3CF84698, 0x80000000
    3.28200341580791294123e-22,   // 0x3B78CC51, 0x60000000
    1.27065575308067607349e-29,   // 0x39F01B83, 0x80000000
    1.22933308981111328932e-36,   // 0x387A2520, 0x40000000
    2.73370053816464559624e-44,   // 0x36E38222, 0x80000000
    2.16741683877804819444e-51,   // 0x3569F31D, 0x00000000
];

const TWO24: f64 = 1.67772160000000000000e+07;
const TWON24: f64 = 5.96046447753906250000e-08;

// ============================================================================
// KERNEL FUNCTIONS
// ============================================================================

/// `__kernel_sin` (k_sin.c): sin(x+y) on [-pi/4, pi/4]. `iy == 0` ignores `y`.
#[inline]
fn kernel_sin(x: f64, y: f64, iy: i32) -> f64 {
    let ix = hi(x) & 0x7fff_ffff;
    if ix < 0x3e40_0000 && (x as i32) == 0 {
        return x;
    }
    let z = x * x;
    let v = z * x;
    let r = S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)));
    if iy == 0 {
        x + v * (S1 + z * r)
    } else {
        x - ((z * (HALF * y - v * r) - y) - v * S1)
    }
}

/// `__kernel_cos` (k_cos.c): cos(x+y) on [-pi/4, pi/4].
#[inline]
fn kernel_cos(x: f64, y: f64) -> f64 {
    let ix = hi(x) & 0x7fff_ffff;
    if ix < 0x3e40_0000 && (x as i32) == 0 {
        return ONE;
    }
    let z = x * x;
    let r = z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))));
    if ix < 0x3FD3_3333 {
        return ONE - (HALF * z - (z * r - x * y));
    }
    let qx = if ix > 0x3fe9_0000 {
        0.28125
    } else {
        f64::from_bits(u64::from(ix - 0x0020_0000) << 32)
    };
    let hz = HALF * z - qx;
    let a = ONE - qx;
    a - (hz - (z * r - x * y))
}

/// `__kernel_tan` (k_tan.c): tan(x+y) if `iy == 1`, else -1/tan(x+y), on [-pi/4, pi/4].
fn kernel_tan(x: f64, y: f64, iy: i32) -> f64 {
    let mut x = x;
    let mut y = y;
    let hx = hi(x) as i32;
    let ix = hx & 0x7fff_ffff;

    if ix < 0x3e30_0000 && (x as i32) == 0 {
        if (ix as u32 | lo(x) | (iy + 1) as u32) == 0 {
            return ONE / x.abs();
        }
        if iy == 1 {
            return x;
        }
        // -1/(x+y) with its low word cleared (C: z = w = x+y; t = a = -1/w; __LO(t) = 0)
        let w = x + y;
        let z = with_lo(w, 0);
        let v = y - (z - x);
        let a = -ONE / w;
        let t = with_lo(a, 0);
        let s = ONE + t * z;
        return t + a * (s + t * v);
    }

    let big = ix >= 0x3FE5_9428;
    if big {
        if hx < 0 {
            x = -x;
            y = -y;
        }
        let z = TAN_PIO4 - x;
        let w = TAN_PIO4LO - y;
        x = z + w;
        y = ZERO;
    }

    let z = x * x;
    let w = z * z;
    let r = T_COEFFS[1] + w * (T_COEFFS[3] + w * (T_COEFFS[5] + w * (T_COEFFS[7] + w * (T_COEFFS[9] + w * T_COEFFS[11]))));
    let v = z * (T_COEFFS[2] + w * (T_COEFFS[4] + w * (T_COEFFS[6] + w * (T_COEFFS[8] + w * (T_COEFFS[10] + w * T_COEFFS[12])))));
    let s = z * x;
    let r = y + z * (s * (r + v) + y);
    let r = r + T_COEFFS[0] * s;
    let w = x + r;

    if big {
        let v = f64::from(iy);
        let sign = f64::from(1 - ((hx >> 30) & 2));
        return sign * (v - 2.0 * (x - (w * w / (w + v) - r)));
    }
    if iy == 1 {
        return w;
    }
    let z = with_lo(w, 0);
    let v = r - (z - x);
    let a = -ONE / w;
    let t = with_lo(a, 0);
    let s = ONE + t * z;
    t + a * (s + t * v)
}

// ============================================================================
// ARGUMENT REDUCTION
// ============================================================================

/// `__kernel_rem_pio2` (k_rem_pio2.c) with `prec = 2` (C: `init_jk[2] = 4`), the only
/// precision `__ieee754_rem_pio2` uses. `x` holds `nx` 24-bit chunks of |x| scaled by 2^-(e0-23);
/// `ipio2` is 2/pi in 24-bit integer chunks. Returns n mod 8; `y` gets the reduced value as a
/// head/tail pair. The sign is applied by the caller, as in e_rem_pio2.c.
fn kernel_rem_pio2(x: &[f64], y: &mut [f64; 2], e0: i32, nx: usize, ipio2: &[u32]) -> i32 {
    const JK: usize = 4;
    const JP: usize = 4;
    let jx = nx - 1;
    let mut jv = (e0 - 3) / 24;
    if jv < 0 {
        jv = 0;
    }
    let jv = jv as usize;
    let mut q0 = e0 - 24 * (jv as i32 + 1);

    // f[0..=jx+jk]: f[i] = ipio2[jv - jx + i], zero below index 0.
    let mut f = [ZERO; 20];
    let mut q = [ZERO; 20];
    let mut iq = [0i32; 20];
    let mut fq = [ZERO; 20];
    let m = jx + JK;
    for (i, fi) in f.iter_mut().enumerate().take(m + 1) {
        let j = jv as isize - jx as isize + i as isize;
        *fi = if j < 0 { ZERO } else { f64::from(ipio2[j as usize]) };
    }

    for i in 0..=JK {
        let mut fw = ZERO;
        for j in 0..=jx {
            fw += x[j] * f[jx + i - j];
        }
        q[i] = fw;
    }

    let mut jz = JK;
    let mut z;
    let mut n;
    let mut ih;
    loop {
        // distill q[] into iq[] reversingly
        let mut i = 0;
        let mut j = jz;
        z = q[jz];
        while j > 0 {
            let fw = f64::from((TWON24 * z) as i32);
            iq[i] = (z - TWO24 * fw) as i32;
            z = q[j - 1] + fw;
            i += 1;
            j -= 1;
        }

        // compute n
        z = scalbn(z, q0);
        z -= 8.0 * floor(z * 0.125);
        n = z as i32;
        z -= f64::from(n);
        ih = 0;
        if q0 > 0 {
            let i = iq[jz - 1] >> (24 - q0);
            n += i;
            iq[jz - 1] -= i << (24 - q0);
            ih = iq[jz - 1] >> (23 - q0);
        } else if q0 == 0 {
            ih = iq[jz - 1] >> 23;
        } else if z >= 0.5 {
            ih = 2;
        }

        if ih > 0 {
            // q > 0.5
            n += 1;
            let mut carry = 0;
            for v in iq.iter_mut().take(jz) {
                let j = *v;
                if carry == 0 {
                    if j != 0 {
                        carry = 1;
                        *v = 0x100_0000 - j;
                    }
                } else {
                    *v = 0xff_ffff - j;
                }
            }
            if q0 > 0 {
                match q0 {
                    1 => iq[jz - 1] &= 0x7f_ffff,
                    2 => iq[jz - 1] &= 0x3f_ffff,
                    _ => {}
                }
            }
            if ih == 2 {
                z = ONE - z;
                if carry != 0 {
                    z -= scalbn(ONE, q0);
                }
            }
        }

        // check if recomputation is needed
        if z == ZERO {
            let mut j = 0;
            for v in &iq[JK..jz] {
                j |= *v;
            }
            if j == 0 {
                // k = number of terms needed
                let mut k = 1;
                while iq[JK - k] == 0 {
                    k += 1;
                }
                for i in (jz + 1)..=(jz + k) {
                    f[jx + i] = f64::from(ipio2[jv + i]);
                    let mut fw = ZERO;
                    for j in 0..=jx {
                        fw += x[j] * f[jx + i - j];
                    }
                    q[i] = fw;
                }
                jz += k;
                continue;
            }
        }
        break;
    }

    // chop off zero terms
    if z == ZERO {
        jz -= 1;
        q0 -= 24;
        while iq[jz] == 0 {
            jz -= 1;
            q0 -= 24;
        }
    } else {
        // break z into 24-bit if necessary
        z = scalbn(z, -q0);
        if z >= TWO24 {
            let fw = f64::from((TWON24 * z) as i32);
            iq[jz] = (z - TWO24 * fw) as i32;
            jz += 1;
            q0 += 24;
            iq[jz] = fw as i32;
        } else {
            iq[jz] = z as i32;
        }
    }

    // convert integer "bit" chunks to floating-point values
    let mut fw = scalbn(ONE, q0);
    for i in (0..=jz).rev() {
        q[i] = fw * f64::from(iq[i]);
        fw *= TWON24;
    }

    // compute PIo2[0,...,jp] * q[jz,...,0]
    for i in (0..=jz).rev() {
        let mut fw = ZERO;
        let mut k = 0;
        while k <= JP && k <= jz - i {
            fw += PIO2_TABLE[k] * q[i + k];
            k += 1;
        }
        fq[jz - i] = fw;
    }

    // compress fq[] into y[] (prec = 2)
    let mut fw = ZERO;
    for i in (0..=jz).rev() {
        fw += fq[i];
    }
    y[0] = if ih == 0 { fw } else { -fw };
    fw = fq[0] - fw;
    for i in 1..=jz {
        fw += fq[i];
    }
    y[1] = if ih == 0 { fw } else { -fw };
    n & 7
}

/// `__ieee754_rem_pio2` (e_rem_pio2.c): x = n*pi/2 + y[0] + y[1], |y[0]+y[1]| <= pi/4.
fn ieee754_rem_pio2(x: f64, y: &mut [f64; 2]) -> i32 {
    let hx = hi(x) as i32;
    let ix = hx & 0x7fff_ffff;

    if ix <= 0x3fe9_21fb {
        y[0] = x;
        y[1] = ZERO;
        return 0;
    }

    if ix < 0x4002_d97c {
        // |x| < 3pi/4: n = +-1
        if hx > 0 {
            let mut z = x - PIO2_1;
            if ix != 0x3ff9_21fb {
                y[0] = z - PIO2_1T;
                y[1] = (z - y[0]) - PIO2_1T;
            } else {
                z -= PIO2_2;
                y[0] = z - PIO2_2T;
                y[1] = (z - y[0]) - PIO2_2T;
            }
            return 1;
        } else {
            let mut z = x + PIO2_1;
            if ix != 0x3ff9_21fb {
                y[0] = z + PIO2_1T;
                y[1] = (z - y[0]) + PIO2_1T;
            } else {
                z += PIO2_2;
                y[0] = z + PIO2_2T;
                y[1] = (z - y[0]) + PIO2_2T;
            }
            return -1;
        }
    }

    if ix <= 0x4139_21fb {
        // |x| <= 2^19 * pi/2: medium size
        let t = x.abs();
        let n = (t * INVPIO2 + HALF) as i32;
        let fnn = f64::from(n);
        let mut r = t - fnn * PIO2_1;
        let mut w = fnn * PIO2_1T;
        if n < 32 && ix as u32 != NPIO2_HW[(n - 1) as usize] {
            y[0] = r - w;
        } else {
            let j = ix >> 20;
            y[0] = r - w;
            let mut i = j - ((hi(y[0]) as i32 >> 20) & 0x7ff);
            if i > 16 {
                // 2nd iteration, good to 118 bits
                let t = r;
                w = fnn * PIO2_2;
                r = t - w;
                w = fnn * PIO2_2T - ((t - r) - w);
                y[0] = r - w;
                i = j - ((hi(y[0]) as i32 >> 20) & 0x7ff);
                if i > 49 {
                    // 3rd iteration
                    let t = r;
                    w = fnn * PIO2_3;
                    r = t - w;
                    w = fnn * PIO2_3T - ((t - r) - w);
                    y[0] = r - w;
                }
            }
        }
        y[1] = (r - y[0]) - w;
        if hx < 0 {
            y[0] = -y[0];
            y[1] = -y[1];
            return -n;
        }
        return n;
    }

    if ix >= 0x7ff0_0000 {
        // inf or NaN
        let nan = x - x;
        y[0] = nan;
        y[1] = nan;
        return 0;
    }

    // large: z = scalbn(|x|, ilogb(x) - 23), split into three 24-bit chunks
    let e0 = (ix >> 20) - 1046;
    let z0 = with_hi(x.abs(), (ix - (e0 << 20)) as u32);
    let mut z = z0;
    let mut tx = [ZERO; 3];
    for t in tx.iter_mut().take(2) {
        *t = f64::from(z as i32);
        z = (z - *t) * TWO24;
    }
    tx[2] = z;
    let mut nx = 3;
    while tx[nx - 1] == ZERO {
        nx -= 1;
    }
    let n = kernel_rem_pio2(&tx[..nx], y, e0, nx, TWO_OVER_PI);
    if hx < 0 {
        y[0] = -y[0];
        y[1] = -y[1];
        return -n;
    }
    n
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Sine function. `sin(+-inf)` and `sin(NaN)` return NaN; `sin(-0.0) = -0.0`.
pub fn sin(x: f64) -> f64 {
    let ix = hi(x) & 0x7fff_ffff;
    if ix <= 0x3fe9_21fb {
        return kernel_sin(x, ZERO, 0);
    }
    if ix >= 0x7ff0_0000 {
        return x - x;
    }
    let mut y = [ZERO; 2];
    let n = ieee754_rem_pio2(x, &mut y);
    match n & 3 {
        0 => kernel_sin(y[0], y[1], 1),
        1 => kernel_cos(y[0], y[1]),
        2 => -kernel_sin(y[0], y[1], 1),
        _ => -kernel_cos(y[0], y[1]),
    }
}

/// Cosine function. `cos(+-inf)` and `cos(NaN)` return NaN.
pub fn cos(x: f64) -> f64 {
    let ix = hi(x) & 0x7fff_ffff;
    if ix <= 0x3fe9_21fb {
        return kernel_cos(x, ZERO);
    }
    if ix >= 0x7ff0_0000 {
        return x - x;
    }
    let mut y = [ZERO; 2];
    let n = ieee754_rem_pio2(x, &mut y);
    match n & 3 {
        0 => kernel_cos(y[0], y[1]),
        1 => -kernel_sin(y[0], y[1], 1),
        2 => -kernel_cos(y[0], y[1]),
        _ => kernel_sin(y[0], y[1], 1),
    }
}

/// Tangent function. `tan(+-inf)` and `tan(NaN)` return NaN; `tan(-0.0) = -0.0`.
pub fn tan(x: f64) -> f64 {
    let ix = hi(x) & 0x7fff_ffff;
    if ix <= 0x3fe9_21fb {
        return kernel_tan(x, ZERO, 1);
    }
    if ix >= 0x7ff0_0000 {
        return x - x;
    }
    let mut y = [ZERO; 2];
    let n = ieee754_rem_pio2(x, &mut y);
    // 1 for even n, -1 for odd n
    let iy = 1 - ((n & 1) << 1);
    kernel_tan(y[0], y[1], iy)
}

/// Both sine and cosine of `x`: `(sin(x), cos(x))`, bitwise equal to `(sin(x), cos(x))`.
/// `sin_cos(+-inf)` and `sin_cos(NaN)` have both components NaN.
pub fn sin_cos(x: f64) -> (f64, f64) {
    let ix = hi(x) & 0x7fff_ffff;
    if ix <= 0x3fe9_21fb {
        return (kernel_sin(x, ZERO, 0), kernel_cos(x, ZERO));
    }
    if ix >= 0x7ff0_0000 {
        let nan = x - x;
        return (nan, nan);
    }
    let mut y = [ZERO; 2];
    let n = ieee754_rem_pio2(x, &mut y);
    match n & 3 {
        0 => (kernel_sin(y[0], y[1], 1), kernel_cos(y[0], y[1])),
        1 => (kernel_cos(y[0], y[1]), -kernel_sin(y[0], y[1], 1)),
        2 => (-kernel_sin(y[0], y[1], 1), -kernel_cos(y[0], y[1])),
        _ => (-kernel_cos(y[0], y[1]), kernel_sin(y[0], y[1], 1)),
    }
}
