// `#[parallel]` and `#[ordered]`. Each rewrites a function into its driver: the
// same name, taking a `trame::Invocation`, the context and the item list, and running the body
// once per item through `trame::run`, the selected backend's lowering. The body survives only as
// a closure inside the driver, so the driver is the one way to run it and `invoke!` the one way to
// call the driver.

mod parse;

use parse::{Fun, Param, Wrong, function, is, refuse, text, word, wrong};
use proc_macro::{Delimiter, Group, Ident, Punct, Spacing, Span, TokenStream, TokenTree};

#[proc_macro_attribute]
pub fn parallel(attr: TokenStream, item: TokenStream) -> TokenStream {
    lower(attr, item)
}

/// Reached only when no `#[parallel]` above it consumed it.
#[proc_macro_attribute]
pub fn ordered(_: TokenStream, _: TokenStream) -> TokenStream {
    refuse(Wrong {
        at: Span::call_site(),
        says: "`#[ordered]` orders a `#[parallel]` function and is written below it".into(),
    })
}

fn lower(attr: TokenStream, item: TokenStream) -> TokenStream {
    if let Some(t) = attr.into_iter().next() {
        return refuse(Wrong {
            at: t.span(),
            says: "`#[parallel]` takes no arguments".into(),
        });
    }
    match function(item).and_then(driver) {
        Ok(out) => out,
        Err(w) => refuse(w),
    }
}

/// The item, the keyed slot if ordered, and the context.
struct Shape<'f> {
    item: &'f Param,
    slot: Option<&'f Param>,
    cx: &'f Param,
}

fn shape(f: &Fun) -> Result<Shape<'_>, Wrong> {
    if let Some(r) = &f.receiver {
        let shared = matches!(r.as_slice(), [a, s] if is(a, '&') && word(s, "self"))
            || matches!(r.as_slice(), [a, q, _, s] if is(a, '&') && is(q, '\'') && word(s, "self"));
        if r.iter().any(|t| word(t, "mut")) && is(&r[0], '&') {
            return wrong(r[0].span(), extra("&mut self"));
        }
        if !shared {
            return wrong(r[0].span(), "`#[parallel]` takes `&self` or no receiver");
        }
    }
    let [item, middle @ .., cx] = f.params.as_slice() else {
        return wrong(
            f.name.span(),
            "`#[parallel]` takes an item and then its context",
        );
    };
    if item.unique() {
        return wrong(item.at(), extra(&item.written()));
    }
    if !cx.unique() {
        return wrong(
            cx.at(),
            format!("`#[parallel]` takes its context as `&mut C`: each call owns it; `{}` is not", cx.written()),
        );
    }
    for p in middle {
        if !p.unique() {
            return wrong(
                p.at(),
                format!("`invoke!` passes an item and a context; `{}` has no source", p.written()),
            );
        }
        if f.ordered.is_none() {
            return wrong(
                p.at(),
                format!(
                    "`{}` is keyed state, which only an `#[ordered]` `#[parallel]` function takes",
                    p.written()
                ),
            );
        }
    }
    let slot = match middle {
        [] if f.ordered.is_some() => {
            return wrong(
                f.name.span(),
                "an `#[ordered]` function takes its keyed state `slot: &mut T` after the item",
            );
        }
        [] => None,
        [slot] => Some(slot),
        [_, second, ..] => return wrong(second.at(), extra(&second.written())),
    };
    Ok(Shape { item, slot, cx })
}

fn extra(written: &str) -> String {
    format!(
        "`{written}` is a second `&mut`: only a `#[parallel]` context and an `#[ordered]` slot are `&mut`"
    )
}

/// `key = item.field.path: K`, as the place and `K`.
fn key(args: &[TokenTree], at: Span, item: &Param) -> Result<(Vec<TokenTree>, Vec<TokenTree>), Wrong> {
    let [k, eq, rest @ ..] = args else {
        return wrong(at, "`#[ordered]` takes `key = item.field: K`");
    };
    if !word(k, "key") || !is(eq, '=') {
        return wrong(k.span(), "`#[ordered]` takes `key = item.field: K`");
    }
    let Some(c) = parse::colon(rest) else {
        return wrong(
            k.span(),
            format!("write the key's type: `key = {}: K`", text(rest)),
        );
    };
    let (place, ty) = (&rest[..c], &rest[c + 1..]);
    let Some(root) = item.name() else {
        return wrong(item.at(), "an `#[ordered]` item is one named parameter, so a key can be rooted in it");
    };
    let rooted = matches!(place.first(), Some(TokenTree::Ident(i)) if i.to_string() == root.to_string())
        && place[1..].chunks(2).all(|step| {
            matches!(step, [dot, TokenTree::Ident(_) | TokenTree::Literal(_)] if is(dot, '.'))
        });
    if !rooted || ty.is_empty() {
        return wrong(
            place.first().map_or(k.span(), |t| t.span()),
            format!(
                "the key is a place in the item, like `{root}.field: K`; `{}` is not",
                text(place)
            ),
        );
    }
    Ok((place.to_vec(), ty.to_vec()))
}

fn driver(f: Fun) -> Result<TokenStream, Wrong> {
    let s = shape(&f)?;
    let hidden = |name: &str| TokenTree::Ident(Ident::new(name, Span::mixed_site()));
    let (cx, items, keyed) = (hidden("cx"), hidden("items"), hidden("keyed"));
    let item_ty = &s.item.ty;

    let mut signature = code("_: ::trame::Invocation,");
    signature.extend([cx.clone(), punct(':')]);
    signature.extend(s.cx.ty.iter().cloned());
    signature.extend([punct(','), items.clone(), punct(':'), punct('&')]);
    signature.push(group(Delimiter::Bracket, item_ty.clone()));
    let mut ret = code("-> ::core::result::Result<(),");
    let mut call = code("::trame::run::");
    let lowering = if f.ordered.is_some() { "ordered" } else { "parallel" };
    call.push(TokenTree::Ident(Ident::new(lowering, Span::call_site())));
    let mut args = vec![cx, punct(','), items, punct(',')];

    let mut closure = vec![punct('|')];
    closure.extend(param(s.item));
    if let (Some(slot), Some((attr, at))) = (s.slot, &f.ordered) {
        let (place, key_ty) = key(attr, *at, s.item)?;
        let pointee = slot.pointee().expect("a keyed slot is `&mut T`");
        signature.extend([punct(','), keyed.clone(), punct(':')]);
        signature.extend(code("::trame::Keyed<'_,"));
        signature.extend(key_ty.iter().cloned());
        signature.push(punct(','));
        signature.extend(pointee);
        signature.push(punct('>'));
        ret.extend(code("::trame::Invoked<"));
        ret.extend(f.error.iter().cloned());
        ret.push(punct('>'));
        args.extend([keyed, punct(',')]);
        args.push(punct('|'));
        args.extend(param(s.item));
        args.extend([punct('|'), joint('-'), punct('>')]);
        args.extend(key_ty);
        args.push(group(Delimiter::Brace, place));
        args.push(punct(','));
        closure.push(punct(','));
        closure.extend(param(slot));
    } else {
        ret.extend(f.error.iter().cloned());
    }
    ret.push(punct('>'));
    closure.push(punct(','));
    closure.extend(param(s.cx));
    closure.extend([punct('|'), joint('-'), punct('>')]);
    closure.extend(code("::core::result::Result<(),"));
    closure.extend(f.error.iter().cloned());
    closure.push(punct('>'));
    closure.push(TokenTree::Group(f.body.clone()));
    args.extend(closure);
    call.push(group(Delimiter::Parenthesis, args));

    let mut all = Vec::new();
    if let Some(r) = f.receiver {
        all.extend(r);
        all.push(punct(','));
    }
    all.extend(signature);
    let mut out = f.attrs;
    out.extend(f.vis);
    out.push(TokenTree::Ident(Ident::new("fn", f.name.span())));
    out.push(TokenTree::Ident(f.name));
    out.push(group(Delimiter::Parenthesis, all));
    out.extend(ret);
    out.push(group(Delimiter::Brace, call));
    Ok(out.into_iter().collect())
}

fn param(p: &Param) -> Vec<TokenTree> {
    let mut out = p.pattern.clone();
    out.push(punct(':'));
    out.extend(p.ty.iter().cloned());
    out
}

fn code(s: &str) -> Vec<TokenTree> {
    s.parse::<TokenStream>()
        .expect("generated code parses")
        .into_iter()
        .collect()
}

fn punct(c: char) -> TokenTree {
    TokenTree::Punct(Punct::new(c, Spacing::Alone))
}

fn joint(c: char) -> TokenTree {
    TokenTree::Punct(Punct::new(c, Spacing::Joint))
}

fn group(d: Delimiter, inner: Vec<TokenTree>) -> TokenTree {
    TokenTree::Group(Group::new(d, inner.into_iter().collect()))
}
