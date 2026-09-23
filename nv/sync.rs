//! Shared-state primitives for the arms of one participant, with `cpu::sync`'s signatures.
//!
//! Under `cuda` the whole warp calls each method in convergence, so one lane makes every atomic
//! read, claim and bookkeeping write, and the others take its answer.

pub mod atomic;
pub mod handoff;
mod turn;

pub use bytemuck::NoUninit;
pub use turn::{Exclusive, Locked};

#[cfg(feature = "cuda")]
use super::warp;
use core::mem::{MaybeUninit, size_of};
use core::ptr;

/// Whether this lane writes on the warp's behalf: 32 lanes racing a read-modify-write would not
/// agree on its result.
#[inline(always)]
fn lane_zero() -> bool {
    #[cfg(feature = "cuda")]
    return warp::lane() == 0;
    #[cfg(not(feature = "cuda"))]
    return true;
}

/// Lane zero's answer on every lane, ordered before what the lanes read next.
#[inline(always)]
fn one_lane(f: impl FnOnce() -> u32) -> u32 {
    #[cfg(feature = "cuda")]
    {
        let mine = if lane_zero() { f() } else { 0 };
        let all = warp::shuffle(mine, 0);
        warp::sync();
        all
    }
    #[cfg(not(feature = "cuda"))]
    f()
}

/// Every lane's writes, then lane zero's store, so a release publishes the whole warp's work.
#[inline(always)]
fn last_lane(f: impl FnOnce()) {
    #[cfg(feature = "cuda")]
    warp::sync();
    if lane_zero() {
        f();
    }
}

#[repr(C)]
pub(crate) struct Padded<V> {
    value: V,
    tail: [u8; 3],
}

/// Lane zero's `from`, word by word, into `to`.
pub(crate) fn copy_words<V: NoUninit>(
    from: Option<&Padded<V>>,
    to: *mut Padded<V>,
    shuffle: impl Fn(u32) -> u32,
) {
    // SAFETY: bytes [0, size_of V) are V's bytes, all initialized since V: NoUninit; the next 3
    // bytes are the zero tail, which repr(C) places right after value since [u8; 3] has align 1;
    // so every word read is initialized and inside the struct.
    for i in 0..size_of::<V>().div_ceil(4) {
        let w = match from {
            Some(p) => unsafe {
                ptr::read_unaligned((p as *const Padded<V> as *const u8).add(4 * i) as *const u32)
            },
            None => 0,
        };
        unsafe { ptr::write_unaligned((to as *mut u8).add(4 * i) as *mut u32, shuffle(w)) }
    }
}

/// Copy lane zero's value to every lane without allowing padding or an unwritten tail word.
#[inline(always)]
fn from_lane_zero<V: NoUninit>(value: Option<V>) -> V {
    #[cfg(feature = "cuda")]
    {
        let from = value.map(|value| Padded { value, tail: [0; 3] });
        let mut buf = MaybeUninit::<Padded<V>>::uninit();
        copy_words(from.as_ref(), buf.as_mut_ptr(), |w| warp::shuffle(w, 0));
        warp::sync();
        // SAFETY: every word of V's bytes in buf was written from lane zero's valid V.
        unsafe { ptr::read_unaligned(buf.as_ptr() as *const V) }
    }
    #[cfg(not(feature = "cuda"))]
    value.unwrap()
}

#[cfg(test)]
mod tests {
    use super::{Padded, copy_words};
    use core::mem::MaybeUninit;
    use core::ptr;

    #[test]
    fn copy_words_round_trips_values_larger_than_256_bytes_and_u64() {
        let original: [u8; 257] = std::array::from_fn(|i| (i * 7 + 3) as u8);
        let padded = Padded {
            value: original,
            tail: [0; 3],
        };
        let mut buf = MaybeUninit::<Padded<[u8; 257]>>::uninit();
        copy_words(Some(&padded), buf.as_mut_ptr(), |w| w);
        // SAFETY: copy_words wrote every byte of the value.
        let value = unsafe { ptr::read_unaligned(buf.as_ptr() as *const [u8; 257]) };
        assert_eq!(value, padded.value);

        let original = 0x0123_4567_89ab_cdefu64;
        let padded = Padded {
            value: original,
            tail: [0; 3],
        };
        let mut buf = MaybeUninit::<Padded<u64>>::uninit();
        copy_words(Some(&padded), buf.as_mut_ptr(), |w| w);
        // SAFETY: copy_words wrote every byte of the value.
        let value = unsafe { ptr::read_unaligned(buf.as_ptr() as *const u64) };
        assert_eq!(value, original);
    }
}
