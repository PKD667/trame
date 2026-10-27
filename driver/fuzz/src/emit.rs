//! Rust source for one abstract program: one example file (a crate of its own, see the README).
//! Every statement reads `x`. Wrappers that need an item (helper fns, impls) are hoisted to the top
//! level, named by a counter, so the emitted text is a function of the program alone.

use crate::prog::{Allow, Forb, Level, Prog, Stmt, Wrap};

struct Emit {
    items: Vec<String>,
    counter: usize,
}

impl Emit {
    fn fresh(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }
}

/// The source of example `id` (a valid crate name, e.g. `e0007`).
pub fn source(prog: &Prog) -> String {
    let mut e = Emit { items: Vec::new(), counter: 0 };
    let mut out = String::new();
    out.push_str(&format!(
        "// trame-fuzz: seed={} index={} (driver/fuzz/README.md). Generated; the expectation is in expect.jsonl.\n",
        prog.seed, prog.index
    ));
    out.push_str("#![allow(dead_code, unused, non_camel_case_types, non_upper_case_globals, unused_must_use)]\n");
    out.push_str("static GLOB: u32 = 3;\nfn nop0(_y: u32) {}\nfn nop1(_y: u32) {}\n");
    for (i, f) in prog.funcs.iter().enumerate() {
        let body = list(&f.body, &mut e);
        out.push_str(&format!("fn f{i}(x: u32) {{\n{body}}}\n"));
    }
    for (k, r) in prog.roots.iter().enumerate() {
        let body = list(&r.body, &mut e);
        match r.level {
            Level::Step => out.push_str(&format!(
                "pub struct P{k} {{ pub x: u32 }}\nimpl P{k} {{ pub fn step(&mut self) {{\nlet x = self.x;\n{body}}} }}\n"
            )),
            _ => out.push_str(&format!("fn r{k}(x: u32) {{\n{body}}}\n")),
        }
    }
    for item in &e.items {
        out.push_str(item);
    }
    out.push_str("fn main() {\n");
    for (k, r) in prog.roots.iter().enumerate() {
        out.push_str(&match r.level {
            Level::Item => format!("    trame::__check::item(&r{k});\n"),
            Level::Worker => format!("    trame::__check::worker(&r{k});\n"),
            Level::Step => format!("    let p{k} = P{k} {{ x: 1 }};\n    trame::__check::step(&p{k});\n"),
        });
    }
    out.push_str("}\n");
    out
}

fn list(stmts: &[Stmt], e: &mut Emit) -> String {
    let mut s = String::new();
    for st in stmts {
        s.push_str(&stmt(st, e));
        s.push('\n');
    }
    s
}

fn forb(c: Forb) -> &'static str {
    match c {
        Forb::Println => "println!(\"{}\", x);",
        Forb::Spawn => "let _ = std::thread::spawn(|| {});",
        Forb::Static => "let _ = GLOB + x;",
        Forb::Cell => "let _ = std::cell::Cell::new(x).get();",
        Forb::DynCall => "{ let d: &dyn Fn(u32) = if x & 1 == 0 { &|_y: u32| {} } else { &|_y: u32| {} }; d(x); }",
        Forb::FnPtr => "{ let g: fn(u32) = if x & 1 == 0 { nop0 } else { nop1 }; g(x); }",
        Forb::Unsafe => "let _ = unsafe { *(&x as *const u32) };",
        Forb::Send => "trame::io::send(x);",
        Forb::Recv => "let _ = trame::io::recv();",
        Forb::Now => "let _ = trame::time::now();",
        Forb::Barrier => "trame::io::barrier();",
        Forb::Concurrent => "trame::concurrent::concurrent();",
        Forb::VecPush => "{ let mut v: Vec<u32> = Vec::new(); v.push(x); }",
        Forb::RangeFromForEach => "(x..).for_each(|_i: u32| {});",
    }
}

fn allow(a: Allow) -> &'static str {
    match a {
        Allow::Arith => "let _ = x.wrapping_mul(3).wrapping_add(1);",
        Allow::OptMap => "let _ = Some(x).map(|y: u32| y.wrapping_add(2));",
        Allow::Index => "{ let arr = [1u32, 2, 3, 4]; let _ = arr[(x & 3) as usize]; }",
        Allow::Exp => "let _ = trame::math::exp(x as f32);",
        Allow::Min => "let _ = x.min(7);",
    }
}

fn stmt(st: &Stmt, e: &mut Emit) -> String {
    match st {
        Stmt::Forb(c) => forb(*c).to_string(),
        Stmt::Allow(a) => allow(*a).to_string(),
        Stmt::Call(j) => format!("f{j}(x);"),
        Stmt::For(b) => format!("for _i in 0..(x & 3) {{\n{}}}", list(b, e)),
        Stmt::While(b) => format!("{{ let mut w = x; while w != 0 {{ w >>= 1;\n{}}} }}", list(b, e)),
        Stmt::If(mask, a, b) => format!("if x & {mask} == 0 {{\n{}}} else {{\n{}}}", list(a, e), list(b, e)),
        Stmt::Wrap(w, b) => wrap(*w, b, e),
    }
}

/// A wrapper: the statements `b` moved into the body of its kind. Each kind is a construct the
/// theory names; the comment on each says which.
fn wrap(w: Wrap, b: &[Stmt], e: &mut Emit) -> String {
    match w {
        // A helper fn, called by name.
        Wrap::Helper => {
            let n = e.fresh("h");
            let inner = list(b, e);
            e.items.push(format!("fn {n}(x: u32) {{\n{inner}}}\n"));
            format!("{n}(x);")
        }
        // A generic fn, instantiated with u8.
        Wrap::Generic => {
            let n = e.fresh("g");
            let inner = list(b, e);
            e.items.push(format!("fn {n}<T>(x: u32, _t: T) {{\n{inner}}}\n"));
            format!("{n}(x, 0u8);")
        }
        // A closure passed to Iterator::for_each.
        Wrap::ForEach => format!("(0..(x & 1)).for_each(|_i: u32| {{\n{}}});", list(b, e)),
        // A closure passed to Iterator::map (consumed by count).
        Wrap::IterMap => format!("let _ = (0..(x & 1)).map(|_i: u32| {{\n{}0u32 }}).count();", list(b, e)),
        // A closure passed to Option::map.
        Wrap::OptionMap => format!("let _ = Some(x).map(|_y: u32| {{\n{}}});", list(b, e)),
        // A trait method, called under static dispatch.
        Wrap::Trait => {
            let n = e.fresh("T");
            let inner = list(b, e);
            e.items.push(format!(
                "pub trait Tr{n} {{ fn run(&self, x: u32); }}\npub struct S{n};\nimpl Tr{n} for S{n} {{ fn run(&self, x: u32) {{\n{inner}}} }}\n"
            ));
            format!("S{n}.run(x);")
        }
        // A Drop impl's body, run by the drop at the end of a block.
        Wrap::Drop => {
            let n = e.fresh("D");
            let inner = list(b, e);
            e.items.push(format!(
                "pub struct D{n}(u32);\nimpl Drop for D{n} {{ fn drop(&mut self) {{\nlet x = self.0;\n{inner}}} }}\n"
            ));
            format!("{{ let _d = D{n}(x); }}")
        }
        // An operator impl, Add::add.
        Wrap::Add => {
            let n = e.fresh("A");
            let inner = list(b, e);
            e.items.push(format!(
                "pub struct A{n}(u32);\nimpl std::ops::Add<()> for A{n} {{ type Output = (); fn add(self, _r: ()) {{\nlet x = self.0;\n{inner}}} }}\n"
            ));
            format!("let _ = A{n}(x) + ();")
        }
        // A Deref impl, reached by the `*` operator.
        Wrap::Deref => {
            let n = e.fresh("Q");
            let inner = list(b, e);
            e.items.push(format!(
                "pub struct Q{n}(u32);\nimpl std::ops::Deref for Q{n} {{ type Target = (); fn deref(&self) -> &() {{\nlet x = self.0;\n{inner}&() }} }}\n"
            ));
            format!("let _ = *Q{n}(x);")
        }
        // A From impl, reached by .into().
        Wrap::Into => {
            let n = e.fresh("F");
            let inner = list(b, e);
            e.items.push(format!(
                "pub struct F{n}(u32);\nimpl From<F{n}> for () {{ fn from(f: F{n}) {{\nlet x = f.0;\n{inner}}} }}\n"
            ));
            format!("let _: () = F{n}(x).into();")
        }
        // EXTRA (outside the theory): a user impl of a core trait for a core type, called by core
        // code. `Iterator::sum` calls `impl Sum<U> for u32`; the driver reads only impls of user
        // types (scan.rs, `impl_edges`), so this body is never read.
        Wrap::SumForeign => {
            let n = e.fresh("U");
            let inner = list(b, e);
            e.items.push(format!(
                "pub struct U{n}(u32);\nimpl std::iter::Sum<U{n}> for u32 {{ fn sum<I: Iterator<Item = U{n}>>(it: I) -> u32 {{\nlet x = it.size_hint().0 as u32;\n{inner}0 }} }}\n"
            ));
            format!("let _ = [U{n}(x)].into_iter().sum::<u32>();")
        }
    }
}
