# transport

Several hosts in one launch (`CONTRACT.md`, Federation). `hosts.rs` and `nodes.rs` say who the hosts are;
`cross/` carries the routes between them (`cross/README.md`).

- **Hosts are nodes.** `MPI_Comm_split_type(SHARED)` gives the nodes of the world. A host is numbered by its
  lowest world rank (F1). `host()` answers that number and `hosts()` the count.
- **Each host launches alone.** Its communicator is the world of its launch: its ranks agree on its plan, it
  maps its own window and segment, and its last rank is its leader. A launch waits for another host only when the two share routes (`cross/README.md`).
- **Each host runs the whole program.** Every leader declares its own topology, with workers numbered from 0; a
  program that wants different work on each host branches on `host()`.
- **Clocks.** A host's clock is `CLOCK_MONOTONIC` from its own start, and its `ClockId` is the host number
  plus one, so readings of two hosts never compare (`INTERFACE.md`, 5.1).
- **Peers.** The leaders form a communicator of their own, ranked by host. `leader_send` is a blocking
  `MPI_Send` to the peer's leader with the message's tag (F4) in its first 8 bytes, and never answers `Full`.
  `leader_recv` probes for every waiting peer message and keeps them, in arrival order, until their tag is
  asked for. Nothing on the host wakes a sleeping leader for such a message, so on more than one host
  `Run::wait` spins until the leader's epoch or the count of peer arrivals moves.
- **Testing on one machine.** `TRAME_MPI_NODE_RANKS=k` cuts each node into hosts of k consecutive ranks.
  `check/programs/run.sh` runs every program on one host and on two hosts of `ranks.txt`'s rank count each. It
  is a test setting and never a default.

Scaling of the transport is in `SCALING.md`.
