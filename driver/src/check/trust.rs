//! Which bodies are judged in full, and which are trusted. Trust is a property of the call path
//! (driver/README.md, "Trame is trusted"): trame is the runtime and its dependencies are its
//! implementation. A body reached only by edges out of trusted bodies is trusted, and is read the same
//! way trame's own bodies are: for its effects, its edges and its allocations (`judge` in mod.rs).
//! Trust stops at the program: a root, or an `Own` edge (scan.rs), judges its callees in full.

use rustc_data_structures::fx::{FxHashMap, FxHashSet};
use rustc_middle::ty::{Instance, TyCtxt};

use super::scan::{Scan, is_trame_instance};

/// The bodies judged in full. Every other scanned body is trusted, and a trame body is never in the set.
/// A body is in the set when a root reaches it, or when an `Own` edge reaches it from anywhere, and then
/// everything it calls is in the set too. So a body whose every caller is trusted is trusted, however
/// long its chain of trusted calls, and a body that a program edge reaches is judged even when trame
/// also calls it.
pub fn judged_in_full<'tcx>(
    tcx: TyCtxt<'tcx>,
    roots: &[Instance<'tcx>],
    scans: &FxHashMap<Instance<'tcx>, Scan<'tcx>>,
) -> FxHashSet<Instance<'tcx>> {
    let mut judged = FxHashSet::default();
    let mut work = Vec::new();
    for n in roots.iter().copied().chain(scans.values().flat_map(|s| s.own_edges())) {
        mark(tcx, n, &mut judged, &mut work);
    }
    while let Some(n) = work.pop() {
        for (callee, _) in scans[&n].edges() {
            mark(tcx, callee, &mut judged, &mut work);
        }
    }
    judged
}

fn mark<'tcx>(
    tcx: TyCtxt<'tcx>,
    n: Instance<'tcx>,
    judged: &mut FxHashSet<Instance<'tcx>>,
    work: &mut Vec<Instance<'tcx>>,
) {
    if !is_trame_instance(tcx, n) && judged.insert(n) {
        work.push(n);
    }
}
