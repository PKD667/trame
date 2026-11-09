//! Bit-level pin: an FNV-1a hash of the output bits over a fixed grid, compared to a constant. A
//! change of any output bit of any function changes the hash, so the test fails loudly.

extern crate std;
use std::eprintln;

use super::super::{exp, exp_m1, ln, powf, powi, round, sqrt};

fn fnv(acc: &mut u64, x: f64) {
    for byte in x.to_bits().to_le_bytes() {
        *acc ^= u64::from(byte);
        *acc = acc.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn grid() -> impl Iterator<Item = f64> {
    (0..4096_i32).map(|i| f64::from(i) * 0.732_421_875 - 1500.0)
}

#[test]
fn outputs_have_pinned_bits() {
    let mut h = [0xcbf2_9ce4_8422_2325_u64; 7];
    for x in grid() {
        fnv(&mut h[0], exp(x * 0.5));
        fnv(&mut h[1], exp_m1(x * 0.01));
        fnv(&mut h[2], ln(x.abs() + 1e-3));
        fnv(&mut h[3], powf(x.abs() * 0.001 + 1e-3, x * 0.003));
        fnv(&mut h[4], sqrt(x.abs()));
        fnv(&mut h[5], round(x * 0.37));
        fnv(&mut h[6], powi(x * 0.01, 7));
    }
    eprintln!("hashes: {h:016x?}");
    assert_eq!(h, PINNED, "output bits changed; if intended, update PINNED");
}

/// Order: exp, expm1, ln, pow, sqrt, round, powi. Pinned on x86_64 with rustc 1.93.1 and unchanged
/// with 1.98.1. The spec says every IEEE 754 target gives these bits; no other target has run this.
const PINNED: [u64; 7] = [
    0xcab3_5232_0de9_a4d4,
    0x2478_64db_ad7b_2131,
    0xf9b1_e01b_76ff_5a80,
    0x53e7_1131_53fe_13be,
    0xbf60_8a0b_c243_bee2,
    0x81c6_f1e5_467e_c284,
    0xef36_776f_4984_4a00,
];
