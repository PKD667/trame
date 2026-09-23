// How a unit of work is run. `#[parallel]` rewrites a function into a driver over a list of items;
// `invoke!` is how that driver is called, and `run` (the selected backend's lowering) is what the
// driver calls. `concurrent!` steps persistent arms through the same `run`.

use core::marker::PhantomData;

/// The first parameter of every driver. A function marked `#[parallel]` cannot be called by its
/// own name without one, and `invoke!` is what makes one.
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

/// What one step of a `concurrent!` arm did, so the runner knows whether to step it again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Progress,
    /// Nothing could advance; on CPU the arm's thread yields before its next step.
    Idle,
    /// Never stepped again.
    Done,
}

/// Step every arm until all are `Done` or one fails; the answer is the first `Err` in source
/// order among those recorded, and no step starts after one is.
///
/// ```ignore
/// trame::concurrent! { || intake(&mut a), || delivery(&mut b) }?;
/// ```
#[macro_export]
macro_rules! concurrent {
    ($($arm:expr),+ $(,)?) => {
        $crate::__concurrent!(@arm __run [] [] [0] $($arm,)+)
    };
}

// Each zero-argument arm is bound once, before the first step, so its state persists; every level
// of the recursion names its binding `arm`, and hygiene keeps the levels' bindings apart.
#[doc(hidden)]
#[macro_export]
macro_rules! __concurrent {
    (@arm $run:ident [$($made:tt)*] [$($steps:tt)*] [$($n:tt)*] $arm:expr, $($rest:tt)*) => {
        $crate::__concurrent!(@arm $run
            [$($made)* let mut arm = $crate::run::arm($arm);]
            [$($steps)* $run.arm(&mut arm);]
            [$($n)* + 1]
            $($rest)*)
    };
    (@arm $run:ident [$($made:tt)*] [$($steps:tt)*] [$($n:tt)*]) => {{
        $($made)*
        $crate::run::concurrent::<_, _, { $($n)* }>(|$run| { $($steps)* })
    }};
}
