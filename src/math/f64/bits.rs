//! Word access for the fdlibm ports: the high and low 32-bit words of an f64 (C's `__HI`, `__LO`).
//! Bit manipulation only; the same bits on every target.

pub(super) const SIGN: u64 = 1 << 63;

pub(super) fn hi(x: f64) -> u32 {
    (x.to_bits() >> 32) as u32
}

pub(super) fn lo(x: f64) -> u32 {
    x.to_bits() as u32
}

pub(super) fn with_hi(x: f64, h: u32) -> f64 {
    f64::from_bits((u64::from(h) << 32) | u64::from(lo(x)))
}

pub(super) fn with_lo(x: f64, l: u32) -> f64 {
    f64::from_bits((x.to_bits() & 0xffff_ffff_0000_0000) | u64::from(l))
}

/// `|mag|` with the sign of `sign`, as C's `copysign`.
pub(super) fn copysign(mag: f64, sign: f64) -> f64 {
    f64::from_bits((mag.to_bits() & !SIGN) | (sign.to_bits() & SIGN))
}
