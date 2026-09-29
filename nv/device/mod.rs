//! Device-side SPSC ring endpoints.

use cuda_device::atomic::{AtomicOrdering, DeviceAtomicU32};

use crate::nv::error::{RecvError, SendError};
use crate::nv::layout::Layout;
use crate::nv::transport::Message;

pub mod recv;
pub mod send;

pub use recv::Rx;
pub use send::Tx;

/// One arrival per scalar owner, over two aligned global-memory words.
/// The entire cohort must be resident; no lane rendezvous occurs here.
#[inline(always)]
pub unsafe fn barrier(words: *mut u32, members: u32) {
    let count = unsafe { DeviceAtomicU32::from_ptr(words) };
    let generation = unsafe { DeviceAtomicU32::from_ptr(words.add(1)) };
    let seen = generation.load(AtomicOrdering::Acquire);
    let arrived = count.fetch_add(1, AtomicOrdering::AcqRel);
    if arrived + 1 == members {
        count.store(0, AtomicOrdering::Relaxed);
        generation.store(seen.wrapping_add(1), AtomicOrdering::Release);
        return;
    }
    while generation.load(AtomicOrdering::Acquire) == seen {}
}

// The owner copies whole unaligned words and assembles only the final partial word.

#[inline(always)]
unsafe fn read_word(data: *const u8, len: u32, word: u32) -> u32 {
    let base = word * 4;
    if base + 4 <= len {
        // A whole word is present. `word * 4` preserves whatever alignment `data` had, so this is
        // an aligned access for the aligned buffers the transport is handed, and stays correct if
        // it is not.
        unsafe { (data.add(base as usize) as *const u32).read_unaligned() }
    } else {
        // The tail: 1 to 3 bytes, which is the only case that needs assembly.
        let mut value = 0;
        let mut byte = 0;
        while byte < 4 && base + byte < len {
            value |= (unsafe { data.add((base + byte) as usize).read() } as u32) << (byte * 8);
            byte += 1;
        }
        value
    }
}

#[inline(always)]
unsafe fn write_word(out: *mut u8, len: u32, word: u32, value: u32) {
    let base = word * 4;
    if base + 4 <= len {
        // As `read_word`: one access for a whole word, assembly only for the tail.
        unsafe { (out.add(base as usize) as *mut u32).write_unaligned(value) };
    } else {
        let mut byte = 0;
        while byte < 4 && base + byte < len {
            unsafe {
                out.add((base + byte) as usize)
                    .write((value >> (byte * 8)) as u8)
            };
            byte += 1;
        }
    }
}
