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
