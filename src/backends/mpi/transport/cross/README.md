# cross: routes between hosts

A route joins a worker of this host to a worker of another, `WorkerId::from((HostId(h), k))`. Both hosts
declare it with the same ends, type, kind and capacity (F3). At launch each leader sends the other host's
leader the routes they share and compares them with what comes back (`connect.rs`). Any difference
refuses both launches, naming the route. Only hosts that share routes wait for each other (F7); a host that declares a route with a host that declares
none back waits for that host's next launch that does. A route
between a leader and another host's worker is refused: leaders talk over the peer channel.

A route is a lane (`cross/`): frames go rank to rank by two-sided MPI on a duplicate of the world, with no
thread between. The receiver posts one receive per slot and sends a credit back for each frame it takes. The
sender sends only while its credits leave room, so every frame finds a posted receive and `Full` means the
capacity is used. MPI keeps the order of one sender's messages of one tag, which keeps T1. At the end every
rank completes its sends and meets the others in a nonblocking barrier before `MPI_Finalize` (`quiesce`); a
lossy frame still waiting for room is dropped.

A frame from another host moves no futex, so a worker with a lane does not sleep: its idle agents poll the
lanes, taking the transport lock for each poll only. An arrival moves the worker's epoch, and the agents' wait
ends as for a frame of its own host (`waiting/`).

The choice rests on the experiment `mpi_cross_host` (no longer in `lab/`; `bench/xhost.py` is the cross-machine
microbenchmark that remains). Between two machines of Lille, a one-way 8-byte frame took a median 30.6 us rank
to rank and 49.6 us through a relay thread, and polling a shared flag between MPI tests cost nothing
measurable (29.9 us). The model of one lane is `src/backends/mpi/proofs/Lane.tla`.

Files: `mod.rs` (lanes, `quiesce`), `sender.rs`, `receiver.rs`, `connect.rs`, `calls.rs` (the MPI calls), `count.rs`.
