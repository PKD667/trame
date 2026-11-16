# rings

Routes between two workers of one host: a ring of `cap` slots in the window (`memory/layout.rs`). A route
between hosts is a lane (`transport/cross/`) and has no ring.

| file | holds |
|---|---|
| `send.rs` | `send` on a ring, and the seqlock frame both sides use |
| `recv.rs` | `recv`: the head of an owned route, and the lossy cursor |
| `poll.rs` | questions that take nothing: whether a frame waits, whether a send would find room |

- **Lossless.** The sender owns P and the receiver owns C, each published with a release store. `Full` is
  decided by an acquire load of C and is the linearization point: `Full` exactly when `P - C == cap`.
- **Lossy.** A slot is a seqlock. The sender never reads C and never waits. A receiver that sees a slot change
  under its copy skips the frame, which is a Drop.
- **Receive.** The head of the first owned route in rotating order that has a frame. The scan starts past the
  route the last receive took, so a route with frames is taken within n receives (n = owned routes). Frames
  are ordered within a route only (`INTERFACE.md`, Offers).
- `poll.rs` answers `room` for the sender's next send only: the receiver can free room, never take it.
- Not argued by a model: the overwrite argument is in the comments of `send.rs` and tested by `check overwrite`.
  The traced build adds SeqCst fences, so its checks exercise a more strongly ordered variant than production.
