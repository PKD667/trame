//! The message types a step receives (spec.md 5.2 Ownership): set per thread for the duration of a
//! step, so `recv` of a type the step does not receive answers `None`.

extern crate std;

use core::any::TypeId;
use core::cell::RefCell;
use std::thread_local;

/// The most message types one step may receive.
pub const MAX_OWNED: usize = 16;

/// The message types one step receives, each with its name for messages, filled from the front.
/// `#[trame::step]` builds it, and the launch checks that no tag has two owners in one worker.
pub type Tags = [Option<(TypeId, &'static str)>; MAX_OWNED];

#[derive(Clone, Copy)]
struct Owned {
    tags: Tags,
}

thread_local! {
    /// The types the step running on this thread receives. `None` outside a step.
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

/// Whether the step running here receives the type `ty`. Outside a step every type is allowed.
pub fn owns(ty: TypeId) -> bool {
    OWNED.with(|c| match *c.borrow() {
        None => true,
        Some(o) => o.tags.iter().flatten().any(|(t, _)| *t == ty),
    })
}
