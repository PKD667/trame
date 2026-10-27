//! Level-independent facts about one monomorphized function: what it calls, which unsafe
//! operations and statics it touches, and how its loops are governed. The rules in `mod.rs` decide
//! what each fact means at each level, so a function is scanned once however many levels reach it.

use rustc_data_structures::fx::{FxHashMap, FxHashSet};
use rustc_hir::BlockCheckMode;
use rustc_hir::UnsafeSource;
use rustc_hir::intravisit::{self, Visitor};
use rustc_middle::mir::visit::{MutatingUseContext, PlaceContext, Visitor as MirVisitor};
use rustc_middle::mir::*;
use rustc_middle::ty::adjustment::PointerCoercion;
use rustc_middle::ty::{self, GenericArg, GenericArgKind, GenericArgsRef, GenericParamDefKind, Instance, InstanceKind, Ty, TyCtxt, TypeFoldable, TypingEnv};
use rustc_span::Span;
use rustc_span::def_id::DefId;

use super::lists::Effect;

/// One fact about a body. Spans point at the offending call, block, or loop.
#[derive(Debug)]
pub enum What<'tcx> {
    /// An `unsafe` block, an `unsafe fn`, or an unsafe operation (raw dereference, union field,
    /// call of an unsafe function, inline assembly).
    Unsafe(String),
    /// A call into a foreign (`extern`) function.
    Ffi(String),
    /// A call that cannot be resolved statically: a function pointer or a trait object.
    Indirect(String),
    /// A static or thread-local static.
    Static(String),
    /// A call into `std`, which carries threads, OS access, and allocation this checker cannot see.
    Std(String),
    /// A trame effect.
    Effect(Effect, String),
    /// A call into `core` or `alloc`, judged by the lists. `key` names it in messages; `names` are the
    /// paths the lists match (`Lists::verdict`). A core body is a leaf: it is never read.
    Core { key: String, names: Vec<String>, alloc: bool },
    /// An allocation reached through trame, which is trusted: refused only in an item.
    TrustedAlloc(String),
    /// A type reached by a core or trame call (in its generic arguments or a field): refused when the
    /// lists deny it at the level, as `Cell` anywhere.
    Type(String),
    /// A user type whose `Iterator` impl core code would drive without a bound.
    Unbounded(String),
    /// A call that the driver checks by reading its body.
    Edge(Instance<'tcx>),
    /// A call the driver could not resolve or read.
    Unresolved(String),
    /// A loop: a back-edge `header <- body`. `counted` is the spec's Definition (Counted loop).
    Loop { counted: bool, reason: String },
}

#[derive(Debug)]
pub struct Site<'tcx> {
    pub span: Span,
    pub what: What<'tcx>,
}

#[derive(Debug, Default)]
pub struct Scan<'tcx> {
    pub sites: Vec<Site<'tcx>>,
    /// A trame body: only its effects, its edges and its allocations are kept (see `trusted_sites`).
    pub trusted: bool,
}

impl<'tcx> Scan<'tcx> {
    /// Callees the driver reads itself: the edges of the call graph.
    pub fn edges(&self) -> impl Iterator<Item = (Instance<'tcx>, Span)> + '_ {
        self.sites.iter().filter_map(|s| match s.what {
            What::Edge(inst) => Some((inst, s.span)),
            _ => None,
        })
    }
}

/// The impls of core, alloc and std traits, by the user ADT they are for (through references).
/// Core and alloc name only their own traits, so a user type's code runs inside them only through
/// these impls (plus closures, fn items and trait objects, which `reach` follows).
pub struct Impls {
    by_adt: FxHashMap<DefId, Vec<DefId>>,
    iterator: Option<DefId>,
}

pub fn core_impls(tcx: TyCtxt<'_>) -> Impls {
    let mut by_adt: FxHashMap<DefId, Vec<DefId>> = FxHashMap::default();
    for tr in tcx.all_traits_including_private() {
        if !is_std(tcx, tr) {
            continue;
        }
        for imp in tcx.trait_impls_of(tr).non_blanket_impls().values().flatten() {
            if let ty::Adt(adt, _) = tcx.type_of(*imp).skip_binder().peel_refs().kind() {
                by_adt.entry(adt.did()).or_default().push(*imp);
            }
        }
    }
    Impls { by_adt, iterator: tcx.lang_items().iterator_trait() }
}

/// Defined in core, alloc or std.
fn is_std(tcx: TyCtxt<'_>, def: DefId) -> bool {
    matches!(tcx.crate_name(def.krate).as_str(), "core" | "alloc" | "std")
}

/// Defined in trame, the runtime: trusted at every level (driver/README.md).
fn is_trame(tcx: TyCtxt<'_>, def: DefId) -> bool {
    tcx.crate_name(def.krate).as_str() == "trame"
}

/// Defined in the program: not core, alloc, std or trame.
pub fn is_user(tcx: TyCtxt<'_>, def: DefId) -> bool {
    !is_std(tcx, def) && !is_trame(tcx, def)
}

fn is_user_ty(tcx: TyCtxt<'_>, t: Ty<'_>) -> bool {
    matches!(t.peel_refs().kind(), ty::Adt(adt, _) if is_user(tcx, adt.did()))
}

/// The sites a trame body keeps. Trame is trusted, so its unsafe, std, loops and core calls are not
/// judged, with two exceptions: its effects keep their level rules, and its allocations are refused
/// in an item (both are reached through trame's internals, so they cannot be hidden there).
fn trusted_sites(sites: Vec<Site<'_>>) -> Vec<Site<'_>> {
    sites
        .into_iter()
        .filter_map(|s| {
            let what = match s.what {
                What::Core { key, alloc: true, .. } => What::TrustedAlloc(key),
                w @ (What::Effect(..) | What::Edge(_) | What::TrustedAlloc(_) | What::Unbounded(_)) => w,
                _ => return None,
            };
            Some(Site { span: s.span, what })
        })
        .collect()
}

/// Unify the impl's self type `pat` (with parameters) with the type `t`, filling `out` by parameter.
fn unify<'tcx>(tcx: TyCtxt<'tcx>, pat: Ty<'tcx>, t: Ty<'tcx>, out: &mut [Option<GenericArg<'tcx>>]) -> bool {
    match (pat.kind(), t.kind()) {
        (ty::Param(p), _) => {
            let g: GenericArg<'tcx> = t.into();
            match out[p.index as usize] {
                Some(prev) => prev == g,
                None => {
                    out[p.index as usize] = Some(g);
                    true
                }
            }
        }
        (ty::Adt(d1, a1), ty::Adt(d2, a2)) => {
            d1 == d2 && a1.len() == a2.len() && a1.iter().zip(a2.iter()).all(|(x, y)| unify_arg(tcx, x, y, out))
        }
        (ty::Ref(_, p, m1), ty::Ref(_, c, m2)) => m1 == m2 && unify(tcx, *p, *c, out),
        (ty::Tuple(p), ty::Tuple(c)) => p.len() == c.len() && p.iter().zip(c.iter()).all(|(x, y)| unify(tcx, x, y, out)),
        (ty::Slice(p), ty::Slice(c)) | (ty::Array(p, _), ty::Array(c, _)) => unify(tcx, *p, *c, out),
        _ => pat == t,
    }
}

fn unify_arg<'tcx>(tcx: TyCtxt<'tcx>, x: GenericArg<'tcx>, y: GenericArg<'tcx>, out: &mut [Option<GenericArg<'tcx>>]) -> bool {
    match (x.kind(), y.kind()) {
        (GenericArgKind::Type(p), GenericArgKind::Type(c)) => unify(tcx, p, c, out),
        (GenericArgKind::Lifetime(_), GenericArgKind::Lifetime(_)) => true,
        (GenericArgKind::Const(a), GenericArgKind::Const(b)) => a == b,
        _ => false,
    }
}

/// The generic arguments of impl `imp` when it is for the type `t`. `Ok(None)`: it is for another
/// type. `Err`: it is for this type but has parameters the driver cannot infer, so it cannot be read.
fn impl_args<'tcx>(tcx: TyCtxt<'tcx>, imp: DefId, t: Ty<'tcx>) -> Result<Option<GenericArgsRef<'tcx>>, String> {
    let generics = tcx.generics_of(imp);
    let mut out = vec![None; generics.count()];
    if !unify(tcx, tcx.type_of(imp).skip_binder(), t, &mut out) {
        return Ok(None);
    }
    let mut built: Vec<GenericArg<'tcx>> = Vec::new();
    for p in &generics.own_params {
        match p.kind {
            GenericParamDefKind::Lifetime => built.push(tcx.lifetimes.re_erased.into()),
            GenericParamDefKind::Type { .. } => match out[p.index as usize] {
                Some(a) => built.push(a),
                None => return Err(format!("an impl for `{t}` has a type parameter the driver cannot infer")),
            },
            GenericParamDefKind::Const { .. } => return Err(format!("an impl for `{t}` has a const parameter")),
        }
    }
    Ok(Some(tcx.mk_args(&built)))
}

/// The canonical definition path of `def`, e.g. `core::ops::range::Range`. Allow-lists and the
/// counted-loop test match on it; `def_path_str` alone prints the shortest visible path instead.
pub fn canon(tcx: TyCtxt<'_>, def: rustc_span::def_id::DefId) -> String {
    rustc_middle::ty::print::with_no_visible_paths!(tcx.def_path_str(def))
}

/// Monomorphize `value` for `inst`'s body.
pub fn mono<'tcx, T: TypeFoldable<TyCtxt<'tcx>>>(tcx: TyCtxt<'tcx>, inst: Instance<'tcx>, value: T) -> T {
    inst.instantiate_mir_and_normalize_erasing_regions(tcx, TypingEnv::fully_monomorphized(), ty::EarlyBinder::bind(value))
}

/// Trait-method or inherent-method keys: `<self shape>::<method>`, or the path of a free function.
pub fn key<'tcx>(tcx: TyCtxt<'tcx>, def: rustc_span::def_id::DefId, args: GenericArgsRef<'tcx>) -> String {
    let name = tcx.item_name(def).to_string();
    if tcx.trait_of_assoc(def).is_some() {
        format!("{}::{name}", shape(tcx, args.type_at(0)))
    } else if tcx.impl_of_assoc(def).is_some() {
        let imp = tcx.impl_of_assoc(def).expect("checked above");
        format!("{}::{name}", shape(tcx, tcx.type_of(imp).skip_binder()))
    } else {
        canon(tcx, def)
    }
}

/// The part of a type that names it in allow-lists. Generic parameters are written `_`.
fn shape<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> String {
    match ty.kind() {
        ty::Adt(adt, _) => canon(tcx, adt.did()),
        ty::Array(..) => "[_; _]".into(),
        ty::Slice(..) => "[_]".into(),
        ty::Str => "str".into(),
        ty::Ref(_, inner, m) => format!("&{}{}", if m.is_mut() { "mut " } else { "" }, shape(tcx, *inner)),
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_) => ty.to_string(),
        _ => "?".into(),
    }
}

pub struct Scanner<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    inst: Instance<'tcx>,
    body: &'a Body<'tcx>,
    scan: Scan<'tcx>,
    impls: &'a Impls,
}

/// Scan `inst`'s body. Bodies of `core`/`alloc`/`std` are never scanned: they are leaves.
pub fn scan<'tcx>(tcx: TyCtxt<'tcx>, inst: Instance<'tcx>, impls: &Impls) -> Scan<'tcx> {
    let mut out = Scan::default();
    out.trusted = matches!(inst.def, InstanceKind::Item(d) if is_trame(tcx, d));
    if let InstanceKind::Item(def) = inst.def {
        if let Some(local) = def.as_local() {
            if matches!(tcx.def_kind(def), rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn | rustc_hir::def::DefKind::Closure) {
                let span = tcx.def_span(def);
                for span in unsafe_blocks(tcx, local) {
                    out.sites.push(Site { span, what: What::Unsafe("unsafe block".into()) });
                }
                if matches!(tcx.def_kind(def), rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn)
                    && tcx.fn_sig(def).skip_binder().safety().is_unsafe()
                {
                    out.sites.push(Site { span, what: What::Unsafe(format!("unsafe fn {}", canon(tcx, def))) });
                }
            }
        }
    }
    let body = tcx.instance_mir(inst.def);
    let mut s = Scanner { tcx, inst, body, scan: out, impls };
    s.visit_body(body);
    s.loops();
    let mut scan = s.scan;
    if scan.trusted {
        scan.sites = trusted_sites(scan.sites);
    }
    scan
}

/// `unsafe { }` blocks written in the body of `def` (not in nested closures, which are scanned as
/// their own bodies).
fn unsafe_blocks(tcx: TyCtxt<'_>, def: rustc_span::def_id::LocalDefId) -> Vec<Span> {
    struct Finder(Vec<Span>);
    impl<'hir> Visitor<'hir> for Finder {
        fn visit_block(&mut self, b: &'hir rustc_hir::Block<'hir>) {
            if let BlockCheckMode::UnsafeBlock(UnsafeSource::UserProvided) = b.rules {
                self.0.push(b.span);
            }
            intravisit::walk_block(self, b);
        }
    }
    let mut f = Finder(Vec::new());
    if let Some(body) = tcx.hir_maybe_body_owned_by(def) {
        f.visit_expr(body.value);
    }
    f.0
}

impl<'a, 'tcx> Scanner<'a, 'tcx> {
    fn mono<T: TypeFoldable<TyCtxt<'tcx>>>(&self, value: T) -> T {
        mono(self.tcx, self.inst, value)
    }

    fn push(&mut self, span: Span, what: What<'tcx>) {
        self.scan.sites.push(Site { span, what });
    }

    /// The paths the lists match for a core or alloc callee: its shape key, and its definition path.
    /// A trait method on a user type is not matched by its trait path: if the user type did not define
    /// the method, core's default body would run user code (an `Iterator` default on a user iterator).
    fn names_of(&self, def: DefId, args: GenericArgsRef<'tcx>) -> Vec<String> {
        let tcx = self.tcx;
        let mut names = vec![key(tcx, def, args)];
        let user_self = tcx.trait_of_assoc(def).is_some() && is_user_ty(tcx, args.type_at(0));
        if !user_self {
            names.push(canon(tcx, def));
        }
        names
    }

    /// Core and trame are trusted callees: a closure or fn item among the types of their call is called
    /// by them, so it is checked at this level. A user type's impls of core traits are called by core
    /// code too, and so are its fields' types. Types are followed through references and ADT arguments.
    fn reach(&mut self, start: Vec<Ty<'tcx>>, span: Span) {
        let tcx = self.tcx;
        let mut work = start;
        let mut seen: FxHashSet<Ty<'tcx>> = FxHashSet::default();
        while let Some(t) = work.pop() {
            if !seen.insert(t) {
                continue;
            }
            for g in t.walk() {
                if let GenericArgKind::Type(c) = g.kind()
                    && c != t
                {
                    work.push(c);
                }
            }
            match t.kind() {
                ty::Closure(def, cargs) if !is_std(tcx, *def) => {
                    self.push(span, What::Edge(Instance::new_raw(*def, *cargs)))
                }
                ty::FnDef(def, fargs) => self.fn_use(*def, *fargs, span),
                _ => {}
            }
            if let ty::Adt(adt, aargs) = t.peel_refs().kind() {
                let did = adt.did();
                self.push(span, What::Type(canon(tcx, did)));
                if is_user(tcx, did) {
                    for f in adt.all_fields() {
                        work.push(f.ty(tcx, aargs));
                    }
                    self.impl_edges(t, did, span);
                }
            }
        }
    }

    /// The impls of core traits for user type `t` (ADT `did`): every function they define is called by
    /// core code, so each is an edge, at this level. A user `Iterator` is also flagged: core would loop.
    fn impl_edges(&mut self, t: Ty<'tcx>, did: DefId, span: Span) {
        let tcx = self.tcx;
        let impls = self.impls;
        let Some(list) = impls.by_adt.get(&did) else { return };
        for imp in list {
            let args = match impl_args(tcx, *imp, t) {
                Ok(Some(args)) => args,
                Ok(None) => continue,
                Err(msg) => {
                    self.push(span, What::Unresolved(msg));
                    continue;
                }
            };
            for item in tcx.associated_item_def_ids(*imp) {
                if tcx.def_kind(*item) != rustc_hir::def::DefKind::AssocFn {
                    continue;
                }
                if tcx.generics_of(*item).own_params.is_empty() {
                    self.push(span, What::Edge(Instance::new_raw(*item, args)));
                } else {
                    self.push(span, What::Unresolved(format!("`{}` has its own generics; the driver cannot read it", canon(tcx, *item))));
                }
            }
            let is_iter = Some(tcx.impl_trait_ref(*imp).skip_binder().def_id) == impls.iterator;
            if is_iter {
                self.push(span, What::Unbounded(canon(tcx, did)));
            }
        }
    }

    fn local_ty(&self, place: &Place<'tcx>) -> Ty<'tcx> {
        self.mono(place.ty(self.body, self.tcx).ty)
    }

    /// A reference to a function (called, passed, or reified). Decides what kind of callee it is.
    fn fn_use(&mut self, def: rustc_span::def_id::DefId, args: GenericArgsRef<'tcx>, span: Span) {
        let tcx = self.tcx;
        let args = self.mono(args);
        let path = canon(tcx, def);
        if tcx.is_foreign_item(def) {
            return self.push(span, What::Ffi(path));
        }
        if tcx.def_kind(def) == rustc_hir::def::DefKind::Fn || tcx.def_kind(def) == rustc_hir::def::DefKind::AssocFn {
            if tcx.fn_sig(def).skip_binder().safety().is_unsafe() {
                self.push(span, What::Unsafe(format!("call to unsafe function {path}")));
            }
        }
        let krate = tcx.crate_name(def.krate).to_string();
        if krate == "std" {
            return self.push(span, What::Std(path));
        }
        if let Some(effect) = super::effect_of(&path) {
            return self.push(span, What::Effect(effect, path));
        }
        match ty::Instance::try_resolve(tcx, TypingEnv::fully_monomorphized(), def, args) {
            Err(_) => self.push(span, What::Unresolved(format!("cannot resolve {path}"))),
            Ok(None) => self.push(span, What::Unresolved(format!("cannot resolve {path}"))),
            Ok(Some(callee)) => self.classify(callee, def, args, span),
        }
    }

    fn classify(&mut self, callee: Instance<'tcx>, def: rustc_span::def_id::DefId, args: GenericArgsRef<'tcx>, span: Span) {
        let tcx = self.tcx;
        match callee.def {
            InstanceKind::Virtual(..) => self.push(span, What::Indirect("call of a trait object method".into())),
            InstanceKind::Intrinsic(..) => {
                let names = self.names_of(def, args);
                self.push(span, What::Core { key: names[0].clone(), names, alloc: false })
            }
            InstanceKind::Item(d) => {
                let krate = tcx.crate_name(d.krate).to_string();
                if tcx.is_foreign_item(d) {
                    self.push(span, What::Ffi(canon(tcx, d)))
                } else if krate == "std" {
                    self.push(span, What::Std(canon(tcx, d)))
                } else if krate == "core" || krate == "alloc" {
                    // A core or alloc callee is a leaf, judged by the lists. Its generic arguments are
                    // walked: a closure or fn item among them is called by the leaf, so it is checked here.
                    self.reach(args.types().collect(), span);
                    let names = self.names_of(def, args);
                    self.push(span, What::Core { key: names[0].clone(), names, alloc: krate == "alloc" })
                } else if krate == "trame" {
                    // Trame is the runtime and trusted at every level (driver/README.md). Its body is read
                    // for effects, edges and allocations only (`trusted_sites`).
                    self.reach(args.types().collect(), span);
                    self.push(span, What::Edge(callee))
                } else if !tcx.is_mir_available(d) {
                    // A user callee with no readable body fails closed: it is not listed, so it is refused.
                    let names = self.names_of(def, args);
                    self.push(span, What::Core { key: names[0].clone(), names, alloc: false })
                } else {
                    self.push(span, What::Edge(callee))
                }
            }
            // Shims (drop glue, closure adapters, clone, fn-pointer shims) are generated bodies.
            _ => self.push(span, What::Edge(callee)),
        }
    }

    /// The loops of the body. A loop is counted only if its header calls `Iterator::next` on a
    /// counted iterator, and the body never mutates that iterator (spec.md, Definition (Counted loop)).
    fn loops(&mut self) {
        let blocks = &self.body.basic_blocks;
        let reachable: FxHashSet<BasicBlock> = blocks.reverse_postorder().iter().copied().collect();
        let doms = blocks.dominators();
        let preds = blocks.predecessors();
        // header -> back-edge sources, in block order (deterministic output)
        let mut headers: Vec<(BasicBlock, Vec<BasicBlock>)> = Vec::new();
        for bb in blocks.reverse_postorder().iter().copied() {
            for succ in blocks[bb].terminator().successors() {
                if reachable.contains(&succ) && doms.dominates(succ, bb) {
                    match headers.iter_mut().find(|(h, _)| *h == succ) {
                        Some((_, srcs)) => srcs.push(bb),
                        None => headers.push((succ, vec![bb])),
                    }
                }
            }
        }
        for (header, sources) in headers {
            // Natural loop: the header plus everything that reaches a back-edge without the header.
            let mut in_loop: FxHashSet<BasicBlock> = FxHashSet::default();
            in_loop.insert(header);
            let mut stack = sources;
            while let Some(b) = stack.pop() {
                if in_loop.insert(b) {
                    stack.extend(preds[b].iter().copied());
                }
            }
            let span = blocks[header].terminator().source_info.span;
            let (counted, reason) = self.counted(header, &in_loop);
            self.push(span, What::Loop { counted, reason });
        }
    }

    fn counted(&self, header: BasicBlock, in_loop: &FxHashSet<BasicBlock>) -> (bool, String) {
        let tcx = self.tcx;
        let blocks = &self.body.basic_blocks;
        let TerminatorKind::Call { func, args, .. } = &blocks[header].terminator().kind else {
            return (false, "the loop does not start with a call to Iterator::next".into());
        };
        let Some(next) = tcx.lang_items().next_fn() else {
            return (false, "Iterator::next is not defined".into());
        };
        let Operand::Constant(c) = func else {
            return (false, "the loop does not start with a call to Iterator::next".into());
        };
        let ty::FnDef(def, fargs) = *self.mono(c.ty()).kind() else {
            return (false, "the loop does not start with a call to Iterator::next".into());
        };
        if def != next {
            return (false, "the loop does not start with a call to Iterator::next".into());
        }
        let self_ty = self.mono(fargs.type_at(0));
        let counted_iter = match self_ty.kind() {
            ty::Adt(adt, _) => matches!(
                canon(tcx, adt.did()).as_str(),
                "core::ops::range::Range"
                    | "core::ops::range::RangeInclusive"
                    | "core::slice::iter::Iter"
                    | "core::slice::iter::IterMut"
                    | "core::array::iter::IntoIter"
            ),
            _ => false,
        };
        if !counted_iter {
            return (
                false,
                format!("its back-edge is governed by Iterator::next on `{self_ty}`, not a Range, slice Iter or array IntoIter"),
            );
        }
        // The iterator local: `_tmp = &mut iter` feeds the call.
        let Some(Operand::Move(arg) | Operand::Copy(arg)) = args.first().map(|a| &a.node) else {
            return (false, "cannot find the iterator of the loop".into());
        };
        let iter = blocks.iter().flat_map(|b| b.statements.iter()).find_map(|st| match &st.kind {
            StatementKind::Assign(bx) => {
                let (lhs, rv) = &**bx;
                match rv {
                    Rvalue::Ref(_, BorrowKind::Mut { .. }, place) if lhs.local == arg.local && lhs.projection.is_empty() => {
                        Some(place.local)
                    }
                    _ => None,
                }
            }
            _ => None,
        });
        let Some(iter) = iter else {
            return (false, "cannot find the iterator of the loop".into());
        };
        for b in in_loop {
            if *b == header {
                continue;
            }
            let data = &blocks[*b];
            for st in &data.statements {
                if let StatementKind::Assign(bx) = &st.kind {
                    let (lhs, rv) = &**bx;
                    let mutates_rv = matches!(rv, Rvalue::Ref(_, BorrowKind::Mut { .. }, p) | Rvalue::RawPtr(RawPtrKind::Mut, p) if p.local == iter);
                    if lhs.local == iter || mutates_rv {
                        return (false, "the body mutates its iterator".into());
                    }
                }
            }
            if let TerminatorKind::Call { destination, .. } = &data.terminator().kind
                && destination.local == iter
            {
                return (false, "the body mutates its iterator".into());
            }
        }
        (true, String::new())
    }
}

impl<'a, 'tcx> MirVisitor<'tcx> for Scanner<'a, 'tcx> {
    fn visit_terminator(&mut self, term: &Terminator<'tcx>, loc: Location) {
        let span = term.source_info.span;
        match &term.kind {
            TerminatorKind::Call { func, args, .. } | TerminatorKind::TailCall { func, args, .. } => {
                let fty = self.mono(func.ty(self.body, self.tcx));
                match fty.kind() {
                    ty::FnDef(def, fargs) => self.fn_use(*def, *fargs, span),
                    ty::FnPtr(..) => self.push(span, What::Indirect("call through a function pointer".into())),
                    _ => self.push(span, What::Unresolved(format!("call of a value of type `{fty}`"))),
                }
                for a in args {
                    self.visit_operand(&a.node, loc);
                }
                if let TerminatorKind::Call { destination, .. } = &term.kind {
                    self.visit_place(destination, PlaceContext::MutatingUse(MutatingUseContext::Call), loc);
                }
            }
            TerminatorKind::Drop { place, .. } => {
                let ty = self.local_ty(place);
                if ty.needs_drop(self.tcx, TypingEnv::fully_monomorphized()) {
                    let tcx = self.tcx;
                    // A core/alloc/std type with its own `Drop` impl is one call, `<T as Drop>::drop`,
                    // listed like any other callee: its glue is not read (spec.md: drop glue counts
                    // as a call). Other drops are read: user `Drop` impls and glue of plain structs.
                    let listed_drop = match ty.kind() {
                        ty::Adt(adt, _) if tcx.adt_destructor(adt.did()).is_some() => {
                            let krate = tcx.crate_name(adt.did().krate).to_string();
                            (krate == "core" || krate == "alloc" || krate == "std").then(|| (krate, shape(tcx, ty)))
                        }
                        _ => None,
                    };
                    if let Some((krate, shape)) = listed_drop {
                        let k = format!("{shape}::drop");
                        // The glue of the listed type's fields is inside its own drop: walk the type.
                        self.reach(vec![ty], span);
                        if krate == "std" {
                            self.push(span, What::Std(k));
                        } else {
                            let names = vec![k.clone()];
                            self.push(span, What::Core { key: k, names, alloc: krate == "alloc" });
                        }
                    } else {
                        let inst = ty::Instance::resolve_drop_in_place(tcx, ty);
                        match inst.def {
                            InstanceKind::DropGlue(_, None) => {}
                            InstanceKind::Virtual(..) => self.push(span, What::Indirect("drop of a trait object".into())),
                            _ => self.push(span, What::Edge(inst)),
                        }
                    }
                }
                self.visit_place(place, PlaceContext::MutatingUse(MutatingUseContext::Drop), loc);
            }
            TerminatorKind::InlineAsm { .. } => self.push(span, What::Unsafe("inline assembly".into())),
            _ => self.super_terminator(term, loc),
        }
    }

    fn visit_rvalue(&mut self, rv: &Rvalue<'tcx>, loc: Location) {
        let span = self.body.source_info(loc).span;
        match rv {
            Rvalue::Aggregate(kind, _) => {
                if let AggregateKind::Closure(def, args) = **kind {
                    let args = self.mono(args);
                    self.push(span, What::Edge(Instance::new_raw(def, args)));
                }
            }
            Rvalue::Cast(CastKind::PointerCoercion(PointerCoercion::ReifyFnPointer(_), _), op, _) => {
                if let ty::FnDef(def, args) = *self.mono(op.ty(self.body, self.tcx)).kind() {
                    self.fn_use(def, args, span);
                }
            }
            Rvalue::Cast(CastKind::PointerCoercion(PointerCoercion::ClosureFnPointer(_), _), op, _) => {
                if let ty::Closure(def, args) = *self.mono(op.ty(self.body, self.tcx)).kind() {
                    self.push(span, What::Edge(Instance::new_raw(def, args)));
                }
            }
            Rvalue::Cast(CastKind::PointerCoercion(PointerCoercion::Unsize, _), op, target) => {
                // An unsizing cast to a trait object: core code calls the trait's methods through the
                // vtable, so the impls of the source type are reached here (see `reach`).
                let target = self.mono(*target);
                if target.walk().any(|g| matches!(g.kind(), GenericArgKind::Type(t) if matches!(t.kind(), ty::Dynamic(..)))) {
                    let src = self.mono(op.ty(self.body, self.tcx));
                    self.reach(vec![src], span);
                }
            }
            Rvalue::ThreadLocalRef(def) => {
                self.push(span, What::Static(format!("thread-local static {}", canon(self.tcx, *def))))
            }
            _ => {}
        }
        self.super_rvalue(rv, loc);
    }

    fn visit_const_operand(&mut self, c: &ConstOperand<'tcx>, loc: Location) {
        let span = self.body.source_info(loc).span;
        let tcx = self.tcx;
        let cty = self.mono(c.ty());
        if let ty::FnDef(def, args) = *cty.kind() {
            self.fn_use(def, args, span);
            return;
        }
        match c.const_ {
            Const::Unevaluated(uv, _) if tcx.is_static(uv.def) => {
                self.push(span, What::Static(canon(tcx, uv.def)));
            }
            Const::Val(ConstValue::Scalar(rustc_middle::mir::interpret::Scalar::Ptr(ptr, _)), _) => {
                if let rustc_middle::mir::interpret::GlobalAlloc::Static(def) = tcx.global_alloc(ptr.provenance.alloc_id()) {
                    self.push(span, What::Static(canon(tcx, def)));
                }
            }
            _ => {}
        }
    }

    fn visit_place(&mut self, place: &Place<'tcx>, ctx: PlaceContext, loc: Location) {
        let span = self.body.source_info(loc).span;
        let mut ty = rustc_middle::mir::PlaceTy::from_ty(self.local_ty(&Place::from(place.local)));
        for elem in place.projection {
            match elem {
                ProjectionElem::Deref if ty.ty.is_raw_ptr() => {
                    self.push(span, What::Unsafe("dereference of a raw pointer".into()));
                }
                ProjectionElem::Field(..) if ty.ty.ty_adt_def().is_some_and(|a| a.is_union()) => {
                    self.push(span, What::Unsafe("access to a union field".into()));
                }
                _ => {}
            }
            ty = ty.projection_ty(self.tcx, elem);
        }
        self.super_place(place, ctx, loc);
    }
}
