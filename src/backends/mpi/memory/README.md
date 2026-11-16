# memory

What every rank of a node maps. Nothing here makes a decision; `rings/`, `waiting/` and `roles/` read and
write the words.

| file | holds |
|---|---|
| `layout.rs` | the byte layout of the window, as constants and the comment that defines it; `Topology`, `Route`, `Layout` |
| `shm.rs` | the window: one `MPI_Win_allocate_shared` on the node's communicator, owned by the last rank. The only file that calls the one-sided API |
| `segment.rs` | the node's segment: one POSIX object mapped at the same address in every rank, the domain of the workers (`CONTRACT.md` M1) |
| `object.rs` | the bytes of one abort message, a POSIX object named from the window's |

- Every rank computes the same layout from the same topology, so no rank sends an offset to another.
- Each route has its own ring of `cap` slots, sized from the route's declared message size. Window bytes per
  route: 128 + cap x (align8(size) + 8).
- The window is never freed: `MPI_Win_free` is collective, so a returning worker would wait for its peers.
- States, the pool, edits and `take` live in the segment, which maps at one address (`MAP_FIXED_NOREPLACE`,
  Linux 4.17). The launch is refused on every rank if one rank cannot map it. Linux only.
- Every object a run makes is unlinked when the run ends. `tests/run.sh` fails on a `/dev/shm/trame-*` left over.
