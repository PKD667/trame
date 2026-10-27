//! Delta-debugging of a mismatching program. Each round proposes every single-edit mutant of the
//! current program and checks them all in one batch (one driver run). The first mutant that is
//! strictly smaller and keeps the mismatch key (see verdict::Diff::keys) becomes the new program.
//! Edits: drop a statement; unwrap a wrapper, loop or `if` (keep its body); inline a call to a
//! function called once and drop that function; drop a root or a function that nothing calls.

use crate::batch::{self, Case};
use crate::prog::{size, Func, Prog, Root, Stmt, Wrap};
use std::path::Path;

/// Single-edit variants of a statement list.
fn list_mutants(list: &[Stmt], funcs: &[Func]) -> Vec<Vec<Stmt>> {
    let mut out = Vec::new();
    for i in 0..list.len() {
        // drop statement i
        let mut v = list.to_vec();
        v.remove(i);
        out.push(v);
        // replace statement i by something smaller
        let splices: Vec<Vec<Stmt>> = match &list[i] {
            Stmt::Wrap(_, b) | Stmt::While(b) | Stmt::For(b) => vec![b.clone()],
            Stmt::If(_, a, b) => vec![a.clone(), b.clone()],
            Stmt::Call(j) if count_calls(list, *j) == 1 && !calls_itself(&funcs[*j], *j) => {
                vec![funcs[*j].body.clone()]
            }
            _ => vec![],
        };
        for body in splices {
            let mut v = list[..i].to_vec();
            v.extend(body);
            v.extend_from_slice(&list[i + 1..]);
            out.push(v);
        }
        // mutate inside statement i
        for (child, rebuild) in children(&list[i]) {
            for m in list_mutants(&child, funcs) {
                let mut v = list.to_vec();
                v[i] = rebuild(m);
                out.push(v);
            }
        }
    }
    out
}

fn count_calls(list: &[Stmt], j: usize) -> usize {
    list.iter()
        .map(|s| match s {
            Stmt::Call(k) => (*k == j) as usize,
            Stmt::While(b) | Stmt::For(b) | Stmt::Wrap(_, b) => count_calls(b, j),
            Stmt::If(_, a, b) => count_calls(a, j) + count_calls(b, j),
            _ => 0,
        })
        .sum()
}

fn calls_itself(f: &Func, j: usize) -> bool {
    count_calls(&f.body, j) > 0
}

type Rebuild = Box<dyn Fn(Vec<Stmt>) -> Stmt>;

/// The child lists of a statement, each with the function that rebuilds the statement around a
/// new list.
fn children(s: &Stmt) -> Vec<(Vec<Stmt>, Rebuild)> {
    match s {
        Stmt::While(b) => {
            vec![(b.clone(), Box::new(|v| Stmt::While(v)) as Rebuild)]
        }
        Stmt::For(b) => vec![(b.clone(), Box::new(|v| Stmt::For(v)) as Rebuild)],
        Stmt::Wrap(w, b) => {
            let w: Wrap = *w;
            vec![(b.clone(), Box::new(move |v| Stmt::Wrap(w, v)) as Rebuild)]
        }
        Stmt::If(m, a, b) => {
            let (m, b2, a2) = (*m, b.clone(), a.clone());
            vec![
                (a.clone(), Box::new(move |v| Stmt::If(m, v, b2.clone())) as Rebuild),
                (b.clone(), Box::new(move |v| Stmt::If(m, a2.clone(), v)) as Rebuild),
            ]
        }
        _ => vec![],
    }
}

/// Renumber calls after function `j` is removed; calls to `j` must already be gone.
fn renumber(list: &[Stmt], j: usize) -> Vec<Stmt> {
    list.iter()
        .map(|s| match s {
            Stmt::Call(k) => Stmt::Call(if *k > j { k - 1 } else { *k }),
            Stmt::While(b) => Stmt::While(renumber(b, j)),
            Stmt::For(b) => Stmt::For(renumber(b, j)),
            Stmt::Wrap(w, b) => Stmt::Wrap(*w, renumber(b, j)),
            Stmt::If(m, a, b) => Stmt::If(*m, renumber(a, j), renumber(b, j)),
            other => other.clone(),
        })
        .collect()
}

/// Remove function `j` from a program (its calls must be gone).
fn drop_func(p: &Prog, j: usize) -> Prog {
    let mut q = p.clone();
    q.funcs.remove(j);
    for f in &mut q.funcs {
        f.body = renumber(&f.body, j);
    }
    for r in &mut q.roots {
        r.body = renumber(&r.body, j);
    }
    q
}

fn calls_anywhere(p: &Prog, j: usize) -> bool {
    p.funcs.iter().any(|f| count_calls(&f.body, j) > 0) || p.roots.iter().any(|r| count_calls(&r.body, j) > 0)
}

/// All single-edit variants of a program.
pub fn mutants(p: &Prog) -> Vec<Prog> {
    let mut out = Vec::new();
    for j in 0..p.funcs.len() {
        if !calls_anywhere(p, j) {
            out.push(drop_func(p, j));
        }
    }
    for k in 0..p.roots.len() {
        if p.roots.len() > 1 {
            let mut q = p.clone();
            q.roots.remove(k);
            out.push(q);
        }
        for body in list_mutants(&p.roots[k].body, &p.funcs) {
            let mut q = p.clone();
            q.roots[k] = Root { level: p.roots[k].level, body };
            out.push(q);
        }
    }
    for f in 0..p.funcs.len() {
        for body in list_mutants(&p.funcs[f].body, &p.funcs) {
            let mut q = p.clone();
            q.funcs[f] = Func { body };
            out.push(q);
        }
    }
    // An inlined call's function can then be dropped too: handled by the call-once splice in
    // list_mutants, followed by drop_func in the next round.
    out.retain(|q| size(q) < size(p));
    out
}

pub struct Shrunk {
    pub prog: Prog,
    pub rounds: usize,
    pub candidates: usize,
}

/// Shrink `p` while the batch verdict keeps `key`. `work` is `work/` (see batch.rs), `tag` names the
/// round directories; `driver_dir` is the driver's bin dir. The check is done by the driver, not by R alone.
pub fn shrink(p: Prog, key: &str, work: &Path, tag: &str, driver_dir: Option<&Path>, max_rounds: usize) -> Result<Shrunk, String> {
    let mut cur = p;
    let mut rounds = 0;
    let mut tried = 0;
    loop {
        if rounds >= max_rounds {
            break;
        }
        let cands = mutants(&cur);
        if cands.is_empty() {
            break;
        }
        rounds += 1;
        tried += cands.len();
        let cases: Vec<Case> = cands.iter().enumerate().map(|(i, q)| Case::new(format!("s{i:05}"), q.clone())).collect();
        let dir = work.join(format!("{tag}-r{rounds}"));
        let outcomes = batch::run(&dir, cases, driver_dir)
            .map_err(|hs| format!("harness problem while shrinking: {}", hs.join(" | ")))?;
        let next = outcomes.iter().position(|o| o.diff.keys().iter().any(|k| k == key));
        match next {
            Some(i) => cur = cands[i].clone(),
            None => break,
        }
        batch::clean_target(&dir);
    }
    Ok(Shrunk { prog: cur, rounds, candidates: tried })
}
