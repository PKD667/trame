# waiting

How an agent sleeps and is woken. No agent polls on a route inside a host.

| file | holds |
|---|---|
| `epoch.rs` | `bump`, `sleep_on`: the wait protocol, and the argument that no wake-up is lost |
| `futex.rs` | the futex calls: sleep on a 32-bit word, wake its sleepers |

- Each worker has an EPOCH word and the leader has LEADER_EPOCH. Every act that can change what an agent waits
  for bumps the word; an agent that must wait sleeps on its own word. A worker's result is its exit.
- SLEEP counts the sleepers of a word. The waiter reads EPOCH, checks, adds 1 to SLEEP, reads EPOCH again; the
  bumper adds 1 to EPOCH, then loads SLEEP. All four accesses are SeqCst, so one total order decides, and the
  kernel sleeps only while EPOCH still equals `seen`.
- Every epoch and futex word is 32 bits: the kernel compares exactly those bits. A wake-up is lost only if EPOCH
  wraps back to `seen` between the waiter's read and the kernel's compare, so after at least 2^32 bumps; nothing
  measures how near a run gets. `Epoch.tla` shows it on a word of 2 values (`counterexamples/epoch-wrap.txt`).
- A worker with a lane does not sleep: a frame from another host moves no futex, so its idle agents poll
  (`roles/worker.rs`, `idle`).
- Model: `src/backends/mpi/proofs/Epoch.tla`, which cites `epoch.rs` and `futex.rs` by line. A change to either
  needs the model's comments re-read. The models assume SeqCst and leave out timeouts.
