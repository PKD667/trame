# trame-driver

Trame's compile-time checker (spec.md, Compile time): a rustc driver, and `cargo trame`, which runs cargo with it.
The driver reads the optimized MIR of each root marker (`trame::__check::{item,step,worker}`, check.rs) and everything reachable from it, and refuses code that breaks the rules. Refusals are rustc errors: `error: [rule] <level> ...`.

Run (from driver/; the compiler is pinned in driver/rust-toolchain.toml and built by flake.nix):

    nix develop "path:$PWD/..#driver" --command cargo test

On a crate: `nix build "path:$PWD/..#trame-driver"`, put its `bin/` on PATH, then `cargo trame check|build|test` in that crate. Checked builds use `target/trame`, apart from plain cargo's `target/`.
Debug: `TRAME_DRIVER_DEBUG=1` prints each root and its level.

## Checks

Per level (Item, Step, Worker; spec.md, Requirement per level), for every function reached from a root:
- Unsafe code, FFI, indirect calls, statics: refused at every level.
- Effects (lists/effects.txt): each effect is allowed at the levels spec.md names for it.
- Core and alloc callees: judged by the lists (below).
- Counted loops (Item, Step), recursion (Item, Step), allocation (Item: no reach to `alloc::alloc` or `__rust_alloc`).

A function reached from several roots is checked at the strictest level that reaches it.

## The lists (lists/)

Each line is a path, or `P::*` for every item under `P`, then a `# reason`. A `-` line is a deny at one level.
- `deny.txt`: refused at every level, and it overrides any allow. Interior mutability, locks, `Rc`, raw pointers, hints, intrinsics, arch.
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

`cargo test` runs `rejections/run.sh` (tests/rejections.rs). Each case there is checked against the real trame crate: accepted cases must compile, refused cases must be refused by exactly the rule and level they name, and macro refusals must match their message. The format and the list of known gaps are in rejections/README.md. There is no stand-in crate; the driver's effect paths must match lists/effects.txt, and the cases that call trame exercise them.

## Known approximations

- The checker reads optimized MIR, not source: asserts and dead code that optimization removed are invisible. Checked crates build with `-Zinline-mir=no`, so `for` keeps its `Iterator::next` call.
- Checked crates use `-Clink-dead-code` (eager collection, so `cargo check` also has roots) and every crate uses `-Zalways-encode-mir`. Both change compile time, not the output.
- Effect paths and allow-list entries are placeholders until the trame surface lands (lists/effects.txt says how renames fail).
- Counted loops: a loop counts if it is a `for` over Range, RangeInclusive, slice Iter/IterMut or array IntoIter, and its iterator is not mutated in the body. The mutation test is shallow (direct assignment or `&mut`).
- A trait method on a user type is matched by its self-type shape only, not by its trait path (`names_of` in scan.rs). So a core default method the user did not override (e.g. `Iterator::for_each` on a user iterator) is not in any list and is refused. This fails closed; it is a refusal the user may not expect.
- Not implemented: the Lock rules (UnsafeCell only in Lock, no Lock in Lock) and the rest of spec.md section 3 outside the milestones.
- Calls with no readable body and not listed are refused (`allow-list`), so a missing body fails closed.
- Diagnostics point at MIR spans, which may be inside macro expansions.
