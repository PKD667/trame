//! The abstract programs the fuzzer generates: the language of driver/fuzz/README.md, "Abstract
//! language". A program is named functions (`funcs`) and roots. Every statement reads the one
//! parameter `x: u32`, so that no condition is a constant the optimizer could fold.

use crate::rng::{mix, Rng};

/// A level of spec.md section 3. Smaller is stricter (the order the driver takes the minimum in).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Item,
    Step,
    Worker,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Item, Level::Step, Level::Worker];

    /// The word the driver uses for the level in `error: [rule] <word> ...`.
    pub fn noun(self) -> &'static str {
        match self {
            Level::Item => "item",
            Level::Step => "step",
            Level::Worker => "worker",
        }
    }

    pub fn parse(s: &str) -> Option<Level> {
        Level::ALL.into_iter().find(|l| l.noun() == s)
    }
}

/// A forbidden construct: a statement that breaks a rule at some levels (the table in the README).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forb {
    Println,
    Spawn,
    Static,
    Cell,
    DynCall,
    FnPtr,
    Unsafe,
    Send,
    Recv,
    Now,
    Barrier,
    Concurrent,
    VecPush,
    RangeFromForEach,
}

impl Forb {
    pub const ALL: [Forb; 14] = [
        Forb::Println,
        Forb::Spawn,
        Forb::Static,
        Forb::Cell,
        Forb::DynCall,
        Forb::FnPtr,
        Forb::Unsafe,
        Forb::Send,
        Forb::Recv,
        Forb::Now,
        Forb::Barrier,
        Forb::Concurrent,
        Forb::VecPush,
        Forb::RangeFromForEach,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Forb::Println => "println",
            Forb::Spawn => "spawn",
            Forb::Static => "static",
            Forb::Cell => "cell",
            Forb::DynCall => "dyn",
            Forb::FnPtr => "fnptr",
            Forb::Unsafe => "unsafe",
            Forb::Send => "send",
            Forb::Recv => "recv",
            Forb::Now => "now",
            Forb::Barrier => "barrier",
            Forb::Concurrent => "concurrent",
            Forb::VecPush => "vecpush",
            Forb::RangeFromForEach => "rangefrom",
        }
    }

    /// The levels at which the construct is refused (spec.md section 3 and the lists).
    pub fn levels(self) -> &'static [Level] {
        use Level::*;
        match self {
            Forb::Println | Forb::Spawn | Forb::Static | Forb::Cell | Forb::DynCall | Forb::FnPtr | Forb::Unsafe => {
                &[Item, Step, Worker]
            }
            Forb::Send | Forb::Recv | Forb::Now | Forb::VecPush => &[Item],
            Forb::Barrier | Forb::Concurrent | Forb::RangeFromForEach => &[Item, Step],
        }
    }

    /// The rule ids the refusal carries (the `[rule]` of the driver's error line).
    /// println has two: `std` for `_print`, and `unsafe` for `core::fmt::Arguments::new`, an unsafe
    /// fn on the pinned nightly (checked by a probe; see driver/fuzz/README.md).
    pub fn rules(self) -> &'static [&'static str] {
        match self {
            Forb::Println => &["std", "unsafe"],
            Forb::Spawn => &["std"],
            Forb::Static => &["static"],
            Forb::Cell => &["deny"],
            Forb::DynCall | Forb::FnPtr => &["indirect"],
            Forb::Unsafe => &["unsafe"],
            Forb::Send | Forb::Recv | Forb::Now | Forb::Barrier | Forb::Concurrent => &["effects"],
            Forb::VecPush => &["allocation"],
            Forb::RangeFromForEach => &["deny"],
        }
    }
}

/// An allowed construct: it must not change the verdict at any level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Allow {
    /// `x.wrapping_mul(3).wrapping_add(1)`
    Arith,
    /// `Some(x).map(|y| y.wrapping_add(2))`
    OptMap,
    /// indexing an array with `x & 3`
    Index,
    /// `trame::math::exp`
    Exp,
    /// `x.min(7)`
    Min,
}

impl Allow {
    pub const ALL: [Allow; 5] = [Allow::Arith, Allow::OptMap, Allow::Index, Allow::Exp, Allow::Min];
}

/// A wrapper: it moves a statement into another function, closure or impl. The first ten are the
/// wrappers of the theory (README, property 3). `SumForeign` is an extra one, outside the theory:
/// a user impl of a core trait for a core type, called by core (`impl Sum<U> for u32`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    Helper,
    Generic,
    ForEach,
    IterMap,
    OptionMap,
    Trait,
    Drop,
    Add,
    Deref,
    Into,
    SumForeign,
}

impl Wrap {
    pub const NORMATIVE: [Wrap; 10] = [
        Wrap::Helper,
        Wrap::Generic,
        Wrap::ForEach,
        Wrap::IterMap,
        Wrap::OptionMap,
        Wrap::Trait,
        Wrap::Drop,
        Wrap::Add,
        Wrap::Deref,
        Wrap::Into,
    ];
    pub const ALL: [Wrap; 11] = [
        Wrap::Helper,
        Wrap::Generic,
        Wrap::ForEach,
        Wrap::IterMap,
        Wrap::OptionMap,
        Wrap::Trait,
        Wrap::Drop,
        Wrap::Add,
        Wrap::Deref,
        Wrap::Into,
        Wrap::SumForeign,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Wrap::Helper => "helper",
            Wrap::Generic => "generic",
            Wrap::ForEach => "foreach",
            Wrap::IterMap => "itermap",
            Wrap::OptionMap => "optmap",
            Wrap::Trait => "trait",
            Wrap::Drop => "drop",
            Wrap::Add => "add",
            Wrap::Deref => "deref",
            Wrap::Into => "into",
            Wrap::SumForeign => "sumforeign(extra)",
        }
    }

    pub fn is_extra(self) -> bool {
        self == Wrap::SumForeign
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    /// A forbidden construct.
    Forb(Forb),
    /// `while w != 0 { w >>= 1; body }`: not a counted loop (Definition (Counted loop)).
    While(Vec<Stmt>),
    Allow(Allow),
    /// A call of function `funcs[j]`.
    Call(usize),
    /// `for _ in 0..(x & 3) { body }`: a counted loop.
    For(Vec<Stmt>),
    /// `if x & mask == 0 { a } else { b }`.
    If(u32, Vec<Stmt>, Vec<Stmt>),
    Wrap(Wrap, Vec<Stmt>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Root {
    pub level: Level,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Func {
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prog {
    pub seed: u64,
    pub index: u64,
    pub funcs: Vec<Func>,
    pub roots: Vec<Root>,
}

/// The canonical path the driver prints for root `k` (`reached from root X`): a fn root is its name,
/// a step root is the inherent `step` method of its process type.
pub fn root_name(k: usize, level: Level) -> String {
    match level {
        Level::Step => format!("P{k}::step"),
        _ => format!("r{k}"),
    }
}

/// Knobs of the generator. `extra` adds the wrappers outside the theory.
#[derive(Clone, Copy, Debug)]
pub struct GenOpts {
    pub extra: bool,
}

const MAX_DEPTH: u32 = 3;

struct Gen {
    nfuncs: usize,
    opts: GenOpts,
}

impl Gen {
    fn list(&self, rng: &mut Rng, depth: u32, n: u64) -> Vec<Stmt> {
        (0..n).map(|_| self.stmt(rng, depth)).collect()
    }

    fn stmt(&self, rng: &mut Rng, depth: u32) -> Stmt {
        // Weights: forbidden, allowed, call, for, while, if, wrap. Nesting stops at MAX_DEPTH.
        let deep = depth >= MAX_DEPTH;
        let w = [
            24,
            20,
            if self.nfuncs > 0 { 10 } else { 0 },
            if deep { 0 } else { 7 },
            if deep { 0 } else { 5 },
            if deep { 0 } else { 7 },
            if deep { 0 } else { 16 },
        ];
        match rng.weighted(&w) {
            0 => Stmt::Forb(rng.pick(&Forb::ALL)),
            1 => Stmt::Allow(rng.pick(&Allow::ALL)),
            2 => Stmt::Call(rng.below(self.nfuncs as u64) as usize),
            3 => {
                let n = 1 + rng.below(2);
                Stmt::For(self.list(rng, depth + 1, n))
            }
            4 => {
                let n = 1 + rng.below(2);
                Stmt::While(self.list(rng, depth + 1, n))
            }
            5 => {
                let mask = 1u32 << rng.below(8);
                let a = 1 + rng.below(2);
                let b = rng.below(2);
                Stmt::If(mask, self.list(rng, depth + 1, a), self.list(rng, depth + 1, b))
            }
            _ => {
                let kinds: &[Wrap] = if self.opts.extra { &Wrap::ALL } else { &Wrap::NORMATIVE };
                let k = rng.pick(kinds);
                let n = 1 + rng.below(2);
                Stmt::Wrap(k, self.list(rng, depth + 1, n))
            }
        }
    }
}

/// The program with index `index` of the run seeded `seed`. Deterministic: the same arguments give
/// the same program, so a shrunk case or a single program can be regenerated from its seed.
pub fn generate(seed: u64, index: u64, opts: GenOpts) -> Prog {
    let mut rng = Rng::new(mix(seed, index));
    let nfuncs = rng.below(4) as usize;
    let nroots = 1 + rng.below(3) as usize;
    let g = Gen { nfuncs, opts };
    let mut funcs = Vec::new();
    for _ in 0..nfuncs {
        let n = 1 + rng.below(3);
        funcs.push(Func { body: g.list(&mut rng, 0, n) });
    }
    let mut roots = Vec::new();
    for _ in 0..nroots {
        let level = rng.pick(&Level::ALL);
        let n = 1 + rng.below(4);
        roots.push(Root { level, body: g.list(&mut rng, 0, n) });
    }
    Prog { seed, index, funcs, roots }
}

/// Size of a program: statements, roots and functions. The shrinker only ever lowers it.
pub fn size(p: &Prog) -> usize {
    fn count(list: &[Stmt]) -> usize {
        list.iter()
            .map(|s| {
                1 + match s {
                    Stmt::While(b) | Stmt::For(b) | Stmt::Wrap(_, b) => count(b),
                    Stmt::If(_, a, b) => count(a) + count(b),
                    _ => 0,
                }
            })
            .sum()
    }
    p.funcs.iter().map(|f| count(&f.body)).sum::<usize>()
        + p.roots.iter().map(|r| count(&r.body) + 1).sum::<usize>()
        + p.funcs.len()
}
