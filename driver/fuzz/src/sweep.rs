//! The exhaustive metamorphic sweep (driver/fuzz/README.md, "Sweep").
//!
//! For every forbidden construct c and every level l at which c is refused, we build:
//!   bare      root at l: [c]
//!   dead      bare, plus an uncalled function that holds a forbidden construct
//!   w1(c)     root at l: [w(c)]                       for every normative wrapper w   (depth 1)
//!   w1w2(c)   root at l: [w1(w2(c))]                  for every ordered pair           (depth 2)
//! R is the expectation for each. Metamorphic checks: the set of (rule, level) refusals of every
//! wrapped or dead program equals the set of its bare program.

use std::collections::BTreeSet;
use std::path::Path;

use crate::batch::{self, Case, Outcome};
use crate::prog::{Forb, Func, Level, Prog, Root, Stmt, Wrap};

pub struct Item {
    /// e.g. `bare println@worker`, `w1 optmap(println)@item`, `dead ...`
    pub label: String,
    /// The bare program this item must agree with (group key), `None` for bare items.
    pub group: String,
    pub prog: Prog,
    pub is_bare: bool,
}

fn prog_of(level: Level, body: Vec<Stmt>, funcs: Vec<Func>) -> Prog {
    Prog { seed: 0, index: 0, funcs, roots: vec![Root { level, body }] }
}

pub fn items() -> Vec<Item> {
    let mut out = Vec::new();
    for c in Forb::ALL {
        for &l in c.levels() {
            let group = format!("{}@{}", c.name(), l.noun());
            let bare = vec![Stmt::Forb(c)];
            out.push(Item {
                label: format!("bare {group}"),
                group: group.clone(),
                prog: prog_of(l, bare.clone(), vec![]),
                is_bare: true,
            });
            let dead = Func { body: vec![Stmt::Forb(Forb::Spawn)] };
            out.push(Item {
                label: format!("dead {group}"),
                group: group.clone(),
                prog: prog_of(l, bare.clone(), vec![dead]),
                is_bare: false,
            });
            for w in Wrap::NORMATIVE {
                out.push(Item {
                    label: format!("w1 {}({}) @{}", w.name(), c.name(), l.noun()),
                    group: group.clone(),
                    prog: prog_of(l, vec![Stmt::Wrap(w, bare.clone())], vec![]),
                    is_bare: false,
                });
                for w2 in Wrap::NORMATIVE {
                    out.push(Item {
                        label: format!("w2 {}({}({})) @{}", w.name(), w2.name(), c.name(), l.noun()),
                        group: group.clone(),
                        prog: prog_of(l, vec![Stmt::Wrap(w, vec![Stmt::Wrap(w2, bare.clone())])], vec![]),
                        is_bare: false,
                    });
                }
            }
        }
    }
    out
}

/// The (rule, level) set of a verdict, without root names.
/// The (rule, level) pairs the driver reported.
fn rule_set(o: &Outcome) -> BTreeSet<(String, String)> {
    o.unit.recs.iter().map(|r| (r.rule.clone(), r.level.clone())).collect()
}

/// The (rule, level) pairs the oracle predicts.
fn predicted(o: &Outcome) -> BTreeSet<(String, String)> {
    o.expect.refusals.iter().map(|r| (r.rule.clone(), r.level.noun().to_string())).collect()
}

/// The levels at which the driver refused.
fn levels(set: &BTreeSet<(String, String)>) -> BTreeSet<String> {
    set.iter().map(|(_, l)| l.clone()).collect()
}

pub struct Report {
    pub cases: usize,
    pub agreements: usize,
    pub verdict_misses: usize,
    pub over: usize,
    pub attr_misses: usize,
    pub extra: usize,
    pub meta_violations: Vec<String>,
    pub mismatches: Vec<(String, Outcome)>,
}

/// Run the sweep, in batches of `k` items. `work` holds the batch directories.
pub fn run(work: &Path, k: usize, driver_dir: Option<&Path>) -> Result<(Report, usize), String> {
    let all = items();
    let n = all.len();
    let mut rep = Report {
        cases: 0,
        agreements: 0,
        verdict_misses: 0,
        over: 0,
        attr_misses: 0,
        extra: 0,
        meta_violations: Vec::new(),
        mismatches: Vec::new(),
    };
    let mut outcomes: Vec<(Item, Outcome)> = Vec::new();
    let mut batches = 0;
    let mut iter = all.into_iter();
    loop {
        let chunk: Vec<Item> = iter.by_ref().take(k).collect();
        if chunk.is_empty() {
            break;
        }
        batches += 1;
        let cases: Vec<Case> =
            chunk.iter().enumerate().map(|(i, it)| Case::new(format!("q{:05}", i), it.prog.clone())).collect();
        let dir = work.join(format!("sweep-{batches}"));
        let res =
            batch::run(&dir, cases, driver_dir).map_err(|hs| format!("sweep batch {batches}: {}", hs.join(" | ")))?;
        batch::clean_target(&dir);
        for (it, o) in chunk.into_iter().zip(res) {
            outcomes.push((it, o));
        }
    }
    // Metamorphic (README, property 4): every item has its bare item's verdict at each level, and its
    // rules too when the oracle predicts the same rules for both. A wrapper refused at its own call
    // site (`.into()` on a user type) ends the walk there, so only its levels are compared.
    type Pairs = BTreeSet<(String, String)>;
    let bare: std::collections::BTreeMap<String, (Pairs, Pairs)> = outcomes
        .iter()
        .filter(|(it, _)| it.is_bare)
        .map(|(it, o)| (it.group.clone(), (rule_set(o), predicted(o))))
        .collect();
    for (it, o) in &outcomes {
        rep.cases += 1;
        rep.verdict_misses += o.diff.verdict_misses.len();
        rep.over += o.diff.over.len();
        rep.attr_misses += o.diff.attr_misses.len();
        rep.extra += o.diff.extra.len();
        if o.diff.agrees() {
            rep.agreements += 1;
        } else {
            rep.mismatches.push((it.label.clone(), o.clone()));
        }
        if let Some((b, b_predicted)) = bare.get(&it.group) {
            let s = rule_set(o);
            let differ = if predicted(o) == *b_predicted { &s != b } else { levels(&s) != levels(b) };
            if differ {
                rep.meta_violations.push(format!(
                    "{}: refusals {:?} differ from bare {}: {:?}",
                    it.label,
                    s.iter().map(|(r, l)| format!("{r}/{l}")).collect::<Vec<_>>(),
                    it.group,
                    b.iter().map(|(r, l)| format!("{r}/{l}")).collect::<Vec<_>>()
                ));
            }
        }
    }
    Ok((rep, n))
}
