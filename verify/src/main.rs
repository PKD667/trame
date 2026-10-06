use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use cargo_metadata::{Metadata, MetadataCommand, Package};
use codespan_reporting::diagnostic::{Diagnostic as RenderedDiagnostic, Label};
use codespan_reporting::files::SimpleFiles;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};
use codespan_reporting::term::{self, Config};
use proc_macro2::Span;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, Item, Macro, Token, UseTree, parenthesized};

#[derive(Clone)]
struct Diagnostic {
    span: Span,
    related: Vec<(Span, String)>,
    message: String,
    help: String,
    code: &'static str,
}

#[derive(Clone)]
enum Receive {
    All(Span),
    Only(Vec<Expr>, Span),
}

struct IoInvocation {
    context: Expr,
    receives: Vec<Receive>,
    arms: Vec<Expr>,
}

struct ReceiveHead;

impl Parse for ReceiveHead {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name = input.parse::<syn::Ident>()?;
        if name != "recv" {
            return Err(syn::Error::new(name.span(), "expected recv"));
        }
        let content;
        parenthesized!(content in input);
        let _: proc_macro2::TokenStream = content.parse()?;
        input.parse::<Token![=>]>()?;
        Ok(Self)
    }
}

struct ContextlessInvocation {
    arms: Vec<Expr>,
}

impl Parse for ContextlessInvocation {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let arms = Punctuated::<Expr, Token![,]>::parse_terminated(input)?;
        if arms.is_empty() {
            return Err(input.error("concurrent! requires at least one arm"));
        }
        Ok(Self {
            arms: arms.into_iter().collect(),
        })
    }
}

impl Parse for IoInvocation {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let context: Expr = input.parse()?;
        input.parse::<Token![;]>()?;
        let mut receives = Vec::new();
        let mut arms = Vec::new();
        while !input.is_empty() {
            let is_recv = input.fork().parse::<ReceiveHead>().is_ok();
            if is_recv {
                let recv = input.parse::<syn::Ident>()?;
                let content;
                parenthesized!(content in input);
                let span = recv.span();
                if content.peek(Token![..]) {
                    content.parse::<Token![..]>()?;
                    if !content.is_empty() {
                        return Err(content.error("recv(..) takes no additional arguments"));
                    }
                    receives.push(Receive::All(span));
                } else {
                    let tags = Punctuated::<Expr, Token![,]>::parse_terminated(&content)?;
                    if tags.is_empty() {
                        return Err(content.error("a tagged recv arm needs at least one tag"));
                    }
                    receives.push(Receive::Only(tags.into_iter().collect(), span));
                }
                input.parse::<Token![=>]>()?;
                arms.push(input.parse()?);
            } else {
                arms.push(input.parse()?);
            }
            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }
        Ok(Self {
            context,
            receives,
            arms,
        })
    }
}

struct UseCollector {
    trees: Vec<UseTree>,
    externs: Vec<(String, String, bool)>,
}

impl Visit<'_> for UseCollector {
    fn visit_item_use(&mut self, item: &syn::ItemUse) {
        self.trees.push(item.tree.clone());
    }

    fn visit_item_extern_crate(&mut self, item: &syn::ItemExternCrate) {
        let local_name = item
            .rename
            .as_ref()
            .map_or_else(|| item.ident.to_string(), |(_, name)| name.to_string());
        let macro_use = item
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("macro_use"));
        self.externs
            .push((item.ident.to_string(), local_name, macro_use));
    }

    fn visit_item_mod(&mut self, _: &syn::ItemMod) {}

    fn visit_block(&mut self, _: &syn::Block) {}
}

fn macro_imports(
    items: &[Item],
    aliases: &mut HashSet<String>,
    output: &mut HashSet<String>,
    inherited: &HashSet<String>,
) {
    let mut collector = UseCollector {
        trees: Vec::new(),
        externs: Vec::new(),
    };
    for item in items {
        collector.visit_item(item);
    }
    for (source, local, macro_use) in &collector.externs {
        if aliases.contains(source) {
            aliases.insert(local.clone());
            if *macro_use {
                output.insert("concurrent".to_string());
            }
        }
    }
    for tree in &collector.trees {
        collect_crate_aliases(tree, &mut Vec::new(), aliases);
    }
    for tree in &collector.trees {
        collect_use(tree, &mut Vec::new(), aliases, inherited, output);
    }
}

fn collect_crate_aliases(tree: &UseTree, path: &mut Vec<String>, aliases: &mut HashSet<String>) {
    match tree {
        UseTree::Path(node) => {
            path.push(node.ident.to_string());
            collect_crate_aliases(&node.tree, path, aliases);
            path.pop();
        }
        UseTree::Rename(node) => {
            let source = node.ident.to_string();
            let is_crate = (path.is_empty() && aliases.contains(&source))
                || (source == "self" && path.last().is_some_and(|part| aliases.contains(part)));
            if is_crate {
                aliases.insert(node.rename.to_string());
            }
        }
        UseTree::Group(node) => {
            for item in &node.items {
                collect_crate_aliases(item, path, aliases);
            }
        }
        UseTree::Name(_) | UseTree::Glob(_) => {}
    }
}

fn collect_use(
    tree: &UseTree,
    path: &mut Vec<String>,
    aliases: &HashSet<String>,
    inherited: &HashSet<String>,
    output: &mut HashSet<String>,
) {
    match tree {
        UseTree::Path(node) => {
            path.push(node.ident.to_string());
            collect_use(&node.tree, path, aliases, inherited, output);
            path.pop();
        }
        UseTree::Name(node) => {
            let source = node.ident.to_string();
            let from_trame =
                source == "concurrent" && path.first().is_some_and(|first| aliases.contains(first));
            let from_parent = path
                .iter()
                .any(|part| part == "super" || part == "crate" || part == "self")
                && inherited.contains(&source);
            if from_trame || from_parent {
                output.insert(source);
            }
        }
        UseTree::Rename(node) => {
            let source = node.ident.to_string();
            let from_trame =
                source == "concurrent" && path.first().is_some_and(|first| aliases.contains(first));
            let from_parent = path
                .iter()
                .any(|part| part == "super" || part == "crate" || part == "self")
                && inherited.contains(&source);
            if from_trame || from_parent {
                output.insert(node.rename.to_string());
            }
        }
        UseTree::Group(node) => {
            for item in &node.items {
                collect_use(item, path, aliases, inherited, output);
            }
        }
        UseTree::Glob(_) => {
            if path.first().is_some_and(|first| aliases.contains(first)) {
                output.insert("concurrent".to_string());
            } else if path.iter().any(|part| part == "super" || part == "crate") {
                output.extend(inherited.iter().cloned());
            }
        }
    }
}

fn rust_crate_name(name: &str) -> String {
    name.replace('-', "_")
}

fn crate_aliases(package: &Package) -> HashSet<String> {
    package
        .dependencies
        .iter()
        .filter(|dependency| dependency.name == "trame")
        .map(|dependency| match &dependency.rename {
            Some(rename) => rust_crate_name(rename),
            None => "trame".to_string(),
        })
        .chain((package.name == "trame").then(|| "trame".to_string()))
        .chain((package.name == "trame").then(|| "crate".to_string()))
        .collect()
}

fn is_trame_concurrent(mac: &Macro, aliases: &HashSet<String>, imports: &HashSet<String>) -> bool {
    let segments: Vec<_> = mac
        .path
        .segments
        .iter()
        .map(|part| part.ident.to_string())
        .collect();
    if segments.len() == 1 {
        return imports.contains(&segments[0]);
    }
    (segments.last().is_some_and(|last| last == "concurrent")
        && segments
            .first()
            .is_some_and(|first| aliases.contains(first)))
        || (segments
            .first()
            .is_some_and(|first| first == "super" || first == "crate" || first == "self")
            && segments.last().is_some_and(|last| imports.contains(last)))
}

type ConstValues = HashMap<String, Option<Expr>>;

fn const_values(items: &[Item]) -> ConstValues {
    let mut values = HashMap::new();
    for item in items {
        if let Item::Const(item) = item {
            let name = item.ident.to_string();
            let expression = (*item.expr).clone();
            match values.entry(name) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Some(expression));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
        }
    }
    values
}

fn is_trame_tag_constructor(path: &syn::Path, aliases: &HashSet<String>) -> bool {
    let names: Vec<_> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    path.leading_colon.is_some()
        && names.len() >= 3
        && names[names.len() - 2] == "Tag"
        && names.last().is_some_and(|name| name == "new")
        && names.first().is_some_and(|name| aliases.contains(name))
}

fn eval_receiver_tag(
    expr: &Expr,
    constants: &ConstValues,
    aliases: &HashSet<String>,
) -> Option<u16> {
    match expr {
        Expr::Path(path)
            if path.qself.is_none()
                && path.path.segments.len() == 2
                && path.path.segments[0].ident == "self" =>
        {
            eval_u16(expr, constants, aliases, &mut HashSet::new())
        }
        Expr::Call(call) => {
            let Expr::Path(function) = &*call.func else {
                return None;
            };
            if !is_trame_tag_constructor(&function.path, aliases) || call.args.len() != 1 {
                return None;
            }
            if !explicit_tag_value(&call.args[0]) {
                return None;
            }
            eval_u16(&call.args[0], constants, aliases, &mut HashSet::new())
        }
        Expr::Paren(paren) => eval_receiver_tag(&paren.expr, constants, aliases),
        Expr::Group(group) => eval_receiver_tag(&group.expr, constants, aliases),
        _ => None,
    }
}

fn explicit_tag_value(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(value) => matches!(value.lit, syn::Lit::Int(_)),
        Expr::Path(path) => {
            path.qself.is_none()
                && path.path.segments.len() == 2
                && path.path.segments[0].ident == "self"
        }
        Expr::Binary(binary) => {
            explicit_tag_value(&binary.left) && explicit_tag_value(&binary.right)
        }
        Expr::Paren(paren) => explicit_tag_value(&paren.expr),
        Expr::Group(group) => explicit_tag_value(&group.expr),
        _ => false,
    }
}

fn eval_u16(
    expr: &Expr,
    constants: &ConstValues,
    aliases: &HashSet<String>,
    visiting: &mut HashSet<String>,
) -> Option<u16> {
    match expr {
        Expr::Lit(value) => match &value.lit {
            syn::Lit::Int(value) => value.base10_parse::<u16>().ok(),
            _ => None,
        },
        Expr::Path(path)
            if path.qself.is_none()
                && (path.path.segments.len() == 1
                    || (path.path.segments.len() == 2
                        && path.path.segments[0].ident == "self")) =>
        {
            let name = path.path.segments.last()?.ident.to_string();
            if !visiting.insert(name.clone()) {
                return None;
            }
            let result = constants
                .get(&name)
                .and_then(Option::as_ref)
                .and_then(|expr| eval_u16(expr, constants, aliases, visiting));
            visiting.remove(&name);
            result
        }
        Expr::Call(call) => {
            let Expr::Path(function) = &*call.func else {
                return None;
            };
            if !is_trame_tag_constructor(&function.path, aliases) || call.args.len() != 1 {
                return None;
            }
            eval_u16(&call.args[0], constants, aliases, visiting)
        }
        Expr::Binary(binary) => {
            let left = eval_u16(&binary.left, constants, aliases, visiting)?;
            let right = eval_u16(&binary.right, constants, aliases, visiting)?;
            match binary.op {
                syn::BinOp::Add(_) => left.checked_add(right),
                syn::BinOp::Sub(_) => left.checked_sub(right),
                syn::BinOp::Mul(_) => left.checked_mul(right),
                syn::BinOp::Div(_) => left.checked_div(right),
                syn::BinOp::Rem(_) => left.checked_rem(right),
                syn::BinOp::BitAnd(_) => Some(left & right),
                syn::BinOp::BitOr(_) => Some(left | right),
                syn::BinOp::BitXor(_) => Some(left ^ right),
                syn::BinOp::Shl(_) => left.checked_shl(u32::from(right)),
                syn::BinOp::Shr(_) => left.checked_shr(u32::from(right)),
                _ => None,
            }
        }
        Expr::Paren(expr) => eval_u16(&expr.expr, constants, aliases, visiting),
        Expr::Group(expr) => eval_u16(&expr.expr, constants, aliases, visiting),
        _ => None,
    }
}

fn check_invocation(
    invocation: &IoInvocation,
    constants: &ConstValues,
    aliases: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let all: Vec<_> = invocation
        .receives
        .iter()
        .filter(|receive| matches!(receive, Receive::All(_)))
        .collect();
    let explicit: Vec<_> = invocation
        .receives
        .iter()
        .filter_map(|receive| match receive {
            Receive::Only(tags, span) => Some((tags, span)),
            Receive::All(_) => None,
        })
        .collect();

    if !all.is_empty() && invocation.receives.len() > 1 {
        if let Some(span) = all.iter().find_map(|receive| match receive {
            Receive::All(span) => Some(*span),
            Receive::Only(_, _) => None,
        }) {
            let mut skipped_primary = false;
            let related = invocation
                .receives
                .iter()
                .filter_map(|receive| match receive {
                    Receive::All(_) if !skipped_primary => {
                        skipped_primary = true;
                        None
                    }
                    Receive::All(other) => {
                        Some((*other, "another catch-all arm also receives".to_string()))
                    }
                    Receive::Only(_, other) => {
                        Some((*other, "a tagged arm also receives".to_string()))
                    }
                })
                .collect();
            diagnostics.push(Diagnostic {
                span,
                related,
                message: "catch-all receive overlaps another receiver".into(),
                help: "keep recv(..) as the only receiving arm, or replace it with disjoint explicit tag sets".into(),
                code: "TR001",
            });
        }
        return;
    }

    if explicit.len() < 2 {
        return;
    }
    let mut owners = BTreeMap::<u16, (usize, Span)>::new();
    for (arm, (tags, _)) in explicit.into_iter().enumerate() {
        for tag in tags {
            let Some(value) = eval_receiver_tag(tag, constants, aliases) else {
                diagnostics.push(Diagnostic {
                    span: tag.span(),
                    related: Vec::new(),
                    message: "cannot prove these receive resources are disjoint".into(),
                    help: "use ::trame::Tag::new(integer_literal) or ::trame::Tag::new(self::CONST); absolute crate paths avoid local constructor shadowing".into(),
                    code: "TR002",
                });
                continue;
            };
            if let Some((previous_arm, _)) = owners.get(&value) {
                if *previous_arm != arm {
                    let (_, first_span) = owners[&value];
                    diagnostics.push(Diagnostic {
                        span: tag.span(),
                        related: vec![(first_span, "first arm claiming this tag".into())],
                        message: format!("receive tag {value} has more than one arm owner"),
                        help: "assign this tag to exactly one receive arm; use distinct tags for concurrent receivers".into(),
                        code: "TR003",
                    });
                }
            } else {
                owners.insert(value, (arm, tag.span()));
            }
        }
    }
}

fn contains_receive_arm(tokens: proc_macro2::TokenStream) -> bool {
    use proc_macro2::{Delimiter, TokenTree};

    let trees: Vec<_> = tokens.into_iter().collect();
    if trees.windows(4).any(|window| {
        matches!(&window[0], TokenTree::Ident(name) if name == "recv")
            && matches!(&window[1], TokenTree::Group(group) if group.delimiter() == Delimiter::Parenthesis)
            && matches!(&window[2], TokenTree::Punct(punct) if punct.as_char() == '=')
            && matches!(&window[3], TokenTree::Punct(punct) if punct.as_char() == '>')
    }) {
        return true;
    }
    trees.into_iter().any(|tree| match tree {
        TokenTree::Group(group) => contains_receive_arm(group.stream()),
        _ => false,
    })
}

fn contains_trame_concurrent_call(
    tokens: &proc_macro2::TokenStream,
    aliases: &HashSet<String>,
    imports: &HashSet<String>,
) -> bool {
    let source = tokens.to_string();
    aliases
        .iter()
        .any(|alias| source.contains(&format!("{alias} :: concurrent !")))
        || imports
            .iter()
            .any(|name| source.contains(&format!("{name} !")))
}

fn unsupported_macro(span: Span) -> Diagnostic {
    Diagnostic {
        span,
        related: Vec::new(),
        message: "receive declarations inside macro expansions cannot be verified".into(),
        help: "write the trame::concurrent! receive arms directly so ownership can be checked"
            .into(),
        code: "TR005",
    }
}

struct MacroVisitor<'a> {
    aliases: &'a HashSet<String>,
    imports: &'a HashSet<String>,
    constants: &'a ConstValues,
    inside_arm: bool,
    diagnostics: Vec<Diagnostic>,
}

impl MacroVisitor<'_> {
    fn inspect_expressions(&mut self, expressions: &[Expr], inside_arm: bool) {
        let mut visitor = MacroVisitor {
            aliases: self.aliases,
            imports: self.imports,
            constants: self.constants,
            inside_arm,
            diagnostics: Vec::new(),
        };
        for expression in expressions {
            visitor.visit_expr(expression);
        }
        self.diagnostics.extend(visitor.diagnostics);
    }
}

impl Visit<'_> for MacroVisitor<'_> {
    fn visit_macro(&mut self, mac: &Macro) {
        if is_trame_concurrent(mac, self.aliases, self.imports) {
            let has_io_separator = mac.tokens.clone().into_iter().any(|token| {
                matches!(token, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == ';')
            });
            if has_io_separator {
                match syn::parse2::<IoInvocation>(mac.tokens.clone()) {
                    Ok(invocation) => {
                        let has_receivers = !invocation.receives.is_empty();
                        if self.inside_arm && has_receivers {
                            self.diagnostics.push(Diagnostic {
                                span: mac.span(),
                                related: Vec::new(),
                                message: "nested concurrent! receive arms cannot share the enclosing arm's resources".into(),
                                help: "lift this receiving invocation out of the arm or give it an independently owned endpoint".into(),
                                code: "TR006",
                            });
                            return;
                        }
                        check_invocation(
                            &invocation,
                            self.constants,
                            self.aliases,
                            &mut self.diagnostics,
                        );
                        self.inspect_expressions(std::slice::from_ref(&invocation.context), false);
                        for receive in &invocation.receives {
                            if let Receive::Only(tags, _) = receive {
                                self.inspect_expressions(tags, false);
                            }
                        }
                        self.inspect_expressions(&invocation.arms, true);
                    }
                    Err(error) => self.diagnostics.push(Diagnostic {
                        span: error.span(),
                        related: Vec::new(),
                        message: "cannot analyze this concurrent! receive declaration".into(),
                        help: format!("the verifier could not parse the arm list: {error}"),
                        code: "TR004",
                    }),
                }
            } else {
                match syn::parse2::<ContextlessInvocation>(mac.tokens.clone()) {
                    Ok(invocation) => self.inspect_expressions(&invocation.arms, self.inside_arm),
                    Err(error) => self.diagnostics.push(Diagnostic {
                        span: error.span(),
                        related: Vec::new(),
                        message: "cannot analyze this concurrent! arm list".into(),
                        help: format!("the verifier could not parse the arm list: {error}"),
                        code: "TR004",
                    }),
                }
            }
        } else if contains_receive_arm(mac.tokens.clone())
            && contains_trame_concurrent_call(&mac.tokens, self.aliases, self.imports)
        {
            self.diagnostics.push(unsupported_macro(mac.span()));
        }
        visit::visit_macro(self, mac);
    }

    fn visit_item_macro(&mut self, item: &syn::ItemMacro) {
        if item.mac.path.is_ident("macro_rules")
            && contains_trame_concurrent_call(&item.mac.tokens, self.aliases, self.imports)
        {
            self.diagnostics.push(unsupported_macro(item.span()));
        }
        visit::visit_item_macro(self, item);
    }

    fn visit_item_mod(&mut self, module: &syn::ItemMod) {
        if let Some((_, items)) = &module.content {
            analyze_items(items, self.aliases, self.imports, &mut self.diagnostics);
        }
    }

    fn visit_block(&mut self, block: &syn::Block) {
        let items: Vec<_> = block
            .stmts
            .iter()
            .filter_map(|statement| match statement {
                syn::Stmt::Item(item) => Some(item.clone()),
                _ => None,
            })
            .collect();
        let mut aliases = self.aliases.clone();
        let mut imports = self.imports.clone();
        macro_imports(&items, &mut aliases, &mut imports, self.imports);
        remove_local_macro_shadows(&items, &mut imports);
        let mut scoped = MacroVisitor {
            aliases: &aliases,
            imports: &imports,
            constants: self.constants,
            inside_arm: self.inside_arm,
            diagnostics: Vec::new(),
        };
        for statement in &block.stmts {
            match statement {
                syn::Stmt::Item(Item::Use(_)) => {}
                syn::Stmt::Item(item) => scoped.visit_item(item),
                syn::Stmt::Local(local) => scoped.visit_local(local),
                syn::Stmt::Expr(expr, _) => scoped.visit_expr(expr),
                syn::Stmt::Macro(mac) => scoped.visit_macro(&mac.mac),
            }
        }
        self.diagnostics.extend(scoped.diagnostics);
    }
}

fn remove_local_macro_shadows(items: &[Item], imports: &mut HashSet<String>) {
    for item in items {
        if let Item::Macro(item) = item {
            if let Some(name) = &item.ident {
                imports.remove(&name.to_string());
            }
        }
    }
}

fn module_base(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let directory = path.parent().map(Path::to_path_buf).unwrap_or_default();
    if stem == "lib" || stem == "main" || stem == "mod" {
        directory
    } else {
        directory.join(stem)
    }
}

fn resolve_child_module(base: &Path, name: &str) -> Option<PathBuf> {
    let direct = base.join(format!("{name}.rs"));
    if direct.is_file() {
        return Some(direct);
    }
    let nested = base.join(name).join("mod.rs");
    if nested.is_file() {
        return Some(nested);
    }
    None
}

fn scan_module_imports(
    items: &[Item],
    base: &Path,
    crate_aliases: &HashSet<String>,
    inherited: &HashSet<String>,
    files: &HashMap<PathBuf, syn::File>,
    inherited_by_file: &mut HashMap<PathBuf, HashSet<String>>,
) {
    let mut aliases = crate_aliases.clone();
    let mut imports = HashSet::new();
    macro_imports(items, &mut aliases, &mut imports, inherited);
    remove_local_macro_shadows(items, &mut imports);
    for item in items {
        let Item::Mod(module) = item else { continue };
        let name = module.ident.to_string();
        if let Some((_, inner)) = &module.content {
            scan_module_imports(
                inner,
                &base.join(&name),
                &aliases,
                &imports,
                files,
                inherited_by_file,
            );
        } else if let Some(path) = resolve_child_module(base, &name) {
            if let Some(file) = files.get(&path) {
                inherited_by_file.insert(path.clone(), imports.clone());
                scan_module_imports(
                    &file.items,
                    &module_base(&path),
                    &aliases,
                    &imports,
                    files,
                    inherited_by_file,
                );
            }
        }
    }
}

fn analyze_items(
    items: &[Item],
    crate_aliases: &HashSet<String>,
    inherited_imports: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let constants = const_values(items);
    let mut aliases = crate_aliases.clone();
    let mut imports = HashSet::new();
    macro_imports(items, &mut aliases, &mut imports, inherited_imports);
    remove_local_macro_shadows(items, &mut imports);
    for item in items {
        if let Item::Mod(module) = item {
            if let Some((_, items)) = &module.content {
                analyze_items(items, &aliases, &imports, diagnostics);
            }
        } else {
            let mut visitor = MacroVisitor {
                aliases: &aliases,
                imports: &imports,
                constants: &constants,
                inside_arm: false,
                diagnostics: Vec::new(),
            };
            visitor.visit_item(item);
            diagnostics.extend(visitor.diagnostics);
        }
    }
}

fn rust_sources(root: &Path, found: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if entry.file_type()?.is_dir() {
            if name == "target"
                || name == ".git"
                || name.starts_with('.')
                || (name == "declare"
                    && path
                        .parent()
                        .and_then(Path::file_name)
                        .is_some_and(|parent| parent == "scripts"))
            {
                continue;
            }
            match fs::metadata(path.join("Cargo.toml")) {
                Ok(metadata) if metadata.is_file() => continue,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            rust_sources(&path, found)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
    Ok(())
}

fn package_is_consumer(package: &Package, root_package: Option<&Package>) -> bool {
    package
        .dependencies
        .iter()
        .any(|dependency| dependency.name == "trame")
        || root_package.is_some_and(|root| root.id == package.id && package.name == "trame")
}

fn check_package(
    package: &Package,
    diagnostics: &mut Vec<(PathBuf, Diagnostic)>,
) -> std::io::Result<usize> {
    let root = package
        .manifest_path
        .as_std_path()
        .parent()
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "package manifest has no parent directory",
            )
        })?;
    let aliases = crate_aliases(package);
    let mut roots = vec![root.to_path_buf()];
    let mut files = package
        .targets
        .iter()
        .map(|target| target.src_path.as_std_path().to_path_buf())
        .collect::<Vec<_>>();
    for target in &package.targets {
        let source = target.src_path.as_std_path();
        if let Some(parent) = source.parent() {
            if !parent.starts_with(root) {
                roots.push(parent.to_path_buf());
            }
        }
    }
    roots.sort();
    roots.dedup();
    for root in roots {
        rust_sources(&root, &mut files)?;
    }
    files.sort();
    files.dedup();
    let mut parsed = HashMap::new();
    for path in &files {
        let source = fs::read_to_string(path)?;
        match syn::parse_file(&source) {
            Ok(file) => {
                parsed.insert(path.clone(), file);
            }
            Err(error) => {
                diagnostics.push((
                    path.clone(),
                    Diagnostic {
                        span: error.span(),
                        related: Vec::new(),
                        message: "cannot parse this Rust source for receive verification".into(),
                        help: format!("fix the Rust syntax before running Cargo: {error}"),
                        code: "TR000",
                    },
                ));
            }
        }
    }
    let mut inherited_by_file = HashMap::new();
    for target in &package.targets {
        let path = target.src_path.as_std_path().to_path_buf();
        if let Some(file) = parsed.get(&path) {
            scan_module_imports(
                &file.items,
                &module_base(&path),
                &aliases,
                &HashSet::new(),
                &parsed,
                &mut inherited_by_file,
            );
        }
    }
    let invoke_impl = root.join("invoke.rs");
    let skip_invoke_impl = package.name == "trame" && files.contains(&invoke_impl);
    let file_count = if skip_invoke_impl {
        files.len() - 1
    } else {
        files.len()
    };
    for path in files {
        if skip_invoke_impl && path == invoke_impl {
            continue;
        }
        let Some(file) = parsed.get(&path) else {
            continue;
        };
        let inherited = inherited_by_file.get(&path).cloned().unwrap_or_default();
        let mut file_diagnostics = Vec::new();
        analyze_items(&file.items, &aliases, &inherited, &mut file_diagnostics);
        diagnostics.extend(
            file_diagnostics
                .into_iter()
                .map(|diagnostic| (path.clone(), diagnostic)),
        );
    }
    Ok(file_count)
}

fn span_offset(source: &str, span: Span, end: bool) -> usize {
    let position = if end { span.end() } else { span.start() };
    let mut offset = 0;
    for (line, text) in source.split_inclusive('\n').enumerate() {
        if line + 1 == position.line {
            let line_text = match text.strip_suffix('\n') {
                Some(line_text) => line_text,
                None => text,
            };
            let byte_column = match line_text.char_indices().nth(position.column) {
                Some((byte_column, _)) => byte_column,
                None => line_text.len(),
            };
            return (offset + byte_column).min(source.len());
        }
        offset += text.len();
    }
    source.len()
}

fn report_diagnostics(
    diagnostics: Vec<(PathBuf, Diagnostic)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut files = SimpleFiles::new();
    let mut ids = HashMap::<PathBuf, usize>::new();
    let mut source_cache = HashMap::<PathBuf, String>::new();
    let writer = StandardStream::stderr(ColorChoice::Auto);
    let config = Config::default();

    for (path, diagnostic) in diagnostics {
        let file_id = if let Some(id) = ids.get(&path) {
            *id
        } else {
            let source = fs::read_to_string(&path)?;
            let id = files.add(path.display().to_string(), source.clone());
            source_cache.insert(path.clone(), source);
            ids.insert(path.clone(), id);
            id
        };
        let source = &source_cache[&path];
        let start = span_offset(source, diagnostic.span, false);
        let mut end = span_offset(source, diagnostic.span, true);
        if end <= start {
            end = (start + 1).min(source.len());
        }
        let mut labels =
            vec![Label::primary(file_id, start..end).with_message(&diagnostic.message)];
        for (span, message) in diagnostic.related {
            let start = span_offset(source, span, false);
            let mut end = span_offset(source, span, true);
            if end <= start {
                end = (start + 1).min(source.len());
            }
            labels.push(Label::secondary(file_id, start..end).with_message(message));
        }
        let rendered = RenderedDiagnostic::error()
            .with_code(diagnostic.code)
            .with_message(diagnostic.message)
            .with_labels(labels)
            .with_notes(vec![format!("help: {}", diagnostic.help)]);
        term::emit(&mut writer.lock(), &config, &files, &rendered)?;
    }
    Err("trame static verification failed".into())
}

fn cargo_manifest(args: &[String]) -> Option<PathBuf> {
    let options_end = match args.iter().position(|arg| arg == "--") {
        Some(index) => index,
        None => args.len(),
    };
    let options = &args[..options_end];
    options.iter().enumerate().find_map(|(index, arg)| {
        if arg == "--manifest-path" {
            options.get(index + 1).map(PathBuf::from)
        } else {
            arg.strip_prefix("--manifest-path=").map(PathBuf::from)
        }
    })
}

fn verify(manifest: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    let mut command = MetadataCommand::new();
    if let Some(manifest) = manifest {
        command.manifest_path(manifest);
    }
    let metadata: Metadata = command.no_deps().exec()?;
    let root_package = metadata.root_package();
    let mut diagnostics = Vec::new();
    let mut checked_packages = 0;
    let mut checked_files = 0;
    for package in &metadata.packages {
        if package_is_consumer(package, root_package) {
            checked_files += check_package(package, &mut diagnostics)?;
            checked_packages += 1;
        }
    }
    if diagnostics.is_empty() {
        println!(
            "cargo-trame: checked {checked_files} Rust files in {checked_packages} trame-using package(s)"
        );
        return Ok(());
    }
    report_diagnostics(diagnostics)
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let Some(first) = args.next() else {
        eprintln!("usage: cargo trame <check|test|build|clippy|run|doc|verify> [cargo args]");
        return Ok(ExitCode::from(2));
    };
    let command = if first == "trame" {
        args.next().ok_or("cargo trame needs a subcommand")?
    } else {
        first
    };
    if command == "--help" || command == "-h" {
        println!("usage: cargo trame <check|test|build|clippy|run|doc|verify> [cargo args]");
        return Ok(ExitCode::SUCCESS);
    }
    let rest: Vec<_> = args.collect();
    let manifest = cargo_manifest(&rest);

    if command == "verify" {
        verify(manifest.as_deref())?;
        return Ok(ExitCode::SUCCESS);
    }
    if !matches!(
        command.as_str(),
        "check" | "test" | "build" | "clippy" | "run" | "doc"
    ) {
        return Err(format!("unsupported cargo command `{command}`").into());
    }
    verify(manifest.as_deref())?;
    let status = Command::new("cargo").arg(command).args(rest).status()?;
    let code = status
        .code()
        .ok_or("cargo was terminated without an exit code")?;
    let code = u8::try_from(code)
        .map_err(|_| "cargo returned an exit code outside the process status range")?;
    Ok(ExitCode::from(code))
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("cargo-trame: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze(source: &str) -> Vec<Diagnostic> {
        let file = syn::parse_file(source).expect("Rust source parses");
        let aliases = HashSet::from(["trame".to_string()]);
        let mut diagnostics = Vec::new();
        analyze_items(&file.items, &aliases, &HashSet::new(), &mut diagnostics);
        diagnostics
    }

    fn analyze_module_tree(files: &[(&str, &str)]) -> Vec<Diagnostic> {
        let directory = std::env::temp_dir().join(format!(
            "trame-verify-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).expect("temp directory");
        let mut parsed = HashMap::new();
        let mut root = PathBuf::new();
        for (name, source) in files {
            let path = directory.join(name);
            std::fs::write(&path, source).expect("write source");
            parsed.insert(path.clone(), syn::parse_file(source).expect("parse source"));
            if root.as_os_str().is_empty() {
                root = path;
            }
        }
        let aliases = HashSet::from(["trame".to_string()]);
        let mut inherited_by_file = HashMap::new();
        scan_module_imports(
            &parsed[&root].items,
            &module_base(&root),
            &aliases,
            &HashSet::new(),
            &parsed,
            &mut inherited_by_file,
        );
        let mut diagnostics = Vec::new();
        for (path, file) in &parsed {
            let inherited = inherited_by_file.get(path).cloned().unwrap_or_default();
            analyze_items(&file.items, &aliases, &inherited, &mut diagnostics);
        }
        std::fs::remove_dir_all(&directory).expect("clean temp directory");
        diagnostics
    }

    #[test]
    fn diagnostic_offset_counts_utf8_bytes_before_macro() {
        struct FirstMacro(Option<Span>);
        impl Visit<'_> for FirstMacro {
            fn visit_macro(&mut self, mac: &Macro) {
                if self.0.is_none() {
                    self.0 = Some(mac.span());
                }
            }
        }
        let source = "fn f() { let _ = \"🧠\"; trame::concurrent!(cx; recv(..) => a); }";
        let file = syn::parse_file(source).expect("the test source parses");
        let mut visitor = FirstMacro(None);
        visitor.visit_file(&file);
        let span = match visitor.0 {
            Some(span) => span,
            None => panic!("the source has a macro"),
        };
        let expected = match source.find("trame::concurrent!") {
            Some(offset) => offset,
            None => panic!("the source contains the macro path"),
        };
        assert_eq!(span_offset(source, span, false), expected);
    }

    #[test]
    fn manifest_path_after_run_separator_is_an_application_argument() {
        let args = vec![
            "--features".to_string(),
            "mpi".to_string(),
            "--".to_string(),
            "--manifest-path=/tmp/application-data".to_string(),
        ];
        assert_eq!(cargo_manifest(&args), None);
    }

    #[test]
    fn manifest_path_before_separator_is_cargo_configuration() {
        let args = vec![
            "--manifest-path=/tmp/application".to_string(),
            "--".to_string(),
            "--manifest-path=/tmp/application-data".to_string(),
        ];
        assert_eq!(
            cargo_manifest(&args),
            Some(PathBuf::from("/tmp/application"))
        );
    }

    #[test]
    fn process_named_recv_is_not_parsed_as_a_receive_setting() {
        let diagnostics = analyze("fn f() { trame::concurrent!(cx; recv); }");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn child_modules_inherit_super_macro_aliases() {
        let diagnostics = analyze_module_tree(&[
            ("lib.rs", "use trame::concurrent as run; mod child;"),
            (
                "child.rs",
                "use super::run; fn f() { run!(cx; recv(::trame::Tag::new(7)) => a, recv(::trame::Tag::new(7)) => b); }",
            ),
        ]);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("tag 7"));
    }

    #[test]
    fn block_local_macros_shadow_imported_trame_aliases() {
        let diagnostics = analyze(
            "use trame::concurrent as run; fn f() { macro_rules! run { ($($t:tt)*) => {}; } run!(cx; recv(x) => a); }",
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn cargo_dependency_aliases_are_normalized_for_rust_paths() {
        assert_eq!(rust_crate_name("trame-api"), "trame_api");
    }

    #[test]
    fn distinct_tag_owners_are_accepted() {
        let diagnostics = analyze(
            "const A: trame::Tag = ::trame::Tag::new(1); const B: trame::Tag = ::trame::Tag::new(2); fn f() { trame::concurrent!(cx; recv(self::A) => a, recv(self::B) => b); }",
        );
        if !diagnostics.is_empty() {
            panic!("{}", diagnostics[0].message);
        }
    }

    #[test]
    fn const_arithmetic_resolves_explicit_tag_sets() {
        let diagnostics = analyze(
            "const A: u16 = 20; const B: u16 = A + 1; fn f() { trame::concurrent!(cx; recv(::trame::Tag::new(self::A)) => a, recv(::trame::Tag::new(self::B)) => b); }",
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn unrelated_qualified_concurrent_macro_is_not_trame() {
        let diagnostics = analyze("fn f() { other::trame::concurrent!(recv(x) => 1); }");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn nested_trame_segment_is_not_a_trame_constructor_root() {
        let diagnostics = analyze(
            "fn f() { trame::concurrent!(cx; recv(::other::trame::Tag::new(1)) => a, recv(::other::trame::Tag::new(2)) => b); }",
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == "TR002")
        );
    }

    #[test]
    fn unqualified_tag_constructor_is_not_assumed_to_be_trame() {
        let diagnostics = analyze(
            "mod Tag { pub const fn new(_: u16) -> trame::Tag { trame::Tag::new(7) } } fn f() { trame::concurrent!(cx; recv(Tag::new(1)) => a, recv(Tag::new(2)) => b); }",
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "TR002")
        );
    }

    #[test]
    fn feature_gated_duplicate_constants_are_refused() {
        let diagnostics = analyze(
            "#[cfg(feature = \"overlap\")] const A: u16 = 1; #[cfg(not(feature = \"overlap\"))] const A: u16 = 2; const B: u16 = 1; fn f() { trame::concurrent!(cx; recv(::trame::Tag::new(self::A)) => a, recv(::trame::Tag::new(self::B)) => b); }",
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "TR002")
        );
    }

    #[test]
    fn block_local_modules_use_their_own_constants() {
        let diagnostics = analyze(
            "const LEFT: trame::Tag = ::trame::Tag::new(1); const RIGHT: trame::Tag = ::trame::Tag::new(2); fn outer() { mod inner { const LEFT: trame::Tag = ::trame::Tag::new(3); const RIGHT: trame::Tag = ::trame::Tag::new(3); fn f() { trame::concurrent!(cx; recv(self::LEFT) => a, recv(self::RIGHT) => b); } } }",
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "TR003")
        );
    }

    #[test]
    fn unqualified_const_names_are_refused_because_they_can_be_shadowed() {
        let diagnostics = analyze(
            "const A: trame::Tag = trame::Tag::new(1); const B: trame::Tag = trame::Tag::new(2); fn f() { const B: trame::Tag = trame::Tag::new(1); trame::concurrent!(cx; recv(A) => a, recv(B) => b); }",
        );
        assert!(!diagnostics.is_empty());
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == "TR002")
        );
    }

    #[test]
    fn wildcard_cannot_compete_with_a_tagged_receiver() {
        let diagnostics = analyze(
            "fn f() { trame::concurrent!(cx; recv(trame::Tag::new(1)) => a, recv(..) => b); }",
        );
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0]
                .message
                .contains("catch-all receive overlaps")
        );
    }

    #[test]
    fn a_tag_repeated_within_one_arm_is_not_a_competing_owner() {
        let diagnostics = analyze(
            "fn f() { trame::concurrent!(cx; recv(::trame::Tag::new(7), ::trame::Tag::new(7)) => a, recv(::trame::Tag::new(8)) => b); }",
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn a_tag_cannot_have_two_receiver_owners() {
        let diagnostics = analyze(
            "fn f() { trame::concurrent!(cx; recv(::trame::Tag::new(7)) => a, recv(::trame::Tag::new(7)) => b); }",
        );
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("tag 7"));
    }

    #[test]
    fn unrelated_parent_macro_imports_are_not_treated_as_trame() {
        let diagnostics = analyze("use crate::assign as set; fn f() { set!(x => 1); }");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn renamed_crate_and_macro_imports_are_checked() {
        let diagnostics = analyze(
            "use trame as api; use api::concurrent as run; fn f() { run!(cx; recv(api::Tag::new(4)) => a, recv(..) => b); }",
        );
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0]
                .message
                .contains("catch-all receive overlaps")
        );
    }

    #[test]
    fn nested_modules_are_checked_in_their_import_scope() {
        let diagnostics = analyze(
            "mod nested { use trame::concurrent as run; fn f() { run!(cx; recv(trame::Tag::new(4)) => a, recv(..) => b); } }",
        );
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0]
                .message
                .contains("catch-all receive overlaps")
        );
    }

    #[test]
    fn parent_module_macro_imports_are_checked() {
        let diagnostics = analyze(
            "use trame::concurrent as run; mod nested { use super::run; fn f() { run!(cx; recv(..) => a, recv(trame::Tag::new(1)) => b); } }",
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "TR001")
        );
    }

    #[test]
    fn function_local_macro_imports_do_not_leak_to_siblings() {
        let diagnostics = analyze(
            "fn scoped() { use trame::concurrent as run; run!(cx; recv(..) => a, recv(::trame::Tag::new(1)) => b); } fn sibling() { run!(x => 1); }",
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "TR001");
    }

    #[test]
    fn nested_receivers_inside_a_tag_expression_are_checked() {
        let diagnostics = analyze(
            "fn f() { trame::concurrent!(cx; recv({ trame::concurrent!(cx; recv(..) => a, recv(::trame::Tag::new(7)) => b); ::trame::Tag::new(1) }) => x, recv(::trame::Tag::new(2)) => y); }",
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "TR001")
        );
    }

    #[test]
    fn local_macro_imports_and_globs_are_checked() {
        for source in [
            "fn f() { use trame::concurrent as run; run!(cx; recv(..) => a, recv(trame::Tag::new(1)) => b); }",
            "fn f() { use trame::*; concurrent!(cx; recv(..) => a, recv(trame::Tag::new(1)) => b); }",
        ] {
            let diagnostics = analyze(source);
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == "TR001")
            );
        }
    }

    #[test]
    fn unrelated_receive_shaped_macros_are_not_attributed_to_trame() {
        let diagnostics = analyze(
            "macro_rules! unrelated { ($($arms:tt)*) => {}; } fn f() { unrelated!(recv(x) => 1); }",
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn macro_wrappers_with_receive_arms_are_refused() {
        let diagnostics = analyze(
            "macro_rules! wrapper { ($($arms:tt)*) => { trame::concurrent!(cx; $($arms)*) }; } fn f() { wrapper!(recv(..) => a, recv(trame::Tag::new(1)) => b); }",
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "TR005")
        );
    }

    #[test]
    fn macro_definition_that_contains_receive_arms_is_refused() {
        let diagnostics = analyze(
            "macro_rules! hidden { () => { trame::concurrent!(cx; recv(..) => a, recv(trame::Tag::new(1)) => b) }; }",
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "TR005")
        );
    }

    #[test]
    fn conflicting_receivers_in_a_context_expression_are_checked() {
        let diagnostics = analyze(
            "fn f() { trame::concurrent!({ trame::concurrent!(cx; recv(..) => a, recv(::trame::Tag::new(4)) => b); cx }; recv(::trame::Tag::new(1)) => c); }",
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "TR001")
        );
    }

    #[test]
    fn contextless_arms_are_walked_for_nested_concurrent_calls() {
        let diagnostics = analyze(
            "fn f() { trame::concurrent!(trame::concurrent!(cx; recv(::trame::Tag::new(7)) => a, recv(::trame::Tag::new(7)) => b)); }",
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "TR003")
        );
    }

    #[test]
    fn nested_compute_only_invocations_in_an_arm_are_allowed() {
        let diagnostics = analyze(
            "fn f() { trame::concurrent!(cx; recv(..) => { trame::concurrent!(Compute, Compute).unwrap(); Inbox }); }",
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn nested_receivers_in_an_arm_are_refused() {
        let diagnostics = analyze(
            "fn f() { trame::concurrent!(cx; recv(trame::Tag::new(1)) => trame::concurrent!(cx; recv(..) => x)); }",
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "TR006")
        );
    }

    #[test]
    fn unprovable_multiple_tag_sets_are_refused() {
        let diagnostics = analyze(
            "fn f(a: trame::Tag, b: trame::Tag) { trame::concurrent!(cx; recv(a) => x, recv(b) => y); }",
        );
        assert_eq!(diagnostics.len(), 2);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.message.contains("cannot prove"))
        );
    }
}
