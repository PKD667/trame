// How a unit of work is run. `#[parallel]` rewrites a function into a driver over a list of items;
// `invoke!` is how that driver is called, and `run` (the selected backend's lowering) is what the
// driver calls. `concurrent!` steps persistent arms through the same `run`.

use core::marker::PhantomData;

use crate::contract::Tag;

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

    pub(crate) fn into_slots(self) -> &'a mut [T] {
        self.state
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
/// trame::invoke!(integrate, &pass, &frames, trame::Keyed::new(&mut slots[..]))?;
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

/// Which tags a `concurrent!` arm receives. The macro builds one per arm.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum Receive<'a> {
    Nothing,
    All,
    Only(&'a [Tag]),
}

impl Receive<'_> {
    fn names(self, tag: Tag) -> bool {
        match self {
            Receive::Nothing => false,
            Receive::All => true,
            Receive::Only(tags) => tags.contains(&tag),
        }
    }
}

/// Plain receives own all tags without embedding a borrowed slice in a device constant;
/// scoped arms still borrow the caller's ordered receive settings.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Owner<'a> {
    All,
    Scoped { arms: &'a [Receive<'a>], at: usize },
}

// A backend that carries frames uses all of this; one that carries none asks only `receives`.
#[allow(dead_code)]
impl<'a> Owner<'a> {
    /// A participant outside `concurrent!`, which owns every tag.
    pub(crate) const ALL: Owner<'static> = Owner::All;

    pub(crate) fn new(arms: &'a [Receive<'a>], at: usize) -> Self {
        Owner::Scoped { arms, at }
    }

    /// This arm's own setting.
    pub(crate) fn mine(self) -> Receive<'a> {
        match self {
            Owner::All => Receive::All,
            Owner::Scoped { arms, at } => arms[at],
        }
    }

    pub(crate) fn receives(self) -> bool {
        !matches!(self.mine(), Receive::Nothing)
    }

    pub(crate) fn owns(self, tag: Tag) -> bool {
        match self {
            Owner::All => true,
            Owner::Scoped { arms, at } => {
                arms[at].names(tag) && !arms[..at].iter().any(|earlier| earlier.names(tag))
            }
        }
    }

    /// Every tag is this arm's, so a receive need not look at tags.
    pub(crate) fn every(self) -> bool {
        match self {
            Owner::All => true,
            Owner::Scoped { arms, at } => {
                matches!(arms[at], Receive::All)
                    && arms[..at].iter().all(|earlier| matches!(earlier, Receive::Nothing))
            }
        }
    }
}

/// Fixes the closure the macro builds around an arm's `step(io)`; `Send` keeps it portable across
/// lowerings.
#[doc(hidden)]
#[inline(always)]
pub fn arm_io<F, E>(arm: F) -> F
where
    F: for<'p> FnMut(&mut crate::Io<'p>) -> Result<Step, E> + Send,
{
    arm
}

/// Step every arm until all are `Done` or one fails; the answer is the first `Err` in source
/// order among those recorded, and no step starts after one is.
///
/// Given a context, each arm takes the `&mut Io` it is lent for one step. `recv(..)` receives
/// every tag, `recv(A, B)` those tags, and a frame goes to the first arm in source order that
/// names its tag.
///
/// Arms are `#[process]` objects, moved in or lent as `&mut`, stepped by their `step` method.
///
/// ```ignore
/// trame::concurrent!(transport, intake, &mut delivery)?;
/// trame::concurrent!(cx; recv(..) => intake, delivery)?;
/// ```
#[macro_export]
macro_rules! concurrent {
    ($cx:expr; $($arms:tt)+) => {
        $crate::__concurrent!(@io $cx, __run [] [] [] [0] $($arms)+)
    };
    ($($arm:expr),+ $(,)?) => {
        $crate::__concurrent!(@arm __run [] [] [0] $($arm,)+)
    };
}

// Each process is bound once, before the first step, so its state persists; every level of the
// recursion names its bindings `process` and `arm`, and hygiene keeps the levels' bindings apart.
#[doc(hidden)]
#[macro_export]
macro_rules! __concurrent {
    (@arm $run:ident [$($made:tt)*] [$($steps:tt)*] [$($n:tt)*] $arm:expr, $($rest:tt)*) => {
        $crate::__concurrent!(@arm $run
            [$($made)* #[allow(unused_mut)] let mut process = $arm; process.__declared(); let mut arm = $crate::run::arm(|| process.step());]
            [$($steps)* $run.arm(&mut arm);]
            [$($n)* + 1]
            $($rest)*)
    };
    (@arm $run:ident [$($made:tt)*] [$($steps:tt)*] [$($n:tt)*]) => {{
        $($made)*
        $crate::run::concurrent::<_, _, { $($n)* }>(|$run| { $($steps)* })
    }};
    // Arms given a context: each also adds its `Receive` to the list the runner reads.
    (@io $cx:expr, $run:ident [$($made:tt)*] [$($steps:tt)*] [$($recv:tt)*] [$($n:tt)*]
        recv(..) => $arm:expr $(, $($rest:tt)*)?) => {
        $crate::__concurrent!(@io $cx, $run
            [$($made)* let mut process = $arm; process.__declared(); let mut arm = $crate::arm_io(|io| process.step(io));]
            [$($steps)* $run.arm(&mut arm);]
            [$($recv)* $crate::Receive::All,]
            [$($n)* + 1]
            $($($rest)*)?)
    };
    (@io $cx:expr, $run:ident [$($made:tt)*] [$($steps:tt)*] [$($recv:tt)*] [$($n:tt)*]
        recv($($tag:expr),+ $(,)?) => $arm:expr $(, $($rest:tt)*)?) => {
        $crate::__concurrent!(@io $cx, $run
            [$($made)* let tags = [$($tag),+]; let mut process = $arm; process.__declared(); let mut arm = $crate::arm_io(|io| process.step(io));]
            [$($steps)* $run.arm(&mut arm);]
            [$($recv)* $crate::Receive::Only(&tags),]
            [$($n)* + 1]
            $($($rest)*)?)
    };
    (@io $cx:expr, $run:ident [$($made:tt)*] [$($steps:tt)*] [$($recv:tt)*] [$($n:tt)*]
        $arm:expr $(, $($rest:tt)*)?) => {
        $crate::__concurrent!(@io $cx, $run
            [$($made)* let mut process = $arm; process.__declared(); let mut arm = $crate::arm_io(|io| process.step(io));]
            [$($steps)* $run.arm(&mut arm);]
            [$($recv)* $crate::Receive::Nothing,]
            [$($n)* + 1]
            $($($rest)*)?)
    };
    (@io $cx:expr, $run:ident [$($made:tt)*] [$($steps:tt)*] [$($recv:tt)*] [$($n:tt)*]) => {{
        $($made)*
        let receive = [$($recv)*];
        $crate::concurrent_io::<_, _, { $($n)* }>($cx, &receive, |$run| { $($steps)* })
    }};
}
