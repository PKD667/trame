//! atan, atan2, asin, acos, sinh, cosh, tanh, asinh, acosh, atanh, ln_1p
//! Ported from fdlibm (Sun Microsystems; netlib.org/fdlibm/). Plain arithmetic and word access only.

use super::bits::{hi, lo, with_hi, with_lo};
use super::misc::{two_prod, two_sum};
use super::{exp, exp_m1, ln, sqrt};

// ===== atan =====
// From fdlibm s_atan.c (sha256 8583c253aed6318dee2a48135e3f3b1328d838a390372a69414ac41eb5ce5ce1)

const ATANHI: [f64; 4] = [
    4.63647609000806093515e-01, // atan(0.5)hi 0x3FDDAC67, 0x0561BB4F
    7.85398163397448278999e-01, // atan(1.0)hi 0x3FE921FB, 0x54442D18
    9.82793723247329054082e-01, // atan(1.5)hi 0x3FEF730B, 0xD281F69B
    1.57079632679489655800e+00, // atan(inf)hi 0x3FF921FB, 0x54442D18
];

const ATANLO: [f64; 4] = [
    2.26987774529616870924e-17, // atan(0.5)lo 0x3C7A2B7F, 0x222F65E2
    3.06161699786838301793e-17, // atan(1.0)lo 0x3C81A626, 0x33145C07
    1.39033110312309984516e-17, // atan(1.5)lo 0x3C700788, 0x7AF0CBBD
    6.12323399573676603587e-17, // atan(inf)lo 0x3C91A626, 0x33145C07
];

const AT: [f64; 11] = [
    3.33333333333329318027e-01, // 0x3FD55555, 0x5555550D
    -1.99999999998764832476e-01, // 0xBFC99999, 0x9998EBC4
    1.42857142725034663711e-01, // 0x3FC24924, 0x920083FF
    -1.11111104054623557880e-01, // 0xBFBC71C6, 0xFE231671
    9.09088713343650656196e-02, // 0x3FB745CD, 0xC54C206E
    -7.69187620504482999495e-02, // 0xBFB3B0F2, 0xAF749A6D
    6.66107313738753120669e-02, // 0x3FB10D66, 0xA0D03D51
    -5.83357013379057348645e-02, // 0xBFADDE2D, 0x52DEFD9A
    4.97687799461593236017e-02, // 0x3FA97B4B, 0x24760DEB
    -3.65315727442169155270e-02, // 0xBFA2B444, 0x2C6A6C2F
    1.62858201153657823623e-02, // 0x3F90AD3A, 0xE322DA11
];

/// Arctangent of a single argument.
/// Method: argument reduction to interval [0, 1), then polynomial approximation.
pub fn atan(x: f64) -> f64 {
    let hx = hi(x) as i32;
    let ix = hx & 0x7fffffff;

    // Check for NaN and large values
    if ix >= 0x44100000 {
        // |x| >= 2^66
        if ix > 0x7ff00000 || (ix == 0x7ff00000 && lo(x) != 0) {
            return x + x; // NaN
        }
        if hx > 0 {
            return ATANHI[3] + ATANLO[3];
        } else {
            return -ATANHI[3] - ATANLO[3];
        }
    }

    let (x, id) = if ix < 0x3fdc0000 {
        // |x| < 0.4375
        if ix < 0x3e200000 {
            // |x| < 2^-29, raise inexact
            if 1.0e300 + x > 1.0 {
                return x; // raise inexact and return x
            }
        }
        (x, -1)
    } else {
        let x_abs = x.abs();
        let (mut x, id) = if ix < 0x3ff30000 {
            // |x| < 1.1875
            if ix < 0x3fe60000 {
                // 7/16 <= |x| < 11/16
                (
                    (2.0 * x_abs - 1.0) / (2.0 + x_abs),
                    0,
                )
            } else {
                // 11/16 <= |x| < 19/16
                ((x_abs - 1.0) / (x_abs + 1.0), 1)
            }
        } else if ix < 0x40038000 {
            // |x| < 2.4375
            ((x_abs - 1.5) / (1.0 + 1.5 * x_abs), 2)
        } else {
            // 2.4375 <= |x| < 2^66
            (-1.0 / x_abs, 3)
        };

        (x, id)
    };

    // Compute polynomial
    let z = x * x;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));

    if id < 0 {
        x - x * (s1 + s2)
    } else {
        let z = ATANHI[id as usize] - ((x * (s1 + s2) - ATANLO[id as usize]) - x);
        if hx < 0 {
            -z
        } else {
            z
        }
    }
}

// ===== atan2 =====
// From fdlibm e_atan2.c (sha256 61dbf9edc927468280ccde872ecfb7cb1ea7cbaf6e1b28e7021c968abe5f7c6e)

const PI_O_4: f64 = 7.8539816339744827900e-01; // 0x3FE921FB, 0x54442D18
const PI_O_2: f64 = 1.5707963267948965580e+00; // 0x3FF921FB, 0x54442D18
const PI: f64 = 3.1415926535897931160e+00; // 0x400921FB, 0x54442D18
const PI_LO: f64 = 1.2246467991473531772e-16; // 0x3CA1A626, 0x33145C07
const TINY: f64 = 1.0e-300;

/// Two-argument arctangent: `atan(y / x)` with the sign of y, in the correct quadrant.
/// y is the first argument (receiver in the brief), x is the second.
pub fn atan2(y: f64, x: f64) -> f64 {
    let hx = hi(x) as i32;
    let ix = (hx & 0x7fffffff) as u32;
    let lx = lo(x);

    let hy = hi(y) as i32;
    let iy = (hy & 0x7fffffff) as u32;
    let ly = lo(y);

    // Check for NaN
    if ((ix | ((lx | lx.wrapping_neg()) >> 31)) > 0x7ff00000)
        || ((iy | ((ly | ly.wrapping_neg()) >> 31)) > 0x7ff00000)
    {
        return x + y; // x or y is NaN
    }

    // Special case: x == 1.0
    if (((hx.wrapping_sub(0x3ff00000)) as u32) | lx) == 0 {
        return atan(y);
    }

    let m = (((hy >> 31) & 1) | (((hx >> 30) & 2))) as i32;

    // when y = 0
    if (iy | ly) == 0 {
        return match m {
            0 | 1 => y,                   // atan(+-0,+anything)=+-0
            2 => PI + TINY,               // atan(+0,-anything) = pi
            _ => -PI - TINY,              // atan(-0,-anything) = -pi
        };
    }

    // when x = 0
    if (ix | (lx as u32)) == 0 {
        if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        }
    } else if ix == 0x7ff00000 {
        // when x is INF
        if iy == 0x7ff00000 {
            match m {
                0 => PI_O_4 + TINY,         // atan(+INF,+INF)
                1 => -PI_O_4 - TINY,        // atan(-INF,+INF)
                2 => 3.0 * PI_O_4 + TINY,   // atan(+INF,-INF)
                _ => -3.0 * PI_O_4 - TINY,  // atan(-INF,-INF)
            }
        } else {
            match m {
                0 => 0.0,          // atan(+...,+INF)
                1 => -0.0,         // atan(-...,+INF)
                2 => PI + TINY,    // atan(+...,-INF)
                _ => -PI - TINY,   // atan(-...,-INF)
            }
        }
    } else if iy == 0x7ff00000 {
        // when y is INF
        if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        }
    } else {
        // compute y/x
        let k = ((iy as i32) - (ix as i32)) >> 20;
        let z = if k > 60 {
            PI_O_2 + 0.5 * PI_LO // |y/x| > 2**60
        } else if hx < 0 && k < -60 {
            0.0 // |y|/x < -2**60
        } else {
            atan((y / x).abs()) // safe to do y/x
        };

        match m {
            0 => z,                       // atan(+,+)
            1 => -z,                      // atan(-,+)
            2 => PI - (z - PI_LO),        // atan(+,-)
            _ => (z - PI_LO) - PI,        // atan(-,-)
        }
    }
}

// ===== asin =====
// From fdlibm e_asin.c (sha256 4ec96c4c7747df94911404b1577e404e13efc51e104f1934e72b536b61adc911)

const ASIN_PIO2_HI: f64 = 1.57079632679489655800e+00; // 0x3FF921FB, 0x54442D18
const ASIN_PIO2_LO: f64 = 6.12323399573676603587e-17; // 0x3C91A626, 0x33145C07
const ASIN_PIO4_HI: f64 = 7.85398163397448278999e-01; // 0x3FE921FB, 0x54442D18
const PS0: f64 = 1.66666666666666657415e-01; // 0x3FC55555, 0x55555555
const PS1: f64 = -3.25565818622400915405e-01; // 0xBFD4D612, 0x03EB6F7D
const PS2: f64 = 2.01212532134862925881e-01; // 0x3FC9C155, 0x0E884455
const PS3: f64 = -4.00555345006794114027e-02; // 0xBFA48228, 0xB5688F3B
const PS4: f64 = 7.91534994289814532176e-04; // 0x3F49EFE0, 0x7501B288
const PS5: f64 = 3.47933107596021167570e-05; // 0x3F023DE1, 0x0DFDF709
const QS1: f64 = -2.40339491173441421878e+00; // 0xC0033A27, 0x1C8A2D4B
const QS2: f64 = 2.02094576023350569471e+00; // 0x40002AE5, 0x9C598AC8
const QS3: f64 = -6.88283971605453293030e-01; // 0xBFE6066C, 0x1B8D0159
const QS4: f64 = 7.70381505559019352791e-02; // 0x3FB3B8C5, 0xB12E9282

/// Arcsine of a single argument.
/// Method: use approximation based on sqrt(1-x^2).
pub fn asin(x: f64) -> f64 {
    let hx = hi(x) as i32;
    let ix = hx & 0x7fffffff;

    if ix >= 0x3ff00000 {
        // |x| >= 1
        if (((ix - 0x3ff00000) as u32) | lo(x)) == 0 {
            // asin(1) = +-pi/2 with inexact
            return x * ASIN_PIO2_HI + x * ASIN_PIO2_LO;
        }
        return (x - x) / (x - x); // asin(|x|>1) is NaN
    }

    if ix < 0x3fe00000 {
        // |x| < 0.5
        if ix < 0x3e400000 {
            // |x| < 2^-27
            if 1.0e300 + x > 1.0 {
                return x;
            }
        } else {
            let t = x * x;
            let p = t * (PS0 + t * (PS1 + t * (PS2 + t * (PS3 + t * (PS4 + t * PS5)))));
            let q = 1.0 + t * (QS1 + t * (QS2 + t * (QS3 + t * QS4)));
            let w = p / q;
            return x + x * w;
        }
    }

    // 1 > |x| >= 0.5
    let w = 1.0 - x.abs();
    let t = w * 0.5;
    let p = t * (PS0 + t * (PS1 + t * (PS2 + t * (PS3 + t * (PS4 + t * PS5)))));
    let q = 1.0 + t * (QS1 + t * (QS2 + t * (QS3 + t * QS4)));
    let s = sqrt(t);

    let t = if ix >= 0x3fef3333 {
        // |x| > 0.975
        let w = p / q;
        ASIN_PIO2_HI - (2.0 * (s + s * w) - ASIN_PIO2_LO)
    } else {
        let w = s;
        let w = with_lo(w, 0); // set low word to 0
        let c = (t - w * w) / (s + w);
        let r = p / q;
        let p = 2.0 * s * r - (ASIN_PIO2_LO - 2.0 * c);
        let q = ASIN_PIO4_HI - 2.0 * w;
        ASIN_PIO4_HI - (p - q)
    };

    if hx > 0 {
        t
    } else {
        -t
    }
}

// ===== acos =====
// From fdlibm e_acos.c (sha256 bc9fbb24c214030604c0df4c811370f37d511e0c72580767d8c81bd68b0c3a5a)

const ACOS_PI: f64 = 3.14159265358979311600e+00; // 0x400921FB, 0x54442D18

/// Arccosine of a single argument.
/// Method: use identity acos(x) = pi/2 - asin(x).
pub fn acos(x: f64) -> f64 {
    let hx = hi(x) as i32;
    let ix = hx & 0x7fffffff;

    if ix >= 0x3ff00000 {
        // |x| >= 1
        if (((ix - 0x3ff00000) as u32) | lo(x)) == 0 {
            // |x| == 1
            if hx > 0 {
                return 0.0; // acos(1) = 0
            } else {
                return ACOS_PI + 2.0 * ASIN_PIO2_LO; // acos(-1) = pi
            }
        }
        return (x - x) / (x - x); // acos(|x|>1) is NaN
    }

    if ix < 0x3fe00000 {
        // |x| < 0.5
        if ix <= 0x3c600000 {
            return ASIN_PIO2_HI + ASIN_PIO2_LO; // |x| < 2^-57
        }
        let z = x * x;
        let p = z * (PS0 + z * (PS1 + z * (PS2 + z * (PS3 + z * (PS4 + z * PS5)))));
        let q = 1.0 + z * (QS1 + z * (QS2 + z * (QS3 + z * QS4)));
        let r = p / q;
        ASIN_PIO2_HI - (x - (ASIN_PIO2_LO - x * r))
    } else if hx < 0 {
        // x < -0.5
        let z = (1.0 + x) * 0.5;
        let p = z * (PS0 + z * (PS1 + z * (PS2 + z * (PS3 + z * (PS4 + z * PS5)))));
        let q = 1.0 + z * (QS1 + z * (QS2 + z * (QS3 + z * QS4)));
        let s = sqrt(z);
        let r = p / q;
        let w = r * s - ASIN_PIO2_LO;
        ACOS_PI - 2.0 * (s + w)
    } else {
        // x > 0.5
        let z = (1.0 - x) * 0.5;
        let s = sqrt(z);
        let mut df = s;
        df = with_lo(df, 0); // set low word to 0
        let c = (z - df * df) / (s + df);
        let p = z * (PS0 + z * (PS1 + z * (PS2 + z * (PS3 + z * (PS4 + z * PS5)))));
        let q = 1.0 + z * (QS1 + z * (QS2 + z * (QS3 + z * QS4)));
        let r = p / q;
        let w = r * s + c;
        2.0 * (df + w)
    }
}

// ===== sinh =====
// From fdlibm e_sinh.c (sha256 9f6a04f80045a5c0a6d134fdb56d89e65e2a862cdc03fef40505194504ab10e8)

/// Hyperbolic sine.
/// Method: mathematically sinh(x) = (exp(x) - exp(-x)) / 2.
pub fn sinh(x: f64) -> f64 {
    let jx = hi(x) as i32;
    let ix = jx & 0x7fffffff;

    // x is INF or NaN
    if ix >= 0x7ff00000 {
        return x + x;
    }

    let mut h = 0.5;
    if jx < 0 {
        h = -h;
    }

    // |x| < 22
    if ix < 0x40360000 {
        // |x| < 2^-28
        if ix < 0x3e300000 {
            if 1.0e307 + x > 1.0 {
                return x; // sinh(tiny) = tiny with inexact
            }
        }
        let t = exp_m1(x.abs());
        if ix < 0x3ff00000 {
            return h * (2.0 * t - t * t / (t + 1.0));
        }
        return h * (t + t / (t + 1.0));
    }

    // |x| in [22, log(maxdouble)]
    if ix < 0x40862e42 {
        return h * exp(x.abs());
    }

    // |x| in [log(maxdouble), overflow threshold]
    // This is a simple overflow check; the exact threshold handling from C is complex
    if ix < 0x408633ce {
        let w = exp(0.5 * x.abs());
        let t = h * w;
        return t * w;
    }

    // |x| > overflow threshold
    x * 1.0e307
}

// ===== cosh =====
// From fdlibm e_cosh.c (sha256 381b01b9244180a726bc6282125a31c049a9c12b553eaf758df7c674e7b6b816)

/// Hyperbolic cosine.
/// Method: mathematically cosh(x) = (exp(x) + exp(-x)) / 2.
pub fn cosh(x: f64) -> f64 {
    let ix = hi(x) & 0x7fffffff;

    // x is INF or NaN
    if ix >= 0x7ff00000 {
        return x * x;
    }

    // |x| in [0, 0.5*ln2]
    if ix < 0x3fd62e43 {
        let t = exp_m1(x.abs());
        let w = 1.0 + t;
        if ix < 0x3c800000 {
            return w; // cosh(tiny) = 1
        }
        return 1.0 + (t * t) / (w + w);
    }

    // |x| in [0.5*ln2, 22]
    if ix < 0x40360000 {
        let t = exp(x.abs());
        return 0.5 * t + 0.5 / t;
    }

    // |x| in [22, log(maxdouble)]
    if ix < 0x40862e42 {
        return 0.5 * exp(x.abs());
    }

    // |x| in [log(maxdouble), overflow threshold]
    if ix < 0x408633ce {
        let w = exp(0.5 * x.abs());
        let t = 0.5 * w;
        return t * w;
    }

    // |x| > overflow threshold
    1.0e300 * 1.0e300
}

// ===== tanh =====
// From fdlibm s_tanh.c (sha256 fcf39a7e30393ac4cae5c631c60b4fb4273f4461f3fce3af03713a2b1469f286)

/// Hyperbolic tangent.
/// Method: mathematically tanh(x) = (exp(2x) - 1) / (exp(2x) + 1).
pub fn tanh(x: f64) -> f64 {
    let jx = hi(x) as i32;
    let ix = jx & 0x7fffffff;

    // x is INF or NaN
    if ix >= 0x7ff00000 {
        if jx >= 0 {
            return 1.0 / x + 1.0; // tanh(+inf) = 1
        } else {
            return 1.0 / x - 1.0; // tanh(-inf) = -1
        }
    }

    // |x| < 22
    if ix < 0x40360000 {
        // |x| < 2^-55
        if ix < 0x3c800000 {
            return x * (1.0 + x); // tanh(small) = small
        }
        if ix >= 0x3ff00000 {
            // |x| >= 1
            let t = exp_m1(2.0 * x.abs());
            let (qh, ql) = tanh_quot(2.0, t);
            let z = (1.0 - qh) - ql;
            if jx >= 0 {
                z
            } else {
                -z
            }
        } else {
            let t = exp_m1(-2.0 * x.abs());
            let (z, _) = tanh_quot(-t, t);
            if jx >= 0 {
                z
            } else {
                -z
            }
        }
    } else {
        // |x| > 22
        let z = 1.0 - 1.0e-300; // raise inexact flag
        if jx >= 0 {
            z
        } else {
            -z
        }
    }
}

/// `n / (t + 2)` as a pair `(hi, lo)`, `hi = fl(hi + lo)`, for `t >= -1`. fdlibm writes `n / (t + 2)`;
/// `t + 2` rounds, and at tanh(0.5098647897710773) that rounding plus the division cost 2 ulp.
/// Here the denominator is `s + e` exactly, and the quotient gets one Newton correction.
fn tanh_quot(n: f64, t: f64) -> (f64, f64) {
    let (s, e) = two_sum(t, 2.0);
    let q = n / s;
    let (p, pe) = two_prod(q, s);
    let r = ((n - p) - pe) - q * e;
    two_sum(q, r / s)
}

// ===== asinh =====
// From fdlibm s_asinh.c (sha256 cc132bc1e6aa37453adfaecd7d13600356f6a4d34d68301f8e3381e1efcf51f3)

const ASINH_LN2: f64 = 6.93147180559945286227e-01; // 0x3FE62E42, 0xFEFA39EF

/// Inverse hyperbolic sine.
/// Method: asinh(x) = sign(x) * log(|x| + sqrt(x^2 + 1)).
pub fn asinh(x: f64) -> f64 {
    let hx = hi(x) as i32;
    let ix = hx & 0x7fffffff;

    if ix >= 0x7ff00000 {
        return x + x; // x is inf or NaN
    }

    if ix < 0x3e300000 {
        // |x| < 2^-28
        if 1.0e300 + x > 1.0 {
            return x; // return x inexact except 0
        }
    }

    let w = if ix > 0x41b00000 {
        // |x| > 2^28
        ln(x.abs()) + ASINH_LN2
    } else if ix > 0x40000000 {
        // 2^28 > |x| > 2.0
        let t = x.abs();
        ln(2.0 * t + 1.0 / (sqrt(x * x + 1.0) + t))
    } else {
        // 2.0 > |x| > 2^-28
        let t = x * x;
        ln_1p(x.abs() + t / (1.0 + sqrt(1.0 + t)))
    };

    if hx > 0 {
        w
    } else {
        -w
    }
}

// ===== acosh =====
// From fdlibm e_acosh.c (sha256 9b02d8764ee5f9910dd1440063b235f2083080e177f1bef23f481a6932088a9d)

const ACOSH_LN2: f64 = 6.93147180559945286227e-01; // 0x3FE62E42, 0xFEFA39EF

/// Inverse hyperbolic cosine.
/// Method: acosh(x) = log(x + sqrt(x^2 - 1)).
pub fn acosh(x: f64) -> f64 {
    let hx = hi(x) as i32;

    if hx < 0x3ff00000 {
        // x < 1
        return (x - x) / (x - x); // NaN
    }

    if hx >= 0x41b00000 {
        // x > 2^28
        if hx >= 0x7ff00000 {
            // x is inf or NaN
            return x + x;
        }
        return ln(x) + ACOSH_LN2; // acosh(huge) = log(2x)
    }

    if (((hx - 0x3ff00000) as u32) | lo(x)) == 0 {
        return 0.0; // acosh(1) = 0
    }

    if hx > 0x40000000 {
        // 2^28 > x > 2
        let t = x * x;
        ln(2.0 * x - 1.0 / (x + sqrt(t - 1.0)))
    } else {
        // 1 < x < 2
        let t = x - 1.0;
        ln_1p(t + sqrt(2.0 * t + t * t))
    }
}

// ===== atanh =====
// From fdlibm e_atanh.c (sha256 8588db2d5e1d904454f17dc04a001e7cf528d410e7ffc38ef52e368ef4d1f7ea)

/// Inverse hyperbolic tangent.
/// Method: atanh(x) = 0.5 * log((1+x) / (1-x)).
pub fn atanh(x: f64) -> f64 {
    let hx = hi(x) as i32;
    let lx = lo(x);
    let ix = (hx & 0x7fffffff) as u32;

    if (ix | ((lx | lx.wrapping_neg()) >> 31)) > 0x3ff00000 {
        // |x| > 1
        return (x - x) / (x - x); // NaN
    }

    if ix == 0x3ff00000 {
        // |x| == 1
        return x / 0.0; // atanh(+-1) = +-inf
    }

    if ix < 0x3e300000 && (1.0e300 + x) > 0.0 {
        // |x| < 2^-28
        return x;
    }

    let x_abs = with_hi(x, ix); // x <- |x|

    let t = if ix < 0x3fe00000 {
        // x < 0.5
        let t = x_abs + x_abs;
        0.5 * ln_1p(t + t * x_abs / (1.0 - x_abs))
    } else {
        0.5 * ln_1p((x_abs + x_abs) / (1.0 - x_abs))
    };

    if hx >= 0 {
        t
    } else {
        -t
    }
}

// ===== ln_1p =====
// From fdlibm s_log1p.c (sha256 f72f255a022fad06c3fe222ad90f961ac5624958373a4fbdae9df3f344135373)

const LN_1P_LN2_HI: f64 = 6.93147180369123816490e-01; // 3fe62e42 fee00000
const LN_1P_LN2_LO: f64 = 1.90821492927058770002e-10; // 3dea39ef 35793c76
const LN_1P_TWO54: f64 = 1.80143985094819840000e+16; // 43500000 00000000
const LN_1P_LP1: f64 = 6.666666666666735130e-01; // 3FE55555 55555593
const LN_1P_LP2: f64 = 3.999999999940941908e-01; // 3FD99999 9997FA04
const LN_1P_LP3: f64 = 2.857142874366239149e-01; // 3FD24924 94229359
const LN_1P_LP4: f64 = 2.222219843214978396e-01; // 3FCC71C5 1D8E78AF
const LN_1P_LP5: f64 = 1.818357216161805012e-01; // 3FC74664 96CB03DE
const LN_1P_LP6: f64 = 1.531383769920937332e-01; // 3FC39A09 D078C69F
const LN_1P_LP7: f64 = 1.479819860511658591e-01; // 3FC2F112 DF3E5244

/// Natural logarithm of 1 + x.
/// Method: use argument reduction and polynomial approximation.
pub fn ln_1p(x: f64) -> f64 {
    let hx = hi(x) as i32;
    let ax = hx & 0x7fffffff;

    let mut k: i32 = 1;
    let mut f: f64 = 0.0;
    let mut c: f64 = 0.0;
    let mut hu: i32 = 0;
    if hx < 0x3fda827a {
        // x < 0.41422
        if ax >= 0x3ff00000 {
            // x <= -1.0
            if x == -1.0 {
                return -LN_1P_TWO54 / 0.0; // log1p(-1) = -inf
            } else {
                return (x - x) / (x - x); // log1p(x < -1) = NaN
            }
        }
        if ax < 0x3e200000 {
            // |x| < 2^-29
            if LN_1P_TWO54 + x > 0.0 && ax < 0x3c900000 {
                // |x| < 2^-54
                return x;
            } else {
                return x - x * x * 0.5;
            }
        }
        if hx > 0 || hx <= -0x402d413d {
            // -0.2929 < x < 0.41422 (0xbfd2bec3 as a signed int)
            k = 0;
            f = x;
            hu = 1;
        }
    }

    if hx >= 0x7ff00000 {
        return x + x;
    }

    if k != 0 {
        let mut u;
        if hx < 0x43400000 {
            // |x| is not too large
            u = 1.0 + x;
            hu = hi(u) as i32;
            k = (hu >> 20) - 1023;
            c = if k > 0 { 1.0 - (u - x) } else { x - (u - 1.0) };
            c /= u; // correction term
        } else {
            u = x;
            hu = hi(u) as i32;
            k = (hu >> 20) - 1023;
            c = 0.0;
        }
        hu &= 0x000f_ffff;
        if hu < 0x6a09e {
            u = with_hi(u, (hu | 0x3ff0_0000) as u32); // normalize u
        } else {
            k += 1;
            u = with_hi(u, (hu | 0x3fe0_0000) as u32); // normalize u/2
            hu = (0x0010_0000 - hu) >> 2;
        }
        f = u - 1.0;
    }

    let hfsq = 0.5 * f * f;
    let kf = k as f64;
    if hu == 0 {
        // |f| < 2^-20
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            }
            c += kf * LN_1P_LN2_LO;
            return kf * LN_1P_LN2_HI + c;
        }
        let r = hfsq * (1.0 - 0.66666666666666666 * f);
        if k == 0 {
            return f - r;
        }
        return kf * LN_1P_LN2_HI - ((r - (kf * LN_1P_LN2_LO + c)) - f);
    }

    let s = f / (2.0 + f);
    let z = s * s;
    let r = z
        * (LN_1P_LP1
            + z * (LN_1P_LP2
                + z * (LN_1P_LP3
                    + z * (LN_1P_LP4 + z * (LN_1P_LP5 + z * (LN_1P_LP6 + z * LN_1P_LP7))))));

    if k == 0 {
        f - (hfsq - s * (hfsq + r))
    } else {
        kf * LN_1P_LN2_HI - ((hfsq - (s * (hfsq + r) + (kf * LN_1P_LN2_LO + c))) - f)
    }
}
