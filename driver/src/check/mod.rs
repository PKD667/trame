//! The compile-time checker: roots, levels, the call graph, and the rules of spec.md section 3.
//!
//! Roots are the monomorphized instances of the markers in trame's `check.rs`
//! (`trame::__check::{item, step, worker}`) that this crate codegens. Each root and everything
//! reachable from it is checked at the strictest level that reaches it. Facts about a body are
//! gathered by `scan`; `judge` decides what each fact means at a level.

mod lists;
mod scan;
mod trust;

use std::collections::VecDeque;
use std::env;
use std::sync::OnceLock;

use rustc_data_structures::fx::{FxHashMap, FxHashSet};
use rustc_middle::mono::MonoItem;
use rustc_middle::ty::{self, GenericArg, Instance, InstanceKind, TyCtxt, TypingEnv};
use rustc_span::Span;

use lists::{Effect, Lists, Verdict};
use scan::{canon, Scan, What};

/// A level of spec.md section 3. Smaller is stricter, so `min` picks the strictest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Item = 0,
    Step = 1,
    Worker = 2,
}

impl Level {
    fn name(self) -> &'static str {
        match self {
            Level::Item => "Item",
            Level::Step => "Step",
            Level::Worker => "Worker",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Level::Item => "item",
            Level::Step => "step",
            Level::Worker => "worker",
        }
    }

    /// The allow-list file of this level (driver/lists/).
    fn list_file(self) -> &'static str {
        match self {
            Level::Item => "A_I",
            Level::Step => "A_C",
            Level::Worker => "A_W",
        }
    }
}

static LISTS: OnceLock<Lists> = OnceLock::new();

fn lists() -> &'static Lists {
    LISTS.get_or_init(|| {
        Lists::load().unwrap_or_else(|e| {
            eprintln!("trame-driver: {e}");
            std::process::exit(2)
        })
    })
}

/// The effect a canonical definition path names, if any.
pub fn effect_of(path: &str) -> Option<Effect> {
    lists().effect(path)
}

fn debug_on() -> bool {
    env::var_os("TRAME_DRIVER_DEBUG").is_some_and(|v| v == "1")
}

/// Check every root of the crate. Returns the number of errors emitted.
pub fn run(tcx: TyCtxt<'_>) -> usize {
    let lists = lists();
    let impls = scan::core_impls(tcx);
    let debug = debug_on();
    let mut errors = 0;

    // Roots: the marker instances among the mono items this crate codegens.
    let partitions = tcx.collect_and_partition_mono_items(());
    let instances: Vec<Instance<'_>> = partitions
        .codegen_units
        .iter()
        .flat_map(|cgu| cgu.items().keys())
        .filter_map(|item| match item {
            MonoItem::Fn(inst) => Some(*inst),
            _ => None,
        })
        .collect();

    if debug {
        for item in partitions.codegen_units.iter().flat_map(|cgu| cgu.items().keys()) {
            eprintln!("trame-driver: mono item {item:?}");
        }
    }
    let mut roots: Vec<(Level, Instance<'_>, Instance<'_>)> = Vec::new();
    for marker in instances {
        let InstanceKind::Item(def) = marker.def else { continue };
        let level = match canon(tcx, def).as_str() {
            "trame::__check::item" => Level::Item,
            "trame::__check::step" => Level::Step,
            "trame::__check::worker" => Level::Worker,
            _ => continue,
        };
        match root_target(tcx, level, marker) {
            Ok(target) => roots.push((level, marker, target)),
            Err(msg) => {
                tcx.dcx().struct_span_err(tcx.def_span(def), format!("[root] {msg}")).emit();
                errors += 1;
            }
        }
    }
    if debug {
        for (level, _, target) in &roots {
            eprintln!("trame-driver: root {} {} level {}", level.noun(), canon(tcx, target.def_id()), level.name());
        }
    }

    // Levels: each function gets the strictest level that reaches it. Item roots are explored
    // first, so the set of nodes already assigned is closed under successors at every stage.
    let mut level: FxHashMap<Instance<'_>, Level> = FxHashMap::default();
    let mut parent: FxHashMap<Instance<'_>, Option<Instance<'_>>> = FxHashMap::default();
    let mut scans: FxHashMap<Instance<'_>, Scan<'_>> = FxHashMap::default();
    let mut order: Vec<Instance<'_>> = Vec::new();
    for lv in [Level::Item, Level::Step, Level::Worker] {
        let mut queue: VecDeque<(Instance<'_>, Option<Instance<'_>>)> =
            roots.iter().filter(|(l, _, _)| *l == lv).map(|(_, _, t)| (*t, None)).collect();
        while let Some((n, p)) = queue.pop_front() {
            if level.contains_key(&n) {
                continue;
            }
            level.insert(n, lv);
            parent.insert(n, p);
            order.push(n);
            scans.entry(n).or_insert_with(|| scan::scan(tcx, n, &impls));
            for (callee, _) in scans[&n].edges() {
                if !level.contains_key(&callee) {
                    queue.push_back((callee, Some(n)));
                }
            }
        }
    }
    if debug {
        eprintln!("trame-driver: {} functions checked", order.len());
    }

    // Trust: a body reached only by trusted edges is read for effects, edges and allocations (trust.rs).
    let judged = trust::judged_in_full(tcx, &roots.iter().map(|(_, _, t)| *t).collect::<Vec<_>>(), &scans);

    // Per-site rules.
    for n in &order {
        let lv = level[n];
        let root = root_name(tcx, *n, &parent);
        let trusted = !judged.contains(n);
        for site in &scans[n].sites {
            if let Some((rule, msg)) = judge(lists, lv, site, trusted) {
                let mut d = tcx.dcx().struct_span_err(site.span, format!("[{rule}] {msg}"));
                d.note(format!("reached from root {root}: {}", path_to(tcx, *n, &parent)));
                d.emit();
                errors += 1;
            }
        }
    }

    // Recursion: a cycle among Item and Step functions. Cycles lie inside one level (a cycle's
    // members are mutually reachable, so they share the same roots).
    let mut state: FxHashMap<Instance<'_>, u8> = FxHashMap::default();
    let mut stack: Vec<Instance<'_>> = Vec::new();
    for n in &order {
        if level[n] != Level::Worker && !state.contains_key(n) {
            errors += recursion(tcx, *n, &level, &scans, &judged, &mut state, &mut stack);
        }
    }
    errors
}

fn recursion<'tcx>(
    tcx: TyCtxt<'tcx>,
    n: Instance<'tcx>,
    level: &FxHashMap<Instance<'tcx>, Level>,
    scans: &FxHashMap<Instance<'tcx>, Scan<'tcx>>,
    judged: &FxHashSet<Instance<'tcx>>,
    state: &mut FxHashMap<Instance<'tcx>, u8>,
    stack: &mut Vec<Instance<'tcx>>,
) -> usize {
    let mut errors = 0;
    state.insert(n, 1);
    stack.push(n);
    for (m, span) in scans[&n].edges() {
        if level.get(&m) == Some(&Level::Worker) {
            continue;
        }
        match state.get(&m).copied() {
            // A cycle whose head is trusted is the runtime's own (trust.rs); a cycle through the program is refused.
            Some(1) if judged.contains(&m) => {
                let at = stack.iter().position(|x| *x == m).expect("on the stack");
                let cycle: Vec<String> =
                    stack[at..].iter().chain(std::iter::once(&m)).map(|x| canon(tcx, x.def_id())).collect();
                let lv = level[&n];
                tcx.dcx().struct_span_err(
                    span,
                    format!(
                        "[recursion] {} recurses: {} (spec.md, Requirement ({}))",
                        lv.noun(),
                        cycle.join(" -> "),
                        lv.name()
                    ),
                )
                .emit();
                errors += 1;
            }
            Some(_) => {}
            None => errors += recursion(tcx, m, level, scans, judged, state, stack),
        }
    }
    stack.pop();
    state.insert(n, 2);
    errors
}

/// The rule a site breaks at `level`, if any: (rule id, message). A trusted body (trust.rs) keeps its
/// effects, its allocations and its user iterators; its other facts are not judged.
fn judge(lists: &Lists, level: Level, site: &scan::Site<'_>, trusted: bool) -> Option<(&'static str, String)> {
    let subj = level.noun();
    let lvl = level.name();
    if trusted {
        match &site.what {
            What::Core { key, alloc: true, .. } => {
                return (level == Level::Item).then(|| {
                    ("allocation", format!("{subj} reaches {key} through trusted code, which allocates: items neither allocate nor call alloc (spec.md, Requirement (Item))"))
                });
            }
            What::Effect(..) | What::Unbounded(_) => {}
            _ => return None,
        }
    }
    match &site.what {
        What::Unsafe(d) => Some(("unsafe", format!("{subj} contains {d}: no unsafe code (spec.md, Requirement (All levels))"))),
        What::Ffi(p) => Some(("ffi", format!("{subj} calls {p}: no FFI (spec.md, Requirement (All levels))"))),
        What::Indirect(d) => Some(("indirect", format!("{subj} {d}: no indirect calls (spec.md, Requirement (All levels))"))),
        What::Static(p) => Some(("static", format!("{subj} uses {p}: only consts are allowed, not globals (spec.md, Requirement (All levels))"))),
        What::Std(p) => Some(("std", format!("{subj} calls {p}: no std (spec.md, Requirement (All levels))"))),
        What::Unresolved(d) => Some(("unresolved", format!("{subj}: {d}; the driver cannot check this call (spec.md, Requirement (All levels))"))),
        What::Effect(e, p) => {
            if e.allowed_at(level) {
                None
            } else if level == Level::Item {
                Some(("effects", format!("{subj} calls {p}: items perform no effects (spec.md, Requirement (Item))")))
            } else {
                Some(("effects", format!("{subj} calls {p}: steps perform only send, recv, now and parallel (spec.md, Requirement (Step))")))
            }
        }
        What::Core { key, names, alloc } => match lists.verdict(level, names) {
            Verdict::Allow => None,
            Verdict::Deny(entry) => Some(("deny", format!("{subj} calls {key}: denied by `{entry}` (driver/lists/deny.txt, {}.txt) (spec.md, Requirement ({lvl}))", level.list_file()))),
            Verdict::Refuse if *alloc && level == Level::Item => {
                Some(("allocation", format!("{subj} calls {key}: items neither allocate nor call alloc (spec.md, Requirement (Item))")))
            }
            Verdict::Refuse => Some((
                "allow-list",
                format!("{subj} calls {key}: not listed at this level (driver/lists/{}.txt) (spec.md, Requirement ({lvl}))", level.list_file()),
            )),
        },
        What::Type(name) => lists.denied(level, name).map(|entry| {
            ("deny", format!("{subj} uses type {name}: denied by `{entry}` (driver/lists/deny.txt, {}.txt) (spec.md, Requirement ({lvl}))", level.list_file()))
        }),
        What::Unbounded(name) => (level != Level::Worker).then(|| {
            ("unbounded", format!("{subj} passes {name}, a user iterator, to core code, which would drive it without a bound (spec.md, Requirement ({lvl}))"))
        }),
        What::Loop { counted, reason } => {
            if *counted || level == Level::Worker {
                None
            } else {
                Some(("counted-loop", format!("{subj} has a loop that is not counted: {reason} (spec.md, Definition (Counted loop))")))
            }
        }
        What::Edge(_) | What::Own(_) => None,
    }
}

/// The instance a marker's argument names: the closure or fn item, or for a step the process's
/// inherent `step` method.
fn root_target<'tcx>(tcx: TyCtxt<'tcx>, level: Level, marker: Instance<'tcx>) -> Result<Instance<'tcx>, String> {
    let arg = marker.args.type_at(0);
    let name = canon(tcx, marker.def_id());
    match (level, arg.kind()) {
        (Level::Item | Level::Worker, ty::Closure(def, args)) => Ok(Instance::new_raw(*def, args)),
        (Level::Item | Level::Worker, ty::FnDef(def, args)) => {
            match Instance::try_resolve(tcx, TypingEnv::fully_monomorphized(), *def, args) {
                Ok(Some(inst)) => Ok(inst),
                _ => Err(format!("{name}: cannot resolve the function {}", canon(tcx, *def))),
            }
        }
        (Level::Step, ty::Adt(adt, pargs)) => {
            for imp in tcx.inherent_impls(adt.did()) {
                let Some(step) = tcx.associated_items(*imp).in_definition_order().find(|a| a.name().as_str() == "step" && a.is_fn()) else {
                    continue;
                };
                let imp_self = tcx.type_of(*imp).skip_binder();
                let ty::Adt(_, imp_args) = imp_self.kind() else { continue };
                // `impl<T..> P<T..>`: the impl's parameters are the ADT's arguments in order.
                let in_order = imp_args.iter().enumerate().all(|(i, a)| matches!(a.kind(), rustc_middle::ty::GenericArgKind::Type(t) if matches!(t.kind(), ty::Param(p) if p.index as usize == i))
                    || matches!(a.kind(), rustc_middle::ty::GenericArgKind::Lifetime(_)));
                if !in_order || tcx.generics_of(step.def_id).count() != pargs.len() {
                    return Err(format!("{name}: cannot map the arguments of `{}::step`", canon(tcx, adt.did())));
                }
                let args: Vec<GenericArg<'tcx>> = pargs.iter().collect();
                return Ok(Instance::new_raw(step.def_id, tcx.mk_args(&args)));
            }
            Err(format!("{name}: `{}` has no inherent `step` method", canon(tcx, adt.did())))
        }
        _ => Err(format!("{name}: expected a closure, a fn item, or a process type, found `{arg}`")),
    }
}

fn root_name<'tcx>(tcx: TyCtxt<'tcx>, n: Instance<'tcx>, parent: &FxHashMap<Instance<'tcx>, Option<Instance<'tcx>>>) -> String {
    let mut cur = n;
    while let Some(Some(p)) = parent.get(&cur) {
        cur = *p;
    }
    canon(tcx, cur.def_id())
}

fn path_to<'tcx>(tcx: TyCtxt<'tcx>, n: Instance<'tcx>, parent: &FxHashMap<Instance<'tcx>, Option<Instance<'tcx>>>) -> String {
    let mut names = vec![canon(tcx, n.def_id())];
    let mut cur = n;
    while let Some(Some(p)) = parent.get(&cur) {
        names.push(canon(tcx, p.def_id()));
        cur = *p;
    }
    names.reverse();
    names.join(" -> ")
}

#[allow(dead_code)]
fn span_of(scan: &Scan<'_>) -> Option<Span> {
    scan.sites.first().map(|s| s.span)
}
