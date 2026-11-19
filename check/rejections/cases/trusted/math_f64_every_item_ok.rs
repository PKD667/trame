//@check-pass
// spec: Section 7 (math::f64): every function of trame::math::f64 is pure code at the item level. One item calls each
// of them, so a function added to the module and not trusted (or one that reaches std) shows up here.
// crates/driver/README.md: a function defined in trame is trusted at every level, so its body is not read.
struct Work {
    index: u32,
    by: f64,
}

#[trame::item(key = x.index as usize)]
fn all(x: &Work, slot: &mut f64, _cx: &()) {
    use trame::math::f64 as m;
    let v = x.by;
    let a = if v < 0.0 { -v } else { v };
    let u = a / (a + 1.0); // in [0, 1)
    let (s, c) = m::sin_cos(v);
    *slot = m::exp(v) + m::exp2(v) + m::exp_m1(v)
        + m::ln(a + 1.0) + m::ln_1p(a) + m::log2(a + 1.0) + m::log10(a + 1.0)
        + m::sqrt(a) + m::cbrt(v) + m::hypot(v, a) + m::powf(a + 1.0, v) + m::powi(v, 3)
        + m::sin(v) + m::cos(v) + m::tan(v) + s + c
        + m::asin(u) + m::acos(u) + m::atan(v) + m::atan2(v, a)
        + m::sinh(v) + m::cosh(v) + m::tanh(v) + m::asinh(v) + m::acosh(a + 1.0) + m::atanh(u)
        + m::floor(v) + m::ceil(v) + m::trunc(v) + m::fract(v) + m::round(v) + m::round_ties_even(v)
        + m::max(v, a) + m::min(v, a) + m::mul_add(v, a, u);
}

fn main() {
    let items = [Work { index: 0, by: 1.5 }];
    let mut slots = [0.0_f64];
    trame::parallel!(all, &(), &items, &mut slots);
}
