//! The transport's refusals, on a real device.
//!
//! `pingpong` checks that a frame crosses the wire. This checks what happens when one *cannot*,
//! because a resident-device backend owes the three cases that are not success paths:
//!
//!   `TooSmall`           a receive buffer smaller than the frame. It must report the length it
//!                        wanted and consume nothing, so that a caller which grows its buffer
//!                        finds the frame still there.
//!   a full lane          a send with no free slot. It must be refused rather than overwrite a
//!                        frame nobody has read.
//!   an unscheduled peer  a peer that never drains. On a device this is the case that matters
//!                        most: a warp spinning on a warp that is not resident makes no progress,
//!                        and the only honest answer is a budget that runs out and says so.
//!
//! Each case is deterministic by construction. The first runs two warps and orders them through
//! the ring itself. The second and third run *one* warp on purpose: the peer is unscheduled
//! because it was never launched, which is the situation a bounded wait is for and the one a
//! second warp could not be made to produce on demand.

use std::env;
use std::ffi::c_void;

use trame::nv::device;
use trame::nv::error::{RecvError, SendError};
use trame::nv::layout::Layout;
use cuda_core::{CudaContext, DeviceBuffer, LaunchConfig, launch_kernel_on_stream};
use cuda_device::{DisjointSlice, kernel, thread, warp};
use cuda_host::{
    CudaKernel, load_embedded_module, push_kernel_device_slice, push_kernel_scalar,
    writable_device_buffer_arg,
};

const DEPTH: u32 = 4;
const BYTES: u32 = 64;
const TAG: u32 = 3;
/// Attempts a bounded loop makes before it reports. The same shape a backend uses: a budget on
/// attempts, never on elapsed time, and exhaustion is a diagnosis rather than a hang.
const BUDGET: u32 = 1_000_000;

mod kernels {
    use super::*;

    #[inline(always)]
    fn pattern(i: u32) -> u8 {
        i.wrapping_mul(37).wrapping_add(11) as u8
    }

    #[kernel]
    #[allow(clippy::too_many_arguments)]
    pub fn cases(
        mut arena: DisjointSlice<u32>,
        mut scratch: DisjointSlice<u32>,
        mut errors: DisjointSlice<u32>,
        mut notes: DisjointSlice<u32>,
        layout: Layout,
        case: u32,
        depth: u32,
    ) {
        let index = thread::index_1d();
        let tid = index.get() as u32;
        let rank = tid / 32;
        let lane = warp::lane_id();
        let arena_ptr = arena.as_mut_ptr();
        let scratch_ptr = scratch.as_mut_ptr();
        let stride = (BYTES as usize).div_ceil(4).max(1);
        let local = unsafe { scratch_ptr.add(rank as usize * stride) as *mut u8 };
        let mut failures = 0u32;
        let mut note = 0u32;

        if case == 0 {
            if rank == 0 {
                let mut tx = unsafe { device::Tx::new(arena_ptr, layout, 0) };
                let mut i = lane;
                while i < BYTES {
                    unsafe { local.add(i as usize).write(pattern(i)) };
                    i += 32;
                }
                warp::sync_mask(u32::MAX);
                let mut retries = 0;
                loop {
                    match unsafe { tx.send(TAG, local, BYTES) } {
                        Ok(()) => break,
                        Err(SendError::Full) if retries < BUDGET => retries += 1,
                        Err(_) => {
                            failures += 1;
                            break;
                        }
                    }
                }
            } else if rank == 1 {
                let mut rx = unsafe { device::Rx::new(arena_ptr, layout) };
                // A buffer one byte short. The first answer that is not `Empty` has to be the
                // refusal, with the length the frame actually needs.
                let short = unsafe { scratch_ptr.add(3 * stride) as *mut u8 };
                let mut retries = 0;
                loop {
                    match unsafe { rx.recv(short, BYTES - 1) } {
                        // It must not have fitted, and it must not have been consumed.
                        Ok(_) => {
                            failures += 1;
                            break;
                        }
                        Err(RecvError::TooSmall { needed }) => {
                            note = needed;
                            break;
                        }
                        Err(RecvError::Empty) if retries < BUDGET => retries += 1,
                        Err(_) => {
                            failures += 1;
                            break;
                        }
                    }
                }
                // The frame the refusal did not consume is still there for a buffer that fits.
                let mut retries = 0;
                let mut got = false;
                while failures == 0 && retries < BUDGET {
                    match unsafe { rx.recv(local, BYTES) } {
                        Ok(message) => {
                            if message.len != BYTES || message.src != 0 || message.tag != TAG {
                                failures += 1;
                            }
                            let mut bad = false;
                            let mut i = lane;
                            while i < BYTES {
                                bad |= unsafe { local.add(i as usize).read() } != pattern(i);
                                i += 32;
                            }
                            if warp::any(bad) {
                                failures += 1;
                            }
                            got = true;
                            break;
                        }
                        Err(RecvError::Empty) => retries += 1,
                        Err(_) => {
                            failures += 1;
                            break;
                        }
                    }
                }
                if !got && failures == 0 {
                    failures += 1;
                }
            }
        } else if case == 1 && rank == 0 {
            let mut tx = unsafe { device::Tx::new(arena_ptr, layout, 0) };
            // Fill the lane exactly. Every one of these has a free slot.
            let mut sent = 0u32;
            while sent < depth && failures == 0 {
                match unsafe { tx.send(TAG, local, 8) } {
                    Ok(()) => sent += 1,
                    Err(_) => failures += 1,
                }
            }
            // The next one has none, and must be refused rather than overwrite a frame nobody
            // has read. A ring that overwrote here would lose the frame silently.
            if failures == 0 {
                match unsafe { tx.send(TAG, local, 8) } {
                    Err(SendError::Full) => note += 1,
                    _ => failures += 1,
                }
            }
            // And a bounded persistent retry loop terminates and reports. This is the unscheduled
            // peer: nothing is draining, so no number of attempts makes room, and the loop has to
            // come back with that answer instead of spinning forever.
            if failures == 0 {
                let mut attempts = 0u32;
                loop {
                    match unsafe { tx.send(TAG, local, 8) } {
                        Ok(()) => {
                            failures += 1;
                            break;
                        }
                        Err(SendError::Full) if attempts < BUDGET => attempts += 1,
                        Err(SendError::Full) => {
                            note += attempts;
                            break;
                        }
                        Err(_) => {
                            failures += 1;
                            break;
                        }
                    }
                }
            }
        }

        // A thread index is not `Copy`, so each output takes its own.
        if let Some(error) = errors.get_mut(thread::index_1d()) {
            *error = failures;
        }
        if let Some(at) = notes.get_mut(thread::index_1d()) {
            *at = note;
        }
    }
}

/// One case, one launch. Returns the per-thread error counters and notes.
fn run(
    stream: &cuda_core::CudaStream,
    fun: &cuda_core::CudaFunction,
    case: u32,
    warps: u32,
) -> (Vec<u32>, Vec<u32>) {
    let layout = Layout::new(DEPTH, BYTES).expect("layout");
    let rings = (warps * warps) as usize;
    let mut arena_host = vec![0u32; rings * layout.words()];
    for ring in 0..rings {
        let range = ring * layout.words()..(ring + 1) * layout.words();
        layout.init(&mut arena_host[range]);
    }
    let stride = (BYTES as usize).div_ceil(4).max(1);
    let scratch_host = vec![0u32; stride * 4];
    let errors_host = vec![0u32; 32 * 2];
    let notes_host = vec![0u32; 32 * 2];

    let mut arena = DeviceBuffer::from_host(stream, &arena_host).unwrap();
    let mut scratch = DeviceBuffer::from_host(stream, &scratch_host).unwrap();
    let mut errors = DeviceBuffer::from_host(stream, &errors_host).unwrap();
    let mut notes = DeviceBuffer::from_host(stream, &notes_host).unwrap();

    let mut args: Vec<*mut c_void> = Vec::new();
    let (mut arena_ptr, mut arena_len) = writable_device_buffer_arg(&mut arena);
    push_kernel_device_slice(&mut args, &mut arena_ptr, &mut arena_len);
    let (mut scratch_ptr, mut scratch_len) = writable_device_buffer_arg(&mut scratch);
    push_kernel_device_slice(&mut args, &mut scratch_ptr, &mut scratch_len);
    let (mut errors_ptr, mut errors_len) = writable_device_buffer_arg(&mut errors);
    push_kernel_device_slice(&mut args, &mut errors_ptr, &mut errors_len);
    let (mut notes_ptr, mut notes_len) = writable_device_buffer_arg(&mut notes);
    push_kernel_device_slice(&mut args, &mut notes_ptr, &mut notes_len);
    let mut layout_arg = layout;
    let mut case_arg = case;
    let mut depth_arg = DEPTH;
    push_kernel_scalar(&mut args, &mut layout_arg);
    push_kernel_scalar(&mut args, &mut case_arg);
    push_kernel_scalar(&mut args, &mut depth_arg);

    let cfg = LaunchConfig {
        grid_dim: (1, 1, 1),
        block_dim: (warps * 32, 1, 1),
        shared_mem_bytes: 0,
    };
    unsafe {
        launch_kernel_on_stream(
            fun,
            cfg.grid_dim,
            cfg.block_dim,
            cfg.shared_mem_bytes,
            stream,
            &mut args,
        )
    }
    .expect("launch");

    (
        errors.to_host_vec(stream).expect("errors"),
        notes.to_host_vec(stream).expect("notes"),
    )
}

fn main() {
    let mut device_id = 1usize;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--device" {
            device_id = args
                .next()
                .expect("--device needs a value")
                .parse()
                .unwrap();
        }
    }
    let ctx = CudaContext::new(device_id).expect("cuda context");
    let stream = ctx.default_stream();
    let module = load_embedded_module(&ctx, env!("CARGO_PKG_NAME")).expect("load module");
    let fun = module
        .load_function(kernels::__cases_CudaKernel::PTX_NAME)
        .expect("kernel function");

    let mut ok = true;
    for (case, warps, what) in [
        (0u32, 2u32, "too_small"),
        (1, 1, "full_lane_and_unscheduled_peer"),
    ] {
        let (errors, notes) = run(&stream, &fun, case, warps);
        let faults: u32 = errors.iter().sum();
        // A note is warp-uniform, so a warp's lane 0 carries it and the index is the *thread*
        // index: rank r's notes start at `32 * r`.
        let case_ok = faults == 0
            && match case {
                // Rank 1 was told the frame needs `BYTES`, and then received it anyway.
                0 => notes[32] == BYTES,
                // Rank 0 saw one refusal and then exhausted a million attempts on a peer that
                // was never launched.
                _ => notes[0] > 1,
            };
        ok &= case_ok;
        println!(
            "{{\"schema\":\"nvmpi.cases.v1\",\"case\":\"{what}\",\"warps\":{warps},\
             \"faults\":{faults},\"needed\":{},\"full_then_attempts\":{},\"verdict\":\"{}\"}}",
            notes[32],
            notes[0],
            if case_ok { "as-declared" } else { "WRONG" },
        );
    }
    if !ok {
        std::process::exit(1);
    }
}
