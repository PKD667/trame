//! f32 `mul_add` is exact: the same bits as std's `f32::mul_add` (correctly rounded), no tolerance.
//! Any NaN equals any NaN. The sweep counts the triples where a double rounding through f64
//! (`(f64(a) * f64(b) + f64(c)) as f32`, the shortcut that is not allowed) differs, and asserts
//! the count is positive, so a test that passes for that shortcut is detected.

extern crate std;
use std::eprintln;

use super::mul_add;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 16
    }
}

fn check(a: f32, b: f32, c: f32) {
    let ours = mul_add(a, b, c);
    let theirs = std::primitive::f32::mul_add(a, b, c);
    if theirs.is_nan() {
        assert!(ours.is_nan(), "mul_add({a:e}, {b:e}, {c:e}) = {ours:e}, std NaN");
    } else {
        assert_eq!(
            ours.to_bits(),
            theirs.to_bits(),
            "mul_add({a:e} [{:08x}], {b:e} [{:08x}], {c:e} [{:08x}]) = {ours:e}, std {theirs:e}",
            a.to_bits(),
            b.to_bits(),
            c.to_bits()
        );
    }
}

fn with_exp(rng: &mut Rng, e: i32) -> f32 {
    let m = (rng.next() as u32) & 0x007f_ffff;
    let s = (rng.next() as u32) & 0x8000_0000;
    f32::from_bits(s | (((e + 127) as u32) << 23) | m)
}

#[test]
fn special_values_match_std() {
    let vs = [
        0.0_f32,
        -0.0,
        1.0,
        -1.0,
        2.0,
        0.5,
        3.0,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        f32::MAX,
        f32::MIN,
        f32::MIN_POSITIVE,
        -f32::MIN_POSITIVE,
        f32::from_bits(1),
        -f32::from_bits(1),
        f32::from_bits(0x007f_ffff),
        1.0 + f32::EPSILON,
        1.0e30,
        1.0e-30,
    ];
    for a in vs {
        for b in vs {
            for c in vs {
                check(a, b, c);
            }
        }
    }
}

fn sweep(seed: u64, name: &str, mut triple: impl FnMut(&mut Rng) -> (f32, f32, f32)) {
    let mut rng = Rng(seed);
    let mut shortcut_differs = 0;
    for _ in 0..1_000_000 {
        let (a, b, c) = triple(&mut rng);
        check(a, b, c);
        let short = (f64::from(a) * f64::from(b) + f64::from(c)) as f32;
        let fused = mul_add(a, b, c);
        if !(short.is_nan() && fused.is_nan()) && short.to_bits() != fused.to_bits() {
            shortcut_differs += 1;
        }
    }
    eprintln!("f32 fma {name}: the f64 shortcut differs from the fused result in {shortcut_differs} of 1000000");
}

#[test]
fn random_bit_patterns() {
    sweep(0x5eed_f101, "random bits", |r| {
        {
            let a = f32::from_bits(r.next() as u32);
            let b = f32::from_bits(r.next() as u32);
            (a, b, f32::from_bits(r.next() as u32))
        }
    });
}

#[test]
fn overlapping_exponents() {
    sweep(0x5eed_f102, "overlap", |r| {
        let ea = (r.next() % 60) as i32 - 30;
        let eb = (r.next() % 60) as i32 - 30;
        let ec = ea + eb + (r.next() % 7) as i32 - 3;
        {
            let a = with_exp(r, ea);
            let b = with_exp(r, eb);
            (a, b, with_exp(r, ec))
        }
    });
}

#[test]
fn cancellation() {
    sweep(0x5eed_f103, "cancel", |r| {
        let (ea, eb) = ((r.next() % 40) as i32 - 20, (r.next() % 40) as i32 - 20);
        let a = with_exp(r, ea);
        let b = with_exp(r, eb);
        let k = (r.next() % 5) as i32 - 2;
        let c = -(a * b);
        (a, b, f32::from_bits((c.to_bits() as i32 + k) as u32))
    });
}

#[test]
fn subnormal_results() {
    sweep(0x5eed_f104, "subnormal", |r| {
        let ea = -((r.next() % 60) as i32);
        let eb = -70 - (r.next() % 80) as i32;
        let ec = -149 + (r.next() % 20) as i32;
        let c = if r.next() % 4 == 0 { 0.0 } else { with_exp(r, ec) };
        {
            let a = with_exp(r, ea);
            (a, with_exp(r, eb.max(-126)), c)
        }
    });
}

#[test]
fn overflow_edge() {
    sweep(0x5eed_f105, "overflow", |r| {
        let ea = 50 + (r.next() % 77) as i32;
        let eb = 127 - ea + (r.next() % 5) as i32 - 3;
        let ec = 120 + (r.next() % 8) as i32;
        {
            let a = with_exp(r, ea.min(127));
            let b = with_exp(r, eb.clamp(-126, 127));
            (a, b, with_exp(r, ec))
        }
    });
}

#[test]
fn unfused_would_fail() {
    // (1 + 2^-12)^2 = 1 + 2^-11 + 2^-24: the unfused f32 product loses the 2^-24.
    let a = 1.0 + 2.0_f32.powi(-12);
    let c = -(1.0 + 2.0_f32.powi(-11));
    assert_ne!((a * a + c).to_bits(), mul_add(a, a, c).to_bits());
    assert_eq!(mul_add(a, a, c), 2.0_f32.powi(-24));
}
