# Hosts

How the MPI backend runs on several shared-memory nodes (`CONTRACT.md`, Federation).

## A host per node

- **Hosts are nodes.** `MPI_Comm_split_type(SHARED)` gives the nodes of the world. Each node is a
  host, numbered by its lowest world rank (`hosts.rs`); `host()` answers that number, and
  `hosts()` the number of hosts.
- **Each host launches alone.** A host's communicator is the world of its launch: its ranks agree
  on its plan, it maps its own window and segment, and its last rank is its leader. Within a host
  the backend is the one-node backend of `README.md`. A launch waits for another host only when the
  two share routes (below).
- **Each host runs the whole program.** Every leader declares its own topology, with workers
  numbered from 0; a program that wants different work on each host branches on `host()`.
- **Clocks.** Each host's clock is `CLOCK_MONOTONIC` from its own start, and its `ClockId` is the
  host number plus one, so readings of two hosts never compare (spec 5.1).

## Peers

The leaders form a communicator of their own, ranked by host. `Run::leader_send` is a blocking
`MPI_Send` to the peer's leader, with the message's tag (F4) in its first 8 bytes; MPI buffers small
messages, so it never answers `Full`. `Run::leader_recv` probes for every waiting peer message and
keeps them, in arrival order, until a `leader_recv` of their tag takes one.

A peer message arrives through MPI, not through the window, so nothing on the host can wake a
sleeping leader for it. On more than one host, `Run::wait` therefore spins: it returns when the
leader's epoch or the count of peer arrivals moves. On one host it sleeps as before.

## Routes between hosts

A route joins a worker of this host to a worker of another (`WorkerId::from((HostId(h), k))`). Both
hosts declare it, with the same ends, type, kind and capacity (F3). At launch each leader sends the
routes it shares with each other host to that host's leader, and compares them with what that host
sends back; any difference refuses the launch of both hosts, with a message naming the route. Only
hosts that share routes wait for each other. A host that declares a route with a host that declares
none back waits for that host's next launch that does.

A route between hosts is a lane (`cross.rs`): frames go from the sending worker's rank to the
receiving worker's rank by two-sided MPI, on a duplicate of the world, with no thread between. The
receiver keeps one posted receive per slot of the capacity and sends a credit back for each frame it
takes; the sender sends only while it has room by the credits it has, so every frame finds a posted
receive and `Full` means the capacity is used. MPI keeps the order of one sender's messages of one
tag, which keeps T1. A route between the leader and another host's worker is refused: leaders talk
over the peer channel.

A frame from another host moves no futex, so a worker with a lane does not sleep: its idle agents
poll the lanes, taking the transport lock for each poll only. An arrival moves the worker's epoch,
and the agents' wait ends as for a frame of its own host. At the end every rank completes its sends
and meets the others in a nonblocking barrier before `MPI_Finalize`; a lossy frame still waiting for
room is dropped.

The choice rests on the experiment `mpi_cross_host` (`experiments/mpi_cross_host`): between two
machines of Lille, a one-way 8-byte frame took a median 30.6 us rank to rank and 49.6 us through a
relay thread, and polling a shared flag between MPI tests cost nothing measurable (29.9 us).

## Stuck runs across hosts

Each end of a lane counts, in its route's two window words, the messages it sent and those it saw
arrive, frames and credits alike; an end that has ended keeps observing its posted receives until
the final barrier. The leaders run rounds of a nonblocking allreduce on the peer channel's
communicator while they wait, join or have ended (`detect.rs`). A round sums the leaders whose
snapshot of their host's words moved since their last round, the messages sent and those arrived,
and the leaders that have not ended. A round with nothing moved and as many arrived as sent shows an
instant at which no step runs, no message is in flight and no epoch can move: every worker without
a result is stuck. A joining leader then ends its workers with `Deadlock`, and a waiting leader
panics, as on seq. An abort on one host is the common case: the other host's worker that waits on
a lane from it ends with `Deadlock` instead of hanging both hosts.

A leader that runs its own code holds the rounds, so nothing is shown while it does; that is sound,
since it may still send. A route that one host declares and the other does not is refused on the
declaring host when the other host's leader ends, since the ending leader answers every connect
still waiting for it.

## Testing on one machine

`TRAME_MPI_NODE_RANKS=k` cuts each shared-memory node into hosts of k consecutive ranks. It runs
several hosts on one machine, and `programs/run.sh` runs every program on one host and on two hosts
of `ranks.txt`'s rank count each. It is a test setting and never a default.
