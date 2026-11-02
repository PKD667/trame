//! One-warp device prototype of programs/post_parallel_pod.rs (backends/nv/PLAN.md, gate D).
//!
//! One warp (32 lanes) is one logical worker. The `parallel!` region is lane-parallel over keys:
//! lane `l` folds keys `l + 32q`, and each key's items are folded in input order. The slots are
//! device global memory. After a warp barrier, lane 0 reads every slot and sends one report per
//! slot. stdout must equal programs/post_parallel_pod.expected. The slot state goes to stderr.
//!
//! Build and run: see README.md. Nothing here is a timing.

use cuda_core::simt::LaunchConfig;
use cuda_core::{CudaContext, DeviceBuffer, DeviceCopy};
use cuda_device::{DisjointSlice, cuda_module, kernel, warp};

/// One input: the slot it folds into, and its value. Same fields as the seq program.
#[repr(C)]
#[derive(Clone, Copy, DeviceCopy)]
pub struct Input {
    pub key: u32,
    pub value: u64,
}

/// A slot: an accumulator and two counters. Plain data, as in the seq program.
#[repr(C)]
#[derive(Clone, Copy, DeviceCopy, Default, Debug)]
pub struct Slot {
    pub acc: u64,
    pub seen: [u32; 2],
}

/// What lane 0 sends to the host, one per slot.
#[repr(C)]
#[derive(Clone, Copy, DeviceCopy, Default, Debug)]
pub struct Report {
    pub acc: u64,
    pub slot: u32,
    pub branch: u32,
}

#[cuda_module]
mod kernels {
    use super::*;

    /// Number of keys (slots). Lanes 0..NKEYS do work; the rest of the warp has no key.
    const NKEYS: u32 = 4;

    /// One warp, 32 lanes. `slots` must hold NKEYS slots; `reports` NKEYS reports.
    #[kernel]
    pub fn post_parallel(inputs: &[Input], salt: u64, mut slots: DisjointSlice<Slot>, mut reports: DisjointSlice<Report>) {
        let lane = warp::lane_id();
        let sp = slots.as_mut_ptr();

        // parallel!: lane l handles keys l + 32q. The items of one key are visited in input order,
        // so the order-dependent fold sees the same sequence as the seq program.
        let mut k = lane;
        while k < NKEYS {
            let slot = unsafe { &mut *sp.add(k as usize) };
            for i in 0..inputs.len() {
                let x = &inputs[i];
                if x.key == k {
                    slot.acc = (slot.acc * 3 + x.value + salt) % 1_000_003;
                    slot.seen[(x.value % 2) as usize] += 1;
                }
            }
            k += 32;
        }

        // Every lane of the warp has finished its keys before lane 0 reads any slot.
        warp::sync_mask(u32::MAX);

        // The rest of the worker: lane 0 alone reads the slots as the items left them and sends.
        if lane == 0 {
            let rp = reports.as_mut_ptr();
            for k in 0..NKEYS {
                let s = unsafe { &*sp.add(k as usize) };
                let branch = if s.seen[0] > s.seen[1] { 1 } else { 2 };
                unsafe { *rp.add(k as usize) = Report { acc: s.acc, slot: k, branch } };
            }
        }
    }
}

fn main() {
    let ctx = CudaContext::new(0).expect("create CUDA context on device 0");
    let stream = ctx.default_stream();
    let module = kernels::load(&ctx).expect("load the embedded module");

    // The inputs and the salt of programs/post_parallel_pod.rs, in the same order.
    let inputs: Vec<Input> = (0..8).map(|i| Input { key: (i % 4) as u32, value: 3 + 7 * i as u64 }).collect();
    let salt: u64 = 5;
    let slots: Vec<Slot> = vec![Slot::default(); 4];

    let in_d = DeviceBuffer::from_host(&stream, &inputs).expect("copy inputs");
    let mut slots_d = DeviceBuffer::from_host(&stream, &slots).expect("copy slots");
    let mut reports_d = DeviceBuffer::<Report>::zeroed(&stream, 4).expect("allocate reports");

    // One block of one warp: one logical worker.
    let cfg = LaunchConfig { grid_dim: (1, 1, 1), block_dim: (32, 1, 1), shared_mem_bytes: 0 };
    // SAFETY: one block of 32 threads; the buffers hold the 4 slots and 4 reports the kernel writes.
    unsafe { module.post_parallel(&stream, cfg, &in_d, salt, &mut slots_d, &mut reports_d) }
        .expect("post_parallel launch");

    let reports = reports_d.to_host_vec(&stream).expect("copy reports back");
    let slots_after = slots_d.to_host_vec(&stream).expect("copy slots back");
    eprintln!("slots after the warp: {slots_after:?}");

    for r in &reports {
        println!("slot {} acc {} branch {}", r.slot, r.acc, r.branch);
    }
    // The seq program prints this after `join`. Here it is printed only after the copy-back succeeded.
    println!("outcomes: [Returned]");
}
