//! Trame: one meaning for a program on every machine it runs on. `spec.md` is the contract for
//! humans; this crate is the same contract in Rust. A change to one is a change to the other.
#![no_std]

extern crate alloc;

// The macros expand to `::trame::...`, so the crate's own tests need the same name.
extern crate self as trame;

/// Re-exported so a message's derives name `::trame::__bytemuck`: users need no bytemuck of their own.
#[doc(hidden)]
pub use bytemuck as __bytemuck;

/// Marks a struct as a message type: `#[trame::message]`. The struct must be plain data, with no
/// padding and no bit pattern that is invalid. A struct with padding, a reference, a raw pointer or
/// a function pointer is refused at compile time; the refusals are cases of rejections/ (the macro
/// reads the field types, since bytemuck would accept a raw pointer). A plain struct is accepted:
///
/// ```
/// #[trame::message]
/// struct Plain {
///     a: u32,
///     b: u32,
/// }
/// ```
pub use trame_macros::message;

/// Marks `fn f(x: &I, slot: &mut S, cx: &C)` as a `parallel!` item, with its key:
/// `#[trame::item(key = x.index as usize)]` (spec.md 5.4).
pub use trame_macros::item;

/// `parallel!(f, &cx, &items, &mut slots)`: apply item `f` to `items` (spec.md 5.4). Without
/// `--cfg trame_checked` it is a compile error; rejections/ tests that. It has no doc test here,
/// because doc tests are built without the flag.
pub use trame_macros::parallel;

/// `#[trame::process(receives = Job, ...)]` on `impl P { fn step(&mut self, a: &mut A, ...) -> Status }`
/// (spec.md 5.2, 5.3). The parameters after `self` are the cells; each is `&mut T` for the program's
/// own type `T`. A primitive or core/alloc type is refused, since it is not a type of the program
/// (rejections/, process/). A cell of the program's own type is accepted:
///
/// ```
/// #[trame::message]
/// struct Job { id: u32 }
/// struct Queue { jobs: u32 }
/// struct P;
/// #[trame::process(receives = Job)]
/// impl P {
///     fn step(&mut self, q: &mut Queue) -> trame::Status {
///         let _ = q.jobs;
///         trame::Status::Done
///     }
/// }
/// ```
pub use trame_macros::process;

/// `concurrent!(p0, p1; c0, c1)`: run the processes over the cells (spec.md 5.3). Answers `Result<(), E>`.
/// Without `--cfg trame_checked` it is a compile error, so it has no doc test here.
pub use trame_macros::concurrent;

/// The runtime the macros call (spec.md 5.3, 5.4). Not for direct use.
#[doc(hidden)]
#[path = "parallel.rs"]
pub mod __rt;

/// Level markers the macros emit and the driver reads. Not for direct use.
#[doc(hidden)]
#[path = "check.rs"]
pub mod __check;

/// Numeric functions, grouped apart from the effects: `trame::math::exp` (spec.md 7).
pub mod math;

// The modules are private: the surface is the root re-exports below (spec.md 7). Their definition
// paths (`trame::io::send`, `trame::time::now`, ...) are what the driver's effects.txt names.
mod io;
mod launch;
mod run;
mod time;
mod topology;

/// The surface of spec.md 7: every public name of the crate, at the root and nowhere else.
pub use io::{barrier, recv, send, Full, Message};
pub use launch::{Refused, MAX_FRAME};
pub use run::{Outcome, Run};
pub use status::Status;
pub use time::{now, Reading};
pub use topology::{Context, Topology, WorkerId, LEADER};

#[cfg(all(feature = "seq", feature = "mpi"))]
compile_error!("features `seq` and `mpi` are exclusive: one backend per build");

#[cfg(feature = "seq")]
#[path = "backends/seq/mod.rs"]
mod seq;

/// The MPI backend: one rank per worker, the leader on the last rank (backends/mpi/README.md).
#[cfg(feature = "mpi")]
#[path = "backends/mpi/mod.rs"]
mod mpi;

/// The MPI backend's transport, for the test binaries under backends/mpi/tests. Not API.
#[cfg(feature = "mpi")]
#[doc(hidden)]
pub mod __mpi {
    pub use crate::mpi::*;
}

/// Internal: the seam between the surface and the selected backend (see backend.rs).
mod backend;

/// Scheduling core, shared by every backend: spec.md 5.3 (sched.rs).
mod sched;

/// The cells a step borrows, and the types a process owns (spec.md 5.2, 5.3). Re-exported by __rt.
mod cells;

/// The runtime of `concurrent!` (spec.md 5.3). Re-exported by __rt.
mod concurrent;

/// What a step returns, and the condition it waits for (spec.md 5.3).
mod status;

#[cfg(all(feature = "seq", not(feature = "mpi")))]
use seq as selected;

// `self::`: the crate also depends on an external crate named `mpi`, and a bare `mpi` is ambiguous.
#[cfg(all(feature = "mpi", not(feature = "seq")))]
use self::mpi as selected;

/// Without a backend module, every launch is refused (see unselected.rs).
#[cfg(not(any(feature = "seq", feature = "mpi")))]
mod unselected;
#[cfg(not(any(feature = "seq", feature = "mpi")))]
use unselected as selected;
