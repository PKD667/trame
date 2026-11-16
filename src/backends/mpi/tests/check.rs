// The MPI-launched checks of the transport. N - 1 workers and the leader as rank N - 1; each
// subcommand is run by mpirun -n N. The leader runs no worker code.
//
//   mpirun -n N check fifo      --seed S --dir D      random routes and random interleavings;
//                                                     FIFO per route; every send and consume logged
//   mpirun -n N check overwrite --seed S --cap C --msgs M [--late]
//                                                     lossy route, 0 -> 1; newest frame kept
//   mpirun -n N check fair      --msgs M              two senders fill two routes to M frames; the
//                                                     receiver must alternate between them (no starvation)
//   mpirun -n 3 check bigroute                        a 256 KiB message on one route, read back whole
//   check verify D                                    no MPI: checks the fifo logs in D
//
// A failed check panics, which makes mpirun stop the job with a nonzero status.

#[path = "check/big.rs"]
mod big_check;
#[path = "check/common.rs"]
mod common;
#[path = "check/fifo.rs"]
mod fifo_check;
#[path = "check/lossy.rs"]
mod lossy;
#[path = "check/sync.rs"]
mod sync;
#[path = "check/verify.rs"]
mod verify_check;

use std::path::Path;

use big_check::big;
use fifo_check::fifo;
use lossy::overwrite;
use sync::fair;
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
    match cmd.as_str() {
        "fifo" => fifo(&world, &args),
        "overwrite" => overwrite(&world, &args),
        "fair" => fair(&world, &args),
        "bigroute" => big(&world, &args),
        other => panic!("unknown check {other:?}"),
    }
    // Finalization: the universe is dropped here, after this rank's work.
    drop(world);
    trame::__backend::finalize();
    std::mem::forget(universe);
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
