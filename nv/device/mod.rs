//! Device-side SPSC ring endpoints.

use cuda_device::atomic::{AtomicOrdering, DeviceAtomicU32};
use cuda_device::warp;

use crate::nv::error::{RecvError, SendError};
use crate::nv::layout::Layout;
use crate::nv::transport::Message;

pub mod recv;
pub mod send;

pub use recv::Rx;
pub use send::Tx;

/// Wait until `members` participants have arrived at the barrier whose words are at `words`: an
/// arrival count and a generation. The last to arrive resets the count and moves the generation,
/// which releases the rest. Warp-uniform: lane 0 touches the words and shares what it read.
#[inline(always)]
pub unsafe fn barrier(words: *mut u32, members: u32) {
    let lane = warp::lane_id();
    let count = unsafe { DeviceAtomicU32::from_ptr(words) };
    let generation = unsafe { DeviceAtomicU32::from_ptr(words.add(1)) };
    let seen = warp::shuffle(if lane == 0 { generation.load(AtomicOrdering::Acquire) } else { 0 }, 0);
    let arrived = warp::shuffle(if lane == 0 { count.fetch_add(1, AtomicOrdering::AcqRel) } else { 0 }, 0);
    if arrived + 1 == members {
        if lane == 0 {
            count.store(0, AtomicOrdering::Relaxed);
            generation.store(seen.wrapping_add(1), AtomicOrdering::Release);
        }
        warp::sync_mask(WARP);
        return;
    }
    while warp::shuffle(if lane == 0 { generation.load(AtomicOrdering::Acquire) } else { 0 }, 0) == seen {}
}

const WARP: u32 = u32::MAX;

// Payload movement is the one place this transport touches caller memory in bulk, and the kernel
// is latency-bound on memory operations rather than bandwidth-bound: at 1 MiB the profiler puts L2
// throughput at 0.14% and DRAM at 0.16%, while a warp issues an instruction only every ~11 cycles.
// So the cost that matters here is the *number* of memory operations, not the bytes.
//
// Both helpers below therefore move a whole word in one access whenever a whole word is present,
// and assemble from bytes only for the final partial word. Assembling every word from four bytes
// was four loads and four stores per word — a four-fold amplification of exactly the resource the
// kernel is short of.

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
