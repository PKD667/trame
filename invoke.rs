// How a unit of work is run. `#[parallel]` and `#[concurrent]` rewrite a function into a driver
// over a list of items; `invoke!` is how that driver is called, and `run` (the selected backend's
// lowering) is what the driver calls.

use core::marker::PhantomData;

/// The first parameter of every driver. A function marked `#[parallel]` or `#[concurrent]` cannot
/// be called by its own name without one, and `invoke!` is what makes one.
pub struct Invocation(());

impl Invocation {
    #[doc(hidden)]
    #[inline(always)]
    pub const fn __new() -> Self {
        Invocation(())
    }
}

/// Mutable state an `#[ordered]` function reaches only through its key.
///
/// `K` is the key's type as the `#[ordered]` attribute names it, so a view built for one key type
/// cannot be handed to a function ordered by another.
pub struct Keyed<'a, K, T> {
    state: &'a mut [T],
    key: PhantomData<fn(K)>,
}

impl<'a, K: Copy + Into<usize>, T> Keyed<'a, K, T> {
    pub fn new(state: &'a mut [T]) -> Self {
        Keyed {
            state,
            key: PhantomData,
        }
    }

    pub(crate) fn slot<E>(&mut self, key: K) -> Result<&mut T, Invoked<E>> {
        let len = self.state.len();
        let key = key.into();
        self.state
            .get_mut(key)
            .ok_or(Invoked::OutOfRange { key, len })
    }
}

/// Why a keyed invocation failed: the body's own error, or an item whose key names no slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invoked<E> {
    Failed(E),
    OutOfRange { key: usize, len: usize },
}

/// Run every item of `items` through `f`, join them all, and return the first `Err` in list order.
///
/// ```ignore
/// trame::invoke!(self.integrate, &mut pass, &frames, trame::Keyed::new(&mut slots[..]))?;
/// trame::invoke!(self.worker, &shared, &[Role::Intake, Role::Delivery])?;
/// ```
#[macro_export]
macro_rules! invoke {
    ($receiver:ident . $f:ident, $cx:expr, $items:expr $(, $keyed:expr)? $(,)?) => {
        $receiver.$f($crate::Invocation::__new(), $cx, $items $(, $keyed)?)
    };
    ($f:path, $cx:expr, $items:expr $(, $keyed:expr)? $(,)?) => {
        $f($crate::Invocation::__new(), $cx, $items $(, $keyed)?)
    };
}
