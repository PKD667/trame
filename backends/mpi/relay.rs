// The relay of a node (NODES.md, The relay): two threads in the node's lowest-ranked process. `out`
// (relay_out.rs) sleeps on the relay epoch and sends what this node has to say; `in` (relay_in.rs) blocks
// in MPI_Recv and applies what the others say. They share one `Ctx`: a copy of the node's window
// pointer, and a few flags. Only `out` sends and only `in` receives, so a blocked send never stops a
// receive.

extern crate std;

use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread::{Builder, JoinHandle};
use std::vec::Vec;

use mpi::ffi;

use super::state::local;
use super::Transport;

/// A release the root's `in` has decided and `out` must send: barrier generation `generation`, with
/// publication `no` and its bytes.
pub(super) struct Release {
    pub generation: u64,
    pub no: u64,
    pub bytes: Vec<u8>,
}

/// What both relay threads share.
pub(super) struct Ctx {
    /// The node's window and layout, through a copy of the transport that never runs its `Drop`.
    pub port: Port,
    /// The world communicator, which carries every relay message.
    pub comm: Comm,
    /// Set by `in` when END arrives: the run is over, and both threads stop once they have sent.
    pub ended: AtomicBool,
    /// Set by `in` when ABORT arrives, before it sets the abort bit: the abort came from the network,
    /// so `out` does not send it back.
    pub aborted: AtomicBool,
    /// Root only: the release `in` decided. `out` takes it and sends it to every node.
    pub release: Mutex<Option<Release>>,
}

/// The node's transport copy, shared by the two threads.
pub(super) struct Port(ManuallyDrop<Transport>);

// SAFETY: the window outlives the process (shm.rs never frees it), and every word the relay threads
// touch is an atomic in it, or a futex word used through atomics. The layout and the node table are
// read-only after launch. The fields that change (cursor, rr, trace, publication, generation) are never
// used by a relay thread, and `ManuallyDrop` keeps `Transport::drop` from signalling an exit.
unsafe impl Send for Port {}
unsafe impl Sync for Port {}

impl Deref for Port {
    type Target = Transport;

    fn deref(&self) -> &Transport {
        &self.0
    }
}

/// The world communicator's handle.
#[derive(Clone, Copy)]
pub(super) struct Comm(pub ffi::MPI_Comm);

// SAFETY: a communicator handle is valid in every thread. The launch requires MPI_THREAD_MULTIPLE for
// several nodes (open.rs), which lets the two relay threads call MPI on it at once.
unsafe impl Send for Comm {}
unsafe impl Sync for Comm {}

/// The two threads of a node's relay. `state::finalize` joins them.
pub(super) struct Relay {
    out: JoinHandle<()>,
    inn: JoinHandle<()>,
}

impl Relay {
    /// Wait for both threads to stop. They stop when the run ends (END).
    pub(super) fn join(self) {
        self.out.join().expect("trame: the relay's out thread panicked");
        self.inn.join().expect("trame: the relay's in thread panicked");
    }
}

/// Start this node's relay threads, on the relay host, before the launch barrier. `t` is the host's
/// own endpoint, and `comm` the world communicator.
pub(super) fn start(t: &Transport, comm: ffi::MPI_Comm) {
    let copy = Transport::new(t.me, t.win.alias(), t.lay.clone(), t.nodes.clone());
    let ctx = Arc::new(Ctx {
        port: Port(ManuallyDrop::new(copy)),
        comm: Comm(comm),
        ended: AtomicBool::new(false),
        aborted: AtomicBool::new(false),
        release: Mutex::new(None),
    });
    let (for_out, for_in) = (ctx.clone(), ctx);
    let out = Builder::new()
        .name("trame-relay-out".into())
        .spawn(move || super::relay_out::run(&for_out))
        .expect("trame: could not start the relay's out thread");
    let inn = Builder::new()
        .name("trame-relay-in".into())
        .spawn(move || super::relay_in::run(&for_in))
        .expect("trame: could not start the relay's in thread");
    local().relay = Some(Relay { out, inn });
}

/// Send `bytes` to world rank `dest` as one message, with tag 0. Blocking. Never MPI_Bsend.
pub(super) fn send(comm: Comm, dest: i32, bytes: &[u8]) {
    let count = i32::try_from(bytes.len()).expect("a relay message fits in i32");
    let rc = unsafe {
        ffi::MPI_Send(bytes.as_ptr() as *const c_void, count, ffi::RSMPI_UINT8_T, dest, 0, comm.0)
    };
    assert_eq!(rc, ffi::MPI_SUCCESS as i32, "trame: MPI_Send to rank {dest} failed");
}

/// Receive one message from any rank, with any tag, into `buf`. Blocking. Returns its length.
pub(super) fn recv(comm: Comm, buf: &mut [u8]) -> usize {
    let count = i32::try_from(buf.len()).expect("a relay buffer fits in i32");
    let mut status: ffi::MPI_Status = unsafe { std::mem::zeroed() };
    let rc = unsafe {
        ffi::MPI_Recv(
            buf.as_mut_ptr() as *mut c_void,
            count,
            ffi::RSMPI_UINT8_T,
            ffi::RSMPI_ANY_SOURCE,
            ffi::RSMPI_ANY_TAG,
            comm.0,
            &mut status,
        )
    };
    assert_eq!(rc, ffi::MPI_SUCCESS as i32, "trame: MPI_Recv failed");
    let mut n: i32 = 0;
    let rc = unsafe { ffi::MPI_Get_count(&status, ffi::RSMPI_UINT8_T, &mut n) };
    assert_eq!(rc, ffi::MPI_SUCCESS as i32, "trame: MPI_Get_count failed");
    n as usize
}
