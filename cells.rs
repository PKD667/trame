//! The cells a step borrows, and the message types a process owns, as the runtime lends them to one
//! step (spec.md 5.2 Ownership, 5.3 Processes). The runtime holds `&mut` to every cell of a
//! `concurrent!` and runs one step at a time, so a step's `&mut` never overlap. The lookup from a
//! step parameter to its cell is by `TypeId`, and it uses `dyn Any` only here, on the runtime's
//! side. A step itself never sees a trait object.

extern crate std;

use core::any::{Any, TypeId};
use core::cell::RefCell;
use std::thread_local;

/// The cells of one `concurrent!`, lent for one step at a time. `refs[k]` is the cell whose type is
/// `tys[k].0`; `tys[k].1` is its name for messages.
pub struct Cells<'a, 'b> {
    refs: &'a mut [&'b mut dyn Any],
    tys: &'a [(TypeId, &'static str)],
}

impl<'a, 'b> Cells<'a, 'b> {
    /// Both arrays have the length `N` of the `concurrent!`'s cells, so their lengths agree by type.
    /// No check runs here: a worker's code calls this, and it may not reach a panic's formatting.
    pub fn new<const N: usize>(refs: &'a mut [&'b mut dyn Any; N], tys: &'a [(TypeId, &'static str); N]) -> Self {
        Cells { refs, tys }
    }

    /// The table of cells: each one's type and name, in the order the cells were given.
    pub fn types(&self) -> &'a [(TypeId, &'static str)] {
        self.tys
    }

    /// The position of the cell of type `T`. The runtime refuses a step whose parameter has no
    /// cell before any step runs, so a miss is a bug in the glue.
    pub fn index<T: Any>(&self) -> usize {
        let want = TypeId::of::<T>();
        self.tys
            .iter()
            .position(|(t, _)| *t == want)
            .unwrap_or_else(|| panic!("trame: no cell of type {}", core::any::type_name::<T>()))
    }
}

/// Borrow the cells at the positions `want`, all at once. Each position is distinct (the runtime
/// refuses two cells of one type), so the borrows are disjoint.
fn lend<'s, const K: usize>(refs: &'s mut [&mut dyn Any], want: [usize; K]) -> [&'s mut dyn Any; K] {
    let mut out: [Option<&'s mut dyn Any>; K] = [const { None }; K];
    for (k, r) in refs.iter_mut().enumerate() {
        if let Some(m) = want.iter().position(|&w| w == k) {
            out[m] = Some(&mut **r);
        }
    }
    out.map(|o| o.expect("trame: positions of a lend are distinct and in range"))
}

/// A tuple of cell types, borrowed together: `<(A, B) as Pick>::pick(cells)` is `(&mut A, &mut B)`.
pub trait Pick<'s> {
    type Out;
    fn pick(cells: &'s mut Cells<'_, '_>) -> Self::Out;
}

macro_rules! pick_tuple {
    ($($T:ident $v:ident),+) => {
        impl<'s, $($T: Any),+> Pick<'s> for ($($T,)+) {
            type Out = ($(&'s mut $T,)+);
            fn pick(cells: &'s mut Cells<'_, '_>) -> Self::Out {
                let want = [$(cells.index::<$T>()),+];
                let [$($v),+] = lend(&mut *cells.refs, want);
                ($($v.downcast_mut::<$T>().expect("trame: a cell has the type its position names"),)+)
            }
        }
    };
}

pick_tuple!(A a);
pick_tuple!(A a, B b);
pick_tuple!(A a, B b, C c);
pick_tuple!(A a, B b, C c, D d);

/// The most message types one process may own. A process beyond this is refused when it starts.
pub const MAX_OWNED: usize = 16;

/// The message types one process owns, each with its name for messages, filled from the front. The
/// macro `#[trame::process]` builds it, and `concurrent!` checks that no tag has two owners.
pub type Tags = [Option<(TypeId, &'static str)>; MAX_OWNED];

#[derive(Clone, Copy)]
struct Owned {
    tags: Tags,
}

thread_local! {
    /// The types the process whose step runs on this thread owns. `None` outside a `concurrent!`.
    static OWNED: RefCell<Option<Owned>> = const { RefCell::new(None) };
}

/// The owned types of the step being run. Dropping it restores the previous set, so a step that
/// starts another run nests correctly.
pub struct Owning {
    prev: Option<Owned>,
}

impl Owning {
    pub fn new(tags: Tags) -> Self {
        let prev = OWNED.with(|c| c.replace(Some(Owned { tags })));
        Owning { prev }
    }
}

impl Drop for Owning {
    fn drop(&mut self) {
        OWNED.with(|c| *c.borrow_mut() = self.prev);
    }
}

/// Whether the process whose step runs here owns the type `ty`. Outside a `concurrent!` every type
/// is allowed: the restriction is the concurrent's, not the worker's.
pub fn owns(ty: TypeId) -> bool {
    OWNED.with(|c| match *c.borrow() {
        None => true,
        Some(o) => o.tags.iter().flatten().any(|(t, _)| *t == ty),
    })
}

/// The type of a cell, for the runtime's table of cells.
pub fn cell_ty<T: 'static>(_cell: &T) -> (TypeId, &'static str) {
    (TypeId::of::<T>(), core::any::type_name::<T>())
}
