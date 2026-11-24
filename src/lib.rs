//! Trame: one meaning for a program on every machine it runs on. `INTERFACE.md` is the contract for
//! humans; this crate is the same contract in Rust. A change to one is a change to the other.
#![no_std]

extern crate alloc;

// The macros expand to `::trame::...`, so the crate's own tests need the same name.
extern crate self as trame;

/// Re-exported so a message's derives name `::trame::__bytemuck`: users need no bytemuck of their own.
#[doc(hidden)]
pub use bytemuck as __bytemuck;

/// Makes a struct a message type: `#[trame::message]`. A message is plain data (`Message`), so the
/// struct must have no padding and no bit pattern that is invalid. A struct with padding, a reference,
/// a raw pointer or a function pointer is refused at compile time; the refusals are cases of
/// check/rejections/ (the macro reads the field types, since bytemuck would accept a raw pointer). A plain
/// struct is accepted:
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
/// `#[trame::item(key = x.index as usize)]` (INTERFACE.md 5.4).
pub use trame_macros::item;

/// `parallel!(f, &cx, &items, &mut slots)`: apply item `f` to `items` (INTERFACE.md 5.4). Without
/// `--cfg trame_checked` it is a compile error; check/rejections/ tests that. It has no doc test here,
/// because doc tests are built without the flag.
pub use trame_macros::parallel;

/// `#[trame::state(steps(a, b), stop = f, pool = P, error = E)]` on a struct: the struct is a
/// worker's state, and the listed functions are its steps (INTERFACE.md 5.3). `stop`, `pool` and `error`
/// are optional. The leader builds the state and declares the worker with `Topology::worker`.
pub use trame_macros::state;

/// `#[trame::step(S, receives = Job, weight = 0.5)]` on a function: a step of the state `S`. Each
/// parameter names a field of `S` and takes it as `&T` (read) or `&mut T` (write), or is `pool: &P`,
/// the host's pool. `receives` and `weight` are optional. The step returns `Status` or
/// `Result<Status, E>`.
pub use trame_macros::step;

/// The runtime the macros call (INTERFACE.md 5.3, 5.4). Not for direct use.
#[doc(hidden)]
#[path = "parallel/mod.rs"]
pub mod __rt;

#[cfg(all(feature = "lanes-avx2", feature = "lanes-avx512"))]
compile_error!("features `lanes-avx2` and `lanes-avx512` are exclusive: choose at most one lane backend");

#[cfg(all(any(feature = "lanes-avx2", feature = "lanes-avx512"), not(target_arch = "x86_64")))]
compile_error!("lane features `lanes-avx2` and `lanes-avx512` are only available on x86_64 (target_arch)");

/// Level markers the macros emit and the driver reads. Not for direct use.
#[doc(hidden)]
#[path = "check.rs"]
pub mod __check;

/// Numeric functions, grouped apart from the effects (INTERFACE.md 7).
pub mod math;

/// Bounded collections for steps: fixed capacity, never reallocated (src/bounded/mod.rs).
mod bounded;

// The modules are private: the surface is the root re-exports below (INTERFACE.md 7). Their definition
// paths (`trame::io::send`, `trame::time::now`, ...) are what the driver's effects.txt names.
mod domain;
mod flow;
mod io;
mod launch;
mod object;
mod plain;
mod run;
pub mod system;
mod time;
mod topology;

pub use bounded::{Bounded, BoundedHeap};
pub use domain::Alloc;
/// The surface of INTERFACE.md 7: every public name of the crate, at the root and nowhere else, but for two
/// namespaces that gather a family of names: `math` (the numeric library) and `system` (what the launch is on).
pub use flow::{Drained, Spent, drain, fuel, lend};
pub use io::{Full, Message, Stamped, host, hosts, is_leader, me, peer_wait, recv, send, workers};
pub use object::{Malformed, Object, peer_recv, peer_send, share};
#[doc(hidden)]
pub mod __object {
    pub use crate::object::{Reader, Shape, Writer};
}
pub use launch::Refused;
pub use plain::Plain;
#[doc(hidden)]
pub use plain::probe as __probe;
pub use run::{Outcome, Run};
pub use status::Status;
pub use time::{ClockId, Reading, now};
pub use topology::{End, Handle, HostId, Leader, Pool, Topology, Worker, WorkerId};
pub use trame_macros::{Object, Plain};

/// The backends and the choice among them (src/backends/mod.rs).
#[path = "backends/mod.rs"]
mod backends;

/// The selected backend's own items, for its test binaries. Not API.
#[doc(hidden)]
pub use backends::__backend;

/// Internal: the seam between the surface and the selected backend (see backend.rs).
mod backend;

/// The scheduling model every backend keeps: INTERFACE.md 5.3 (sched.rs).
mod sched;

/// The message types a step receives (INTERFACE.md 5.2). Re-exported by __rt.
mod cells;

/// A worker: its state, its steps, and the reference scheduler (INTERFACE.md 5.3). Re-exported by __rt.
mod worker;

/// What a step returns, and the condition it waits for (INTERFACE.md 5.3).
mod status;

// The lib's own test binary installs the wrapper like any trame program (domain.rs).
#[cfg(test)]
extern crate std;
#[cfg(test)]
#[global_allocator]
static TEST_ALLOC: Alloc = Alloc::new(std::alloc::System);
