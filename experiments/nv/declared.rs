//! `#[parallel]` on a real warp.
//!
//! `nv/tests/declare.rs` checks the lowering against the host model, where a warp is 32
//! sequential passes over shared memory. What that model cannot check is the thing the lowering
//! is *for*: that 32 lanes of one warp take their shares of a list at the same time and
//! rendezvous before `invoke!` returns.
//!
//! The discriminating number is one counter per index. A partitioned range increments each index
//! once — its own lane did it and no other lane touches it. A range that was left whole is run by
//! every lane, so each index is incremented 32 times. Nothing else distinguishes the two, and the
//! host checks `counts[i] == 1` for every i rather than that the total is right, because a
//! replication also gets the total wrong in the same direction: it would be 32×.
//!
//! This is the evidence a resident-device backend owes, and it is deliberately the
//! *execution* half rather than the transport half: the transport is what `pingpong` and
//! `nbody-device` already check on hardware, and the lowering had never run on a device at all.

use std::env;
use std::ffi::c_void;

use cuda_core::{CudaContext, DeviceBuffer, LaunchConfig, launch_kernel_on_stream};
use cuda_device::{DisjointSlice, kernel, thread};
use cuda_host::{
    CudaKernel, load_embedded_module, push_kernel_device_slice, writable_device_buffer_arg,
};

/// Items in the list. Wider than a warp on purpose: a list of 32 or fewer is satisfied by
/// assigning one item per lane, which a lowering that did not stride would also do by accident.
const INDICES: u32 = 96;

const LIST: [u32; INDICES as usize] = {
    let mut list = [0; INDICES as usize];
    let mut k = 0;
    while k < INDICES {
        list[k as usize] = k;
        k += 1;
    }
    list
};

/// Host-only code in the same crate as a kernel.
///
/// Never called and never reachable from `declared`. It is here to answer one question the
/// lowering's design turns on: does the device toolchain compile the *whole* crate for the device,
/// or only the bodies reachable from a `#[kernel]`? If the whole crate goes to the device target,
/// this module breaks the build and a worker and its host services cannot share a crate. If only
/// the kernels are lowered, it does not, and a boundary test is enough.
#[allow(dead_code)]
mod host_only {
    /// Sockets, files and OS threads: three things a device has none of.
    pub fn host_services() {
        let _ = std::thread::spawn(|| {});
        let _ = std::net::TcpListener::bind("127.0.0.1:0");
        let _ = std::fs::read("/etc/hostname");
    }
}

mod kernels {
    use super::*;

    /// One participant's state. A participant is one warp here, so this is a local of the kernel
    /// and not a table any other warp can see — the property the entry rules require and a process
    /// global cannot give.
    struct Cell {
        counts: *mut u32,
    }

    impl Cell {
        /// Each lane writes only the counters of its own items, so no atomic is needed. If the
        /// lowering did not partition, every lane would run the whole list and the counters would
        /// not be 1.
        #[trame::parallel]
        fn bump(&self, k: u32, _: &mut ()) -> Result<(), ()> {
            unsafe {
                let at = self.counts.add(k as usize);
                at.write(at.read().wrapping_add(1));
            }
            Ok(())
        }
    }

    #[kernel]
    pub fn declared(mut counts: DisjointSlice<u32>, mut errors: DisjointSlice<u32>) {
        let index = thread::index_1d();
        let cell = Cell {
            counts: counts.as_mut_ptr(),
        };
        let answer = trame::invoke!(cell.bump, &mut (), &LIST);

        if let Some(slot) = errors.get_mut(index) {
            *slot = answer.is_err() as u32;
        }
    }
}

fn main() {
    let ctx = CudaContext::new(0).expect("cuda context");
    let stream = ctx.default_stream();
    let module = load_embedded_module(&ctx, env!("CARGO_PKG_NAME")).expect("load module");
    let fun = module
        .load_function(kernels::__declared_CudaKernel::PTX_NAME)
        .expect("kernel function");

    let mut counts_host = vec![0u32; INDICES as usize];
    let mut errors_host = vec![7u32; 32];
    let mut counts = DeviceBuffer::from_host(&stream, &counts_host).unwrap();
    let mut errors = DeviceBuffer::from_host(&stream, &errors_host).unwrap();

    let mut args: Vec<*mut c_void> = Vec::new();
    let (mut counts_ptr, mut counts_len) = writable_device_buffer_arg(&mut counts);
    push_kernel_device_slice(&mut args, &mut counts_ptr, &mut counts_len);
    let (mut errors_ptr, mut errors_len) = writable_device_buffer_arg(&mut errors);
    push_kernel_device_slice(&mut args, &mut errors_ptr, &mut errors_len);

    let cfg = LaunchConfig {
        grid_dim: (1, 1, 1),
        block_dim: (32, 1, 1),
        shared_mem_bytes: 0,
    };
    unsafe {
        launch_kernel_on_stream(
            &fun,
            cfg.grid_dim,
            cfg.block_dim,
            cfg.shared_mem_bytes,
            &stream,
            &mut args,
        )
    }
    .expect("launch");

    counts_host = counts.to_host_vec(&stream).expect("counts");
    errors_host = errors.to_host_vec(&stream).expect("errors");

    let replicated: Vec<u32> = counts_host.iter().copied().filter(|&c| c != 1).collect();
    let wrong = replicated.len();
    let verdict = wrong == 0 && errors_host.iter().all(|&e| e == 0);
    println!(
        "{{\"schema\":\"nvmpi.declared.v1\",\"indices\":{INDICES},\"lanes\":32,\
         \"each_index_once\":{},\"wrong\":{wrong},\"errors\":{},\"verdict\":\"{}\"}}",
        wrong == 0,
        errors_host[0],
        if verdict { "partitioned" } else { "WRONG" },
    );
    if !verdict {
        println!(
            "counts[..8] = {:?}",
            &counts_host[..8.min(counts_host.len())]
        );
        std::process::exit(1);
    }
}
