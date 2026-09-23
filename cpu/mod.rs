// Host-thread mechanisms: what a backend that runs on host threads imports, and what a backend
// that does not, does not.
//
// They live here together for one reason: every one of them is a fact
// about a host process rather than about the backend contract.
//
//   `clock`  the process's monotonic origin and the readings taken from it
//   `sync`   the step primitives, over `std`'s atomics
//   `run`    the lowering of `#[parallel]` and `concurrent!`: a loop, and scoped threads
//
// The transport surface in `../lib.rs` is what every backend answers. This is what a *particular
// kind* of backend answers: one whose participants are OS threads in one address space. `mpi/`,
// `rma/` and `lossy/` are that kind, and so is `none/`; `nv/` is not, because a device has no
// thread to start, no mutex to take, and no clock to read. Reading `trame::cpu::…` is therefore
// reading "this runs on the host", which is a stronger and more useful statement than
// `trame::…` was: it stops the application from believing the mechanism travels with it.
//
// Nothing here is a model type, and nothing here decides anything about a transport. A backend
// that does not run on host threads imports none of it — `tests/boundary.rs` checks that `nv/`
// does not — and a future device backend supplies its own equivalents beside its own transport,
// as `nv/` does for the warp.

pub mod clock;
pub mod run;
pub mod sync;

#[cfg(test)]
mod tests;
