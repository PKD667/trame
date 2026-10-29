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
use rustc_middle::ty::{
    self, GenericArg, GenericArgKind, GenericArgsRef, GenericParamDefKind, Instance, InstanceKind, Ty, TyCtxt,
    TypeFoldable, TypingEnv,
};
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
    /// A type reached by a core or trame call (in its generic arguments or a field): refused when the
    /// lists deny it at the level, as `Cell` anywhere.
    Type(String),
    /// A user type whose `Iterator` impl core code would drive without a bound.
    Unbounded(String),
    /// A call that the driver checks by reading its body. It carries trust: a trusted body's edge
    /// makes its callee trusted (`trust.rs`).
    Edge(Instance<'tcx>),
    /// An edge to the program's own code: a closure or fn item passed in as a generic argument, a user
    /// type's impl of a core trait, a user impl item, drop glue or a shim. It does not carry trust, so
    /// its body is judged in full whatever reaches it (`trust.rs`).
    Own(Instance<'tcx>),
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
}

impl<'tcx> Scan<'tcx> {
    /// Callees the driver reads itself: the edges of the call graph, trusting or not.
    pub fn edges(&self) -> impl Iterator<Item = (Instance<'tcx>, Span)> + '_ {
        self.sites.iter().filter_map(|s| match s.what {
            What::Edge(inst) | What::Own(inst) => Some((inst, s.span)),
            _ => None,
        })
    }

    /// The edges to the program's own code (`What::Own`).
    pub fn own_edges(&self) -> impl Iterator<Item = Instance<'tcx>> + '_ {
        self.sites.iter().filter_map(|s| match s.what {
            What::Own(inst) => Some(inst),
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

/// The traits in `roots` and all their supertraits.
fn trait_closure(tcx: TyCtxt<'_>, roots: Vec<DefId>) -> FxHashSet<DefId> {
    let mut out = FxHashSet::default();
    let mut work = roots;
    while let Some(tr) = work.pop() {
        if out.insert(tr) {
            work.extend(
                tcx.explicit_super_predicates_of(tr)
                    .skip_binder()
                    .iter()
                    .filter_map(|(c, _)| c.as_trait_clause().map(|t| t.skip_binder().def_id())),
            );
        }
    }
    out
}

/// Defined in core, alloc or std.
fn is_std(tcx: TyCtxt<'_>, def: DefId) -> bool {
    matches!(tcx.crate_name(def.krate).as_str(), "core" | "alloc" | "std")
}

/// Defined in trame, the runtime: trusted at every level (driver/README.md).
fn is_trame(tcx: TyCtxt<'_>, def: DefId) -> bool {
    tcx.crate_name(def.krate).as_str() == "trame"
}

/// The body of `inst` is trame's own (an item defined in trame, closures included).
pub fn is_trame_instance(tcx: TyCtxt<'_>, inst: Instance<'_>) -> bool {
    matches!(inst.def, InstanceKind::Item(d) if is_trame(tcx, d))
}

/// Defined in the program: not core, alloc, std or trame.
pub fn is_user(tcx: TyCtxt<'_>, def: DefId) -> bool {
    !is_std(tcx, def) && !is_trame(tcx, def)
}

fn is_user_ty(tcx: TyCtxt<'_>, t: Ty<'_>) -> bool {
    matches!(t.peel_refs().kind(), ty::Adt(adt, _) if is_user(tcx, adt.did()))
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
        (ty::Tuple(p), ty::Tuple(c)) => {
            p.len() == c.len() && p.iter().zip(c.iter()).all(|(x, y)| unify(tcx, x, y, out))
        }
        (ty::Slice(p), ty::Slice(c)) | (ty::Array(p, _), ty::Array(c, _)) => unify(tcx, *p, *c, out),
        _ => pat == t,
    }
}

fn unify_arg<'tcx>(
    tcx: TyCtxt<'tcx>,
    x: GenericArg<'tcx>,
    y: GenericArg<'tcx>,
    out: &mut [Option<GenericArg<'tcx>>],
) -> bool {
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
    inst.instantiate_mir_and_normalize_erasing_regions(
        tcx,
        TypingEnv::fully_monomorphized(),
        ty::EarlyBinder::bind(value),
    )
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
        ty::RawPtr(_, m) => format!("*{} _", if m.is_mut() { "mut" } else { "const" }),
        _ => "?".into(),
    }
}

pub struct Scanner<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    inst: Instance<'tcx>,
    body: &'a Body<'tcx>,
    scan: Scan<'tcx>,
    impls: &'a Impls,
    /// The body is compiler-generated (a shim: drop glue, clone glue, fn-pointer adapters). Its raw
    /// derefs and union accesses are the compiler's, not the program's; its calls are still scanned.
    shim: bool,
}

/// Scan `inst`'s body. Bodies of `core`/`alloc`/`std` are never scanned: they are leaves.
pub fn scan<'tcx>(tcx: TyCtxt<'tcx>, inst: Instance<'tcx>, impls: &Impls) -> Scan<'tcx> {
    let mut out = Scan::default();
    if let InstanceKind::Item(def) = inst.def {
        if let Some(local) = def.as_local() {
            if matches!(
                tcx.def_kind(def),
                rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn | rustc_hir::def::DefKind::Closure
            ) {
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
    let shim = !matches!(inst.def, InstanceKind::Item(_));
    let mut s = Scanner { tcx, inst, body, scan: out, impls, shim };
    s.visit_body(body);
    s.loops();
    s.scan
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

    /// Every edge pushed since `mark` is an edge to the program's own code (`What::Own`).
    fn own_since(&mut self, mark: usize) {
        for s in &mut self.scan.sites[mark..] {
            if let What::Edge(inst) = s.what {
                s.what = What::Own(inst);
            }
        }
    }

    /// The paths the lists match for a core or alloc callee: its shape key, and its definition path.
    /// A default method of a core trait, on a user type, is matched by its trait path as well: its body is
    /// core's, and the user code it calls is reached through `reach`. A user `Iterator` is refused by
    /// `Unbounded` (below Worker), whatever the trait path allows.
    fn names_of(&self, def: DefId, args: GenericArgsRef<'tcx>) -> Vec<String> {
        vec![key(self.tcx, def, args), canon(self.tcx, def)]
    }

    /// The traits that core code can call on a user type through callee `def`: the traits its generics
    /// bound (with their supertraits), and Drop, since drop glue runs in any container. This is what
    /// `reach` follows; the other impls of a user type are not called by the callee, so are not edges.
    fn callable_traits(&self, def: DefId) -> FxHashSet<DefId> {
        let tcx = self.tcx;
        let bound = |c: &ty::Clause<'tcx>| c.as_trait_clause().map(|t| t.skip_binder().def_id());
        let mut work: Vec<DefId> = tcx
            .predicates_of(def)
            .instantiate_identity(tcx)
            .predicates
            .iter()
            .filter_map(|c| bound(&c.skip_normalization()))
            .collect();
        work.extend(tcx.lang_items().drop_trait());
        trait_closure(tcx, work)
    }

    /// Core and trame are trusted callees: a closure or fn item among the types of their call is called
    /// by them, so it is checked at this level. A user type's impls of core traits are called by core
    /// code too, and so are its fields' types. Types are followed through references and ADT arguments.
    fn reach(&mut self, start: Vec<Ty<'tcx>>, span: Span, traits: &FxHashSet<DefId>) {
        let mark = self.scan.sites.len();
        self.reach_types(start, span, traits);
        self.own_since(mark);
    }

    fn reach_types(&mut self, start: Vec<Ty<'tcx>>, span: Span, traits: &FxHashSet<DefId>) {
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
                ty::FnDef(def, fargs) => {
                    self.fn_use(*def, *fargs, span);
                    // A fn item's signature holds the types core code passes it and gets back. A user type there
                    // is reached as it is through a closure's signature (the walk above): `map(mk).max()` with
                    // `mk -> X` calls X's Ord, and X is in no generic argument of `max`.
                    let sig = tcx.fn_sig(*def).instantiate(tcx, self.mono(*fargs)).skip_binder();
                    work.extend(sig.inputs_and_output.iter().map(|c| self.mono(c)));
                }
                _ => {}
            }
            if let ty::Adt(adt, aargs) = t.peel_refs().kind() {
                let did = adt.did();
                self.push(span, What::Type(canon(tcx, did)));
                if is_user(tcx, did) {
                    for f in adt.all_fields() {
                        work.push(f.ty(tcx, aargs));
                    }
                    self.impl_edges(t, did, span, traits);
                }
            }
        }
    }

    /// The impls of core traits for user type `t` (ADT `did`): every function they define is called by
    /// core code, so each is an edge, at this level. A user `Iterator` is also flagged: core would loop.
    fn impl_edges(&mut self, t: Ty<'tcx>, did: DefId, span: Span, traits: &FxHashSet<DefId>) {
        let tcx = self.tcx;
        let impls = self.impls;
        let Some(list) = impls.by_adt.get(&did) else { return };
        for imp in list {
            let trait_id = tcx.impl_trait_ref(*imp).skip_binder().def_id;
            if !traits.contains(&trait_id) {
                continue;
            }
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
                    self.push(
                        span,
                        What::Unresolved(format!(
                            "`{}` has its own generics; the driver cannot read it",
                            canon(tcx, *item)
                        )),
                    );
                }
            }
            let is_iter = Some(trait_id) == impls.iterator;
            if is_iter {
                self.push(span, What::Unbounded(canon(tcx, did)));
            }
        }
    }

    /// An impl item (an explicit `impl` method, not a trait's default body) whose self type is the
    /// program's: the program wrote the code, whichever trait or crate called it.
    fn is_user_impl_item(&self, callee: Instance<'tcx>) -> bool {
        let tcx = self.tcx;
        let Some(imp) = tcx.impl_of_assoc(callee.def_id()) else { return false };
        let self_ty = tcx.type_of(imp).instantiate(tcx, callee.args).skip_normalization();
        is_user_ty(tcx, self_ty)
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

    fn classify(
        &mut self,
        callee: Instance<'tcx>,
        def: rustc_span::def_id::DefId,
        args: GenericArgsRef<'tcx>,
        span: Span,
    ) {
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
                    let traits = self.callable_traits(def);
                    self.reach(args.types().collect(), span, &traits);
                    let names = self.names_of(def, args);
                    self.push(span, What::Core { key: names[0].clone(), names, alloc: krate == "alloc" })
                } else if krate == "trame" {
                    // Trame is the runtime and trusted at every level (driver/README.md). Its body is read
                    // for effects, edges and allocations only (`trust.rs`).
                    let traits = self.callable_traits(def);
                    self.reach(args.types().collect(), span, &traits);
                    self.push(span, What::Edge(callee))
                } else if !tcx.is_mir_available(d) {
                    // A user callee with no readable body fails closed: it is not listed, so it is refused.
                    let names = self.names_of(def, args);
                    self.push(span, What::Core { key: names[0].clone(), names, alloc: false })
                } else if matches!(tcx.def_kind(d), rustc_hir::def::DefKind::Closure) || self.is_user_impl_item(callee)
                {
                    // The program's own code, reached through a dependency or trame: judged in full.
                    self.push(span, What::Own(callee))
                } else {
                    self.push(span, What::Edge(callee))
                }
            }
            // Shims (drop glue, closure adapters, clone, fn-pointer shims) are generated from the types
            // they are for: the program's own types and closures, so they are judged in full.
            _ => self.push(span, What::Own(callee)),
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

    /// A for-loop source the spec counts: a range, a slice or an array iterator, or one of these reversed
    /// (Definition (Counted loop)). `Rev` is the only adapter admitted.
    fn counted_source(&self, ty: Ty<'tcx>) -> bool {
        let tcx = self.tcx;
        let ty::Adt(adt, args) = ty.kind() else { return false };
        match canon(tcx, adt.did()).as_str() {
            "core::ops::range::Range"
            | "core::ops::range::RangeInclusive"
            | "core::slice::iter::Iter"
            | "core::slice::iter::IterMut"
            | "core::array::iter::IntoIter" => true,
            "core::iter::adapters::rev::Rev" => args.types().next().is_some_and(|inner| self.counted_source(inner)),
            _ => false,
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
        if !self.counted_source(self_ty) {
            return (
                false,
                format!(
                    "its back-edge is governed by Iterator::next on `{self_ty}`, not a Range, slice Iter or array IntoIter"
                ),
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
                    Rvalue::Ref(_, BorrowKind::Mut { .. }, place)
                        if lhs.local == arg.local && lhs.projection.is_empty() =>
                    {
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

impl<'a, 'tcx> Scanner<'a, 'tcx> {
    /// Whether the raw pointer `local` (reached with no projection) is the one ElaborateBoxDerefs makes from a
    /// Box: its only definition is `local = transmute(box.0.0)` (Unique, then NonNull), see `is_box_field_chain`.
    /// The deref of such a pointer is the Box's own deref, which the program writes safely (`*b`, `vec![..]`).
    fn box_pointer(&self, local: Local) -> bool {
        let blocks = &self.body.basic_blocks;
        let mut defs = blocks.iter().flat_map(|b| b.statements.iter()).filter_map(|st| match &st.kind {
            StatementKind::Assign(bx) if bx.0.local == local && bx.0.projection.is_empty() => Some(&bx.1),
            _ => None,
        });
        let calls_into = blocks.iter().any(
            |b| matches!(&b.terminator().kind, TerminatorKind::Call { destination, .. } if destination.local == local),
        );
        match (defs.next(), defs.next(), calls_into) {
            (Some(Rvalue::Cast(CastKind::Transmute, op, _)), None, false) => match op {
                Operand::Copy(p) | Operand::Move(p) => self.is_box_field_chain(p),
                _ => false,
            },
            _ => false,
        }
    }

    /// Whether `place` is `b.0.0` for a Box-typed `b` (possibly behind a field path): the two fields are Box's
    /// `Unique` and `NonNull`, which only ElaborateBoxDerefs and the box initialiser reach.
    fn is_box_field_chain(&self, place: &Place<'tcx>) -> bool {
        let proj = place.projection;
        let n = proj.len();
        if n < 2 || !proj[n - 2..].iter().all(|e| matches!(e, ProjectionElem::Field(f, _) if f.as_usize() == 0)) {
            return false;
        }
        let mut ty = rustc_middle::mir::PlaceTy::from_ty(self.mono(self.body.local_decls[place.local].ty));
        for elem in &proj[..n - 2] {
            ty = ty.projection_ty(self.tcx, *elem);
        }
        ty.ty.boxed_ty().is_some()
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
                        let traits = trait_closure(tcx, tcx.lang_items().drop_trait().into_iter().collect());
                        self.reach(vec![ty], span, &traits);
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
                            InstanceKind::Virtual(..) => {
                                self.push(span, What::Indirect("drop of a trait object".into()))
                            }
                            _ => self.push(span, What::Own(inst)),
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
                    self.push(span, What::Own(Instance::new_raw(def, args)));
                }
            }
            Rvalue::Cast(CastKind::PointerCoercion(PointerCoercion::ReifyFnPointer(_), _), op, _) => {
                if let ty::FnDef(def, args) = *self.mono(op.ty(self.body, self.tcx)).kind() {
                    self.fn_use(def, args, span);
                }
            }
            Rvalue::Cast(CastKind::PointerCoercion(PointerCoercion::ClosureFnPointer(_), _), op, _) => {
                if let ty::Closure(def, args) = *self.mono(op.ty(self.body, self.tcx)).kind() {
                    self.push(span, What::Own(Instance::new_raw(def, args)));
                }
            }
            Rvalue::Cast(CastKind::PointerCoercion(PointerCoercion::Unsize, _), op, target) => {
                // An unsizing cast to a trait object: core code calls the trait's methods through the
                // vtable, so the impls of the source type are reached here (see `reach`).
                let target = self.mono(*target);
                let principal = target.walk().find_map(|g| match g.kind() {
                    GenericArgKind::Type(t) => match t.kind() {
                        ty::Dynamic(preds, ..) => Some(preds.principal_def_id()),
                        _ => None,
                    },
                    _ => None,
                });
                if let Some(principal) = principal {
                    let src = self.mono(op.ty(self.body, self.tcx));
                    let mut roots: Vec<DefId> = principal.into_iter().collect();
                    roots.extend(self.tcx.lang_items().drop_trait());
                    let traits = trait_closure(self.tcx, roots);
                    self.reach(vec![src], span, &traits);
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
                if let rustc_middle::mir::interpret::GlobalAlloc::Static(def) =
                    tcx.global_alloc(ptr.provenance.alloc_id())
                {
                    self.push(span, What::Static(canon(tcx, def)));
                }
            }
            _ => {}
        }
    }

    fn visit_place(&mut self, place: &Place<'tcx>, ctx: PlaceContext, loc: Location) {
        let span = self.body.source_info(loc).span;
        let mut ty = rustc_middle::mir::PlaceTy::from_ty(self.local_ty(&Place::from(place.local)));
        // Set once the place has dereferenced a Box's pointer: the union fields after it are the Box's own
        // (MaybeUninit, in the box initialiser), not the program's.
        let mut in_box = false;
        for (i, elem) in place.projection.iter().enumerate() {
            match elem {
                ProjectionElem::Deref if ty.ty.is_raw_ptr() => {
                    // A raw deref with no projection before it: the pointer is a local, and it may be a Box's.
                    let boxed = i == 0 && self.box_pointer(place.local);
                    in_box |= boxed;
                    if !boxed && !self.shim {
                        self.push(span, What::Unsafe("dereference of a raw pointer".into()));
                    }
                }
                ProjectionElem::Field(..)
                    if !in_box && !self.shim && ty.ty.ty_adt_def().is_some_and(|a| a.is_union()) =>
                {
                    self.push(span, What::Unsafe("access to a union field".into()));
                }
                _ => {}
            }
            ty = ty.projection_ty(self.tcx, elem);
        }
        self.super_place(place, ctx, loc);
    }
}
