# trame-driver

Trame's compile-time checker (spec.md, Compile time): a rustc driver, and `cargo trame`, which runs cargo with it.
The driver reads the optimized MIR of each root marker (`trame::__check::{item,step,worker}`, check.rs) and everything reachable from it, and refuses code that breaks the rules. Refusals are rustc errors: `error: [rule] <level> ...`.

Run from driver/. The compiler is pinned in driver/rust-toolchain.toml and built by flake.nix.

    nix develop "path:$PWD/..#driver" --command cargo test

On a crate: `nix build "path:$PWD/..#trame-driver"`, put its `bin/` on PATH, then `cargo trame check|build|test` in that crate. Checked builds use `target/trame`, apart from plain cargo's `target/`.
Debug: `TRAME_DRIVER_DEBUG=1` prints each root and its level.

## Checks

Per level (Item, Step, Worker, as in spec.md, Requirement per level), for every function reached from a root:
- Unsafe code, FFI, indirect calls, statics: refused at every level.
- Effects (lists/effects.txt): each effect is allowed at the levels spec.md names for it.
- Core and alloc callees: judged by the lists (below).
- Counted loops (Item, Step), recursion (Item, Step), allocation (Item: no reach to `alloc::alloc` or `__rust_alloc`).

A function reached from several roots is checked at the strictest level that reaches it.

## The lists (lists/)

Each line is a path, or `P::*` for every item under `P`, then a `# reason`. A `-` line is a deny at one level.
- `deny.txt`: refused at every level; where a deny and an allow both match, the more specific entry decides, and a deny wins a tie. Interior mutability, locks, `Rc`, raw pointers, hints, intrinsics, arch.
- `A_I.txt`: what an item may call. Read with deny.txt. The infinite iterator sources are denied here (`-` lines), because an item has no unbounded loop.
- `A_C.txt`: what a step may call. Every line of A_I, plus allocation (`alloc::vec`, `boxed`, `collections`, `string`, `slice`, `fmt`, `alloc`).
- `A_W.txt`: what a worker may call. Every line of A_C, without the infinite-source denies, plus the `Arc` snapshot handle.

The levels nest: an allow in a stricter level is allowed in every looser one (checked at start-up), and a deny at a looser level is denied at every stricter one. Start-up fails loudly otherwise.

A closure or fn item passed as a generic argument to a trusted callee is called by that callee, so it is a root at the caller's level and its body is checked. The same goes for a user type's impls of core traits (its `Clone`, `Debug`, `Iterator`, ...): core code calls them, so they are read at the caller's level. A user `Iterator` passed to core code is refused below Worker, since core would drive it without a bound.

## Trust follows the call path (and what that costs)

Trust is a property of the call path, decided in `trust.rs`. Functions defined in crate `trame` are trusted at every level, except the effects in lists/effects.txt, which keep their level rules. A body that is reached only by edges out of trusted bodies is trusted too: trame is the runtime and its dependencies are its implementation. `Message::zeroed` (trame) calls bytemuck's `Zeroable::zeroed`, so bytemuck's body is trusted as trame's is. No crate is named in the rule.

A trusted body is read only for its effects, its edges, its allocations (in an item), and its user iterators. Its unsafe code, std use, loops and core calls are not judged, as trame's own never were.

Trust stops at the program. A body is judged in full when a root reaches it, or when an `Own` edge reaches it (scan.rs): a closure or fn item passed in as a generic argument, a user type's impl of a core trait, an impl item whose self type is a user type, drop glue, and a shim. A dependency function called directly from a worker is judged in full too, since the worker is not trusted. Trust does not depend on the path's length: a body is judged if any path to it is untrusted.

Why: the program can only reach trame and its dependencies through their public API, and the rules are about what the program does. Checking the runtime's body would fail on the runtime itself.

What it costs: a non-effect trusted function that does I/O, or unbounded work, goes unnoticed by the driver. The effect list is the only place a trame function's effect is named, so trame must keep such functions out of reach of the program: a new effect goes into effects.txt, and a host-only API (for example `Topology`) is harmless in a worker but useless there, so it should not be an effect. A review of trame's public API against effects.txt is the check that replaces the body reading. A dependency's unsafe code reached through trame is also not read; the review must cover it too. And the `Own` test is by self type, not by crate: an impl item of a dependency type that the program does not own is judged in full (over-refusal, which fails closed).

## The test

`cargo test` runs the rejection suite with the driver it has just built (tests/rejections.rs). Each case is checked against the real trame crate: a refused case must carry exactly the diagnostics its `//~ ERROR` annotations name, on their lines, and an accepted case none. The format is in rejections/README.md. There is no stand-in crate; the driver's effect paths must match lists/effects.txt, and the cases that call trame exercise them.

## Known approximations

- The checker reads optimized MIR, not source: asserts and dead code that optimization removed are invisible. Checked crates build with `-Zinline-mir=no`, so `for` keeps its `Iterator::next` call.
- Checked crates use `-Clink-dead-code` (eager collection, so `cargo check` also has roots) and every crate uses `-Zalways-encode-mir`. Both change compile time, not the output.
- Effect paths and allow-list entries are placeholders until the trame surface lands (lists/effects.txt says how renames fail).
- Counted loops: a loop counts if it is a `for` over Range, RangeInclusive, slice Iter/IterMut or array IntoIter, or one of these under `Rev` (`counted_source`, scan.rs), and its iterator is not mutated in the body. The mutation test is shallow (direct assignment or `&mut`).
- A default method of a core trait on a user type (`ne` of a derived `PartialEq`) is matched by its trait path, as `core::cmp::*` (`names_of`, scan.rs). Its body is core's, and the user code it calls is reached through the callee's trait bounds (`callable_traits`), so a user impl of a trait the callee does not bound is not walked. This assumes core reaches user code only through its bounds and `Drop`; specialization inside core could break that, and it holds on stable Rust because the traits core specializes on (`TrustedLen`, ...) cannot be implemented there. The reach also follows a fn item's signature (`map(mk)` with `mk -> X`), since a user type can appear only there. A user `Iterator` is refused as unbounded below Worker regardless.
- Box: a deref of a Box is the Box's own pointer, not a raw-pointer deref of the program's, and is not [unsafe]. The driver recognises the MIR shape it observed for this (`box_pointer`, scan.rs: a local whose only definition is a transmute of `b.0.0`, a field path user code cannot name). That is a matched shape, not a proof about std; `unsafe/box_deref_step_ok` and `allocation/vec_macro_step_ok` are the cases that pin it.
- Drop glue is compiler code (a shim): its raw derefs and union accesses are not [unsafe]. Its calls are scanned. A program's own `Drop` impl is an `Own` edge and is judged in full, so `Box<X>` with an unsafe `X::drop` is refused, and `Box<u32>` is not.
- Not implemented: the Lock rules (UnsafeCell only in Lock, no Lock in Lock) and the rest of spec.md section 3 outside the milestones.
- Calls with no readable body and not listed are refused (`allow-list`), so a missing body fails closed.
- Diagnostics point at MIR spans, which may be inside macro expansions.
