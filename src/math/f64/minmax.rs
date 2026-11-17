//! `max` and `min` of two f64, with the zero sign fixed, so the result is the same bits on every
//! platform. Core's `f64::max` and `min` are not allowed in items: the sign of a zero result is
//! unspecified there. Each function here is comparisons plus `is_nan` and `is_sign_negative`.

/// The larger of `a` and `b`. If one is NaN, the other is returned (two NaNs give NaN). If they are
/// equal and zero, the result is the one with its sign bit clear, so `max(-0.0, 0.0)` is `0.0` in
/// both argument orders.
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return b;
    }
    if b.is_nan() {
        return a;
    }
    if a > b {
        a
    } else if b > a {
        b
    } else if b.is_sign_negative() {
        a
    } else {
        b
    }
}

/// The smaller of `a` and `b`. If one is NaN, the other is returned (two NaNs give NaN). If they are
/// equal and zero, the result is the one with its sign bit set, so `min(0.0, -0.0)` is `-0.0` in
/// both argument orders.
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return b;
    }
    if b.is_nan() {
        return a;
    }
    if a < b {
        a
    } else if b < a {
        b
    } else if a.is_sign_negative() {
        a
    } else {
        b
    }
}
