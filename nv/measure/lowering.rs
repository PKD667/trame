//! Production receive lowering on the device; this is not a raw ring probe.

use std::ffi::c_void;

use crate::nv::launch::{Description, MAGIC, VERSION};
use crate::nv::layout::Layout;
use crate::nv::leader::Route;
use crate::nv::peers::Arena;
use crate::nv;
use crate::{
    Addr, BackendFault, Channel, Deployment, Error, FailureKind, Invalid, Launch, Receive, Tag,
};
use cuda_core::{CudaContext, DeviceBuffer, LaunchConfig, launch_kernel_on_stream};
use cuda_device::{DisjointSlice, kernel, thread};
use cuda_host::{
    CudaKernel, load_embedded_module, push_kernel_device_slice, push_kernel_scalar,
    writable_device_buffer_arg,
};

const DEPTH: u32 = 4;
const DESCRIPTION: &str = "cuda_oxide_const_246e25db_TRAME_NV_LAUNCH";

mod kernels {
    use super::*;

    #[kernel]
    pub fn lowering(mut scratch: DisjointSlice<u32>, mut errors: DisjointSlice<u32>) {
        if !nv::warp::is_owner() { return; }
        let index = thread::index_1d();
        let rank = nv::warp::here_id();
        let host0 = [Launch::new(0), Launch::new(1)];
        let host1 = [Launch::new(3)];
        let hosts = [&host0[..], &host1[..]];
        let deployment = match Deployment::new(&hosts, 0, Launch::new(2)) {
            Ok(deployment) => deployment,
            Err(_) => {
                if let Some(out) = errors.get_mut(index) {
                    *out = 2;
                }
                return;
            }
        };
        let mut failures = 0u32;
        let mut cx = match nv::init(nv::Environment::default(), deployment) {
            Ok(cx) => cx,
            Err(_) => {
                if let Some(out) = errors.get_mut(index) {
                    *out = 2;
                }
                return;
            }
        };
        if nv::rank(&cx) != rank || nv::size(&cx) != 2 {
            failures += 1;
        }

        let ptr = scratch.as_mut_ptr();
        let base = unsafe { ptr.add((rank as usize) * 16) as *mut u8 };
        let data = unsafe { core::slice::from_raw_parts_mut(base, 16) };
        let mut i = 0;
        while i < 16 {
            unsafe { base.add(i as usize).write((rank * 19 + i) as u8) };
            i += 1;
        }

        let to = Addr::Local(1 - rank);
        for tag in [7u16, 8, 9] {
            if nv::send(&mut cx, to, Channel::Message(Tag::new(tag)), &data[..4]).is_err() {
                failures += 1;
            }
        }

        nv::barrier(&mut cx);

        let out = unsafe { core::slice::from_raw_parts_mut(base.add(8), 8) };
        match nv::recv(&mut cx, &mut out[..2]) {
            Err(Error::TooSmall { needed: 4 }) => (),
            _ => failures += 1,
        }
        match nv::recv(&mut cx, out) {
            Ok(Some(frame)) => {
                if frame.source() != Some(to) || frame.tag() != Tag::new(7) || frame.len() != 4 {
                    failures += 1;
                }
                let mut byte = 0;
                while byte < 4 {
                    if out[byte] != ((1 - rank) * 19 + byte as u32) as u8 {
                        failures += 1;
                    }
                    byte += 1;
                }
            }
            _ => failures += 1,
        }

        let mut leader_first = false;
        let nothing_arms = [Receive::Nothing];
        let mut nothing = nv::Io::new(
            &mut cx,
            crate::invoke::Owner::new(&nothing_arms, 0),
            &mut leader_first,
        );
        if !matches!(
            nothing.recv(out),
            Err(Error::Invalid(Invalid::NotReceiving))
        ) {
            failures += 1;
        }
        drop(nothing);

        let empty: [Tag; 0] = [];
        let empty_arms = [Receive::Only(&empty)];
        let mut empty_io = nv::Io::new(
            &mut cx,
            crate::invoke::Owner::new(&empty_arms, 0),
            &mut leader_first,
        );
        match empty_io.recv(out) {
            Ok(None) => (),
            _ => failures += 1,
        }
        drop(empty_io);

        let only_nine = [Tag::new(9)];
        let nine_arms = [Receive::Only(&only_nine)];
        let mut scoped = nv::Io::new(
            &mut cx,
            crate::invoke::Owner::new(&nine_arms, 0),
            &mut leader_first,
        );
        match scoped.recv(out) {
            Ok(None) => (), // tag 8 is still at the head; tag 9 must not be scanned past it.
            _ => failures += 1,
        }
        drop(scoped);

        let only_eight = [Tag::new(8)];
        let eight_arms = [Receive::Only(&only_eight)];
        let mut scoped = nv::Io::new(
            &mut cx,
            crate::invoke::Owner::new(&eight_arms, 0),
            &mut leader_first,
        );
        match scoped.recv(out) {
            Ok(Some(frame))
                if frame.source() == Some(to) && frame.tag() == Tag::new(8) && frame.len() == 4 =>
            {
                ()
            }
            _ => failures += 1,
        }
        drop(scoped);

        let overlap = [Tag::new(9)];
        let overlap_arms = [Receive::Only(&overlap), Receive::All];
        let mut later = nv::Io::new(
            &mut cx,
            crate::invoke::Owner::new(&overlap_arms, 1),
            &mut leader_first,
        );
        if !matches!(later.recv(out), Ok(None)) {
            failures += 1;
        }
        drop(later);
        let mut first = nv::Io::new(
            &mut cx,
            crate::invoke::Owner::new(&overlap_arms, 0),
            &mut leader_first,
        );
        match first.recv(out) {
            Ok(Some(frame))
                if frame.source() == Some(to) && frame.tag() == Tag::new(9) && frame.len() == 4 =>
            {
                ()
            }
            _ => failures += 1,
        }
        drop(first);

        // A valid remote destination reaches the production named refusal, including its
        // pointer-bearing Error::Failed path; there is no host-side transport substitute here.
        match nv::send(
            &mut cx,
            Addr::Remote { host: 1, rank: 0 },
            Channel::Message(Tag::new(11)),
            &data[..4],
        ) {
            Err(Error::Failed(failure)) => {
                let bytes = failure.operation.as_bytes();
                if bytes.len() != 4
                    || bytes[0] != b's'
                    || bytes[1] != b'e'
                    || bytes[2] != b'n'
                    || bytes[3] != b'd'
                    || failure.kind != FailureKind::Backend(BackendFault::Unimplemented)
                {
                    failures += 1;
                }
            }
            _ => failures += 1,
        }
        if !matches!(
            nv::send(
                &mut cx,
                Addr::Local(2),
                Channel::Message(Tag::new(11)),
                &data[..4]
            ),
            Err(Error::Invalid(Invalid::RankOutsideJob))
        ) {
            failures += 1;
        }

        // Exercise the Io peer, leader and flush methods; a host route consumer is a separate W6 gate.
        let mut io_ctx = nv::Io::new(&mut cx, crate::invoke::Owner::ALL, &mut leader_first);
        if io_ctx.send(to, Channel::Message(Tag::new(10)), &data[..4]).is_err() {
            failures += 1;
        }
        if io_ctx.lead(Tag::new(12), &data[..4]).is_err() {
            failures += 1;
        }
        if io_ctx.flush().is_err() {
            failures += 1;
        }
        drop(io_ctx);

        nv::barrier(&mut cx);

        // An all-tag catchall is the same production Io receive path after earlier Nothing arms.
        let arms = [Receive::Nothing, Receive::All];
        let mut all = nv::Io::new(
            &mut cx,
            crate::invoke::Owner::new(&arms, 1),
            &mut leader_first,
        );
        match all.recv(out) {
            Ok(Some(frame))
                if frame.source() == Some(to)
                    && frame.tag() == Tag::new(10)
                    && frame.len() == 4 =>
            {
                ()
            }
            _ => failures += 1,
        }
        drop(all);
        match nv::recv(&mut cx, out) {
            Ok(None) => (),
            _ => failures += 1,
        }
        match nv::leader::recv(&mut cx, out) {
            Ok(None) => (), // the host leader is deliberately not faked; W6 supplies it.
            _ => failures += 1,
        }

        if let Some(out) = errors.get_mut(index) {
            *out = failures + 1;
        }
    }
}

pub(super) fn main() {
    let ctx = CudaContext::new(0).expect("cuda context");
    let stream = ctx.default_stream();
    let module = load_embedded_module(&ctx, env!("CARGO_PKG_NAME")).expect("load module");
    let function = module
        .load_function(kernels::__lowering_CudaKernel::PTX_NAME)
        .expect("kernel");
    let layout = Layout::new(DEPTH, nv::MAX_FRAME as u32).expect("layout");
    let mut host = vec![0u32; 4 * layout.words() + nv::peers::BARRIER];
    for ring in 0..4 {
        layout.init(&mut host[ring * layout.words()..(ring + 1) * layout.words()]);
    }
    let arena = DeviceBuffer::from_host(&stream, &host).expect("arena");
    let route = Route::sized(2).expect("leader geometry");
    let mut leader_host = vec![0u32; route.words()];
    route.init(&mut leader_host);
    let _leader = DeviceBuffer::from_host(&stream, &leader_host).expect("leader route");
    let mut scratch = DeviceBuffer::<u32>::zeroed(&stream, 64).expect("scratch");
    let mut errors = DeviceBuffer::<u32>::zeroed(&stream, 64).expect("errors");
    let description = Description {
        magic: MAGIC,
        version: VERSION,
        bytes: size_of::<Description>() as u32,
        size: 2,
        leader: 2,
        depth: layout.depth(),
        capacity: layout.capacity(),
        arena: Arena {
            base: arena.cu_deviceptr() as *mut u32,
            words: arena.len(),
        },
        leader_region: _leader.cu_deviceptr() as *mut u32,
        leader_words: _leader.len(),
    };
    let (symbol, bytes) = module.get_global(DESCRIPTION).expect("description");
    assert_eq!(bytes, size_of::<Description>());
    stream.synchronize().expect("uploads");
    unsafe {
        module.copy_bytes_to_device_global_sync(
            symbol,
            (&raw const description).cast(),
            size_of::<Description>(),
        )
    }
    .expect("description write");

    let mut args: Vec<*mut c_void> = Vec::new();
    let (mut p, mut n) = writable_device_buffer_arg(&mut scratch);
    push_kernel_device_slice(&mut args, &mut p, &mut n);
    let (mut p, mut n) = writable_device_buffer_arg(&mut errors);
    push_kernel_device_slice(&mut args, &mut p, &mut n);
    let cfg = LaunchConfig {
        grid_dim: (1, 1, 1),
        block_dim: (64, 1, 1),
        shared_mem_bytes: 0,
    };
    unsafe {
        launch_kernel_on_stream(
            &function,
            cfg.grid_dim,
            cfg.block_dim,
            cfg.shared_mem_bytes,
            &stream,
            &mut args,
        )
    }
    .expect("launch");
    let errors = errors.to_host_vec(&stream).expect("errors");
    for (thread, &report) in errors.iter().enumerate() {
        assert_eq!(report, if thread % 32 == 0 { 1 } else { 0 },
            "production lowering thread {thread}: 0 unentered, 1 completed cleanly, >1 failed");
    }
    eprintln!(
        "lowering: 2 workers; init/send/recv/Io/leader worker calls executed; scalar owners completed; host Leader route pending W6"
    );
}
