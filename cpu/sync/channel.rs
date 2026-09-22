// A host-only, unbounded, many-sender one-receiver channel, re-exported for the one thing that
// needs it: an application's *control* plane.
//
// This is not a worker mailbox and it is not Family B. `sync::handoff` is the bounded ownership
// transfer with declared pressure that data between participants goes through; this is the
// queue a server's connection threads put a request on while a control loop takes them one at a
// time. The difference is the contract, not the shape:
//
//   `handoff`  bounded, refusal is visible, a sender learns that the receiver is behind
//   `channel`  unbounded, a send never reports pressure, so a producer that outruns the
//              receiver grows the process's memory instead of being told to stop
//
// Unbounded is the right answer for a control plane whose producers are human-paced requests and
// whose loss would strand a client waiting for a reply. It is the wrong answer for anything
// carrying model data at rate, which is why this module is documented as host-only and why no
// device backend is expected to supply it. A future device worker mailbox is a bounded ownership
// handoff to a resident body, as `nv/sync/handoff.rs` is; it is not this.

pub use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel};
