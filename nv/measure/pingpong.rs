//! Two warps, one round trip at a time, over a pair of rings: a frame crosses the wire intact, and
//! under `--bench` how long a round trip takes. Run as `nv::measure` says.

use std::ffi::c_void;

use crate::nv::device;
use crate::nv::error::{RecvError, SendError};
use crate::nv::layout::Layout;
use cuda_core::{CudaContext, DeviceBuffer, LaunchConfig, launch_kernel_on_stream};
use cuda_device::{DisjointSlice, debug, kernel, thread, warp};
use cuda_host::{
    CudaKernel, load_embedded_module, push_kernel_device_slice, push_kernel_scalar,
    writable_device_buffer_arg,
};

const DEPTH: u32 = 4;
const TAG: u32 = 7;
const RETRIES: u32 = 10_000_000;

mod kernels {
    use super::*;

    #[inline(always)]
    fn expected(round: u32, i: u32) -> u8 {
        round
            .wrapping_mul(29)
            .wrapping_add(i.wrapping_mul(131))
            .wrapping_add(17) as u8
    }

    #[kernel]
    pub fn pingpong(
        mut arena: DisjointSlice<u32>,
        mut scratch: DisjointSlice<u32>,
        mut errors: DisjointSlice<u32>,
        mut elapsed: DisjointSlice<u64>,
        layout: Layout,
        bytes: u32,
        warmup: u32,
        rounds: u32,
        check: u32,
    ) {
        let index = thread::index_1d();
        let tid = index.get() as u32;
        let rank = tid / 32;
        let lane = warp::lane_id();
        let arena_ptr = arena.as_mut_ptr();
        let scratch_ptr = scratch.as_mut_ptr();
        let elapsed_ptr = elapsed.as_mut_ptr();
        let stride = bytes.div_ceil(4).max(1) as usize;
        let local = unsafe { scratch_ptr.add(rank as usize * stride) as *mut u8 };
        let reverse = unsafe { arena_ptr.add(layout.words()) };
        let mut failures = 0;
        let mut start = 0;

        if rank == 0 {
            let mut tx = unsafe { device::Tx::new(arena_ptr, layout, 0) };
            let mut rx = unsafe { device::Rx::new(reverse, layout) };
            let mut round = 0;
            while round < warmup + rounds && failures == 0 {
                if round == warmup {
                    let now = debug::globaltimer();
                    if lane == 0 {
                        start = now;
                    }
                }
                if check != 0 {
                    let mut i = lane;
                    while i < bytes {
                        unsafe { local.add(i as usize).write(expected(round, i)) };
                        i += 32;
                    }
                    warp::sync_mask(u32::MAX);
                }
                let mut retries = 0;
                loop {
                    match unsafe { tx.send(TAG, local, bytes) } {
                        Ok(()) => break,
                        Err(SendError::Full) if retries < RETRIES => retries += 1,
                        Err(_) => {
                            failures += 1;
                            break;
                        }
                    }
                }
                if failures != 0 {
                    break;
                }
                if check != 0 {
                    let mut i = lane;
                    while i < bytes {
                        unsafe { local.add(i as usize).write(!expected(round, i)) };
                        i += 32;
                    }
                    warp::sync_mask(u32::MAX);
                }
                retries = 0;
                loop {
                    match unsafe { rx.recv(local, bytes) } {
                        Ok(message) => {
                            if check != 0
                                && (message.src != 1 || message.tag != TAG || message.len != bytes)
                            {
                                failures += 1;
                            }
                            if check != 0 {
                                let mut bad = false;
                                let mut i = lane;
                                while i < bytes {
                                    bad |= unsafe { local.add(i as usize).read() }
                                        != expected(round, i);
                                    i += 32;
                                }
                                if warp::any(bad) {
                                    failures += 1;
                                }
                            }
                            break;
                        }
                        Err(RecvError::Empty) if retries < RETRIES => retries += 1,
                        Err(_) => {
                            failures += 1;
                            break;
                        }
                    }
                }
                round += 1;
            }
            if lane == 0 {
                unsafe { elapsed_ptr.write(debug::globaltimer().wrapping_sub(start)) };
            }
        } else if rank == 1 {
            let mut rx = unsafe { device::Rx::new(arena_ptr, layout) };
            let mut tx = unsafe { device::Tx::new(reverse, layout, 1) };
            let mut round = 0;
            while round < warmup + rounds && failures == 0 {
                let mut retries = 0;
                loop {
                    match unsafe { rx.recv(local, bytes) } {
                        Ok(message) => {
                            if check != 0
                                && (message.src != 0 || message.tag != TAG || message.len != bytes)
                            {
                                failures += 1;
                            }
                            if check != 0 {
                                let mut bad = false;
                                let mut i = lane;
                                while i < bytes {
                                    bad |= unsafe { local.add(i as usize).read() }
                                        != expected(round, i);
                                    i += 32;
                                }
                                if warp::any(bad) {
                                    failures += 1;
                                }
                            }
                            break;
                        }
                        Err(RecvError::Empty) if retries < RETRIES => retries += 1,
                        Err(_) => {
                            failures += 1;
                            break;
                        }
                    }
                }
                retries = 0;
                loop {
                    match unsafe { tx.send(TAG, local, bytes) } {
                        Ok(()) => break,
                        Err(SendError::Full) if retries < RETRIES => retries += 1,
                        Err(_) => {
                            failures += 1;
                            break;
                        }
                    }
                }
                round += 1;
            }
        }

        if let Some(out) = errors.get_mut(index) {
            *out = failures;
        }
    }
}

struct Config {
    bench: bool,
    sizes: Vec<u32>,
    samples: u32,
    rounds: Option<u32>,
    device: usize,
}

pub(super) fn main() {
    let config = parse_args();
    let ctx = CudaContext::new(config.device).expect("cuda context");
    let stream = ctx.default_stream();
    let module = load_embedded_module(&ctx, env!("CARGO_PKG_NAME")).expect("load module");
    let fun = module
        .load_function(kernels::__pingpong_CudaKernel::PTX_NAME)
        .expect("kernel function");

    for bytes in config.sizes {
        let rounds = config.rounds.unwrap_or_else(|| rounds_for(bytes));
        let warmup = if config.bench {
            (rounds / 10).clamp(20, 1_000)
        } else {
            0
        };
        for sample in 0..config.samples {
            let elapsed = run(&stream, &fun, bytes, warmup, rounds, !config.bench);
            if config.bench {
                println!(
                    "{{\"schema\":\"nvmpi.pingpong.v1\",\"type\":\"sample\",\"transport\":\"warp\",\"size_bytes\":{bytes},\"sample\":{sample},\"warmup_roundtrips\":{warmup},\"roundtrips\":{rounds},\"elapsed_ns\":{elapsed}}}"
                );
            }
        }
    }

    if !config.bench {
        eprintln!("transport check passed");
    }
}

fn run(
    stream: &cuda_core::CudaStream,
    fun: &cuda_core::CudaFunction,
    bytes: u32,
    warmup: u32,
    rounds: u32,
    check: bool,
) -> u64 {
    let layout = Layout::new(DEPTH, bytes).unwrap();
    warmup.checked_add(rounds).expect("round count overflow");
    let mut arena_host = vec![0; layout.words() * 2];
    layout.init(&mut arena_host[..layout.words()]);
    layout.init(&mut arena_host[layout.words()..]);
    let mut arena = DeviceBuffer::from_host(stream, &arena_host).unwrap();

    let stride = bytes.div_ceil(4).max(1) as usize;
    let mut scratch_host = vec![0xa5a5_a5a5; stride * 2];
    for i in 0..bytes as usize {
        set_byte(&mut scratch_host[..stride], i, pattern(0, i));
    }
    let expected_round = if check { rounds - 1 } else { 0 };
    let mut expected = vec![0xa5a5_a5a5; stride * 2];
    for rank in 0..2 {
        for i in 0..bytes as usize {
            set_byte(
                &mut expected[rank * stride..][..stride],
                i,
                pattern(expected_round, i),
            );
        }
    }
    let mut scratch = DeviceBuffer::from_host(stream, &scratch_host).unwrap();
    let mut errors = DeviceBuffer::<u32>::zeroed(stream, 64).unwrap();
    let mut elapsed = DeviceBuffer::<u64>::zeroed(stream, 1).unwrap();

    let mut args: Vec<*mut c_void> = Vec::new();
    let (mut arena_ptr, mut arena_len) = writable_device_buffer_arg(&mut arena);
    push_kernel_device_slice(&mut args, &mut arena_ptr, &mut arena_len);
    let (mut scratch_ptr, mut scratch_len) = writable_device_buffer_arg(&mut scratch);
    push_kernel_device_slice(&mut args, &mut scratch_ptr, &mut scratch_len);
    let (mut error_ptr, mut error_len) = writable_device_buffer_arg(&mut errors);
    push_kernel_device_slice(&mut args, &mut error_ptr, &mut error_len);
    let (mut elapsed_ptr, mut elapsed_len) = writable_device_buffer_arg(&mut elapsed);
    push_kernel_device_slice(&mut args, &mut elapsed_ptr, &mut elapsed_len);
    let mut layout_arg = layout;
    let mut bytes_arg = bytes;
    let mut warmup_arg = warmup;
    let mut rounds_arg = rounds;
    let mut check_arg = u32::from(check);
    push_kernel_scalar(&mut args, &mut layout_arg);
    push_kernel_scalar(&mut args, &mut bytes_arg);
    push_kernel_scalar(&mut args, &mut warmup_arg);
    push_kernel_scalar(&mut args, &mut rounds_arg);
    push_kernel_scalar(&mut args, &mut check_arg);

    let cfg = LaunchConfig {
        grid_dim: (1, 1, 1),
        block_dim: (64, 1, 1),
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

    let errors = errors.to_host_vec(stream).expect("error counters");
    assert!(
        errors.iter().all(|error| *error == 0),
        "device timeout or protocol error: {errors:?}"
    );
    scratch_host = scratch.to_host_vec(stream).expect("scratch");
    assert_eq!(scratch_host, expected, "payload or guard corruption");
    elapsed.to_host_vec(stream).expect("elapsed time")[0]
}

fn parse_args() -> Config {
    let mut bench = false;
    let mut sizes = None;
    let mut samples = 5;
    let mut rounds = None;
    // Which device to measure on. Defaulted rather than fixed, because a shared host runs other
    // work on device 0 and a timing taken against a co-tenant is not a timing of this transport.
    let mut device = 0;
    let mut args = super::arguments();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bench" => bench = true,
            "--device" => {
                device = args
                    .next()
                    .expect("--device needs a value")
                    .parse()
                    .expect("--device takes a number")
            }
            "--sizes" => sizes = Some(parse_sizes(&args.next().expect("--sizes needs a value"))),
            "--samples" => {
                samples = args
                    .next()
                    .expect("--samples needs a value")
                    .parse()
                    .unwrap()
            }
            "--roundtrips" => {
                rounds = Some(
                    args.next()
                        .expect("--roundtrips needs a value")
                        .parse()
                        .unwrap(),
                )
            }
            _ => panic!("unknown argument: {arg}"),
        }
    }
    let sizes = sizes.unwrap_or_else(|| {
        if bench {
            vec![0, 1, 4, 16, 64, 256, 1_024, 4_096, 65_536, 1_048_576]
        } else {
            vec![0, 1, 3, 31, 32, 33, 127, 128, 129, 4_096]
        }
    });
    assert!(samples > 0, "--samples must be positive");
    if let Some(rounds) = rounds {
        assert!(rounds > 0, "--roundtrips must be positive");
    }
    Config {
        bench,
        sizes,
        samples,
        rounds: rounds.or((!bench).then_some(128)),
        device,
    }
}

fn parse_sizes(value: &str) -> Vec<u32> {
    value.split(',').map(|size| size.parse().unwrap()).collect()
}

fn rounds_for(bytes: u32) -> u32 {
    (64 * 1024 * 1024 / bytes.max(1)).clamp(100, 20_000)
}

fn pattern(round: u32, i: usize) -> u8 {
    round
        .wrapping_mul(29)
        .wrapping_add((i as u32).wrapping_mul(131))
        .wrapping_add(17) as u8
}

fn set_byte(words: &mut [u32], i: usize, value: u8) {
    let shift = (i % 4) * 8;
    words[i / 4] = (words[i / 4] & !(0xff << shift)) | ((value as u32) << shift);
}
