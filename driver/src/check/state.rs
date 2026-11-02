//! State memory: what a `#[trame::state]` struct and its pool type may hold (CONTRACT.md, M2, M3).
//!
//! The state macro calls `trame::__check::state::<S>()`, and `pool::<P>()` when it names a pool
//! (check.rs). Each instance is one type, read here: its fields are walked through user structs,
//! enums, tuples, arrays and slices, and through the generic arguments of the types of core, alloc and
//! std (`Vec<T>`, `Box<T>`, `Option<T>`), whose own fields (a `Vec`'s raw pointer) belong to the
//! container and are not read. A type that breaks M2 (it means something only in the process or device
//! that made it) or M3 (a field changes behind its lock) is refused at the state's field.
//!
//! What this does not read: a closure or a pointer that a user type hides behind an opaque type, and
//! the address-dependent meaning of a value that is plain data (a `usize` that holds an address, a
//! `HashMap`'s per-process seed). The check is on types, not on values.

use rustc_data_structures::fx::FxHashSet;
use rustc_middle::ty::{self, Ty, TyCtxt, TypingEnv, Unnormalized};
use rustc_span::{DUMMY_SP, Span};

use super::scan::canon;

/// Which marker named the type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    State,
    Pool,
}

/// The clause of backend.rs that a refusal cites.
#[derive(Clone, Copy)]
enum Clause {
    M2,
    M3,
}

impl Clause {
    fn name(self) -> &'static str {
        match self {
            Clause::M2 => "M2",
            Clause::M3 => "M3",
        }
    }
}

struct Cx<'tcx> {
    tcx: TyCtxt<'tcx>,
    /// The user types being or already walked: a recursive type (`Option<Box<Node>>`) ends here.
    seen: FxHashSet<Ty<'tcx>>,
    /// The span a refusal points at: the state's field, or the pool's.
    span: Span,
    errors: usize,
    /// The refusals already made, so one field that holds a thing twice says it once.
    said: FxHashSet<String>,
}

/// Check the state type, or pool type, `ty`. Returns the number of errors emitted.
pub fn check<'tcx>(tcx: TyCtxt<'tcx>, _held: Held, ty: Ty<'tcx>) -> usize {
    let mut cx = Cx { tcx, seen: FxHashSet::default(), span: DUMMY_SP, errors: 0, said: FxHashSet::default() };
    match ty.kind() {
        ty::Adt(adt, args) if adt.is_struct() && !is_std(tcx, adt.did()) => {
            let name = tcx.item_name(adt.did());
            // The state's fields are the places a refusal points at; so are a user pool's.
            cx.seen.insert(ty);
            for field in adt.all_fields() {
                cx.span = tcx.def_span(field.did);
                let fty = normalize(tcx, field.ty(tcx, args));
                cx.walk(fty, &format!("{name}.{}", field.name), fty);
            }
        }
        _ => {
            // A pool that is not a user struct (`Vec<u32>`, `[Cell<u8>; 2]`): one place, the type itself.
            cx.span = match ty.kind() {
                ty::Adt(adt, _) => tcx.def_span(adt.did()),
                _ => DUMMY_SP,
            };
            cx.walk(ty, "pool", ty);
        }
    }
    cx.errors
}

fn normalize<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Ty<'tcx> {
    tcx.normalize_erasing_regions(TypingEnv::fully_monomorphized(), Unnormalized::new(ty))
}

/// Defined in core, alloc or std.
fn is_std(tcx: TyCtxt<'_>, def: rustc_span::def_id::DefId) -> bool {
    matches!(tcx.crate_name(def.krate).as_str(), "core" | "alloc" | "std")
}

/// What the types of core, alloc and std refused whole say: a path prefix, what it holds, the clause.
/// A prefix ends in `::` and matches a module; the others are exact type paths.
const REFUSED: &[(&str, &str, Clause)] = &[
    ("core::cell::", "interior mutability", Clause::M3),
    ("core::sync::atomic::", "an atomic", Clause::M3),
    ("std::sync::", "a lock or a shared cell", Clause::M3),
    ("alloc::rc::", "an Rc (a reference count in one process)", Clause::M2),
    ("alloc::sync::", "an Arc (a reference count in one process)", Clause::M2),
    ("core::ptr::non_null::NonNull", "a raw pointer", Clause::M2),
    ("core::ptr::unique::Unique", "a raw pointer", Clause::M2),
    ("std::fs::", "a file handle", Clause::M2),
    ("std::net::", "a socket", Clause::M2),
    ("std::os::", "an operating-system handle", Clause::M2),
    ("std::process::", "a process handle", Clause::M2),
    ("std::thread::", "a thread handle", Clause::M2),
    ("std::io::", "an I/O handle", Clause::M2),
    ("std::time::Instant", "an instant of one process's clock", Clause::M2),
];

impl<'tcx> Cx<'tcx> {
    /// Walk `ty`, which lies under the field `path` (whose type is `field`).
    fn walk(&mut self, ty: Ty<'tcx>, path: &str, field: Ty<'tcx>) {
        let tcx = self.tcx;
        match ty.kind() {
            ty::FnPtr(..) => self.hit(path, field, "a function pointer (a code address)", Some(ty), Clause::M2),
            ty::Closure(..) | ty::CoroutineClosure(..) | ty::Coroutine(..) => {
                self.hit(path, field, "a closure (a code address and its captures)", Some(ty), Clause::M2)
            }
            ty::Dynamic(..) => self.hit(path, field, "a trait object (a vtable address)", Some(ty), Clause::M2),
            ty::Ref(..) => self.hit(path, field, "a reference (an address in one process)", Some(ty), Clause::M2),
            ty::RawPtr(..) => self.hit(path, field, "a raw pointer", Some(ty), Clause::M2),
            ty::Array(t, _) | ty::Slice(t) => self.walk(*t, path, field),
            ty::Tuple(ts) => {
                for (i, t) in ts.iter().enumerate() {
                    self.walk(t, &format!("{path}.{i}"), if ts.len() == 1 { field } else { t });
                }
            }
            ty::Adt(adt, args) => {
                let name = canon(tcx, adt.did());
                if let Some((_, what, clause)) = REFUSED.iter().find(|(p, _, _)| {
                    if p.ends_with("::") { name.starts_with(p) } else { name == *p }
                }) {
                    return self.hit(path, field, what, Some(ty), *clause);
                }
                if is_std(tcx, adt.did()) {
                    // A container of core, alloc or std: its fields are its own; what it holds is its
                    // arguments. A `PhantomData` holds nothing.
                    if name == "core::marker::PhantomData" {
                        return;
                    }
                    for t in args.types() {
                        self.walk(normalize(tcx, t), path, field);
                    }
                    return;
                }
                if !self.seen.insert(ty) {
                    return;
                }
                for variant in adt.variants() {
                    for f in &variant.fields {
                        let fty = normalize(tcx, f.ty(tcx, args));
                        self.walk(fty, &format!("{path}.{}", f.name), fty);
                    }
                }
            }
            _ => {}
        }
    }

    /// `offender` is the type found, named when it is not the field's own type (a `Cell` inside a `Vec`).
    fn hit(&mut self, path: &str, field: Ty<'tcx>, what: &str, offender: Option<Ty<'tcx>>, clause: Clause) {
        let inside = match offender {
            Some(o) if o != field => format!(", found `{o}`"),
            _ => String::new(),
        };
        let msg = format!("[state-memory] {path}: {field} holds {what}{inside} (backend.rs {})", clause.name());
        if self.said.insert(msg.clone()) {
            self.tcx.dcx().struct_span_err(self.span, msg).emit();
            self.errors += 1;
        }
    }
}
