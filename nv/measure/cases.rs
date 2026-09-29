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
//!   discovery           every warp enters `trame::init` through `Environment::default()`, which
//!                        reads the launch description this launcher wrote before the kernel
//!                        started, and must come back with its own warp as its rank.
//!
//! Each case is deterministic by construction. The first runs two warps and orders them through
//! the ring itself. The second and third run *one* warp on purpose: the peer is unscheduled
//! because it was never launched, which is the situation a bounded wait is for and the one a
//! second warp could not be made to produce on demand.

use std::ffi::c_void;
use std::sync::Arc;

use crate::nv::device;
use crate::nv::error::{RecvError, SendError};
use crate::nv::launch::{Description, MAGIC, NO_LEADER, VERSION};
use crate::nv::layout::Layout;
use crate::nv::peers::Arena;
use crate::{Deployment, Environment, Launch};
use cuda_core::{CudaContext, CudaModule, DeviceBuffer, LaunchConfig, launch_kernel_on_stream};
use cuda_device::{DisjointSlice, kernel, thread};
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

/// The name `trame`'s `#[constant]` description is exported under.
const DESCRIPTION: &str = "cuda_oxide_const_246e25db_TRAME_NV_LAUNCH";

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
        if !crate::nv::warp::is_owner() { return; }
        let index = thread::index_1d();
        let tid = index.get() as u32;
        let rank = tid / 32;
        let arena_ptr = arena.as_mut_ptr();
        let scratch_ptr = scratch.as_mut_ptr();
        let stride = (BYTES as usize).div_ceil(4).max(1);
        let local = unsafe { scratch_ptr.add(rank as usize * stride) as *mut u8 };
        let mut failures = 0u32;
        let mut note = 0u32;

        if case == 0 {
            if rank == 0 {
                let mut tx = unsafe { device::Tx::new(arena_ptr, layout, 0) };
                let mut i = 0;
                while i < BYTES {
                    unsafe { local.add(i as usize).write(pattern(i)) };
                    i += 1;
                }
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
                            let mut i = 0;
                            while i < BYTES {
                                bad |= unsafe { local.add(i as usize).read() } != pattern(i);
                                i += 1;
                            }
                            if bad {
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
        } else if case == 2 {
            // Every warp is a worker of the two-warp launch; the note is its rank plus one, so a
            // zero is a warp that did not enter.
            let workers = [Launch::new(0), Launch::new(1)];
            match Deployment::new(&[&workers[..]], 0, Launch::new(2)) {
                Ok(deployment) => match crate::init(Environment::default(), deployment) {
                    Ok(cx) => note = crate::rank(&cx) + 1,
                    Err(_) => failures += 1,
                },
                Err(_) => failures += 1,
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
    module: &Arc<CudaModule>,
    fun: &cuda_core::CudaFunction,
    case: u32,
    warps: u32,
) -> (Vec<u32>, Vec<u32>) {
    // `init` refuses peer slots that cannot hold `MAX_FRAME`, so the discovery case provisions them.
    let layout = if case == 2 {
        Layout::new(2, crate::MAX_FRAME as u32)
    } else {
        Layout::new(DEPTH, BYTES)
    }
    .expect("layout");
    let rings = (warps * warps) as usize;
    let mut arena_host = vec![0u32; rings * layout.words() + crate::nv::peers::BARRIER];
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
    if case == 2 {
        describe(stream, module, &arena, layout, warps);
    }

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

/// Write the launch description `trame` discovers: this launch's warps, its peer arena and no
/// leader.
fn describe(
    stream: &cuda_core::CudaStream,
    module: &Arc<CudaModule>,
    arena: &DeviceBuffer<u32>,
    layout: Layout,
    warps: u32,
) {
    let description = Description {
        magic: MAGIC,
        version: VERSION,
        bytes: size_of::<Description>() as u32,
        size: warps,
        leader: NO_LEADER,
        depth: layout.depth(),
        capacity: layout.capacity(),
        arena: Arena {
            base: arena.cu_deviceptr() as *mut u32,
            words: arena.len(),
        },
        leader_region: core::ptr::null_mut(),
        leader_words: 0,
    };
    let (symbol, bytes) = module.get_global(DESCRIPTION).expect("trame's launch description");
    assert_eq!(
        bytes,
        size_of::<Description>(),
        "the description symbol is not this build's description"
    );
    // The arena upload above was queued on `stream`; the write below is synchronous and does not
    // order against it, so the upload is finished first.
    stream.synchronize().expect("arena uploaded");
    // SAFETY: `symbol` is the device global `get_global` resolved in a module this process holds,
    // and it is exactly `size_of::<Description>()` bytes (asserted above), which is what is copied
    // from a live local. What this writer guarantees, and the only thing `init` takes on trust:
    // `arena` is `arena.len()` words of device global memory this process allocated, holding
    // `warps * warps` rings of `layout`, each initialised by `Layout::init` at
    // `(src * warps + dst) * layout.words()`; it outlives the kernel, because `run` drops it only
    // after reading the results back, which waits for the kernel; nothing but the launched warps
    // touches it while they run; and there is no leader region, which is what the
    // null pointers and zero lengths say. No kernel reads the symbol while it is written, because
    // the copy returns before the launch is issued.
    unsafe {
        module.copy_bytes_to_device_global_sync(
            symbol,
            (&raw const description).cast(),
            size_of::<Description>(),
        )
    }
    .expect("description written");
}

pub(super) fn main() {
    let mut device_id = 1usize;
    let mut args = super::arguments();
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
        (2, 2, "discovery"),
    ] {
        let (errors, notes) = run(&stream, &module, &fun, case, warps);
        let faults: u32 = errors.iter().sum();
        // A note is warp-uniform, so a warp's lane 0 carries it and the index is the *thread*
        // index: rank r's notes start at `32 * r`.
        let case_ok = faults == 0
            && match case {
                // Rank 1 was told the frame needs `BYTES`, and then received it anyway.
                0 => notes[32] == BYTES,
                // Rank 0 saw one refusal and then exhausted a million attempts on a peer that
                // was never launched.
                1 => notes[0] > 1,
                // Each warp entered, and as itself.
                _ => notes[0] == 1 && notes[32] == 2,
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
