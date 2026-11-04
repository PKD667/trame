# Hosts

How the MPI backend runs on several shared-memory nodes (`CONTRACT.md`, Federation).

## A host per node

- **Hosts are nodes.** `MPI_Comm_split_type(SHARED)` gives the nodes of the world. Each node is a
  host, numbered by its lowest world rank (`hosts.rs`); `host()` answers that number, and
  `__hosts()` the number of hosts.
- **Each host launches alone.** A host's communicator is the world of its launch: its ranks agree
  on its plan, it maps its own window and segment, and its last rank is its leader. Within a host
  the backend is the one-node backend of `README.md`; nothing of one host's launch waits for another
  host.
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

A route to another host's worker is refused at launch (`Refused::UnknownWorker`). Frames between
hosts, sent rank to rank with two-sided MPI and no relay thread, are the next part of this backend;
the experiment `mpi_cross_host` records the measurements behind that choice.

## Testing on one machine

`TRAME_MPI_NODE_RANKS=k` cuts each shared-memory node into hosts of k consecutive ranks. It runs
several hosts on one machine, and `programs/run.sh` runs every program on one host and on two hosts
of `ranks.txt`'s rank count each. It is a test setting and never a default.
