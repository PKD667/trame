//! The driver's verdict V(P), read from cargo's output, and the comparison with R(P).
//!
//! The driver prints, per refused site, `error: [rule] <level> ...` with an `--> file:line` span and
//! a `= note: reached from root X: ...` note (recursion refusals have no note). Cargo prints the
//! diagnostics of one example, then `error: could not compile` naming that example. A diagnostic
//! goes to the example its span names. A span outside every example is inside core or alloc (the
//! driver reads their bodies) and is kept as an *internal* record.

use std::collections::BTreeSet;

use crate::oracle::{Expect, Refusal};
use crate::prog::Level;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VRec {
    pub rule: String,
    /// `item`, `step` or `worker`.
    pub level: String,
    /// The root the note names; None for a recursion refusal (the driver names none).
    pub root: Option<String>,
    /// The span is in core/alloc, not in the program: the refusal comes from reading library code.
    pub internal: bool,
    /// The reach note passes through `core::ptr::drop_in_place`: the refusal comes from drop glue.
    pub via_glue: bool,
}

impl VRec {
    pub fn describe(&self) -> String {
        let at = match (self.internal, self.via_glue) {
            (true, true) => " [in core/alloc, drop glue]",
            (true, false) => " [in core/alloc]",
            _ => "",
        };
        match &self.root {
            Some(r) => format!("[{}] {} (root {}){at}", self.rule, self.level, r),
            None => format!("[{}] {} (no root named){at}", self.rule, self.level),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Unit {
    pub recs: Vec<VRec>,
    /// The diagnostic lines printed under this example, verbatim.
    pub raw: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Parsed {
    /// Example name -> its refusals. An example that cargo built cleanly has no entry.
    pub units: std::collections::BTreeMap<String, Unit>,
    /// Anything that is not a driver refusal: a rustc error, an unattributed line, a cargo failure,
    /// the stand-in's own `[root]` check. Any entry here means the run is invalid.
    pub harness: Vec<String>,
}

struct Pending {
    rec: VRec,
    /// The example whose file the span names, if any.
    span_unit: Option<String>,
    raw: Vec<String>,
}

/// The example name in an `--> examples/NAME.rs:L:C` span, if the span is in an example.
fn example_of_span(path: &str) -> Option<String> {
    let i = path.find("examples/")?;
    let rest = &path[i + "examples/".len()..];
    let name = rest.split(':').next()?;
    name.strip_suffix(".rs").map(str::to_string)
}

/// `error: could not compile `pkg` (example "NAME") due to ...`: the example NAME.
fn unit_of_summary(line: &str) -> Option<String> {
    let i = line.find("(example \"")?;
    let rest = &line[i + "(example \"".len()..];
    rest.split('"').next().map(str::to_string)
}

pub fn parse(out: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let mut cur: Option<Pending> = None;
    let mut pending: Vec<Pending> = Vec::new();

    fn finish(cur: &mut Option<Pending>, pending: &mut Vec<Pending>) {
        if let Some(p) = cur.take() {
            pending.push(p);
        }
    }

    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("error: [") {
            finish(&mut cur, &mut pending);
            let Some((rule, after)) = rest.split_once("] ") else {
                parsed.harness.push(line.to_string());
                continue;
            };
            let level = after.split_whitespace().next().unwrap_or("").to_string();
            if rule == "root" {
                // The stand-in's own check (check.rs), not an analysis refusal: a generator bug.
                parsed.harness.push(line.to_string());
                continue;
            }
            if !matches!(level.as_str(), "item" | "step" | "worker") {
                parsed.harness.push(format!("unrecognised level in: {line}"));
            }
            cur = Some(Pending {
                rec: VRec { rule: rule.to_string(), level, root: None, internal: true, via_glue: false },
                span_unit: None,
                raw: vec![line.to_string()],
            });
            continue;
        }
        if line.starts_with("error: could not compile `") {
            finish(&mut cur, &mut pending);
            match unit_of_summary(line) {
                Some(unit) => {
                    for p in pending.drain(..) {
                        let mut rec = p.rec;
                        let target = match &p.span_unit {
                            Some(u) => {
                                rec.internal = false;
                                u.clone()
                            }
                            None => unit.clone(),
                        };
                        let entry = parsed.units.entry(target).or_default();
                        entry.recs.push(rec);
                        entry.raw.extend(p.raw);
                    }
                    parsed.units.entry(unit).or_default().raw.push(line.to_string());
                }
                None => {
                    parsed.harness.push(line.to_string());
                    pending.clear();
                }
            }
            continue;
        }
        if line.starts_with("error") {
            // rustc errors (`error[E...]`), cargo failures, anything else: the run is invalid.
            finish(&mut cur, &mut pending);
            parsed.harness.push(line.to_string());
            continue;
        }
        if let Some(p) = cur.as_mut() {
            p.raw.push(line.to_string());
            let t = line.trim_start();
            if let Some(span) = t.strip_prefix("--> ")
                && p.span_unit.is_none()
            {
                p.span_unit = example_of_span(span);
            }
            if let Some(i) = line.find("reached from root ") {
                let rest = &line[i + "reached from root ".len()..];
                let name = rest.split(": ").next().unwrap_or(rest).trim();
                p.rec.root = Some(name.to_string());
                p.rec.via_glue = rest.contains("drop_in_place");
            }
            if line.trim().is_empty() {
                finish(&mut cur, &mut pending);
            }
        }
    }
    finish(&mut cur, &mut pending);
    if !pending.is_empty() {
        parsed.harness.push(format!("{} diagnostic(s) with no `could not compile` line after them", pending.len()));
    }
    parsed
}

/// R against V for one example. Levels are the unit of verdict; rules are attribution.
///
/// - verdict miss (soundness, severe): R refuses at a level and V reports nothing at that level.
/// - attribution miss: R refuses (rule, level), V reports something at that level, but not that rule
///   in the program's own code. The driver stops reading a callee after a refused call, so this is
///   expected in some programs; it is counted and reported, not hidden.
/// - over-refusal (precision): V refuses at a level where R accepts, from program code or from
///   core internals. A core-internal refusal reached through drop glue is undecided instead (the
///   spec does not settle whether drop glue is code the rules apply to; see README.md).
/// - extra rule: V's rule at a level R refuses, that R does not predict.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Diff {
    pub verdict_misses: Vec<Level>,
    pub attr_misses: Vec<Refusal>,
    pub over: Vec<VRec>,
    pub undecided: Vec<VRec>,
    pub extra: Vec<VRec>,
}

impl Diff {
    /// Agreement: no verdict miss, no attribution miss, no over-refusal, no extra rule. Undecided
    /// records do not break agreement; they are counted.
    pub fn agrees(&self) -> bool {
        self.verdict_misses.is_empty() && self.attr_misses.is_empty() && self.over.is_empty() && self.extra.is_empty()
    }

    /// The differences as keys that name the bug. Shrinking keeps programs that keep one key.
    pub fn keys(&self) -> Vec<String> {
        let mut ks: Vec<String> = Vec::new();
        ks.extend(self.verdict_misses.iter().map(|l| format!("verdict-miss {}", l.noun())));
        ks.extend(self.attr_misses.iter().map(|r| format!("attr-miss {} {}", r.rule, r.level.noun())));
        ks.extend(self.over.iter().map(|v| format!("over {} {}", v.level, v.rule)));
        ks.extend(self.extra.iter().map(|v| format!("extra {} {}", v.rule, v.level)));
        ks.sort();
        ks.dedup();
        ks
    }

    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        for l in &self.verdict_misses {
            parts.push(format!("VERDICT-MISS at {} (R refuses, V accepts at this level)", l.noun()));
        }
        for r in &self.attr_misses {
            let roots = if r.roots.is_empty() { "any root".to_string() } else { r.roots.join("|") };
            parts.push(format!("ATTR-MISS [{}] {} (root {})", r.rule, r.level.noun(), roots));
        }
        for v in &self.over {
            parts.push(format!("OVER-REFUSAL {}", v.describe()));
        }
        for v in &self.extra {
            parts.push(format!("EXTRA {}", v.describe()));
        }
        if parts.is_empty() {
            return if self.undecided.is_empty() { "agree".into() } else { format!("agree (undecided: {})", self.undecided.len()) };
        }
        parts.join("; ")
    }
}

pub fn compare(e: &Expect, recs: &[VRec]) -> Diff {
    let mut d = Diff::default();
    let r_levels: BTreeSet<&str> = e.refusals.iter().map(|r| r.level.noun()).collect();
    let v_levels: BTreeSet<&str> = recs.iter().map(|v| v.level.as_str()).collect();

    for l in &r_levels {
        if !v_levels.contains(l) {
            d.verdict_misses.push(Level::parse(l).expect("oracle level"));
        }
    }
    for r in &e.refusals {
        if !v_levels.contains(r.level.noun()) {
            continue;
        }
        let own = recs.iter().any(|v| !v.internal && v.rule == r.rule && v.level == r.level.noun());
        if !own {
            d.attr_misses.push(r.clone());
        }
    }
    for v in recs {
        if r_levels.contains(v.level.as_str()) {
            if v.internal {
                continue; // a core-internal refusal at a level R refuses: attributed to R's refusal
            }
            let predicted = e.refusals.iter().any(|r| r.rule == v.rule && r.level.noun() == v.level);
            if !predicted {
                d.extra.push(v.clone());
            }
        } else if v.internal && v.via_glue {
            d.undecided.push(v.clone());
        } else {
            d.over.push(v.clone());
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prog::Level;

    const SAMPLE: &str = "\
warning: unused variable
error: [std] item calls std::io::stdio::_print: no std (spec.md, Requirement (All levels))
 --> examples/e0001.rs:1:16
  |
  = note: reached from root r0: r0

error: [recursion] item recurses: f0 -> f0 (spec.md, Requirement (Item))
 --> examples/e0002.rs:1:32

error: could not compile `pkg` (example \"e0001\") due to 1 previous error
error: could not compile `pkg` (example \"e0002\") due to 1 previous error
";

    #[test]
    fn attributes_diagnostics_to_examples() {
        let p = parse(SAMPLE);
        assert!(p.harness.is_empty(), "{:?}", p.harness);
        let u1 = &p.units["e0001"];
        assert_eq!(
            u1.recs,
            vec![VRec { rule: "std".into(), level: "item".into(), root: Some("r0".into()), internal: false, via_glue: false }]
        );
        let u2 = &p.units["e0002"];
        assert_eq!(u2.recs, vec![VRec { rule: "recursion".into(), level: "item".into(), root: None, internal: false, via_glue: false }]);
    }

    #[test]
    fn internal_span_is_internal() {
        let p = parse(
            "error: [unsafe] item contains dereference of a raw pointer: no unsafe code (spec.md, x)\n   --> /nix/store/x/core/ptr/mod.rs:809:1\n    = note: reached from root f: f -> core::ptr::drop_in_place\n\nerror: could not compile `pkg` (example \"e0009\") due to 1 previous error\n",
        );
        assert!(p.harness.is_empty(), "{:?}", p.harness);
        assert!(p.units["e0009"].recs[0].internal);
    }

    #[test]
    fn rustc_error_is_harness() {
        let p = parse("error[E0425]: cannot find value `x`\nerror: could not compile `pkg` (example \"e0003\")\n");
        assert!(!p.harness.is_empty());
    }

    #[test]
    fn compare_matches_on_level_rule_and_root() {
        let e = Expect {
            roots: vec![("r0".into(), Level::Item)],
            refusals: vec![Refusal { rule: "std".into(), level: Level::Item, roots: vec!["r0".into()] }],
            drop_glue: false,
        };
        let recs = vec![VRec { rule: "std".into(), level: "item".into(), root: Some("r0".into()), internal: false, via_glue: false }];
        assert!(compare(&e, &recs).agrees());
        assert!(!compare(&e, &[]).agrees());
        assert_eq!(compare(&e, &[]).verdict_misses, vec![Level::Item]);
    }

    #[test]
    fn drop_glue_unsafe_is_undecided_not_over_refusal() {
        let e = Expect { roots: vec![], refusals: vec![], drop_glue: true };
        let recs = vec![VRec { rule: "unsafe".into(), level: "item".into(), root: None, internal: true, via_glue: true }];
        let d = compare(&e, &recs);
        assert!(d.agrees());
        assert_eq!(d.undecided.len(), 1);
    }

    #[test]
    fn over_refusal_is_reported() {
        let e = Expect { roots: vec![], refusals: vec![], drop_glue: false };
        let recs = vec![VRec { rule: "unsafe".into(), level: "item".into(), root: None, internal: true, via_glue: false }];
        assert_eq!(compare(&e, &recs).over.len(), 1);
    }
}
