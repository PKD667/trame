//! The reference R(P): what the driver must refuse, computed from the abstract program alone.
//!
//! The program is a graph. A node is a function body: a named fn, a root, or a wrapper's body
//! (a closure, a helper, an impl method). Its sites are the statements that can be refused in
//! it, and its edges are the calls into other bodies. A node's level is the strictest level of
//! the roots that reach it (spec.md section 3, and the driver judges a function at its strictest
//! level). A refusal is a site whose construct is forbidden at that level; a cyclic node at Item
//! or Step is a recursion refusal.

use crate::prog::{root_name, Forb, Level, Prog, Stmt, Wrap};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    Forb(Forb),
    While,
    /// A refused call at the call site, at every level: the allow-list rule (driver/lists).
    AllowList,
}

#[derive(Debug, Default)]
pub struct Node {
    pub sites: Vec<Site>,
    pub edges: Vec<usize>,
}

#[derive(Debug)]
pub struct Graph {
    pub nodes: Vec<Node>,
    /// (level, node of the root body, name the driver prints for it)
    pub roots: Vec<(Level, usize, String)>,
}

pub fn graph(prog: &Prog) -> Graph {
    let nf = prog.funcs.len();
    let mut g = Graph { nodes: (0..nf + prog.roots.len()).map(|_| Node::default()).collect(), roots: Vec::new() };
    for (i, f) in prog.funcs.iter().enumerate() {
        build(&mut g, i, &f.body);
    }
    for (k, r) in prog.roots.iter().enumerate() {
        build(&mut g, nf + k, &r.body);
        g.roots.push((r.level, nf + k, root_name(k, r.level)));
    }
    g
}

/// Add the sites and edges of `stmts` to `node`. A wrapper gets a node of its own.
fn build(g: &mut Graph, node: usize, stmts: &[Stmt]) {
    for s in stmts {
        match s {
            Stmt::Forb(c) => g.nodes[node].sites.push(Site::Forb(*c)),
            Stmt::While(b) => {
                g.nodes[node].sites.push(Site::While);
                build(g, node, b);
            }
            Stmt::Allow(_) => {}
            Stmt::Call(j) => g.nodes[node].edges.push(*j),
            Stmt::For(b) => build(g, node, b),
            Stmt::If(_, a, b) => {
                build(g, node, a);
                build(g, node, b);
            }
            Stmt::Wrap(w, b) => {
                // `.into()` on a user type: Into::into is a default method of core, which the
                // allow-list does not match on user types (driver/lists/A_I.txt header). The call
                // is refused at the call site; the From impl's body is still a node of its own.
                if *w == Wrap::Into {
                    g.nodes[node].sites.push(Site::AllowList);
                }
                let id = g.nodes.len();
                g.nodes.push(Node::default());
                g.nodes[node].edges.push(id);
                build(g, id, b);
            }
        }
    }
}

/// One refusal R predicts: `rule` at `level`, attributed to one of `roots` (any root if empty:
/// the driver's recursion refusal names no root).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub rule: String,
    pub level: Level,
    pub roots: Vec<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Expect {
    /// (root name, level) of every root, in program order.
    pub roots: Vec<(String, Level)>,
    pub refusals: Vec<Refusal>,
    /// The program has a Drop impl. The driver then reads the drop glue, which contains a raw
    /// pointer dereference (`core::ptr::drop_in_place`). Whether that is `unsafe` code is not
    /// settled by spec.md (it says only that drop glue counts as a call), so an `unsafe` refusal
    /// from core internals is reported as undecided, not as a mismatch. See README.md.
    pub drop_glue: bool,
}

impl Expect {
    pub fn accepts(&self) -> bool {
        self.refusals.is_empty()
    }
}

fn reachable(g: &Graph, from: usize) -> Vec<bool> {
    let mut seen = vec![false; g.nodes.len()];
    let mut stack = vec![from];
    seen[from] = true;
    while let Some(n) = stack.pop() {
        for &m in &g.nodes[n].edges {
            if !seen[m] {
                seen[m] = true;
                stack.push(m);
            }
        }
    }
    seen
}

fn has_wrap(prog: &Prog, w: Wrap) -> bool {
    fn in_list(list: &[Stmt], w: Wrap) -> bool {
        list.iter().any(|s| match s {
            Stmt::Wrap(k, b) => *k == w || in_list(b, w),
            Stmt::While(b) | Stmt::For(b) => in_list(b, w),
            Stmt::If(_, a, b) => in_list(a, w) || in_list(b, w),
            _ => false,
        })
    }
    prog.funcs.iter().any(|f| in_list(&f.body, w)) || prog.roots.iter().any(|r| in_list(&r.body, w))
}

pub fn expect(prog: &Prog) -> Expect {
    let g = graph(prog);
    let n = g.nodes.len();
    let reach: Vec<Vec<bool>> = (0..n).map(|s| reachable(&g, s)).collect();

    // Level of each node: the strictest level among the roots that reach it; and those roots.
    let mut level: Vec<Option<Level>> = vec![None; n];
    let mut reached_by: Vec<Vec<(Level, String)>> = vec![Vec::new(); n];
    for (lv, rnode, name) in &g.roots {
        for m in 0..n {
            if reach[*rnode][m] {
                level[m] = Some(level[m].map_or(*lv, |l| l.min(*lv)));
                reached_by[m].push((*lv, name.clone()));
            }
        }
    }

    let mut out = Expect {
        roots: g.roots.iter().map(|(l, _, name)| (name.clone(), *l)).collect(),
        refusals: Vec::new(),
        drop_glue: has_wrap(prog, Wrap::Drop),
    };
    let add = |out: &mut Expect, rule: &str, lv: Level, roots: Vec<String>| {
        match out.refusals.iter_mut().find(|r| r.rule == rule && r.level == lv) {
            Some(r) => {
                for name in roots {
                    if !r.roots.contains(&name) {
                        r.roots.push(name);
                    }
                }
            }
            None => out.refusals.push(Refusal { rule: rule.to_string(), level: lv, roots }),
        }
    };

    for m in 0..n {
        let Some(lv) = level[m] else { continue };
        // Roots at the node's own level: the ones that name it in the driver's note.
        let names: Vec<String> = reached_by[m].iter().filter(|(l, _)| *l == lv).map(|(_, s)| s.clone()).collect();
        for site in &g.nodes[m].sites {
            match site {
                Site::Forb(c) => {
                    if c.levels().contains(&lv) {
                        for rule in c.rules() {
                            add(&mut out, rule, lv, names.clone());
                        }
                    }
                }
                Site::While => {
                    if lv != Level::Worker {
                        add(&mut out, "counted-loop", lv, names.clone());
                    }
                }
                Site::AllowList => add(&mut out, "allow-list", lv, names.clone()),
            }
        }
        // Cyclic: m reaches itself through at least one edge.
        let cyclic = g.nodes[m].edges.iter().any(|&s| reach[s][m]);
        if cyclic && lv != Level::Worker {
            add(&mut out, "recursion", lv, Vec::new());
        }
    }
    out.refusals.sort_by(|a, b| (a.level, &a.rule).cmp(&(b.level, &b.rule)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prog::{Func, Root};

    fn prog(funcs: Vec<Vec<Stmt>>, roots: Vec<(Level, Vec<Stmt>)>) -> Prog {
        Prog {
            seed: 0,
            index: 0,
            funcs: funcs.into_iter().map(|body| Func { body }).collect(),
            roots: roots.into_iter().map(|(level, body)| Root { level, body }).collect(),
        }
    }

    fn rules(e: &Expect) -> Vec<(String, Level)> {
        e.refusals.iter().map(|r| (r.rule.clone(), r.level)).collect()
    }

    #[test]
    fn println_is_std_and_unsafe_everywhere() {
        let p = prog(vec![], vec![(Level::Worker, vec![Stmt::Forb(Forb::Println)])]);
        let e = expect(&p);
        assert_eq!(rules(&e), vec![("std".into(), Level::Worker), ("unsafe".into(), Level::Worker)]);
    }

    #[test]
    fn send_is_refused_only_at_item() {
        let body = vec![Stmt::Forb(Forb::Send)];
        assert!(expect(&prog(vec![], vec![(Level::Step, body.clone())])).accepts());
        assert!(!expect(&prog(vec![], vec![(Level::Item, body)])).accepts());
    }

    #[test]
    fn strictest_level_wins() {
        // f is reached from a worker and from an item root: judged at item, attributed to the item.
        let p = prog(
            vec![vec![Stmt::Forb(Forb::Barrier)]],
            vec![(Level::Worker, vec![Stmt::Call(0)]), (Level::Item, vec![Stmt::Call(0)])],
        );
        let e = expect(&p);
        assert_eq!(e.refusals.len(), 1);
        assert_eq!(e.refusals[0].level, Level::Item);
        assert_eq!(e.refusals[0].roots, vec!["r1".to_string()]);
    }

    #[test]
    fn dead_function_is_invisible() {
        let p = prog(vec![vec![Stmt::Forb(Forb::Println)]], vec![(Level::Item, vec![])]);
        assert!(expect(&p).accepts());
    }

    #[test]
    fn self_call_is_recursion_at_item_and_step_not_worker() {
        let p = prog(vec![vec![Stmt::Call(0)]], vec![(Level::Item, vec![Stmt::Call(0)])]);
        assert_eq!(rules(&expect(&p)), vec![("recursion".into(), Level::Item)]);
        let p = prog(vec![vec![Stmt::Call(0)]], vec![(Level::Worker, vec![Stmt::Call(0)])]);
        assert!(expect(&p).accepts());
    }

    #[test]
    fn closure_cycle_is_recursion() {
        let p = prog(
            vec![vec![Stmt::Wrap(crate::prog::Wrap::OptionMap, vec![Stmt::Call(0)])]],
            vec![(Level::Step, vec![Stmt::Call(0)])],
        );
        assert_eq!(rules(&expect(&p)), vec![("recursion".into(), Level::Step)]);
    }

    #[test]
    fn while_is_refused_at_item_and_step_only() {
        let body = vec![Stmt::While(vec![])];
        assert_eq!(rules(&expect(&prog(vec![], vec![(Level::Step, body.clone())]))), vec![("counted-loop".into(), Level::Step)]);
        assert!(expect(&prog(vec![], vec![(Level::Worker, body)])).accepts());
    }
}
