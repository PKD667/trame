// The MPI-launched checks of the transport. N - 1 workers and the leader as rank N - 1; each
// subcommand is run by mpirun -n N. The leader runs no worker code.
//
//   mpirun -n N check fifo      --seed S --dir D      random routes and random interleavings;
//                                                     FIFO per route; every send and consume logged
//   mpirun -n N check overwrite --seed S --cap C --msgs M [--late]
//                                                     lossy route, 0 -> 1; newest frame kept
//   mpirun -n N check barrier   --seed S --rounds R   leader deposits each round; every worker sees
//                                                     the publications in order, the last one last
//   mpirun -n N check fair      --msgs M              two senders fill two routes to M frames; the
//                                                     receiver must alternate between them (no starvation)
//   mpirun -n N check deadlock                        worker 0 returns; the others must get Deadlock
//   mpirun -n 3 check publication                     a 256 KiB message; publications of 1 MiB, 3 bytes,
//                                                     4 MiB, each read back by both workers
//   mpirun -n 3 check small     --msgs M              small routes and one large, each frame checked; the
//                                                     lane count of each process is checked (check_small.rs)
//   check verify D                                    no MPI: checks the fifo logs in D
//
// A failed check panics, which makes mpirun stop the job with a nonzero status.

#[path = "check_common.rs"]
mod common;
#[path = "check_fifo.rs"]
mod fifo_check;
#[path = "check_lossy.rs"]
mod lossy;
#[path = "check_publish.rs"]
mod publish;
#[path = "check_small.rs"]
mod small_check;
#[path = "check_sync.rs"]
mod sync;
#[path = "check_verify.rs"]
mod verify_check;

use std::path::Path;

use fifo_check::fifo;
use lossy::{deadlock, overwrite};
use mpi::traits::*;
use publish::publication;
use small_check::small;
use sync::{barrier, fair};
use trame::__mpi::clock;
use verify_check::verify;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).cloned().unwrap_or_default();
    if cmd == "verify" {
        verify(Path::new(args.get(2).expect("verify wants a directory")));
        return;
    }
    let (universe, _) = mpi::initialize_with_threading(mpi::Threading::Multiple).expect("MPI");
    let world = universe.world();
    // The lane count `small` expects this process to show (check_small.rs), checked after finalization.
    let mut lane_expected = None;
    match cmd.as_str() {
        "small" => lane_expected = Some(small(&world, &args)),
        "fifo" => fifo(&world, &args),
        "overwrite" => overwrite(&world, &args),
        "barrier" => barrier(&world, &args),
        "fair" => fair(&world, &args),
        "publication" => publication(&world, &args),
        "deadlock" => deadlock(&world),
        other => panic!("unknown check {other:?}"),
    }
    // Finalization: the universe is dropped here, after this rank's work. The deadlock check
    // times it, to see that finalization does not wait for the rank still working.
    let me = world.rank();
    drop(world);
    // The library's finalize joins the relay threads before MPI_Finalize (NODES.md, End).
    trame::__mpi::finalize();
    if let Some(expected) = lane_expected {
        let lanes = trame::__mpi::lane_frames();
        assert_eq!(lanes, expected, "rank {me}: small frames sent by lane (TRAME_MPI_SMALL, TRAME_MPI_NODE_RANKS)");
    }
    std::mem::forget(universe);
    if cmd == "deadlock" && me == 0 {
        println!("deadlock rank 0: finalized at {}", clock());
    }
}
